use super::{Database, budget, digest, event, now};
use crate::database::{Provider, RequestUsage};
use crate::request_diagnostics::RequestDiagnostics;
use axum::http::HeaderMap;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};
pub(crate) use crate::cancellation::{Cancellation, CancellationState};

#[derive(Default)]
pub(crate) struct TaskRuntime {
    signals: Mutex<HashMap<String, Weak<CancellationState>>>,
    _database_lock: Option<File>,
    #[cfg(test)]
    pub(crate) retry_pause: Mutex<Option<Arc<RetryPause>>>,
}

#[cfg(test)]
#[derive(Default)]
pub(crate) struct RetryPause {
    pub(crate) entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}

impl TaskRuntime {
    pub(crate) fn acquire(path: &Path, connection: &Connection) -> Result<Arc<Self>, String> {
        if path == Path::new(":memory:") || path.as_os_str().is_empty() {
            return Ok(Arc::new(Self::default()));
        }
        static RUNTIMES: OnceLock<Mutex<HashMap<PathBuf, Weak<TaskRuntime>>>> = OnceLock::new();
        let mut runtimes = RUNTIMES
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| "task_state_unavailable")?;
        let path = std::fs::canonicalize(path).map_err(|_| "task_store_unavailable")?;
        if let Some(runtime) = runtimes.get(&path).and_then(Weak::upgrade) {
            return Ok(runtime);
        }
        let mut lock_path = path.as_os_str().to_owned();
        lock_path.push(".tasks.lock");
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(PathBuf::from(lock_path))
            .map_err(|_| "task_store_unavailable")?;
        lock.try_lock().map_err(|_| "task_database_in_use")?;
        connection.execute("UPDATE proxy_attempts SET status='interrupted',error_kind='process_restart',
            usage_state=CASE WHEN usage_state='pending' THEN 'missing' ELSE usage_state END,
            cost_state=CASE WHEN actual_microusd IS NULL THEN 'unknown' ELSE cost_state END,finished_at=?1
            WHERE status IN ('sending','response_pending')", [crate::model_verification::now_ms()])
            .map_err(|_| "task_store_unavailable")?;
        let runtime = Arc::new(Self {
            signals: Mutex::default(),
            _database_lock: Some(lock),
            #[cfg(test)]
            retry_pause: Mutex::default(),
        });
        runtimes.retain(|_, runtime| runtime.strong_count() > 0);
        runtimes.insert(path, Arc::downgrade(&runtime));
        Ok(runtime)
    }

    #[cfg(test)]
    pub(crate) async fn pause_retry(&self) {
        let pause = self.retry_pause.lock().unwrap().clone();
        if let Some(pause) = pause {
            pause.entered.notify_one();
            pause.release.notified().await;
        }
    }

    fn signal(&self, id: &str) -> Result<Cancellation, String> {
        let mut signals = self.signals.lock().map_err(|_| "task_state_unavailable")?;
        if let Some(signal) = signals.get(id).and_then(Weak::upgrade) {
            return Ok(Cancellation(signal));
        }
        if signals.len() >= 512 {
            signals.retain(|_, signal| signal.strong_count() > 0);
        }
        let signal = Cancellation::default();
        signals.insert(id.to_owned(), Arc::downgrade(&signal.0));
        Ok(signal)
    }
}

#[derive(Clone)]
pub(crate) struct TaskScope(pub String);

#[derive(Clone, Debug)]
pub(crate) struct Identity {
    pub id: String,
    pub source: String,
    stable: bool,
}

fn identifier(value: &str) -> Result<&str, String> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err("task_identity_invalid".into())
    } else {
        Ok(value)
    }
}

fn header_id<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, String> {
    let values = headers.get_all(name);
    if values.iter().count() > 1 {
        return Err("task_identity_ambiguous".into());
    }
    values
        .iter()
        .next()
        .map(|value| {
            value
                .to_str()
                .map_err(|_| "task_identity_invalid".to_string())
                .and_then(identifier)
        })
        .transpose()
}

fn metadata_id(metadata: &Value) -> Result<Option<(String, String)>, String> {
    let field = |name: &str| -> Result<Option<&str>, String> {
        metadata
            .get(name)
            .filter(|value| !value.is_null())
            .map(|value| {
                value
                    .as_str()
                    .ok_or_else(|| "task_identity_invalid".to_string())
                    .and_then(identifier)
            })
            .transpose()
    };
    let session = field("session_id")?.or(field("thread_id")?);
    let turn = field("root_turn_id")?.or(field("turn_id")?);
    Ok(session
        .zip(turn)
        .map(|(session, turn)| (session.to_owned(), turn.to_owned())))
}

impl Identity {
    pub(crate) fn from_request(
        headers: &HeaderMap,
        body: &[u8],
        scope: &str,
        synthetic: bool,
    ) -> Result<Self, String> {
        let explicit = header_id(headers, "x-xy-task-id")?;
        let idempotency = header_id(headers, "idempotency-key")?;
        if explicit
            .zip(idempotency)
            .is_some_and(|(left, right)| left != right)
        {
            return Err("task_identity_ambiguous".into());
        }
        if let Some(id) = explicit.or(idempotency) {
            return Ok(Self {
                id: digest(&serde_json::to_vec(&(scope, "explicit", id)).unwrap_or_default()),
                source: if explicit.is_some() {
                    "task_header"
                } else {
                    "idempotency_key"
                }
                .into(),
                stable: true,
            });
        }
        let body_metadata = serde_json::from_slice::<Value>(body)
            .ok()
            .and_then(|value| value.get("client_metadata").cloned());
        let body_id = body_metadata
            .as_ref()
            .map(metadata_id)
            .transpose()?
            .flatten();
        let metadata_values = headers.get_all("x-codex-turn-metadata");
        if metadata_values.iter().count() > 1 {
            return Err("task_identity_ambiguous".into());
        }
        let header_metadata = metadata_values
            .iter()
            .next()
            .map(|value| {
                if value.as_bytes().len() > 16384 {
                    return Err("task_identity_invalid".to_string());
                }
                serde_json::from_slice::<Value>(value.as_bytes())
                    .map_err(|_| "task_identity_invalid".to_string())
            })
            .transpose()?;
        let header_id = header_metadata
            .as_ref()
            .map(metadata_id)
            .transpose()?
            .flatten();
        if body_id
            .as_ref()
            .zip(header_id.as_ref())
            .is_some_and(|(left, right)| left != right)
        {
            return Err("task_identity_ambiguous".into());
        }
        if let Some((session, turn)) = body_id.or(header_id) {
            return Ok(Self {
                id: digest(
                    &serde_json::to_vec(&(scope, "codex_turn", session, turn)).unwrap_or_default(),
                ),
                source: "codex_turn".into(),
                stable: true,
            });
        }
        let nonce = uuid::Uuid::new_v4().to_string();
        Ok(Self {
            id: digest(&serde_json::to_vec(&(scope, "request", nonce)).unwrap_or_default()),
            source: if synthetic {
                "synthetic"
            } else {
                "request_only"
            }
            .into(),
            stable: synthetic,
        })
    }
}

#[derive(Clone)]
pub(crate) struct Context {
    pub identity: Identity,
    pub request_id: String,
    pub signal: Cancellation,
    pub connection: Cancellation,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TaskSummary {
    pub id: String,
    pub identity_source: String,
    pub max_attempts: Option<i64>,
    pub max_microusd: Option<i64>,
    pub attempts: i64,
    pub reserved_microusd: i64,
    pub committed_microusd: i64,
    pub known_microusd: i64,
    pub unknown_cost_attempts: i64,
    pub unreserved_attempts: i64,
    pub active_attempts: i64,
    pub cancelled: bool,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attempt {
    pub id: String,
    pub request_id: String,
    pub ordinal: i64,
    pub provider_id: String,
    pub model: String,
    pub route: String,
    pub reserved_tokens: Option<i64>,
    pub reserved_microusd: Option<i64>,
    pub status: String,
    pub status_code: Option<u16>,
    pub error_kind: Option<String>,
    pub event_id: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub actual_microusd: Option<i64>,
    pub usage_state: String,
    pub cost_state: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskDetail {
    pub task: TaskSummary,
    pub attempts: Vec<Attempt>,
}

pub(crate) fn init(c: &Connection) -> Result<(), String> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS proxy_tasks(
        id TEXT PRIMARY KEY,identity_source TEXT NOT NULL,max_attempts INTEGER,max_microusd INTEGER,
        attempts INTEGER NOT NULL DEFAULT 0,reserved_microusd INTEGER NOT NULL DEFAULT 0,
        cancelled INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL);
        CREATE TABLE IF NOT EXISTS proxy_attempts(
        id TEXT PRIMARY KEY,task_id TEXT NOT NULL,request_id TEXT NOT NULL,ordinal INTEGER NOT NULL,
        provider_id TEXT NOT NULL,model TEXT NOT NULL,route TEXT NOT NULL,reserved_tokens INTEGER,
        reserved_microusd INTEGER,status TEXT NOT NULL,status_code INTEGER,error_kind TEXT,event_id INTEGER,
        input_tokens INTEGER,output_tokens INTEGER,actual_microusd INTEGER,usage_state TEXT NOT NULL DEFAULT 'pending',
        cost_state TEXT NOT NULL DEFAULT 'pending',started_at INTEGER NOT NULL,finished_at INTEGER,
        UNIQUE(task_id,ordinal));
        CREATE INDEX IF NOT EXISTS proxy_attempts_request ON proxy_attempts(request_id,provider_id);
        CREATE INDEX IF NOT EXISTS proxy_attempts_event ON proxy_attempts(event_id);
        CREATE INDEX IF NOT EXISTS proxy_tasks_updated ON proxy_tasks(updated_at);")
        .map_err(|error| error.to_string())
}

fn register_on(
    c: &Connection,
    identity: &Identity,
    policy: &budget::BudgetPolicy,
) -> Result<(), String> {
    if policy.enabled && !identity.stable {
        return Err("task_identity_required".into());
    }
    c.execute("INSERT OR IGNORE INTO proxy_tasks(id,identity_source,max_attempts,max_microusd,created_at,updated_at)
        VALUES(?1,?2,?3,?4,?5,?5)",
        params![identity.id, identity.source, policy.enabled.then_some(policy.task_max_attempts),
            policy.enabled.then_some(policy.task_max_microusd), now()]).map_err(|_| "task_store_unavailable")?;
    if policy.enabled {
        c.execute(
            "UPDATE proxy_tasks SET max_attempts=MIN(COALESCE(max_attempts,?2),?2),
            max_microusd=MIN(COALESCE(max_microusd,?3),?3) WHERE id=?1",
            params![
                identity.id,
                policy.task_max_attempts,
                policy.task_max_microusd
            ],
        )
        .map_err(|_| "task_store_unavailable")?;
    }
    Ok(())
}

impl Context {
    pub(crate) fn from_event(db: &Database, event_id: i64) -> Result<Self, String> {
        let identity = {
            let connection = db.conn();
            let connection = connection.lock().map_err(|_| "task_store_unavailable")?;
            connection
                .query_row(
                    "SELECT t.id,t.identity_source FROM proxy_tasks t
                JOIN proxy_attempts a ON a.task_id=t.id WHERE a.event_id=?1 LIMIT 1",
                    [event_id],
                    |row| {
                        Ok(Identity {
                            id: row.get(0)?,
                            source: row.get(1)?,
                            stable: true,
                        })
                    },
                )
                .optional()
                .map_err(|_| "task_store_unavailable")?
                .ok_or("task_identity_required")?
        };
        Self::new(db, identity, Cancellation::default())
    }

    pub(crate) fn new(
        db: &Database,
        identity: Identity,
        connection: Cancellation,
    ) -> Result<Self, String> {
        let signal = db.tasks.signal(&identity.id)?;
        {
            let _gate = signal.0.gate.lock().map_err(|_| "task_state_unavailable")?;
            let connection = db.conn();
            let mut connection = connection.lock().map_err(|_| "task_store_unavailable")?;
            let tx = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(|_| "task_store_unavailable")?;
            let policy = budget::policy_on(&tx)?;
            register_on(&tx, &identity, &policy)?;
            let cancelled: bool = tx
                .query_row(
                    "SELECT cancelled FROM proxy_tasks WHERE id=?1",
                    [&identity.id],
                    |row| row.get(0),
                )
                .map_err(|_| "task_store_unavailable")?;
            tx.commit().map_err(|_| "task_store_unavailable")?;
            if cancelled {
                signal.0.changed.send_replace(true);
            }
        }
        Ok(Self {
            identity,
            request_id: uuid::Uuid::new_v4().to_string(),
            signal,
            connection,
        })
    }
}

#[derive(Debug)]
pub(crate) struct AdmissionError {
    pub code: &'static str,
    pub reason: String,
}
impl From<String> for AdmissionError {
    fn from(reason: String) -> Self {
        Self {
            code: "budget_rejected",
            reason,
        }
    }
}

pub(crate) fn admit_and_start<Request, ResultValue>(
    db: &Database,
    context: &Context,
    provider: &Provider,
    payload: &mut Value,
    format: &str,
    source: &str,
    build: impl FnOnce(&Value) -> Result<Request, String>,
    start: impl FnOnce(Request, String, i64) -> ResultValue,
) -> Result<ResultValue, AdmissionError> {
    admit_and_start_on(
        db,
        context,
        provider,
        payload,
        format,
        source,
        build,
        |_| Ok(()),
        |request, id, ordinal, ()| start(request, id, ordinal),
    )
}

pub(crate) fn admit_and_start_on<Request, Extra, ResultValue>(
    db: &Database,
    context: &Context,
    provider: &Provider,
    payload: &mut Value,
    format: &str,
    source: &str,
    build: impl FnOnce(&Value) -> Result<Request, String>,
    reserve: impl FnOnce(&Connection) -> Result<Extra, String>,
    start: impl FnOnce(Request, String, i64, Extra) -> ResultValue,
) -> Result<ResultValue, AdmissionError> {
    admit_with_model_and_start_on(
        db, context, provider, payload, format, source, None, build, reserve, start,
    )
}

fn admit_with_model_and_start_on<Request, Extra, ResultValue>(
    db: &Database,
    context: &Context,
    provider: &Provider,
    payload: &mut Value,
    format: &str,
    source: &str,
    model: Option<&str>,
    build: impl FnOnce(&Value) -> Result<Request, String>,
    reserve: impl FnOnce(&Connection) -> Result<Extra, String>,
    start: impl FnOnce(Request, String, i64, Extra) -> ResultValue,
) -> Result<ResultValue, AdmissionError> {
    let _connection_gate = context
        .connection
        .0
        .gate
        .lock()
        .map_err(|_| "task_state_unavailable".to_string())?;
    let _task_gate = context
        .signal
        .0
        .gate
        .lock()
        .map_err(|_| "task_state_unavailable".to_string())?;
    if context.connection.is_cancelled() || context.signal.is_cancelled() {
        return Err(AdmissionError {
            code: "request_cancelled",
            reason: "task_cancelled".into(),
        });
    }
    let connection = db.conn();
    let mut connection = connection
        .lock()
        .map_err(|_| "task_store_unavailable".to_string())?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|_| "task_store_unavailable".to_string())?;
    let mut policy = budget::policy_on(&tx)?;
    register_on(&tx, &context.identity, &policy)?;
    let (attempts, reserved, max_attempts, max_cost, cancelled): (i64, i64, Option<i64>, Option<i64>, bool) = tx.query_row(
        "SELECT attempts,reserved_microusd,max_attempts,max_microusd,cancelled FROM proxy_tasks WHERE id=?1",
        [&context.identity.id], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    ).map_err(|_| "task_store_unavailable".to_string())?;
    let admission = (|| {
        if cancelled {
            return Err(AdmissionError {
                code: "request_cancelled",
                reason: "task_cancelled".into(),
            });
        }
        let enforce_daily = policy.enabled;
        policy.enabled |= max_attempts.is_some() || max_cost.is_some();
        let ordinal = attempts
            .checked_add(1)
            .ok_or_else(|| "task_attempt_overflow".to_string())?;
        if max_attempts.is_some_and(|limit| ordinal > limit) {
            return Err("task_attempt_budget_exhausted".to_string().into());
        }
        let (overage, unreserved): (i64, i64) = tx.query_row(
            "SELECT COALESCE(SUM(MAX(COALESCE(actual_microusd,0)-COALESCE(reserved_microusd,0),0)),0),
                COALESCE(SUM(reserved_microusd IS NULL),0) FROM proxy_attempts WHERE task_id=?1",
            [&context.identity.id], |row| Ok((row.get(0)?, row.get(1)?)),
        ).map_err(|_| "task_store_unavailable".to_string())?;
        if max_cost.is_some() && unreserved > 0 {
            return Err("task_unreserved_cost".to_string().into());
        }
        let reservation = budget::quote_on(&tx, &policy, payload, format, source)?;
        let new_reserved = reserved
            .checked_add(reservation.as_ref().map_or(0, |value| value.microusd))
            .ok_or_else(|| "task_cost_overflow".to_string())?;
        let committed = new_reserved
            .checked_add(overage)
            .ok_or_else(|| "task_cost_overflow".to_string())?;
        if max_cost.is_some_and(|limit| committed > limit) {
            return Err("task_cost_budget_exhausted".to_string().into());
        }
        if enforce_daily && let Some(reservation) = &reservation {
            budget::check_daily_on(&tx, &policy, reservation)?;
        }
        let request = build(payload).map_err(|reason| AdmissionError {
            code: "request_build",
            reason,
        })?;
        Ok((request, reservation, ordinal, new_reserved))
    })();
    let (request, reservation, ordinal, new_reserved) = match admission {
        Ok(admitted) => admitted,
        Err(error) => {
            tx.commit()
                .map_err(|_| "task_store_unavailable".to_string())?;
            return Err(error);
        }
    };
    let extra = reserve(&tx)?;
    let attempt_id = uuid::Uuid::new_v4().to_string();
    if let Some(reservation) = &reservation {
        budget::save_reservation_on(&tx, &attempt_id, provider, payload, source, reservation)?;
    }
    tx.execute("INSERT INTO proxy_attempts(id,task_id,request_id,ordinal,provider_id,model,route,reserved_tokens,reserved_microusd,status,started_at)
        VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'sending',?10)",
        params![attempt_id, context.identity.id, context.request_id, ordinal, provider.id,
            model.or_else(|| payload.get("model").and_then(Value::as_str)).unwrap_or(&provider.model), format,
            reservation.as_ref().map(|value| value.tokens), reservation.as_ref().map(|value| value.microusd),
            crate::model_verification::now_ms()]).map_err(|_| "task_store_unavailable".to_string())?;
    tx.execute(
        "UPDATE proxy_tasks SET attempts=?2,reserved_microusd=?3,updated_at=?4 WHERE id=?1",
        params![context.identity.id, ordinal, new_reserved, now()],
    )
    .map_err(|_| "task_store_unavailable".to_string())?;
    tx.commit()
        .map_err(|_| "task_store_unavailable".to_string())?;
    Ok(start(request, attempt_id, ordinal, extra))
}

pub(crate) struct PreparedAttempt {
    pub id: String,
    model: String,
    format: String,
    source: String,
    policy: budget::BudgetPolicy,
    output_limit: Option<i64>,
}

pub(crate) fn admit_prepared_attempt(
    db: &Database,
    context: &Context,
    provider: &Provider,
    payload: &mut Value,
    model: &str,
    format: &str,
    source: &str,
) -> Result<PreparedAttempt, AdmissionError> {
    admit_with_model_and_start_on(
        db,
        context,
        provider,
        payload,
        format,
        source,
        Some(model),
        |payload| {
            if model.trim().is_empty() || (format != "gemini"
                && payload.get("model").and_then(Value::as_str) != Some(model)) {
                return Err("budget_model_missing".to_string());
            }
            let output_limit = payload.get("max_completion_tokens")
                .or_else(|| payload.get("max_tokens"))
                .or_else(|| payload.get("max_output_tokens"))
                .or_else(|| payload.pointer("/generationConfig/maxOutputTokens"))
                .and_then(Value::as_i64);
            Ok((model.to_owned(), output_limit))
        },
        |connection| {
            let mut policy = budget::policy_on(connection)?;
            let enforced: bool = connection.query_row(
                "SELECT max_attempts IS NOT NULL OR max_microusd IS NOT NULL FROM proxy_tasks WHERE id=?1",
                [&context.identity.id], |row| row.get(0),
            ).map_err(|_| "task_store_unavailable".to_string())?;
            policy.enabled |= enforced;
            Ok(policy)
        },
        |(model, output_limit), id, _ordinal, policy| PreparedAttempt {
            id, model, format: format.to_owned(), source: source.to_owned(), policy, output_limit,
        },
    )
}

pub(crate) fn validate_prepared_attempt(
    db: &Database,
    context: &Context,
    provider: &Provider,
    attempt: &PreparedAttempt,
    payload: &mut Value,
    model: &str,
) -> Result<(), AdmissionError> {
    let _connection_gate = context.connection.0.gate.lock()
        .map_err(|_| "task_state_unavailable".to_string())?;
    let _task_gate = context.signal.0.gate.lock()
        .map_err(|_| "task_state_unavailable".to_string())?;
    let cancelled = || AdmissionError { code: "request_cancelled", reason: "task_cancelled".into() };
    if context.connection.is_cancelled() || context.signal.is_cancelled() {
        return Err(cancelled());
    }
    let connection = db.conn();
    let connection = connection.lock().map_err(|_| "task_store_unavailable".to_string())?;
    validate_prepared_attempt_on(&connection, context, provider, attempt, payload, model)
}

pub(crate) fn validate_prepared_attempt_on(
    connection: &Connection,
    context: &Context,
    provider: &Provider,
    attempt: &PreparedAttempt,
    payload: &mut Value,
    model: &str,
) -> Result<(), AdmissionError> {
    let cancelled = || AdmissionError { code: "request_cancelled", reason: "task_cancelled".into() };
    if context.connection.is_cancelled() || context.signal.is_cancelled() {
        return Err(cancelled());
    }
    let saved = connection.query_row(
        "SELECT a.task_id,a.request_id,a.provider_id,a.model,a.route,a.status,a.reserved_tokens,a.reserved_microusd,
            t.cancelled,t.max_attempts,t.max_microusd,t.attempts,t.reserved_microusd
         FROM proxy_attempts a JOIN proxy_tasks t ON t.id=a.task_id WHERE a.id=?1",
        [&attempt.id], |row| Ok((
            row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
            row.get::<_, String>(3)?, row.get::<_, String>(4)?, row.get::<_, String>(5)?,
            row.get::<_, Option<i64>>(6)?, row.get::<_, Option<i64>>(7)?, row.get::<_, bool>(8)?,
            row.get::<_, Option<i64>>(9)?, row.get::<_, Option<i64>>(10)?,
            row.get::<_, i64>(11)?, row.get::<_, i64>(12)?,
        )),
    ).optional().map_err(|_| "task_store_unavailable".to_string())?
        .ok_or_else(|| "task_attempt_not_admitted".to_string())?;
    if saved.8 { return Err(cancelled()); }
    if saved.0 != context.identity.id || saved.1 != context.request_id || saved.2 != provider.id
        || saved.3 != attempt.model || saved.4 != attempt.format || saved.5 != "sending"
        || model != attempt.model
        || (attempt.format != "gemini"
            && payload.get("model").and_then(Value::as_str) != Some(model))
    {
        return Err("task_attempt_not_admitted".to_string().into());
    }
    if saved.9.is_some_and(|limit| saved.11 > limit) {
        return Err("task_attempt_budget_exhausted".to_string().into());
    }
    let overage: i64 = connection.query_row(
        "SELECT COALESCE(SUM(MAX(COALESCE(actual_microusd,0)-COALESCE(reserved_microusd,0),0)),0)
         FROM proxy_attempts WHERE task_id=?1", [&context.identity.id], |row| row.get(0),
    ).map_err(|_| "task_store_unavailable".to_string())?;
    let committed = saved.12.checked_add(overage).ok_or_else(|| "task_cost_overflow".to_string())?;
    if saved.10.is_some_and(|limit| committed > limit) {
        return Err("task_cost_budget_exhausted".to_string().into());
    }
    let current = budget::policy_on(&connection)?;
    if current.enabled && saved.11 > current.task_max_attempts {
        return Err("task_attempt_budget_exhausted".to_string().into());
    }
    if current.enabled && committed > current.task_max_microusd {
        return Err("task_cost_budget_exhausted".to_string().into());
    }
    let enforced = current.enabled || attempt.policy.enabled || saved.9.is_some() || saved.10.is_some();
    if !enforced { return Ok(()); }
    let (reserved_tokens, reserved_cost) = saved.6.zip(saved.7)
        .ok_or_else(|| "task_unreserved_cost".to_string())?;
    let mut policy = attempt.policy.clone();
    policy.enabled = true;
    policy.max_output_tokens = attempt.output_limit
        .ok_or_else(|| "budget_invalid_output_limit".to_string())?;
    if current.enabled {
        policy.input_reservation = policy.input_reservation.min(current.input_reservation);
        policy.max_output_tokens = policy.max_output_tokens.min(current.max_output_tokens);
        budget::check_daily_on(&connection, &current, &budget::Reservation { tokens: 0, microusd: 0 })?;
    }
    let reservation = budget::quote_on(&connection, &policy, payload, &attempt.format, &attempt.source)?
        .ok_or_else(|| "task_unreserved_cost".to_string())?;
    if reservation.tokens > reserved_tokens || reservation.microusd > reserved_cost {
        return Err("budget_prepared_reservation_exceeded".to_string().into());
    }
    Ok(())
}

pub(crate) fn finish_attempt(
    db: &Database,
    id: &str,
    status: &str,
    status_code: Option<u16>,
    code: Option<&str>,
) -> Result<(), String> {
    let connection = db.conn();
    let connection = connection.lock().map_err(|_| "task_store_unavailable")?;
    let pending = status == "response_pending";
    connection.execute("UPDATE proxy_attempts SET status=?2,status_code=?3,error_kind=?4,
        usage_state=CASE WHEN ?5 THEN usage_state ELSE 'missing' END,
        cost_state=CASE WHEN ?5 THEN cost_state ELSE 'unknown' END,finished_at=CASE WHEN ?5 THEN NULL ELSE ?6 END
        WHERE id=?1 AND status IN ('sending','response_pending')",
        params![id, status, status_code, code, pending, crate::model_verification::now_ms()]).map_err(|error| error.to_string())?;
    Ok(())
}

pub(crate) fn attach_event_on(
    c: &Connection,
    event_id: i64,
    provider_id: &str,
    diagnostic: Option<&RequestDiagnostics>,
) -> Result<(), String> {
    if let Some(request_id) = diagnostic.and_then(|value| value.task_request_id.as_deref()) {
        c.execute("UPDATE proxy_attempts SET event_id=?1 WHERE request_id=?2 AND provider_id=?3 AND event_id IS NULL",
            params![event_id, request_id, provider_id]).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn event_attempt(c: &Connection, event_id: i64) -> Result<Option<String>, String> {
    c.query_row(
        "SELECT json_extract(diagnostics_json,'$.attemptId') FROM request_events WHERE id=?1",
        [event_id],
        |row| row.get(0),
    )
    .optional()
    .map(|value: Option<Option<String>>| value.flatten())
    .map_err(|error| error.to_string())
}

pub(crate) fn settle_event_on(
    c: &Connection,
    event_id: i64,
    usage: &RequestUsage,
    cost: Option<f64>,
) -> Result<(), String> {
    let Some(id) = event_attempt(c, event_id)? else {
        return Ok(());
    };
    let usage_state = match (usage.input_tokens, usage.output_tokens) {
        (Some(_), Some(_)) => "complete",
        (None, None) => "missing",
        _ => "partial",
    };
    let actual = cost.and_then(microusd);
    c.execute("UPDATE proxy_attempts SET input_tokens=?2,output_tokens=?3,actual_microusd=?4,
        usage_state=?5,cost_state=?6,status=CASE WHEN status IN ('sending','response_pending') THEN 'completed' ELSE status END,
        finished_at=COALESCE(finished_at,?7) WHERE id=?1",
        params![id, usage.input_tokens, usage.output_tokens, actual, usage_state,
            if actual.is_some() { "known" } else { "unknown" }, crate::model_verification::now_ms()]).map_err(|error| error.to_string())?;
    Ok(())
}

fn microusd(cost: f64) -> Option<i64> {
    let scaled = cost * 1_000_000.0;
    if !scaled.is_finite() || scaled < 0.0 || scaled >= i64::MAX as f64 {
        return None;
    }
    let nearest = scaled.round();
    let amount = if (scaled - nearest).abs() <= f64::EPSILON * scaled.abs().max(1.0) * 4.0 {
        nearest
    } else {
        scaled.ceil()
    };
    Some(amount as i64)
}

pub(crate) fn fail_event_on(
    c: &Connection,
    event_id: i64,
    code: &str,
) -> Result<Option<String>, String> {
    let Some(id) = event_attempt(c, event_id)? else {
        return Ok(None);
    };
    let cancelled = matches!(code, "stream_cancelled" | "request_cancelled");
    c.execute("UPDATE proxy_attempts SET status=CASE WHEN status='cancelled' THEN status ELSE ?2 END,
        error_kind=CASE WHEN status='cancelled' THEN error_kind ELSE ?3 END,cost_state=CASE WHEN actual_microusd IS NULL THEN 'unknown' ELSE cost_state END,
        usage_state=CASE WHEN usage_state='pending' THEN 'missing' ELSE usage_state END,finished_at=?4 WHERE id=?1",
        params![id, if cancelled { "cancelled" } else { "interrupted" }, code, crate::model_verification::now_ms()]).map_err(|error| error.to_string())?;
    let task: Option<String> = c
        .query_row(
            "SELECT task_id FROM proxy_attempts WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(|error| error.to_string())?;
    if cancelled && let Some(task) = &task {
        c.execute(
            "UPDATE proxy_tasks SET cancelled=1,updated_at=?2 WHERE id=?1",
            params![task, now()],
        )
        .map_err(|error| error.to_string())?;
    }
    Ok(if cancelled { task } else { None })
}

pub(crate) fn cancel(db: &Database, id: &str) -> Result<(), String> {
    let signal = db.tasks.signal(id)?;
    let _gate = signal.0.gate.lock().map_err(|_| "task_state_unavailable")?;
    let connection = db.conn();
    let mut connection = connection.lock().map_err(|_| "task_store_unavailable")?;
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| error.to_string())?;
    let changed = tx
        .execute(
            "UPDATE proxy_tasks SET cancelled=1,updated_at=?2 WHERE id=?1",
            params![id, now()],
        )
        .map_err(|error| error.to_string())?;
    if changed == 0 {
        return Err("task_not_found".into());
    }
    tx.execute("UPDATE proxy_attempts SET status='cancelled',error_kind='request_cancelled',
        usage_state=CASE WHEN usage_state='pending' THEN 'missing' ELSE usage_state END,
        cost_state=CASE WHEN actual_microusd IS NULL THEN 'unknown' ELSE cost_state END,finished_at=?2
        WHERE task_id=?1 AND status IN ('sending','response_pending')", params![id, crate::model_verification::now_ms()])
        .map_err(|error| error.to_string())?;
    tx.commit().map_err(|error| error.to_string())?;
    signal.0.changed.send_replace(true);
    Ok(())
}

pub(crate) fn notify_cancel(db: &Database, id: &str) {
    if let Ok(signal) = db.tasks.signal(id) {
        signal.cancel();
    }
}

pub(crate) struct RunGuard {
    pub context: Context,
    database: Database,
    finished: bool,
}
impl RunGuard {
    pub(crate) fn new(database: Database, context: Context) -> Self {
        Self {
            context,
            database,
            finished: false,
        }
    }
    pub(crate) fn finish(&mut self) {
        self.finished = true;
    }
}
impl Drop for RunGuard {
    fn drop(&mut self) {
        if !self.finished
            && let Err(error) = cancel(&self.database, &self.context.identity.id)
        {
            tracing::warn!(%error, "failed to persist task cancellation");
        }
    }
}

const SUMMARY_SQL: &str = "SELECT t.id,t.identity_source,t.max_attempts,t.max_microusd,t.attempts,t.reserved_microusd,
    COALESCE((SELECT SUM(actual_microusd) FROM proxy_attempts WHERE task_id=t.id),0),
    (SELECT COUNT(*) FROM proxy_attempts WHERE task_id=t.id AND cost_state!='known'),
    (SELECT COUNT(*) FROM proxy_attempts WHERE task_id=t.id AND reserved_microusd IS NULL),
    (SELECT COUNT(*) FROM proxy_attempts WHERE task_id=t.id AND status IN ('sending','response_pending')),
    t.cancelled,t.created_at,t.updated_at,
    t.reserved_microusd+COALESCE((SELECT SUM(MAX(COALESCE(actual_microusd,0)-COALESCE(reserved_microusd,0),0))
        FROM proxy_attempts WHERE task_id=t.id),0) FROM proxy_tasks t";
fn summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskSummary> {
    Ok(TaskSummary {
        id: row.get(0)?,
        identity_source: row.get(1)?,
        max_attempts: row.get(2)?,
        max_microusd: row.get(3)?,
        attempts: row.get(4)?,
        reserved_microusd: row.get(5)?,
        known_microusd: row.get(6)?,
        unknown_cost_attempts: row.get(7)?,
        unreserved_attempts: row.get(8)?,
        active_attempts: row.get(9)?,
        cancelled: row.get(10)?,
        created_at: row.get(11)?,
        updated_at: row.get(12)?,
        committed_microusd: row.get(13)?,
    })
}
pub(crate) fn summary(db: &Database, id: &str) -> Result<TaskSummary, String> {
    let connection = db.conn();
    let connection = connection.lock().map_err(|_| "task_store_unavailable")?;
    connection
        .query_row(&format!("{SUMMARY_SQL} WHERE t.id=?1"), [id], summary_row)
        .map_err(|error| error.to_string())
}
#[tauri::command]
pub fn list_proxy_tasks(db: tauri::State<'_, Database>) -> Result<Vec<TaskSummary>, String> {
    list(&db)
}
pub(crate) fn list(db: &Database) -> Result<Vec<TaskSummary>, String> {
    let connection = db.conn();
    let connection = connection.lock().map_err(|_| "task_store_unavailable")?;
    let mut statement = connection
        .prepare(&format!(
            "{SUMMARY_SQL} ORDER BY t.updated_at DESC,t.rowid DESC LIMIT 50"
        ))
        .map_err(|error| error.to_string())?;
    statement
        .query_map([], summary_row)
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())
}
#[tauri::command]
pub fn get_proxy_task(id: String, db: tauri::State<'_, Database>) -> Result<TaskDetail, String> {
    detail(&db, &id)
}
pub(crate) fn detail(db: &Database, id: &str) -> Result<TaskDetail, String> {
    let task = summary(db, id)?;
    let connection = db.conn();
    let connection = connection.lock().map_err(|_| "task_store_unavailable")?;
    let mut statement = connection.prepare("SELECT id,request_id,ordinal,provider_id,model,route,reserved_tokens,reserved_microusd,
        status,status_code,error_kind,event_id,input_tokens,output_tokens,actual_microusd,usage_state,cost_state,started_at,finished_at
        FROM proxy_attempts WHERE task_id=?1 ORDER BY ordinal").map_err(|error| error.to_string())?;
    let attempts = statement
        .query_map([id], |row| {
            Ok(Attempt {
                id: row.get(0)?,
                request_id: row.get(1)?,
                ordinal: row.get(2)?,
                provider_id: row.get(3)?,
                model: row.get(4)?,
                route: row.get(5)?,
                reserved_tokens: row.get(6)?,
                reserved_microusd: row.get(7)?,
                status: row.get(8)?,
                status_code: row.get(9)?,
                error_kind: row.get(10)?,
                event_id: row.get(11)?,
                input_tokens: row.get(12)?,
                output_tokens: row.get(13)?,
                actual_microusd: row.get(14)?,
                usage_state: row.get(15)?,
                cost_state: row.get(16)?,
                started_at: row.get(17)?,
                finished_at: row.get(18)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    Ok(TaskDetail { task, attempts })
}
#[tauri::command]
pub fn cancel_proxy_task(
    id: String,
    confirmed: bool,
    db: tauri::State<'_, Database>,
) -> Result<(), String> {
    crate::platform::require_desktop("Task cancellation")?;
    if !confirmed {
        return Err("Explicit confirmation required".into());
    }
    cancel(&db, &id)?;
    event(&db, "task_cancelled", &id)
}

#[cfg(test)]
#[path = "task_tests.rs"]
mod tests;
