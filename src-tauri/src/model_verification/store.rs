use super::{Capture, VerificationEvidence, VerificationSettings};
use crate::model_audit::AuditRecord;
use rusqlite::{Connection, OptionalExtension, params};

pub const SETTINGS_KEY: &str = "model_verification_v1";
pub const INPUT_RESERVATION: u64 = 32;
pub const DAY_MS: i64 = 86_400_000;
pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS verification_samples (
 event_id INTEGER PRIMARY KEY REFERENCES request_events(id) ON DELETE CASCADE,
 provider_id TEXT NOT NULL, model TEXT NOT NULL, context_key TEXT NOT NULL,
 created_at INTEGER NOT NULL, evidence_json TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS verification_context ON verification_samples(context_key,created_at DESC,event_id DESC);
CREATE TABLE IF NOT EXISTS verification_probes (
 id TEXT PRIMARY KEY, anomaly_id INTEGER NOT NULL UNIQUE, provider_id TEXT NOT NULL,
 model TEXT NOT NULL, context_key TEXT NOT NULL, day INTEGER NOT NULL,
 created_at INTEGER NOT NULL, state TEXT NOT NULL, reserved_tokens INTEGER NOT NULL,
 billed_tokens INTEGER, budget_tokens INTEGER NOT NULL);
CREATE INDEX IF NOT EXISTS verification_budget ON verification_probes(provider_id,model,day);
CREATE TABLE IF NOT EXISTS verification_approvals (
 context_key TEXT PRIMARY KEY, confirmed_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS verification_config_changes (
 id INTEGER PRIMARY KEY AUTOINCREMENT, created_at INTEGER NOT NULL,
 action TEXT NOT NULL, config_json TEXT NOT NULL);
";

pub fn load_settings(conn: &Connection) -> Result<VerificationSettings, String> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key=?1",
            [SETTINGS_KEY],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| "无法读取验证设置")?;
    let settings = text
        .map(|s| serde_json::from_str::<VerificationSettings>(&s))
        .transpose()
        .map_err(|_| "验证设置损坏")?
        .unwrap_or_default();
    settings.validate()?;
    Ok(settings)
}

pub fn history(
    conn: &Connection,
    context: &str,
    limit: usize,
) -> Result<Vec<VerificationEvidence>, String> {
    let mut statement = conn.prepare("SELECT evidence_json FROM verification_samples WHERE context_key=?1 ORDER BY created_at DESC,event_id DESC LIMIT ?2").map_err(|_| "无法读取响应证据")?;
    let rows = statement
        .query_map(params![context, limit as i64], |r| r.get::<_, String>(0))
        .map_err(|_| "无法读取响应证据")?;
    rows.map(|r| {
        r.map_err(|_| "无法读取响应证据".to_string())
            .and_then(|s| serde_json::from_str(&s).map_err(|_| "响应证据损坏".into()))
    })
    .collect()
}

pub fn persist(conn: &mut Connection, event_id: i64, audit: &AuditRecord) -> Result<(), String> {
    let evidence = audit.verification.as_ref().ok_or("缺少验证证据")?;
    let encoded = serde_json::to_string(audit).map_err(|_| "无法编码响应证据")?;
    let meta = serde_json::to_string(evidence).map_err(|_| "无法编码响应证据")?;
    let o = &audit.observation;
    let count = |n: Option<u64>| n.and_then(|n| i64::try_from(n).ok());
    let tx = conn.transaction().map_err(|_| "无法写入响应证据")?;
    let changed = tx.execute("UPDATE request_events SET requested_model=?2,upstream_model=?3,total_tokens=?4,reasoning_tokens=?5,visible_output_bytes=?6,tool_payload_bytes=?7,max_tokens=?8,text_delta_count=?9,audit_flags=?10,audit_json=?11 WHERE id=?1",
        params![event_id,o.requested_model,o.upstream_model,count(o.total_tokens),count(o.reasoning_tokens),count(o.visible_output_bytes),count(o.tool_payload_bytes),count(o.max_tokens),count(o.stream.as_ref().map(|s|s.text_delta_count)),audit.flags.join(","),encoded]).map_err(|_| "无法写入响应证据")?;
    if changed != 1 {
        return Err("响应日志已不存在".into());
    }
    tx.execute("INSERT INTO verification_samples(event_id,provider_id,model,context_key,created_at,evidence_json) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(event_id) DO UPDATE SET evidence_json=excluded.evidence_json",params![event_id,evidence.upstream_id,evidence.requested_model,evidence.context_key,evidence.observed_at,meta]).map_err(|_| "无法写入响应证据")?;
    // Bound the analysis index without deleting the user's request log or audit record.
    tx.execute("DELETE FROM verification_samples WHERE event_id IN (SELECT event_id FROM verification_samples ORDER BY created_at DESC,event_id DESC LIMIT -1 OFFSET 5000)",[]).map_err(|_| "无法维护证据索引")?;
    tx.commit().map_err(|_| "无法提交响应证据".into())
}

pub fn approved(conn: &Connection, context: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM verification_approvals WHERE context_key=?1)",
        [context],
        |r| r.get(0),
    )
    .map_err(|_| "无法读取费用边界确认".into())
}

/// The transaction is the single gate used by automatic and manual callers.
/// Quotas deliberately ignore credential/protocol revisions so rotating them cannot reset spending.
pub fn reserve(
    conn: &mut Connection,
    capture: &Capture,
    settings: &VerificationSettings,
    anomaly: i64,
    now: i64,
) -> Result<(String, u64), String> {
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|_| "budget_storage_unavailable")?;
    let text: String = tx
        .query_row(
            "SELECT evidence_json FROM verification_samples WHERE event_id=?1",
            [anomaly],
            |r| r.get(0),
        )
        .map_err(|_| "anomaly_missing")?;
    let evidence: VerificationEvidence =
        serde_json::from_str(&text).map_err(|_| "anomaly_invalid")?;
    if evidence.context_key != capture.context_key
        || !evidence.eligible_anomaly()
        || now < evidence.observed_at
        || now - evidence.observed_at > DAY_MS
    {
        return Err("anomaly_stale_or_ineligible".into());
    }
    if !approved(&tx, &capture.context_key)? {
        return Err("cost_boundary_unconfirmed".into());
    }
    let duplicate: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM verification_probes WHERE anomaly_id=?1)",
            [anomaly],
            |r| r.get(0),
        )
        .map_err(|_| "budget_storage_unavailable")?;
    if duplicate {
        return Err("anomaly_already_checked".into());
    }
    let (count,used,last,busy):(u64,u64,Option<i64>,u64) = tx.query_row(
        "SELECT COALESCE(SUM(CASE WHEN day=?3 THEN 1 ELSE 0 END),0),COALESCE(SUM(CASE WHEN day=?3 THEN budget_tokens ELSE 0 END),0),MAX(created_at),COALESCE(SUM(CASE WHEN state IN ('reserved','running') THEN 1 ELSE 0 END),0) FROM verification_probes WHERE provider_id=?1 AND model=?2",
        params![capture.provider_id,capture.requested_model,now.div_euclid(DAY_MS)],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).map_err(|_| "budget_storage_unavailable")?;
    if busy > 0 {
        return Err("probe_in_flight".into());
    }
    if count >= u64::from(settings.max_requests_per_day) {
        return Err("daily_request_limit".into());
    }
    if last.is_some_and(|last| {
        now < last || now.saturating_sub(last) < (settings.cooldown_hours * 3_600_000) as i64
    }) {
        return Err("cooldown".into());
    }
    let reserved = INPUT_RESERVATION + u64::from(settings.max_output_tokens);
    if used
        .checked_add(reserved)
        .is_none_or(|n| n > settings.soft_token_budget_per_day)
    {
        return Err("daily_token_budget".into());
    }
    let id = uuid::Uuid::new_v4().to_string();
    tx.execute("INSERT INTO verification_probes(id,anomaly_id,provider_id,model,context_key,day,created_at,state,reserved_tokens,budget_tokens) VALUES(?1,?2,?3,?4,?5,?6,?7,'reserved',?8,?8)",params![id,anomaly,capture.provider_id,capture.requested_model,capture.context_key,now.div_euclid(DAY_MS),now,reserved]).map_err(|_| "budget_storage_unavailable")?;
    tx.commit().map_err(|_| "budget_storage_unavailable")?;
    Ok((id, reserved))
}

pub fn settle(conn: &Connection, id: &str, billed: Option<u64>, state: &str) -> Result<(), String> {
    let billed = billed.and_then(|n| i64::try_from(n).ok());
    // Unknown / contradictory usage retains the full reservation; known overruns are charged in full.
    let changed = conn.execute("UPDATE verification_probes SET state=?2,billed_tokens=?3,budget_tokens=CASE WHEN ?3 IS NULL THEN reserved_tokens ELSE ?3 END WHERE id=?1 AND state IN ('reserved','running')",params![id,state,billed]).map_err(|_| "无法核销复核预算")?;
    if changed != 1 {
        return Err("复核预算记录不存在".into());
    }
    Ok(())
}
