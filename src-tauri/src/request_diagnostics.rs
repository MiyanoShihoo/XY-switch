use crate::database::{Database, RequestRecordResult};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestDiagnostics {
    pub failure_stage: Option<String>,
    pub failure_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub budget_reason: Option<String>,
    pub send_attempts: u32,
    pub upstream_response_received: bool,
    pub attempts: Vec<RequestAttempt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task: Option<crate::governance::tasks::TaskSummary>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_request_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attempt_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub privacy: Option<crate::privacy::PrivacyReport>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestAttempt {
    pub status_code: Option<u16>,
    pub error_kind: Option<String>,
}

impl RequestDiagnostics {
    pub fn fail(&mut self, stage: &str, code: &str) {
        self.failure_stage = Some(stage.to_owned());
        self.failure_reason = Some(failure_reason(code).to_owned());
    }
}

pub fn failure_reason(code: &str) -> &'static str {
    match code {
        "loop_detected" => "The provider endpoint points back to the local proxy.",
        "provider_not_selected" => "No provider is selected.",
        "provider_disabled" => "The selected provider is disabled.",
        "database_error" => "The application could not read its local configuration.",
        "credential_error" => "The application could not read the provider credential.",
        "api_key_missing" => "No available provider has an API key configured.",
        "invalid_json" => "The request is not a valid JSON object.",
        "unsupported_operation" => "The requested Gemini operation is unsupported.",
        "invalid_diagnostic_target" => "The diagnostic provider selection is invalid.",
        "budget_rejected" => "A local budget rule rejected the request.",
        "circuit_open" => "The available provider circuits are temporarily open.",
        "timeout" => "The upstream request timed out.",
        "connect" => "The application could not connect to the provider.",
        "upstream_status" => "The provider returned an unsuccessful HTTP status.",
        "invalid_upstream_response" => "The provider returned an invalid response.",
        "stream_interrupted" => "The response stream ended before completion.",
        "stream_cancelled" => "The response stream was closed before completion.",
        "response_error" => "The application could not read the provider response.",
        "response_conversion" => "The application could not convert the provider response.",
        "invalid_request" => "The request or provider configuration could not be converted.",
        "request_build" => "The provider request configuration is invalid.",
        "privacy_policy_unavailable" => "Privacy settings could not be read; sending was blocked.",
        "privacy_sensitive_structure" => "Sensitive data was found in an executable or structural field; sending was blocked.",
        "privacy_attachment_uninspected" => "An attachment could not be inspected; sending was blocked.",
        "privacy_scan_limit" => "The privacy inspection limit was exceeded; sending was blocked.",
        "privacy_scan_failed" => "Privacy inspection failed; sending was blocked.",
        "privacy_response_sensitive_structure" => "Sensitive response data was found in an executable or structural field; delivery was blocked.",
        "privacy_response_attachment_uninspected" => "A response attachment could not be inspected; delivery was blocked.",
        "privacy_response_scan_limit" => "The response privacy inspection limit was exceeded; delivery was blocked.",
        "privacy_response_scan_failed" => "Response privacy inspection failed; delivery was blocked.",
        _ => "The request could not be completed.",
    }
}

pub(crate) fn migrate(connection: &Connection) -> Result<(), String> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS unassigned_request_events (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            route TEXT NOT NULL,
            status_code INTEGER,
            latency_ms INTEGER NOT NULL,
            error_kind TEXT NOT NULL,
            diagnostics_json TEXT NOT NULL,
            created_at INTEGER NOT NULL
        );
        CREATE VIEW IF NOT EXISTS usage_request_events AS
            SELECT id,provider_id,route,status_code,success,latency_ms,retry_count,error_kind,
                model,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,cost,
                price_source,price_version,created_at,audit_json,total_tokens,reasoning_tokens,
                diagnostics_json,affects_health FROM request_events
            UNION ALL
            SELECT -id,'',route,status_code,0,latency_ms,0,error_kind,
                NULL,0,0,0,0,0.0,'local_rejection',NULL,created_at,NULL,0,NULL,
                diagnostics_json,0 FROM unassigned_request_events;"
    ).map_err(|error| error.to_string())
}

pub(crate) fn refresh_provider_health(
    transaction: &Transaction<'_>,
    provider_id: &str,
) -> Result<i64, String> {
    let latest = transaction.query_row(
        "SELECT success,latency_ms,error_kind,created_at FROM request_events
         WHERE provider_id=?1 AND affects_health=1 AND route NOT LIKE 'model_probe/%'
         ORDER BY id DESC LIMIT 1",
        [provider_id],
        |row| Ok((row.get::<_, bool>(0)?, row.get::<_, i64>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, i64>(3)?)),
    ).optional().map_err(|error| error.to_string())?;
    let Some((success, latency, error_kind, checked_at)) = latest else {
        transaction.execute(
            "UPDATE provider_health SET status='unknown',latency_ms=NULL,success_rate=0,
             consecutive_failures=0,last_error=NULL,last_checked_at=0 WHERE provider_id=?1",
            [provider_id],
        ).map_err(|error| error.to_string())?;
        return Ok(0);
    };
    let failures = transaction.query_row(
        "SELECT COUNT(*) FROM request_events WHERE provider_id=?1 AND affects_health=1
         AND route NOT LIKE 'model_probe/%' AND id>COALESCE((
            SELECT MAX(id) FROM request_events WHERE provider_id=?1 AND affects_health=1
            AND route NOT LIKE 'model_probe/%' AND success=1),0)",
        [provider_id], |row| row.get::<_, i64>(0),
    ).map_err(|error| error.to_string())?;
    let rate = transaction.query_row(
        "SELECT COALESCE(AVG(success)*100.0,0) FROM (
            SELECT success FROM request_events WHERE provider_id=?1 AND affects_health=1
            AND route NOT LIKE 'model_probe/%' ORDER BY id DESC LIMIT 100)",
        [provider_id], |row| row.get::<_, f64>(0),
    ).map_err(|error| error.to_string())?;
    transaction.execute(
        "INSERT INTO provider_health(provider_id,status,latency_ms,success_rate,consecutive_failures,last_error,last_checked_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(provider_id) DO UPDATE SET
         status=excluded.status,latency_ms=excluded.latency_ms,success_rate=excluded.success_rate,
         consecutive_failures=excluded.consecutive_failures,last_error=excluded.last_error,last_checked_at=excluded.last_checked_at",
        params![provider_id, if success { "healthy" } else { "unhealthy" }, latency, rate, failures, error_kind, checked_at],
    ).map_err(|error| error.to_string())?;
    Ok(failures)
}

impl Database {
    pub(crate) fn update_response_privacy(
        &self,
        event_id: Option<i64>,
        report: &crate::privacy::PrivacyReport,
        blocked_code: Option<&str>,
    ) -> Result<(), String> {
        let Some(event_id) = event_id.filter(|id| *id >= 0) else { return Ok(()); };
        if let Some(code) = blocked_code {
            self.mark_request_failed(event_id, "response_privacy", code, false)?;
        }
        let connection = self.conn();
        let mut connection = connection.lock().map_err(|_| "database lock poisoned".to_owned())?;
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let serialized: Option<String> = transaction.query_row(
            "SELECT diagnostics_json FROM request_events WHERE id=?1", [event_id], |row| row.get(0),
        ).optional().map_err(|error| error.to_string())?.flatten();
        let mut diagnostic: RequestDiagnostics = serialized.as_deref().map(serde_json::from_str)
            .transpose().map_err(|_| "Invalid request diagnostics".to_owned())?.unwrap_or_default();
        diagnostic.privacy.get_or_insert_with(Default::default).merge(report);
        if let Some(code) = blocked_code { diagnostic.fail("response_privacy", code); }
        let serialized = serde_json::to_string(&diagnostic).map_err(|error| error.to_string())?;
        transaction.execute(
            "UPDATE request_events SET diagnostics_json=?2,error_kind=CASE WHEN ?3 IS NULL THEN error_kind ELSE ?3 END WHERE id=?1",
            params![event_id, serialized, blocked_code],
        ).map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn record_unassigned_request(
        &self,
        route: &str,
        status_code: u16,
        latency_ms: i64,
        error_kind: &str,
        diagnostics: &RequestDiagnostics,
        created_at: i64,
    ) -> Result<RequestRecordResult, String> {
        let diagnostics_json = serde_json::to_string(diagnostics).map_err(|error| error.to_string())?;
        let connection = self.conn();
        let connection = connection.lock().map_err(|_| "database lock poisoned".to_owned())?;
        connection.execute(
            "INSERT INTO unassigned_request_events(route,status_code,latency_ms,error_kind,diagnostics_json,created_at)
             VALUES(?1,?2,?3,?4,?5,?6)",
            params![route, status_code, latency_ms, error_kind, diagnostics_json, created_at],
        ).map_err(|error| error.to_string())?;
        Ok(RequestRecordResult { event_id: -connection.last_insert_rowid(), consecutive_failures: 0 })
    }

    pub fn mark_request_failed(
        &self,
        event_id: i64,
        stage: &str,
        code: &str,
        affects_health: bool,
    ) -> Result<(), String> {
        if event_id < 0 { return Ok(()); }
        let connection = self.conn();
        let mut connection = connection.lock().map_err(|_| "database lock poisoned".to_owned())?;
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let saved = transaction.query_row(
            "SELECT provider_id,affects_health,diagnostics_json,success FROM request_events WHERE id=?1",
            [event_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, bool>(3)?)),
        ).optional().map_err(|error| error.to_string())?;
        let Some((provider_id, previously_affected_health, diagnostic_json, success)) = saved else { return Ok(()); };
        if !success { return Ok(()); }
        let diagnostic_json = diagnostic_json.and_then(|json| serde_json::from_str::<RequestDiagnostics>(&json).ok())
            .map(|mut diagnostic| { diagnostic.fail(stage, code); serde_json::to_string(&diagnostic) })
            .transpose().map_err(|error| error.to_string())?;
        transaction.execute(
            "UPDATE request_events SET success=0,error_kind=?2,diagnostics_json=?3,affects_health=?4 WHERE id=?1",
            params![event_id, code, diagnostic_json, previously_affected_health && affects_health],
        ).map_err(|error| error.to_string())?;
        if previously_affected_health {
            let failures = refresh_provider_health(&transaction, &provider_id)?;
            if failures >= 3 {
                transaction.execute(
                    "UPDATE provider_health SET circuit_open_until=MAX(COALESCE(circuit_open_until,0),?2) WHERE provider_id=?1",
                    params![provider_id, crate::model_verification::now_ms() + 60_000],
                ).map_err(|error| error.to_string())?;
            }
        }
        let cancelled_task = crate::governance::tasks::fail_event_on(&transaction, event_id, code)?;
        transaction.commit().map_err(|error| error.to_string())?;
        drop(connection);
        if let Some(task) = cancelled_task { crate::governance::tasks::notify_cancel(self, &task); }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
