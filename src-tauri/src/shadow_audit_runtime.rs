//! Opt-in shadow differential work. The forwarding path only hashes bounded
//! structures and tries a bounded queue; tokenization and SQLite run here.
use crate::model_audit::{LocalTokenizer, ShadowCompleted, ShadowEngine, ShadowSnapshot};
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{SystemTime, UNIX_EPOCH},
};

const QUEUE_CAPACITY: usize = 32;
const MAX_ADDED_BYTES: usize = 8 * 1024;
const MAX_HISTORY_MESSAGES: usize = 1024;
const MAX_REQUEST_BYTES: usize = 1024 * 1024;
const SETTINGS_KEY: &str = "shadow_audit_settings";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ShadowSettings {
    pub enabled: bool,
    pub provider_id: String,
    pub model: String,
    pub tokenizer_path: String,
    pub pretokenizer: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShadowStatus {
    #[serde(flatten)]
    pub settings: ShadowSettings,
    pub source_sha256: Option<String>,
    pub error: Option<String>,
    pub dropped_requests: u64,
}

struct Configuration {
    status: ShadowStatus,
    tokenizer: Option<Arc<LocalTokenizer>>,
}

/// No request hash or text is serialized. The request snapshot exists only until
/// its one completed usage observation enters the bounded worker queue.
#[derive(Clone)]
pub(crate) struct ShadowCapture {
    snapshot: ShadowSnapshot,
    provider_id: String,
    model: String,
    generation: u64,
    tokenizer: Arc<LocalTokenizer>,
    unreliable: bool,
}

impl ShadowCapture {
    pub(crate) fn mark_unreliable(&mut self) {
        self.unreliable = true;
    }
}

struct Work {
    capture: ShadowCapture,
    completion: ShadowCompleted,
    event_id: i64,
}

pub struct ShadowAuditRuntime {
    connection: Weak<Mutex<Connection>>,
    configs: Mutex<BTreeMap<(String, String), Configuration>>,
    sender: Mutex<Option<SyncSender<Work>>>,
    generation: Arc<AtomicU64>,
    dropped: Arc<AtomicU64>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(u64::MAX as u128) as u64
}

impl ShadowAuditRuntime {
    pub fn new(connection: Weak<Mutex<Connection>>) -> Self {
        let runtime = Self {
            connection,
            configs: Mutex::new(BTreeMap::new()),
            sender: Mutex::new(None),
            generation: Arc::new(AtomicU64::new(0)),
            dropped: Arc::new(AtomicU64::new(0)),
        };
        let saved = runtime.connection.upgrade().and_then(|connection| {
            let connection = connection.lock().ok()?;
            connection
                .query_row(
                    "SELECT value FROM app_settings WHERE key=?1",
                    [SETTINGS_KEY],
                    |row| row.get::<_, String>(0),
                )
                .ok()
        });
        if let Some(saved) = saved {
            if let Ok(settings) = serde_json::from_str::<Vec<ShadowSettings>>(&saved) {
                for setting in settings {
                    let _ = runtime.install(setting, false);
                }
            }
        }
        runtime
    }

    pub fn configure(&self, settings: ShadowSettings) -> Result<ShadowStatus, String> {
        self.install(settings, true)
    }

    fn install(&self, settings: ShadowSettings, persist: bool) -> Result<ShadowStatus, String> {
        if settings.provider_id.trim().is_empty() || settings.model.trim().is_empty() {
            return Err(
                "Choose an exact provider and effective model for shadow collection".into(),
            );
        }
        let loaded = if settings.enabled {
            Some(LocalTokenizer::load(
                &settings.tokenizer_path,
                settings.pretokenizer.as_deref(),
            ))
        } else {
            None
        };
        let (tokenizer, error) = match loaded {
            Some(Ok(tokenizer)) => (Some(Arc::new(tokenizer)), None),
            Some(Err(error)) => (None, Some(error)),
            None => (None, None),
        };
        let status = ShadowStatus {
            source_sha256: tokenizer
                .as_ref()
                .map(|tokenizer| tokenizer.sha256().to_owned()),
            settings: settings.clone(),
            error,
            dropped_requests: self.dropped.load(Ordering::Relaxed),
        };
        {
            let mut configs = self
                .configs
                .lock()
                .map_err(|_| "Shadow configuration lock unavailable")?;
            let mut saved = configs
                .values()
                .filter(|config| {
                    config.status.settings.provider_id != settings.provider_id
                        || config.status.settings.model != settings.model
                })
                .map(|config| config.status.settings.clone())
                .collect::<Vec<_>>();
            saved.push(settings.clone());
            if persist {
                let connection = self.connection.upgrade().ok_or("Database closed")?;
                let connection = connection.lock().map_err(|_| "Database lock unavailable")?;
                connection.execute("INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                    rusqlite::params![SETTINGS_KEY,serde_json::to_string(&saved).map_err(|error|error.to_string())?]).map_err(|error|error.to_string())?;
            }
            configs.insert(
                (settings.provider_id.clone(), settings.model.clone()),
                Configuration {
                    status: status.clone(),
                    tokenizer,
                },
            );
            self.generation.fetch_add(1, Ordering::SeqCst);
        }
        if self
            .configs
            .lock()
            .map_err(|_| "Shadow configuration lock unavailable")?
            .values()
            .any(|config| config.tokenizer.is_some())
        {
            self.start_worker()?;
        } else {
            self.sender
                .lock()
                .map_err(|_| "Shadow worker lock unavailable")?
                .take();
        }
        Ok(status)
    }

    pub fn statuses(&self) -> Vec<ShadowStatus> {
        self.configs
            .lock()
            .map(|configs| {
                configs
                    .values()
                    .map(|config| {
                        let mut status = config.status.clone();
                        status.dropped_requests = self.dropped.load(Ordering::Relaxed);
                        status
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn start_worker(&self) -> Result<(), String> {
        let mut sender = self
            .sender
            .lock()
            .map_err(|_| "Shadow worker lock unavailable")?;
        if sender.is_some() {
            return Ok(());
        }
        let (send, recv) = mpsc::sync_channel::<Work>(QUEUE_CAPACITY);
        let connection = self.connection.clone();
        let generation = self.generation.clone();
        let dropped = self.dropped.clone();
        std::thread::Builder::new().name("xy-shadow-audit".into()).spawn(move || {
            let mut current_generation=u64::MAX;
            let mut engine=ShadowEngine::new(uuid::Uuid::new_v4().to_string());
            while let Ok(work)=recv.recv() {
                let active_generation=generation.load(Ordering::SeqCst);
                if work.capture.generation != active_generation { continue; }
                if current_generation != active_generation {
                    current_generation=active_generation;
                    engine=ShadowEngine::new(uuid::Uuid::new_v4().to_string());
                }
                let outcome=engine.observe(work.completion,&work.capture.tokenizer,now_ms());
                if generation.load(Ordering::SeqCst)!=active_generation { continue; }
                let Some(connection)=connection.upgrade() else { break; };
                let Ok(connection)=connection.lock() else { break; };
                if generation.load(Ordering::SeqCst)!=active_generation { continue; }
                let result=serde_json::to_string(&outcome).map_err(|error|error.to_string()).and_then(|serialized| {
                    connection.execute("INSERT INTO shadow_audit_samples(event_id,provider_id,model,tokenizer_sha256,created_at,result_json) VALUES(?1,?2,?3,?4,?5,?6)",
                        rusqlite::params![work.event_id,work.capture.provider_id,work.capture.model,work.capture.tokenizer.sha256(),now_ms().min(i64::MAX as u64) as i64,serialized]).map_err(|error|error.to_string())?;
                    connection.execute("DELETE FROM shadow_audit_samples WHERE id <= (SELECT COALESCE(MAX(id),0)-50000 FROM shadow_audit_samples)",[]).map(|_|()).map_err(|error|error.to_string())
                });
                if result.is_err() { dropped.fetch_add(1,Ordering::Relaxed); }
            }
        }).map_err(|error|error.to_string())?;
        *sender = Some(send);
        Ok(())
    }

    /// Capture continuity from the untouched client body, before adapters and
    /// provider overrides can drop fields or rewrite message containers.
    pub(crate) fn begin_capture(
        &self,
        provider: &str,
        protocol: &str,
        body: &[u8],
    ) -> Option<ShadowSnapshot> {
        let configs = match self.configs.try_lock() {
            Ok(configs) => configs,
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        };
        if !configs
            .iter()
            .any(|((id, _), config)| id == provider && config.tokenizer.is_some())
        {
            return None;
        }
        drop(configs);
        if body.len() > MAX_REQUEST_BYTES {
            return Some(ShadowSnapshot {
                exclusion_reason: Some("request_budget_exceeded".into()),
                ..Default::default()
            });
        }
        let payload = serde_json::from_slice::<Value>(body).ok()?;
        Some(request_snapshot(provider, "", None, "", protocol, &payload))
    }

    pub(crate) fn capture(
        &self,
        client: ShadowSnapshot,
        provider: &str,
        model: &str,
        requested_model: Option<&str>,
        endpoint: &str,
        protocol: &str,
        payload: &Value,
    ) -> Option<ShadowCapture> {
        // Configuration changes must never block forwarding.
        let configs = match self.configs.try_lock() {
            Ok(configs) => configs,
            Err(_) => {
                self.dropped.fetch_add(1, Ordering::Relaxed);
                return None;
            }
        };
        let config = configs.get(&(provider.to_owned(), model.to_owned()))?;
        let tokenizer = config.tokenizer.clone()?;
        let generation = self.generation.load(Ordering::SeqCst);
        drop(configs);
        let forwarded = request_snapshot_with_encoding(
            provider,
            model,
            requested_model,
            endpoint,
            protocol,
            payload,
            true,
        );
        Some(ShadowCapture {
            snapshot: combine_snapshots(client, forwarded),
            provider_id: provider.into(),
            model: model.into(),
            generation,
            tokenizer,
            unreliable: false,
        })
    }

    pub(crate) fn submit(
        &self,
        mut capture: ShadowCapture,
        event_id: Option<i64>,
        input_tokens: Option<u64>,
        cache_read_tokens: Option<u64>,
        cache_creation_tokens: Option<u64>,
        complete: bool,
        failover: bool,
        assistant_sha256: Option<String>,
        upstream_model: Option<&str>,
    ) {
        let Some(event_id) = event_id else {
            return;
        };
        // A channel can change the self-reported model without changing the
        // client's requested name. Never pair those distinct observations.
        capture.snapshot.partition_key =
            hash(&json!([capture.snapshot.partition_key, upstream_model,]));
        let completion = ShadowCompleted {
            snapshot: capture.snapshot.clone(),
            input_tokens,
            cache_read_tokens,
            cache_creation_tokens,
            complete,
            failover: failover || capture.unreliable,
            assistant_sha256,
        };
        let work = Work {
            capture,
            completion,
            event_id,
        };
        if self
            .sender
            .try_lock()
            .ok()
            .and_then(|sender| sender.as_ref().map(|sender| sender.try_send(work).is_ok()))
            != Some(true)
        {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

fn hash(value: &(impl Serialize + ?Sized)) -> String {
    // serde_json's map order is deterministic in this project. Length and type
    // boundaries stay encoded, so concatenation cannot create false prefix links.
    struct HashWriter(Sha256);
    impl std::io::Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    if serde_json::to_writer(&mut writer, value).is_err() {
        return String::new();
    }
    format!("{:x}", writer.0.finalize())
}

fn plain_content<'a>(value: &'a Value, protocol: &str) -> Option<&'a str> {
    if let Some(text) = value.as_str() {
        return Some(text);
    }
    let parts = value.as_array()?;
    if parts.len() != 1 {
        return None;
    }
    let part = parts.first()?.as_object()?;
    if protocol == "gemini" {
        if part.len() != 1 {
            return None;
        }
    } else if part.keys().any(|key| {
        key != "type"
            && key != "text"
            && !(protocol == "responses"
                && matches!(key.as_str(), "annotations" | "logprobs")
                && part[key].as_array().is_some_and(Vec::is_empty))
    }) || !matches!(
        part.get("type").and_then(Value::as_str),
        Some("text" | "input_text" | "output_text")
    ) {
        return None;
    }
    part.get("text")?.as_str()
}

fn request_snapshot(
    provider: &str,
    model: &str,
    requested_model: Option<&str>,
    endpoint: &str,
    protocol: &str,
    payload: &Value,
) -> ShadowSnapshot {
    request_snapshot_with_encoding(
        provider,
        model,
        requested_model,
        endpoint,
        protocol,
        payload,
        false,
    )
}

fn request_snapshot_with_encoding(
    provider: &str,
    model: &str,
    requested_model: Option<&str>,
    endpoint: &str,
    protocol: &str,
    payload: &Value,
    canonical_history: bool,
) -> ShadowSnapshot {
    let history_field = match protocol {
        "responses" => "input",
        "gemini" => "contents",
        _ => "messages",
    };
    let history = &payload[history_field];
    let mut snapshot = ShadowSnapshot {
        eligible: false,
        exclusion_reason: Some("unsupported_history".into()),
        ..Default::default()
    };
    let Some(messages) = history.as_array() else {
        return snapshot;
    };
    if messages.len() > MAX_HISTORY_MESSAGES {
        snapshot.exclusion_reason = Some("history_budget_exceeded".into());
        return snapshot;
    }
    struct BudgetWriter(usize);
    impl std::io::Write for BudgetWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(bytes.len());
            if self.0 > MAX_REQUEST_BYTES {
                return Err(std::io::Error::other("request budget"));
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    if serde_json::to_writer(BudgetWriter(0), payload).is_err() {
        snapshot.exclusion_reason = Some("request_budget_exceeded".into());
        return snapshot;
    }
    let config: serde_json::Map<String, Value> = payload
        .as_object()
        .into_iter()
        .flat_map(|object| object.iter())
        .filter(|(key, _)| {
            key.as_str() != history_field
                && key.as_str() != "stream"
                && key.as_str() != "stream_options"
        })
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    snapshot.partition_key = hash(&json!([
        provider,
        model,
        requested_model,
        endpoint,
        protocol,
        config
    ]));
    let top_system = json!([
        payload.get("system"),
        payload.get("systemInstruction"),
        payload.get("instructions")
    ]);
    snapshot.history_key = hash(&json!([top_system, history]));
    if ["tools", "functions"].iter().any(|key| {
        payload
            .get(*key)
            .is_some_and(|value| !value.is_null() && !value.as_array().is_some_and(Vec::is_empty))
    }) || [
        "previous_response_id",
        "conversation",
        "cached_content",
        "cachedContent",
        "context_management",
    ]
    .iter()
    .any(|key| payload.get(*key).is_some_and(|value| !value.is_null()))
        || payload
            .get("truncation")
            .is_some_and(|value| !value.is_null() && value != "disabled")
        || payload["n"].as_u64().unwrap_or(1) != 1
        || payload
            .pointer("/generationConfig/candidateCount")
            .and_then(Value::as_u64)
            .unwrap_or(1)
            != 1
    {
        snapshot.exclusion_reason = Some("tools_server_state_or_multiple_choices".into());
        return snapshot;
    }
    if payload
        .get("system")
        .is_some_and(|value| !value.is_null() && plain_content(value, "anthropic").is_none())
        || payload
            .get("systemInstruction")
            .is_some_and(|value| plain_content(&value["parts"], "gemini").is_none())
    {
        snapshot.exclusion_reason = Some("unsupported_system_content".into());
        return snapshot;
    }
    let mut conversation = Vec::new();
    let mut system = Vec::new();
    let mut begun = false;
    for message in messages {
        let Some(object) = message.as_object() else {
            return snapshot;
        };
        let role = message["role"].as_str().unwrap_or("");
        if matches!(role, "system" | "developer") && !begun {
            if object.keys().any(|key| key != "role" && key != "content")
                || plain_content(&message["content"], protocol).is_none()
            {
                return snapshot;
            }
            system.push(message);
            continue;
        }
        begun = true;
        let role = if protocol == "gemini" && role == "model" {
            "assistant"
        } else {
            role
        };
        if !matches!(role, "user" | "assistant") {
            return snapshot;
        }
        let content_key = if protocol == "gemini" {
            "parts"
        } else {
            "content"
        };
        if object.keys().any(|key| {
            key != "role"
                && key != content_key
                && !(key == "type" && message["type"] == "message")
                && !(protocol == "responses" && key == "id" && message["id"].is_string())
                && !(protocol == "responses" && key == "status" && message["status"] == "completed")
        }) {
            return snapshot;
        }
        let Some(text) = plain_content(&message[content_key], protocol) else {
            return snapshot;
        };
        let expected = if conversation.len() % 2 == 0 {
            "user"
        } else {
            "assistant"
        };
        if role != expected {
            return snapshot;
        }
        conversation.push((role, text));
    }
    if conversation.is_empty() || conversation.last().map(|(role, _)| *role) != Some("user") {
        return snapshot;
    }
    snapshot.partition_key = hash(&json!([snapshot.partition_key, system]));
    snapshot.history_messages = conversation.len() as u64;
    if canonical_history {
        snapshot.history_key = hash(&conversation);
    }
    if conversation.len() >= 3 {
        if conversation[conversation.len() - 2]
            .1
            .len()
            .saturating_add(conversation[conversation.len() - 1].1.len())
            > MAX_ADDED_BYTES
        {
            snapshot.exclusion_reason = Some("text_budget_exceeded".into());
            return snapshot;
        }
        snapshot.parent_history_key = Some(if canonical_history {
            hash(&conversation[..conversation.len() - 2])
        } else {
            hash(&json!([top_system, &messages[..messages.len() - 2]]))
        });
        snapshot.added_assistant = Some(conversation[conversation.len() - 2].1.to_owned());
        snapshot.added_user = Some(conversation[conversation.len() - 1].1.to_owned());
    }
    snapshot.eligible = true;
    snapshot.exclusion_reason = None;
    snapshot
}

fn combine_snapshots(mut client: ShadowSnapshot, forwarded: ShadowSnapshot) -> ShadowSnapshot {
    // Client history determines continuity. The actual forwarded prefix must
    // also remain unchanged; normalized roles/text avoid adapter-only wrappers.
    client.partition_key = hash(&json!([client.partition_key, forwarded.partition_key]));
    client.history_key = hash(&json!([client.history_key, forwarded.history_key]));
    client.parent_history_key = client
        .parent_history_key
        .zip(forwarded.parent_history_key)
        .map(|(left, right)| hash(&json!([left, right])));
    if !client.eligible {
        return client;
    }
    if !forwarded.eligible {
        client.eligible = false;
        client.exclusion_reason = Some(format!(
            "shadow.forwarded_{}",
            forwarded
                .exclusion_reason
                .as_deref()
                .unwrap_or("unsupported_history")
        ));
    } else if client.history_messages != forwarded.history_messages
        || client.added_assistant != forwarded.added_assistant
        || client.added_user != forwarded.added_user
    {
        // The BPE input must be exactly the added text that was billed. Never
        // silently count client text after a lossy conversion or override.
        client.eligible = false;
        client.exclusion_reason = Some("shadow.forwarded_text_changed".into());
    }
    client
}

#[cfg(test)]
pub(crate) fn byte_tokenizer_fixture() -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!("xy-shadow-{}.tiktoken", uuid::Uuid::new_v4()));
    let alphabet = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let ranks = (0u16..=255)
        .map(|byte| {
            format!(
                "{}{}== {byte}\n",
                alphabet[(byte >> 2) as usize] as char,
                alphabet[((byte & 3) << 4) as usize] as char
            )
        })
        .collect::<String>();
    std::fs::write(&path, ranks).unwrap();
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(protocol: &str, payload: &Value) -> ShadowSnapshot {
        request_snapshot(
            "provider",
            "native",
            Some("alias"),
            "https://fixture/v1",
            protocol,
            payload,
        )
    }

    #[test]
    fn shadow_audit_snapshot_strict_prefix_supports_four_plain_protocols() {
        for (protocol, field, first, assistant, next) in [
            (
                "chat",
                "messages",
                json!({"role":"user","content":"first"}),
                json!({"role":"assistant","content":"actual reply"}),
                json!({"role":"user","content":"next"}),
            ),
            (
                "responses",
                "input",
                json!({"role":"user","content":[{"type":"input_text","text":"first"}]}),
                json!({"role":"assistant","content":[{"type":"output_text","text":"actual reply"}]}),
                json!({"role":"user","content":"next"}),
            ),
            (
                "anthropic",
                "messages",
                json!({"role":"user","content":"first"}),
                json!({"role":"assistant","content":[{"type":"text","text":"actual reply"}]}),
                json!({"role":"user","content":"next"}),
            ),
            (
                "gemini",
                "contents",
                json!({"role":"user","parts":[{"text":"first"}]}),
                json!({"role":"model","parts":[{"text":"actual reply"}]}),
                json!({"role":"user","parts":[{"text":"next"}]}),
            ),
        ] {
            let mut payload = json!({"model":"native"});
            payload[field] = json!([first]);
            let baseline = snapshot(protocol, &payload);
            assert!(baseline.eligible, "{protocol}");
            payload[field]
                .as_array_mut()
                .unwrap()
                .extend([assistant, next]);
            let subsequent = snapshot(protocol, &payload);
            assert!(subsequent.eligible, "{protocol}");
            assert_eq!(subsequent.partition_key, baseline.partition_key);
            assert_eq!(subsequent.parent_history_key, Some(baseline.history_key));
            assert_eq!(subsequent.added_assistant.as_deref(), Some("actual reply"));
            assert_eq!(subsequent.added_user.as_deref(), Some("next"));
            assert_eq!(subsequent.history_messages, 3);
        }
    }

    #[test]
    fn shadow_audit_snapshot_isolates_config_and_rejects_state_tools_and_rewrites() {
        let baseline = json!({"model":"native","messages":[{"role":"system","content":"fixed"},{"role":"user","content":"first"}],"temperature":0});
        let first = snapshot("chat", &baseline);
        assert!(first.eligible);
        for (provider, model, requested, endpoint) in [
            ("different", "native", Some("alias"), "https://fixture/v1"),
            ("provider", "different", Some("alias"), "https://fixture/v1"),
            ("provider", "native", None, "https://fixture/v1"),
            ("provider", "native", Some("alias"), "https://other/v1"),
        ] {
            assert_ne!(
                first.partition_key,
                request_snapshot(provider, model, requested, endpoint, "chat", &baseline)
                    .partition_key
            );
        }
        let mut changed = baseline.clone();
        changed["messages"][0]["content"] = json!("rewritten");
        assert_ne!(
            first.partition_key,
            snapshot("chat", &changed).partition_key
        );
        changed = baseline.clone();
        changed["temperature"] = json!(1);
        assert_ne!(
            first.partition_key,
            snapshot("chat", &changed).partition_key
        );
        for (key, value) in [
            ("tools", json!([{"type":"function"}])),
            ("previous_response_id", json!("server-state")),
            ("truncation", json!("auto")),
            ("n", json!(2)),
        ] {
            changed = baseline.clone();
            changed[key] = value;
            assert!(!snapshot("chat", &changed).eligible, "{key}");
        }
        changed = baseline.clone();
        changed["messages"]
            .as_array_mut()
            .unwrap()
            .push(json!({"role":"system","content":"late rewrite"}));
        assert!(!snapshot("chat", &changed).eligible);
        changed = baseline.clone();
        changed["messages"][1]["content"] =
            json!([{"type":"image_url","image_url":{"url":"fixture"}}]);
        assert!(!snapshot("chat", &changed).eligible);
        changed = baseline.clone();
        changed["messages"][1]["content"] = json!("x".repeat(MAX_REQUEST_BYTES));
        assert_eq!(
            snapshot("chat", &changed).exclusion_reason.as_deref(),
            Some("request_budget_exceeded")
        );
    }

    #[test]
    fn shadow_audit_queue_is_bounded_and_never_waits_for_worker_capacity() {
        let path =
            std::env::temp_dir().join(format!("xy-shadow-queue-{}.db", uuid::Uuid::new_v4()));
        let db = crate::database::Database::open(&path).unwrap();
        let payload = json!({"messages":[{"role":"user","content":"first"}]});
        assert!(
            db.shadow
                .begin_capture("p", "chat", payload.to_string().as_bytes())
                .is_none()
        );
        let tokenizer_path = byte_tokenizer_fixture();
        db.shadow
            .configure(ShadowSettings {
                enabled: true,
                provider_id: "p".into(),
                model: "native".into(),
                tokenizer_path: tokenizer_path.to_string_lossy().into_owned(),
                pretokenizer: Some("cl100k".into()),
            })
            .unwrap();
        let capture = db
            .shadow
            .capture(
                db.shadow
                    .begin_capture("p", "chat", payload.to_string().as_bytes())
                    .unwrap(),
                "p",
                "native",
                None,
                "fixture",
                "chat",
                &payload,
            )
            .unwrap();
        let (sender, receiver) = mpsc::sync_channel(QUEUE_CAPACITY);
        *db.shadow.sender.lock().unwrap() = Some(sender);
        for id in 0..=QUEUE_CAPACITY {
            db.shadow.submit(
                capture.clone(),
                Some(id as i64),
                Some(10),
                Some(0),
                Some(0),
                true,
                false,
                Some(format!("{:x}", Sha256::digest(b"answer"))),
                Some("native"),
            );
        }
        assert_eq!(db.shadow.dropped.load(Ordering::Relaxed), 1);
        assert_eq!(receiver.try_iter().count(), QUEUE_CAPACITY);
        drop(db);
        std::fs::remove_file(tokenizer_path).unwrap();
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn shadow_audit_client_and_forwarded_prefixes_survive_protocol_conversion() {
        for (protocol, field, first, assistant, user, system_key, system) in [
            (
                "responses",
                "input",
                json!({"role":"user","content":"first"}),
                json!({"type":"message","id":"msg_reply","role":"assistant","status":"completed","content":[{"type":"output_text","text":"actual reply","annotations":[],"logprobs":[]}]}),
                json!({"role":"user","content":"next"}),
                "instructions",
                json!("fixed"),
            ),
            (
                "anthropic",
                "messages",
                json!({"role":"user","content":"first"}),
                json!({"role":"assistant","content":[{"type":"text","text":"actual reply"}]}),
                json!({"role":"user","content":"next"}),
                "system",
                json!("fixed"),
            ),
            (
                "gemini",
                "contents",
                json!({"role":"user","parts":[{"text":"first"}]}),
                json!({"role":"model","parts":[{"text":"actual reply"}]}),
                json!({"role":"user","parts":[{"text":"next"}]}),
                "systemInstruction",
                json!({"parts":[{"text":"fixed"}]}),
            ),
        ] {
            let convert = |body: &Value| match protocol {
                "responses" => crate::adapters::responses_to_openai_chat(body).unwrap(),
                "anthropic" => {
                    crate::adapters::anthropic_to_openai_chat(body.clone(), "native").unwrap()
                }
                "gemini" => crate::adapters::gemini_to_openai_chat(body, "native").unwrap(),
                _ => unreachable!(),
            };
            let forwarded = |body: &Value| {
                request_snapshot_with_encoding(
                    "provider",
                    "native",
                    Some("alias"),
                    "https://fixture/v1",
                    "chat",
                    body,
                    true,
                )
            };
            let mut body = json!({"model":"alias"});
            body[field] = json!([first]);
            body[system_key] = system;
            let baseline = combine_snapshots(snapshot(protocol, &body), forwarded(&convert(&body)));
            assert!(
                baseline.eligible,
                "{protocol}: {:?}",
                baseline.exclusion_reason
            );
            body[field]
                .as_array_mut()
                .unwrap()
                .extend([assistant, user]);
            let upstream = convert(&body);
            let next = combine_snapshots(snapshot(protocol, &body), forwarded(&upstream));
            assert!(next.eligible, "{protocol}: {:?}", next.exclusion_reason);
            assert_eq!(
                next.parent_history_key.as_deref(),
                Some(baseline.history_key.as_str()),
                "{protocol}"
            );
            assert_eq!(next.partition_key, baseline.partition_key, "{protocol}");
            let mut changed_upstream = upstream.clone();
            changed_upstream["messages"]
                .as_array_mut()
                .unwrap()
                .last_mut()
                .unwrap()["content"] = json!("changed billing text");
            let changed =
                combine_snapshots(snapshot(protocol, &body), forwarded(&changed_upstream));
            assert!(!changed.eligible);
            assert_eq!(
                changed.exclusion_reason.as_deref(),
                Some("shadow.forwarded_text_changed")
            );
            let mut changed_client = body.clone();
            changed_client[system_key] = json!("rewritten");
            let raw_before = snapshot(protocol, &body);
            let raw_after = snapshot(protocol, &changed_client);
            assert_ne!(
                raw_before.history_key, raw_after.history_key,
                "top-level system must bind the client history"
            );
            assert_ne!(raw_before.partition_key, raw_after.partition_key);
        }
    }
}
