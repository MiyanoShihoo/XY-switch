use super::{Capture, VerificationSettings, VerificationStatus, engine, safe_id, store};
use crate::{
    database::{Database, Provider},
    model_audit::{AuditRecord, Observation},
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc, Mutex, OnceLock, Weak,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};
use zeroize::Zeroizing;

struct State {
    settings: VerificationSettings,
    secret: Option<Zeroizing<String>>,
    generation: u64,
    last_probe: std::collections::HashMap<(String, String), std::time::Instant>,
}
struct Job {
    database: Database,
    event: i64,
    observation: Observation,
}
pub struct VerificationRuntime {
    connection: Weak<Mutex<Connection>>,
    state: Mutex<State>,
    sender: OnceLock<mpsc::SyncSender<Job>>,
    cancel: tokio::sync::watch::Sender<u64>,
    dropped: AtomicU64,
    storage_error: AtomicBool,
}

pub struct Reservation {
    pub id: String,
    pub anomaly_id: i64,
    pub capture: Capture,
    pub settings: VerificationSettings,
    #[cfg_attr(not(test), allow(dead_code))]
    pub reserved_tokens: u64,
    pub cancellation: tokio::sync::watch::Receiver<u64>,
}

pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

pub fn hmac_hex(key: &[u8], message: &[u8]) -> String {
    let mut block = Zeroizing::new([0u8; 64]);
    if key.len() > 64 {
        block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        block[..key.len()].copy_from_slice(key);
    }
    let mut inner = Zeroizing::new([0x36u8; 64]);
    let mut outer = Zeroizing::new([0x5cu8; 64]);
    for i in 0..64 {
        inner[i] ^= block[i];
        outer[i] ^= block[i];
    }
    let mut hash = Sha256::new();
    hash.update(inner.as_ref());
    hash.update(message);
    let digest = hash.finalize();
    let mut hash = Sha256::new();
    hash.update(outer.as_ref());
    hash.update(digest);
    format!("{:x}", hash.finalize())
}
fn new_secret() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}
fn secret() -> Result<Zeroizing<String>, String> {
    #[cfg(test)]
    {
        return Ok(Zeroizing::new(new_secret()));
    }
    #[cfg(not(test))]
    {
        #[cfg(debug_assertions)]
        if std::env::var_os("XY_SWITCH_TEST_DATA_DIR").is_some() {
            return Ok(Zeroizing::new(new_secret()));
        }
        let existing =
            crate::credential_store::get_secret("internal", "model-verification-hmac-v1")?;
        let value = match existing {
            Some(value) => value,
            None => {
                let value = new_secret();
                crate::credential_store::set_secret(
                    "internal",
                    "model-verification-hmac-v1",
                    &value,
                )?;
                value
            }
        };
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("验证指纹密钥不可用".into());
        }
        Ok(Zeroizing::new(value))
    }
}

impl VerificationRuntime {
    pub fn new(connection: Weak<Mutex<Connection>>) -> Arc<Self> {
        let loaded = (|| {
            let connection = connection.upgrade().ok_or("数据库不可用")?;
            let conn = connection.lock().map_err(|_| "数据库锁不可用")?;
            conn.execute_batch(store::SCHEMA)
                .map_err(|_| "无法初始化验证存储")?;
            // A restart cannot know whether an interrupted request was billed.
            conn.execute("UPDATE verification_probes SET state='interrupted' WHERE state IN ('reserved','running')",[]).map_err(|_|"无法恢复验证预算")?;
            store::load_settings(&conn)
        })();
        let mut failed = loaded.is_err();
        let mut settings = loaded.unwrap_or_default();
        let secret = if settings.enabled {
            match secret() {
                Ok(s) => Some(s),
                Err(_) => {
                    failed = true;
                    settings.enabled = false;
                    None
                }
            }
        } else {
            None
        };
        let (cancel, _) = tokio::sync::watch::channel(1);
        Arc::new(Self {
            connection,
            state: Mutex::new(State {
                settings,
                secret,
                generation: 1,
                last_probe: Default::default(),
            }),
            sender: OnceLock::new(),
            cancel,
            dropped: AtomicU64::new(0),
            storage_error: AtomicBool::new(failed),
        })
    }
    pub fn settings(&self) -> Result<VerificationSettings, String> {
        self.state
            .lock()
            .map(|s| s.settings.clone())
            .map_err(|_| "验证设置不可用".into())
    }
    pub fn set_settings(
        &self,
        settings: VerificationSettings,
    ) -> Result<VerificationSettings, String> {
        settings.validate()?;
        let mut state = self.state.lock().map_err(|_| "验证设置不可用")?;
        // Shutdown is effective even if persisting the preference subsequently fails.
        if !settings.enabled || !settings.active_probe_enabled {
            state.settings.enabled = settings.enabled && state.settings.enabled;
            state.settings.active_probe_enabled =
                settings.active_probe_enabled && state.settings.active_probe_enabled;
            state.generation = state.generation.wrapping_add(1);
            self.cancel.send_replace(state.generation);
        }
        // A zero budget revokes spending immediately, even if persistence fails.
        if settings.soft_token_budget_per_day == 0 && state.settings.soft_token_budget_per_day != 0 {
            state.settings.soft_token_budget_per_day = 0;
            state.generation = state.generation.wrapping_add(1);
            self.cancel.send_replace(state.generation);
        }
        let next_secret = if settings.enabled && state.secret.is_none() {
            Some(secret()?)
        } else {
            None
        };
        let connection = self.connection.upgrade().ok_or("数据库不可用")?;
        let mut conn = connection.lock().map_err(|_| "数据库不可用")?;
        conn.execute_batch(store::SCHEMA)
            .map_err(|_| "无法初始化验证存储")?;
        let text = serde_json::to_string(&settings).map_err(|_| "无法保存验证设置")?;
        let tx = conn.transaction().map_err(|_| "无法保存验证设置")?;
        tx.execute("INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP",params![store::SETTINGS_KEY,text]).map_err(|_|"无法保存验证设置")?;
        tx.execute("INSERT INTO verification_config_changes(created_at,action,config_json) VALUES(?1,'settings',?2)",params![now_ms(),text]).map_err(|_|"无法保存设置审计")?;
        tx.commit().map_err(|_| "无法保存验证设置")?;
        if let Some(key) = next_secret {
            state.secret = Some(key);
        }
        state.settings = settings.clone();
        state.generation = state.generation.wrapping_add(1);
        self.cancel.send_replace(state.generation);
        self.storage_error.store(false, Ordering::Relaxed);
        Ok(settings)
    }
    pub fn capture(
        &self,
        provider: &Provider,
        key: &str,
        protocol: &str,
        requested: &str,
    ) -> Option<Capture> {
        let state = self.state.lock().ok()?;
        Self::capture_locked(&state, provider, key, protocol, requested)
    }
    fn capture_locked(
        state: &State,
        provider: &Provider,
        key: &str,
        protocol: &str,
        requested: &str,
    ) -> Option<Capture> {
        if !state.settings.enabled || key.is_empty() || !provider.enabled {
            return None;
        }
        let requested = safe_id(requested)?;
        let provider_id = safe_id(&provider.id)?;
        if !matches!(protocol, "chat" | "responses" | "anthropic" | "gemini") {
            return None;
        }
        let secret = state.secret.as_ref()?;
        let fingerprint = format!("hmac-v1:{}", hmac_hex(secret.as_bytes(), key.as_bytes()));
        // Hash all route-affecting configuration; never persist URLs, header values or credentials.
        let material = Zeroizing::new(
            serde_json::to_vec(&(
                "context-v1",
                &provider_id,
                &fingerprint,
                protocol,
                &requested,
                &provider.endpoint,
                &provider.provider_type,
                &provider.model,
                &provider.advanced,
                &state.settings.aliases,
            ))
            .ok()?,
        );
        Some(Capture {
            provider_id,
            requested_model: requested,
            protocol: protocol.into(),
            credential_fingerprint: fingerprint,
            context_key: hmac_hex(secret.as_bytes(), &material),
            generation: state.generation,
            store_diagnostic_ids: state.settings.store_diagnostic_ids,
        })
    }
    pub fn valid(&self, capture: &Capture) -> bool {
        self.state
            .lock()
            .is_ok_and(|s| s.settings.enabled && s.generation == capture.generation)
    }
    pub fn submit(self: &Arc<Self>, database: Database, event: i64, observation: Observation) {
        if !observation
            .verification_capture
            .as_ref()
            .is_some_and(|c| self.valid(c))
        {
            return;
        }
        let sender = self.sender.get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<Job>(128);
            let runtime = Arc::downgrade(self);
            let _ = std::thread::Builder::new()
                .name("xy-model-verification".into())
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        let Some(runtime) = runtime.upgrade() else {
                            break;
                        };
                        match runtime.process(job.event, job.observation, "passive") {
                            Ok(Some(audit)) => {
                                if let Some(evidence) = audit.verification {
                                    if evidence.eligible_anomaly()
                                        && runtime.settings().is_ok_and(|s| s.active_probe_enabled)
                                    {
                                        let db = job.database;
                                        tauri::async_runtime::spawn(async move {
                                            let _ = crate::response_model_probe::automatic_probe(
                                                db, evidence,
                                            )
                                            .await;
                                        });
                                    }
                                }
                            }
                            Ok(None) => (),
                            Err(_) => runtime.storage_error.store(true, Ordering::Relaxed),
                        }
                    }
                });
            sender
        });
        if sender
            .try_send(Job {
                database,
                event,
                observation,
            })
            .is_err()
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
    pub fn process(
        &self,
        event: i64,
        mut observation: Observation,
        source: &str,
    ) -> Result<Option<AuditRecord>, String> {
        let Some(capture) = observation.verification_capture.take() else {
            return Ok(None);
        };
        let state = self.state.lock().map_err(|_| "验证设置不可用")?;
        if !state.settings.enabled || capture.generation != state.generation {
            return Ok(None);
        }
        if !state.settings.store_diagnostic_ids {
            observation.response_id = None;
            observation.provider_request_id = None;
        }
        let connection = self.connection.upgrade().ok_or("数据库不可用")?;
        let mut conn = connection.lock().map_err(|_| "数据库不可用")?;
        let (provider, at, status): (String, i64, Option<u16>) = conn
            .query_row(
                "SELECT provider_id,created_at,status_code FROM request_events WHERE id=?1",
                [event],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| "响应日志已不存在")?;
        if provider != capture.provider_id {
            return Err("响应关联不一致".into());
        }
        let duplicate: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM verification_samples WHERE event_id=?1)",
                [event],
                |r| r.get(0),
            )
            .map_err(|_| "无法读取观察去重记录")?;
        if duplicate {
            return Ok(None);
        }
        let history: Vec<_> = store::history(
            &conn,
            &capture.context_key,
            state.settings.rolling_window_size,
        )?
        .into_iter()
        .filter(|e| e.source == source)
        .collect();
        let mut audit = engine::evaluate(
            observation,
            &capture,
            event,
            at,
            status,
            source,
            &state.settings,
            &history,
        );
        if let Some(e) = audit.verification.as_mut() {
            if e.eligible_anomaly() && state.settings.active_probe_enabled {
                e.active_probe_state = if store::approved(&conn, &capture.context_key)? {
                    "eligible"
                } else {
                    "cost_boundary_unconfirmed"
                }
                .into();
            }
        }
        store::persist(&mut conn, event, &audit)?;
        Ok(Some(audit))
    }
    pub fn approve(&self, capture: &Capture, confirmed: bool) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "验证设置不可用")?;
        if !state.settings.enabled || state.generation != capture.generation {
            return Err("请先开启模型验证".into());
        }
        let connection = self.connection.upgrade().ok_or("数据库不可用")?;
        let mut conn = connection.lock().map_err(|_| "数据库不可用")?;
        let tx = conn.transaction().map_err(|_| "无法保存费用边界")?;
        if confirmed {tx.execute("INSERT INTO verification_approvals(context_key,confirmed_at) VALUES(?1,?2) ON CONFLICT(context_key) DO UPDATE SET confirmed_at=excluded.confirmed_at",params![capture.context_key,now_ms()])}
        else {tx.execute("DELETE FROM verification_approvals WHERE context_key=?1",[&capture.context_key])}.map_err(|_|"无法保存费用边界")?;
        let text =
            serde_json::json!({"contextKey":capture.context_key,"confirmed":confirmed}).to_string();
        tx.execute("INSERT INTO verification_config_changes(created_at,action,config_json) VALUES(?1,'cost-boundary',?2)",params![now_ms(),text]).map_err(|_|"无法保存费用审计")?;
        tx.commit().map_err(|_| "无法保存费用边界".into())
    }
    pub fn reserve(&self, capture: Capture, anomaly: i64, now: i64) -> Result<Reservation, String> {
        self.reserve_at(capture, anomaly, now, std::time::Instant::now())
    }
    pub(super) fn reserve_at(
        &self,
        capture: Capture,
        anomaly: i64,
        now: i64,
        monotonic: std::time::Instant,
    ) -> Result<Reservation, String> {
        let mut state = self.state.lock().map_err(|_| "verification_unavailable")?;
        if !state.settings.enabled
            || !state.settings.active_probe_enabled
            || state.generation != capture.generation
        {
            return Err("active_probing_disabled".into());
        }
        let connection = self
            .connection
            .upgrade()
            .ok_or("budget_storage_unavailable")?;
        let mut conn = connection
            .lock()
            .map_err(|_| "budget_storage_unavailable")?;
        let scope = (capture.provider_id.clone(), capture.requested_model.clone());
        let cooldown = std::time::Duration::from_secs(state.settings.cooldown_hours * 3600);
        if state.last_probe.get(&scope).is_some_and(|last| {
            monotonic
                .checked_duration_since(*last)
                .is_none_or(|elapsed| elapsed < cooldown)
        }) {
            return Err("cooldown".into());
        }
        let (id, reserved_tokens) =
            store::reserve(&mut conn, &capture, &state.settings, anomaly, now)?;
        state.last_probe.insert(scope, monotonic);
        Ok(Reservation {
            id,
            anomaly_id: anomaly,
            capture,
            settings: state.settings.clone(),
            reserved_tokens,
            cancellation: self.cancel.subscribe(),
        })
    }
    pub fn mark_running(&self, reservation: &Reservation) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| "verification_unavailable")?;
        if !state.settings.enabled
            || !state.settings.active_probe_enabled
            || state.generation != reservation.capture.generation
        {
            return Err("active_probing_disabled".into());
        }
        let connection = self
            .connection
            .upgrade()
            .ok_or("budget_storage_unavailable")?;
        let conn = connection
            .lock()
            .map_err(|_| "budget_storage_unavailable")?;
        if !store::approved(&conn, &reservation.capture.context_key)? {
            return Err("cost_boundary_unconfirmed".into());
        }
        let changed = conn
            .execute(
                "UPDATE verification_probes SET state='running' WHERE id=?1 AND state='reserved'",
                [&reservation.id],
            )
            .map_err(|_| "budget_storage_unavailable")?;
        if changed != 1 {
            return Err("reservation_already_started".into());
        }
        Ok(())
    }
    pub fn settle(
        &self,
        reservation: &Reservation,
        billed: Option<u64>,
        outcome: &str,
    ) -> Result<(), String> {
        let connection = self
            .connection
            .upgrade()
            .ok_or("budget_storage_unavailable")?;
        let conn = connection
            .lock()
            .map_err(|_| "budget_storage_unavailable")?;
        store::settle(&conn, &reservation.id, billed, outcome)
    }
    pub fn note_probe(&self, event: i64, generation: u64, state_text: &str, tokens: Option<u64>) {
        self.update_probe_note(event, generation, state_text, tokens, false);
    }
    pub fn note_skipped_probe(&self, event: i64, generation: u64, reason: &str) {
        self.update_probe_note(event, generation, reason, Some(0), true);
    }
    fn update_probe_note(
        &self,
        event: i64,
        generation: u64,
        state_text: &str,
        tokens: Option<u64>,
        unreserved_only: bool,
    ) {
        let result = (|| {
            let state = self.state.lock().map_err(|_| ())?;
            if !state.settings.enabled || state.generation != generation {
                return Ok(());
            }
            let connection = self.connection.upgrade().ok_or(())?;
            let mut conn = connection.lock().map_err(|_| ())?;
            if unreserved_only {
                let exists: bool = conn
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM verification_probes WHERE anomaly_id=?1)",
                        [event],
                        |r| r.get(0),
                    )
                    .map_err(|_| ())?;
                if exists {
                    return Ok(());
                }
            }
            let text: String = conn
                .query_row(
                    "SELECT audit_json FROM request_events WHERE id=?1",
                    [event],
                    |r| r.get(0),
                )
                .map_err(|_| ())?;
            let mut audit: AuditRecord = serde_json::from_str(&text).map_err(|_| ())?;
            if let Some(e) = audit.verification.as_mut() {
                e.active_probe_state = state_text.into();
                e.active_probe_tokens = tokens;
            }
            store::persist(&mut conn, event, &audit).map_err(|_| ())
        })();
        if result.is_err() {
            self.storage_error.store(true, Ordering::Relaxed);
        }
    }
    pub fn status(
        &self,
        provider: &str,
        model: &str,
        capture: Option<&Capture>,
    ) -> Result<VerificationStatus, String> {
        let state = self.state.lock().map_err(|_| "验证设置不可用")?;
        let connection = self.connection.upgrade().ok_or("数据库不可用")?;
        let conn = connection.lock().map_err(|_| "数据库不可用")?;
        let recent = match capture {
            Some(c) => store::history(&conn, &c.context_key, 10)?,
            None => vec![],
        };
        let observed_responses = match capture {
            Some(c) => conn
                .query_row(
                    "SELECT COUNT(*) FROM verification_samples WHERE context_key=?1",
                    [&c.context_key],
                    |r| r.get(0),
                )
                .map_err(|_| "无法读取观测统计")?,
            None => 0,
        };
        let (requests,known,unknown,budget):(u64,u64,u64,u64)=conn.query_row("SELECT COUNT(*),COALESCE(SUM(billed_tokens),0),COALESCE(SUM(CASE WHEN billed_tokens IS NULL THEN 1 ELSE 0 END),0),COALESCE(SUM(budget_tokens),0) FROM verification_probes WHERE provider_id=?1 AND model=?2 AND day=?3",params![provider,model,now_ms().div_euclid(store::DAY_MS)],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|_|"无法读取复核预算")?;
        Ok(VerificationStatus {
            settings: state.settings.clone(),
            context_error: None,
            recent,
            observed_responses,
            probe_requests_today: requests,
            probe_tokens_today: (unknown == 0).then_some(known),
            budget_used_today: budget,
            capability_confirmed: match capture {
                Some(c) => store::approved(&conn, &c.context_key)?,
                None => false,
            },
            dropped_observations: self.dropped.load(Ordering::Relaxed),
            storage_error: self.storage_error.load(Ordering::Relaxed),
        })
    }
}
