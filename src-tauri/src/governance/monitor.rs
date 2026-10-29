use super::*;
use crate::proxy_manager::ProxyManager;
use futures_util::StreamExt;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
use std::time::{Duration, Instant};

pub const KEY: &str = "synthetic_monitor_v1";
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MonitorPolicy {
    pub enabled: bool,
    pub provider_id: String,
    pub model: String,
    pub mode: String,
    pub interval_minutes: u32,
    pub max_runs_per_day: u32,
}
impl Default for MonitorPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            provider_id: String::new(),
            model: String::new(),
            mode: "chat".into(),
            interval_minutes: 30,
            max_runs_per_day: 10,
        }
    }
}
impl MonitorPolicy {
    fn validate(&self, needs_target: bool) -> Result<(), String> {
        if !(5..=1440).contains(&self.interval_minutes)
            || !(1..=100).contains(&self.max_runs_per_day)
            || !matches!(
                self.mode.as_str(),
                "chat" | "stream" | "tools" | "responses" | "anthropic" | "gemini"
            )
        {
            return Err("Invalid synthetic monitor limits or mode".into());
        }
        if needs_target
            && (self.provider_id.is_empty()
                || self.model.trim().is_empty()
                || self.model.len() > 256
                || self.model.chars().any(char::is_control))
        {
            return Err("Choose an enabled provider and an explicit model".into());
        }
        if self.mode == "gemini"
            && !self
                .model
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
        {
            return Err("Unsupported model path characters".into());
        }
        Ok(())
    }
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Evidence {
    pub http_status: Option<u16>,
    pub reported_model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub provider_id: String,
    pub model: String,
    pub mode: String,
    pub status: String,
    pub latency_ms: i64,
    pub detail: String,
    pub created_at: i64,
    pub evidence: Evidence,
}
fn request(mode: &str, model: &str) -> Result<(String, Value), String> {
    let chat = json!({"model":model,"messages":[{"role":"user","content":"Reply exactly OK."}],"max_tokens":16,"stream":mode=="stream"});
    Ok(match mode {
        "chat" | "stream" => ("/v1/chat/completions".into(), chat),
        "tools" => (
            "/v1/chat/completions".into(),
            json!({"model":model,"messages":[{"role":"user","content":"Call xy_health_echo with value OK."}],"max_tokens":48,"tools":[{"type":"function","function":{"name":"xy_health_echo","description":"Synthetic test only; not executed","parameters":{"type":"object","properties":{"value":{"type":"string"}},"required":["value"]}}}],"tool_choice":{"type":"function","function":{"name":"xy_health_echo"}}}),
        ),
        "responses" => (
            "/v1/responses".into(),
            json!({"model":model,"input":"Reply exactly OK.","max_output_tokens":16,"stream":false,"store":false}),
        ),
        "anthropic" => (
            "/v1/messages".into(),
            json!({"model":model,"messages":[{"role":"user","content":"Reply exactly OK."}],"max_tokens":16,"stream":false}),
        ),
        "gemini" => (
            format!("/v1beta/models/{model}:generateContent"),
            json!({"contents":[{"role":"user","parts":[{"text":"Reply exactly OK."}]}],"generationConfig":{"maxOutputTokens":16}}),
        ),
        _ => return Err("Unsupported acceptance mode".into()),
    })
}
fn collect_evidence(v: &Value, evidence: &mut Evidence) {
    if let Some(model) = v
        .get("model")
        .or_else(|| v.get("modelVersion"))
        .and_then(Value::as_str)
    {
        if model.len() <= 160
            && !model.starts_with("sk-")
            && !model.starts_with("xy_")
            && model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._/-:".contains(&b))
        {
            evidence.reported_model = Some(model.into());
        }
    }
    let usage = v
        .get("usage")
        .or_else(|| v.get("usageMetadata"))
        .unwrap_or(&Value::Null);
    let number = |keys: &[&str]| {
        keys.iter()
            .find_map(|k| usage.get(k).and_then(Value::as_i64).filter(|n| *n >= 0))
    };
    evidence.input_tokens =
        number(&["prompt_tokens", "input_tokens", "promptTokenCount"]).or(evidence.input_tokens);
    evidence.output_tokens =
        number(&["completion_tokens", "output_tokens", "candidatesTokenCount"])
            .or(evidence.output_tokens);
    evidence.total_tokens = number(&["total_tokens", "totalTokenCount"]).or(evidence.total_tokens);
}
fn verify(mode: &str, bytes: &[u8], evidence: &mut Evidence) -> Result<String, String> {
    let text = std::str::from_utf8(bytes).map_err(|_| "non_utf8_response")?;
    if mode == "stream" {
        let (mut content, mut done, mut finish) = (String::new(), false, false);
        for line in text.lines() {
            if let Some(data) = line.strip_prefix("data:") {
                let data = data.trim();
                if done {
                    return Err("sse_data_after_terminal_event".into());
                }
                if data == "[DONE]" {
                    done = true;
                    continue;
                }
                let v: Value = serde_json::from_str(data).map_err(|_| "malformed_sse")?;
                if v.get("error").is_some_and(|error| !error.is_null()) {
                    return Err("sse_error_event".into());
                }
                collect_evidence(&v, evidence);
                if let Some(s) = v["choices"][0]["delta"]["content"].as_str() {
                    content.push_str(s);
                }
                if let Some(reason) = v["choices"][0]["finish_reason"].as_str() {
                    if reason != "stop" {
                        return Err("stream_incomplete_finish_reason".into());
                    }
                    finish = true;
                }
            }
        }
        if !done || !finish || content.trim() != "OK" {
            return Err("stream_completion_or_exact_text_missing".into());
        }
        return Ok("sse_terminal_finish_and_exact_text_verified".into());
    }
    let v: Value = serde_json::from_slice(bytes).map_err(|_| "invalid_response_json")?;
    if v.get("error").is_some_and(|error| !error.is_null()) {
        return Err("response_error_field".into());
    }
    collect_evidence(&v, evidence);
    if mode == "tools" {
        let calls = v["choices"][0]["message"]["tool_calls"]
            .as_array()
            .ok_or("tool_call_missing")?;
        if calls.len() != 1
            || calls[0]["function"]["name"] != "xy_health_echo"
            || v["choices"][0]["finish_reason"] != "tool_calls"
        {
            return Err("unexpected_tool_or_incomplete_finish".into());
        }
        let args: Value = serde_json::from_str(
            calls[0]["function"]["arguments"]
                .as_str()
                .ok_or("tool_arguments_missing")?,
        )
        .map_err(|_| "invalid_tool_arguments")?;
        if args != json!({"value":"OK"}) {
            return Err("unexpected_tool_arguments".into());
        }
        return Ok("tool_name_arguments_and_finish_verified_not_executed".into());
    }
    let join = |value: &Value| {
        value
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default()
    };
    let (output, complete) = match mode {
        "chat" => (
            v["choices"][0]["message"]["content"]
                .as_str()
                .unwrap_or("")
                .to_owned(),
            v["choices"][0]["finish_reason"] == "stop",
        ),
        "anthropic" => (join(&v["content"]), v["stop_reason"] == "end_turn"),
        "gemini" => (
            join(&v["candidates"][0]["content"]["parts"]),
            v["candidates"][0]["finishReason"] == "STOP",
        ),
        "responses" => (
            v["output"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| join(&v["content"]))
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default(),
            v["status"] == "completed",
        ),
        _ => return Err("unsupported_response_mode".into()),
    };
    if !complete || output.trim() != "OK" {
        return Err("incomplete_response_or_exact_text_missing".into());
    }
    Ok("response_schema_finish_and_exact_text_verified".into())
}
pub async fn execute(
    db: &Database,
    proxy: &ProxyManager,
    policy: &MonitorPolicy,
    scheduled: bool,
) -> Result<Run, String> {
    let status = proxy.status().await?;
    if !status.running {
        return Err("Start the local proxy before end-to-end acceptance".into());
    }
    let token = super::access::managed_token(db)?;
    execute_at(db, &status.address, &token, policy, scheduled).await
}
// A real loopback socket is mandatory; tests use the same helper and full router.
pub(crate) async fn execute_at(
    db: &Database,
    address: &str,
    token: &str,
    policy: &MonitorPolicy,
    scheduled: bool,
) -> Result<Run, String> {
    policy.validate(true)?;
    let addr: std::net::SocketAddr = address
        .strip_prefix("http://")
        .ok_or("Invalid local proxy scheme")?
        .parse()
        .map_err(|_| "Invalid local proxy address")?;
    if !addr.ip().is_loopback() || addr.port() == 0 {
        return Err("Acceptance only calls the loopback proxy".into());
    }
    let provider = db
        .get_provider(&policy.provider_id)?
        .filter(|p| p.enabled)
        .ok_or("Enabled provider required")?;
    if !load::<super::budget::BudgetPolicy>(db, super::budget::KEY)?.enabled {
        return Err("Acceptance checks require the hard admission budget".into());
    }
    let mut run = Run {
        id: uuid::Uuid::new_v4().to_string(),
        provider_id: provider.id,
        model: policy.model.clone(),
        mode: policy.mode.clone(),
        status: "running".into(),
        latency_ms: 0,
        detail: if scheduled { "scheduled" } else { "manual" }.into(),
        created_at: now(),
        evidence: Evidence::default(),
    };
    {
        let c = db.conn();
        let mut c = c.lock().map_err(|_| "database lock")?;
        let tx = c.transaction().map_err(|e| e.to_string())?;
        // Re-read the schedule under the same admission transaction. Disabling or
        // editing it invalidates a queued check; an already sent check may finish.
        let raw: Option<String> = tx.query_row("SELECT value FROM app_settings WHERE key=?1", [KEY], |r| r.get(0)).optional().map_err(|e| e.to_string())?;
        let saved: MonitorPolicy = raw.map(|s| serde_json::from_str(&s).map_err(|_| "Invalid synthetic policy")).transpose()?.unwrap_or_default();
        saved.validate(false)?;
        if scheduled {
            if !saved.enabled || &saved != policy {
                return Err("Scheduled policy changed; request cancelled".into());
            }
            let latest: i64 = tx.query_row("SELECT COALESCE(MAX(created_at),0) FROM synthetic_runs WHERE provider_id=?1 AND model=?2 AND mode=?3", params![policy.provider_id,policy.model,policy.mode], |r| r.get(0)).map_err(|e| e.to_string())?;
            if now() - latest < i64::from(saved.interval_minutes) * 60 {
                return Err("Scheduled interval has not elapsed".into());
            }
        }
        let daily_cap = policy.max_runs_per_day.min(saved.max_runs_per_day);
        tx.execute("UPDATE synthetic_runs SET status='failed',detail='interrupted_or_expired' WHERE status='running' AND created_at<?1", [now()-90]).map_err(|e| e.to_string())?;
        let count: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM synthetic_runs WHERE created_at>=?1",
                [(now() / 86400) * 86400],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if count >= i64::from(daily_cap) {
            return Err(
                "Synthetic daily request limit reached (manual and scheduled share it)".into(),
            );
        }
        let busy: i64 = tx
            .query_row(
                "SELECT COUNT(*) FROM synthetic_runs WHERE status='running'",
                [],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        if busy > 0 {
            return Err("Another acceptance request is running".into());
        }
        tx.execute("INSERT INTO synthetic_runs(id,provider_id,model,mode,status,latency_ms,detail,created_at) VALUES(?1,?2,?3,?4,?5,0,?6,?7)", params![run.id,run.provider_id,run.model,run.mode,run.status,run.detail,run.created_at]).map_err(|e|e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
    }
    let started = Instant::now();
    let result = async {
        let (path, body) = request(&policy.mode, &policy.model)?;
        let client = crate::network::client_builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(60))
            .build()
            .map_err(|_| "local_client_unavailable")?;
        let response = client
            .post(format!("http://{addr}{path}"))
            .bearer_auth(token)
            .header("x-xy-diagnostic-provider", &run.provider_id)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    "local_request_timeout"
                } else {
                    "local_request_failed"
                }
            })?;
        run.evidence.http_status = Some(response.status().as_u16());
        if !response.status().is_success() {
            return Err(format!("local_http_{}", response.status().as_u16()));
        }
        if policy.mode == "stream"
            && !response
                .headers()
                .get("content-type")
                .and_then(|v| v.to_str().ok())
                .is_some_and(|s| s.starts_with("text/event-stream"))
        {
            return Err("stream_content_type_missing".into());
        }
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "response_read_failed")?;
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err("response_limit_exceeded".into());
            }
            bytes.extend_from_slice(&chunk);
        }
        verify(&policy.mode, &bytes, &mut run.evidence)
    }
    .await;
    run.latency_ms = started.elapsed().as_millis() as i64;
    match result {
        Ok(detail) => {
            run.status = "passed".into();
            run.detail = detail;
        }
        Err(detail) => {
            run.status = "failed".into();
            run.detail = detail;
        }
    }
    {
        let c = db.conn();
        let c = c.lock().map_err(|_| "database lock")?;
        let last: Option<String> = c.query_row("SELECT status FROM synthetic_runs WHERE provider_id=?1 AND model=?2 AND mode=?3 AND id<>?4 AND status IN ('passed','failed') ORDER BY created_at DESC,rowid DESC LIMIT 1",params![run.provider_id,run.model,run.mode,run.id],|r|r.get(0)).optional().map_err(|e|e.to_string())?;
        c.execute("UPDATE synthetic_runs SET status=?1,latency_ms=?2,detail=?3,evidence_json=?4 WHERE id=?5", params![run.status,run.latency_ms,run.detail,serde_json::to_string(&run.evidence).map_err(|e|e.to_string())?,run.id]).map_err(|e|e.to_string())?;
        if last.as_deref() != Some(&run.status) && (run.status == "failed" || last.is_some()) {
            event_on(&c, if run.status == "passed" { "monitor_recovered" } else { "monitor_failed" }, &run.id)?;
        }
        // Keep today's admission count intact; retain 30 days of bounded receipts.
        c.execute(
            "DELETE FROM synthetic_runs WHERE created_at<?1",
            [now() - 30 * 86400],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(run)
}
fn latest_run_at(db: &Database, policy: &MonitorPolicy) -> Result<i64, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    c.query_row("SELECT COALESCE(MAX(created_at),0) FROM synthetic_runs WHERE provider_id=?1 AND model=?2 AND mode=?3", params![policy.provider_id,policy.model,policy.mode],|r|r.get(0)).map_err(|e|e.to_string())
}
pub fn start(db: Database, proxy: ProxyManager) {
    if cfg!(mobile) { return; }
    tauri::async_runtime::spawn(async move {
        let mut last_attempt = 0;
        loop {
            tokio::time::sleep(Duration::from_secs(15)).await;
            let Ok(p) = load::<MonitorPolicy>(&db, KEY) else {
                continue;
            };
            let Ok(persisted) = latest_run_at(&db, &p) else {
                continue;
            };
            if !p.enabled
                || p.validate(true).is_err()
                || now() - persisted.max(last_attempt) < i64::from(p.interval_minutes) * 60
            {
                continue;
            }
            last_attempt = now();
            // Quiet while unchanged; execute records only failure/recovery transitions.
            let _ = execute(&db, &proxy, &p, true).await;
        }
    });
}
pub fn runs(db: &Database) -> Result<Vec<Run>, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    let mut s=c.prepare("SELECT id,provider_id,model,mode,status,latency_ms,detail,created_at,evidence_json FROM synthetic_runs ORDER BY created_at DESC,rowid DESC LIMIT 100").map_err(|e|e.to_string())?;
    s.query_map([], |r| {
        Ok(Run {
            id: r.get(0)?,
            provider_id: r.get(1)?,
            model: r.get(2)?,
            mode: r.get(3)?,
            status: r.get(4)?,
            latency_ms: r.get(5)?,
            detail: r.get(6)?,
            created_at: r.get(7)?,
            evidence: serde_json::from_str(&r.get::<_, String>(8)?).unwrap_or_default(),
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}
pub fn read_only_status(db: &Database) -> Result<Value, String> {
    let providers: Vec<Value> = db.list_provider_health()?.iter().map(|p|json!({"providerId":p.provider_id,"status":p.status,"latencyMs":p.latency_ms,"successRate":p.success_rate,"lastCheckedAt":p.last_checked_at})).collect();
    let receipts: Vec<Value> = runs(db)?.iter().take(20).map(|r|json!({"id":r.id,"providerId":r.provider_id,"mode":r.mode,"status":r.status,"latencyMs":r.latency_ms,"createdAt":r.created_at})).collect();
    Ok(
        json!({"time":now(),"budget":super::budget::status(db)?,"requests":db.request_summary()?,"providers":providers,"checks":receipts}),
    )
}
#[tauri::command]
pub fn get_governance_status(db: tauri::State<'_, Database>) -> Result<Value, String> {
    read_only_status(&db)
}
#[tauri::command]
pub fn get_synthetic_monitor(db: tauri::State<'_, Database>) -> Result<MonitorPolicy, String> {
    load(&db, KEY)
}
#[tauri::command]
pub fn save_synthetic_monitor(
    policy: MonitorPolicy,
    confirmation: String,
    db: tauri::State<'_, Database>,
) -> Result<(), String> {
    crate::platform::require_desktop("Governance changes")?;
    if policy.enabled && confirmation != "ALLOW PAID CHECKS" {
        return Err("Explicit paid-check confirmation required".into());
    }
    policy.validate(policy.enabled)?;
    if policy.enabled {
        if !load::<super::budget::BudgetPolicy>(&db, super::budget::KEY)?.enabled {
            return Err("Enable the hard admission budget before scheduled checks".into());
        }
        if db
            .get_provider(&policy.provider_id)?
            .is_none_or(|p| !p.enabled)
        {
            return Err("Enabled provider required".into());
        }
    }
    save(&db, KEY, &policy)?;
    event(
        &db,
        "monitor_policy_changed",
        if policy.enabled {
            "enabled_while_app_running"
        } else {
            "disabled"
        },
    )
}
#[tauri::command]
pub async fn run_synthetic_check(
    policy: MonitorPolicy,
    confirmation: String,
    db: tauri::State<'_, Database>,
    proxy: tauri::State<'_, ProxyManager>,
) -> Result<Run, String> {
    crate::platform::require_desktop("Governance changes")?;
    if confirmation != "ALLOW PAID CHECKS" {
        return Err("Explicit paid-check confirmation required".into());
    }
    execute(&db, &proxy, &policy, false).await
}
#[tauri::command]
pub fn list_synthetic_runs(db: tauri::State<'_, Database>) -> Result<Vec<Run>, String> {
    runs(&db)
}

#[cfg(test)]
#[path = "monitor_tests.rs"]
mod tests;
