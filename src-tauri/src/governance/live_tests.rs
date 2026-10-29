//! Opt-in live acceptance. The one-shot Windows pipe is the only credential
//! input; no environment variable, command argument, file or OS vault holds it.
use super::*;
use crate::governance::{self, access, budget, monitor, test_support};
use futures_util::StreamExt;
use serde::{Deserialize, Deserializer};
use std::io::Read;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LiveInput {
    endpoint: String,
    phase: String,
    #[serde(deserialize_with = "secret")]
    api_key: Zeroizing<String>,
    model: Option<String>,
    upstream_format: Option<String>,
    confirmation: Option<String>,
}
fn secret<'de, D: Deserializer<'de>>(d: D) -> Result<Zeroizing<String>, D::Error> {
    String::deserialize(d).map(Zeroizing::new)
}
fn input() -> Result<LiveInput, String> {
    let name = std::env::var("XY_GOVERNANCE_LIVE_PIPE")
        .map_err(|_| "A one-shot credential pipe is required")?;
    let id = name.strip_prefix(r"\\.\pipe\xy-switch-acceptance-")
        .ok_or("Invalid acceptance pipe name")?;
    uuid::Uuid::parse_str(id).map_err(|_| "Invalid acceptance pipe id")?;
    let pipe = std::fs::OpenOptions::new().read(true).open(name)
        .map_err(|_| "Cannot open acceptance pipe")?;
    let mut bytes = Zeroizing::new(Vec::new());
    pipe.take(16_385).read_to_end(&mut bytes)
        .map_err(|_| "Cannot read acceptance pipe")?;
    if bytes.len() > 16_384 { return Err("Acceptance input is too large".into()); }
    let config: LiveInput = serde_json::from_slice(&bytes)
        .map_err(|_| "Invalid acceptance input")?;
    let url = reqwest::Url::parse(&config.endpoint).map_err(|_| "Invalid HTTPS endpoint")?;
    if url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.query().is_some() || url.fragment().is_some()
        || url.path() != "/" || config.api_key.is_empty() || config.api_key.len() > 4096
    { return Err("Acceptance requires an HTTPS origin and an in-memory credential".into()); }
    Ok(config)
}
fn live_client() -> Result<reqwest::Client, String> {
    // Unlike isolated unit fixtures, live requests honor configured system
    // proxies. TLS verification remains enabled. No redirects or retries.
    reqwest::Client::builder()
        .retry(reqwest::retry::never())
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(55))
        .build().map_err(|_| "Cannot build live HTTPS client".into())
}
fn safe_model(model: &str) -> bool {
    !model.is_empty() && model.len() <= 160 && !model.starts_with("sk-")
        && !model.starts_with("xy_")
        && model.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-/:".contains(&b))
}
fn report(value: Value) {
    println!("XY_LIVE_REPORT {value}");
}
// Only proxy-owned classifications are emitted, never upstream error text.
fn error_category(value: Option<&str>) -> Option<&'static str> {
    value.map(|v| match v {
        "upstream_status" => "upstream_status",
        "timeout" => "timeout",
        "connect" => "connect",
        "request" => "request",
        _ => "other",
    })
}
fn latest_event_id(db: &Database) -> Result<i64, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "Database lock")?;
    c.query_row("SELECT COALESCE(MAX(id),0) FROM request_events", [], |r| r.get(0))
        .map_err(|_| "Cannot read isolated event cursor".into())
}
fn upstream_events_since(db: &Database, since: i64) -> Result<Vec<Value>, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "Database lock")?;
    let mut stmt = c.prepare("SELECT status_code,error_kind,retry_count FROM request_events WHERE id>?1 ORDER BY id LIMIT 6")
        .map_err(|_| "Cannot prepare isolated event query")?;
    stmt.query_map([since], |r| {
        let status: Option<i64> = r.get(0)?;
        let kind: Option<String> = r.get(1)?;
        let retries: i64 = r.get(2)?;
        Ok(json!({"statusCode":status.filter(|s| (100..=599).contains(s)),
            "errorKind":error_category(kind.as_deref()),"retryCount":retries}))
    }).map_err(|_| "Cannot read isolated events")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "Cannot decode isolated events".into())
}
#[test]
fn governance_live_error_categories_never_report_arbitrary_messages() {
    assert_eq!(error_category(Some("upstream_status")), Some("upstream_status"));
    assert_eq!(error_category(Some("untrusted upstream message")), Some("other"));
    assert_eq!(error_category(None), None);
}
async fn discover(config: &LiveInput) -> Result<(), String> {
    let started = Instant::now();
    let response = match live_client()?.get(format!("{}/v1/models", config.endpoint.trim_end_matches('/')))
        .bearer_auth(config.api_key.as_str()).send().await {
        Ok(r) => r,
        Err(_) => {
            report(json!({"phase":"discover","status":"transport_failed","generationRequests":0,
                "latencyMs":started.elapsed().as_millis()}));
            return Err("Live model discovery transport failed (details redacted)".into());
        }
    };
    let status = response.status().as_u16();
    if !response.status().is_success() {
        report(json!({"phase":"discover","httpStatus":status,"generationRequests":0}));
        return Err("Live model discovery returned a non-success HTTP status".into());
    }
    let mut body = Zeroizing::new(Vec::new());
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "Model discovery body transport failed")?;
        if body.len() + chunk.len() > 1_048_576 { return Err("Model list exceeds 1 MiB".into()); }
        body.extend_from_slice(&chunk);
    }
    let value: Value = serde_json::from_slice(&body).map_err(|_| "Invalid model list JSON")?;
    let data = value["data"].as_array().ok_or("Missing model list data")?;
    let mut models: Vec<&str> = data.iter().filter_map(|v| v["id"].as_str())
        .filter(|id| safe_model(id) && !id.contains(config.api_key.as_str()))
        .collect();
    models.sort_unstable(); models.dedup();
    let candidates: Vec<_> = models.iter().copied().filter(|id|
        ["mini", "nano", "flash", "haiku", "deepseek"].iter().any(|s| id.contains(s)))
        .take(60).collect();
    report(json!({"phase":"discover","httpStatus":status,"generationRequests":0,
        "latencyMs":started.elapsed().as_millis(),"modelCount":models.len(),"models":models.iter().take(60).collect::<Vec<_>>(),"smallModelCandidates":candidates}));
    Ok(())
}

// Diagnostics emit only allowlisted classifications, never arbitrary upstream
// messages, URLs, response bodies or credentials.
fn diagnostic_error(value: &Value) -> Value {
    let error = value.get("error").unwrap_or(value);
    let message = error.get("message").and_then(Value::as_str).unwrap_or("").to_ascii_lowercase();
    let flags: Vec<_> = ["model", "unsupported", "not supported", "invalid", "required", "stream",
        "max_tokens", "max_completion_tokens", "max_output_tokens", "quota", "balance", "credit",
        "rate limit", "authentication", "api key", "permission", "expired", "unavailable",
        "overload", "upstream", "internal server error", "timeout", "session", "capacity"]
        .into_iter().filter(|flag| message.contains(flag)).collect();
    let known_code = |name: &str| error.get(name).and_then(Value::as_str).map(|v| match v {
        "invalid_request_error" | "server_error" | "internal_error" | "upstream_error" |
        "authentication_error" | "permission_error" | "rate_limit_exceeded" |
        "insufficient_quota" | "model_not_found" | "unsupported_parameter" => v,
        _ => "other",
    });
    json!({"code":known_code("code"),"type":known_code("type"),"messageFlags":flags})
}
#[test]
fn governance_live_diagnostics_never_emit_upstream_text() {
    let value = diagnostic_error(&json!({"error":{"code":"private-token", "type":"secret-kind",
        "message":"Sensitive data: unsupported max_output_tokens; internal server error"}}));
    assert_eq!(value["code"], "other");
    assert_eq!(value["type"], "other");
    assert_eq!(value["messageFlags"], json!(["unsupported","max_output_tokens","internal server error"]));
    assert!(!value.to_string().contains("Sensitive"));
}
async fn diagnose(config: &LiveInput) -> Result<(), String> {
    if config.confirmation.as_deref() != Some("ALLOW PAID CHECKS") {
        return Err("Explicit paid diagnostic confirmation required".into());
    }
    let model = config.model.as_deref().filter(|m| safe_model(m)).ok_or("Explicit diagnostic model required")?;
    let client = live_client()?;
    // At most three dispatched calls, 320 output tokens. No retries, failover,
    // tool execution or production configuration/credential changes.
    for (variant, path, payload) in [
        ("chat", "/v1/chat/completions", json!({"model":model,"messages":[{"role":"user","content":"Reply exactly OK."}],"max_completion_tokens":64,"stream":false})),
        ("responses", "/v1/responses", json!({"model":model,"input":[{"role":"user","content":[{"type":"input_text","text":"Reply exactly OK."}]}],"max_output_tokens":128,"store":false,"stream":false})),
        ("responses_stream", "/v1/responses", json!({"model":model,"input":[{"role":"user","content":[{"type":"input_text","text":"Reply exactly OK."}]}],"max_output_tokens":128,"store":false,"stream":true})),
    ] {
        let started = Instant::now();
        let response = client.post(format!("{}{path}", config.endpoint.trim_end_matches('/')))
            .bearer_auth(config.api_key.as_str()).json(&payload).send().await
            .map_err(|_| "Diagnostic HTTPS transport failed")?;
        let status = response.status().as_u16();
        let mut stream = response.bytes_stream();
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| "Diagnostic response read failed")?;
            if bytes.len() + chunk.len() > 1_048_576 { return Err("Diagnostic response too large".into()); }
            bytes.extend_from_slice(&chunk);
        }
        let value: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
        let (has_text, complete, usage) = if status < 400 {
            if variant == "chat" {
                (value.pointer("/choices/0/message/content").and_then(Value::as_str).is_some_and(|s| s.trim() == "OK"),
                 value.pointer("/choices/0/finish_reason").and_then(Value::as_str) == Some("stop"), value.get("usage").cloned())
            } else {
                let mut final_value = value.clone();
                if variant == "responses_stream" {
                    for line in std::str::from_utf8(&bytes).unwrap_or("").lines().filter_map(|l| l.strip_prefix("data:")) {
                        if let Ok(event) = serde_json::from_str::<Value>(line.trim()) {
                            if event.get("type").and_then(Value::as_str) == Some("response.completed") {
                                final_value = event.get("response").cloned().unwrap_or(Value::Null);
                            }
                        }
                    }
                }
                (final_value.get("output").and_then(Value::as_array).is_some_and(|items| items.iter().any(|i| i.get("content").and_then(Value::as_array).is_some_and(|parts| parts.iter().any(|p| p.get("text").and_then(Value::as_str).is_some_and(|t| t.trim() == "OK"))))),
                 final_value.get("status").and_then(Value::as_str) == Some("completed"), final_value.get("usage").cloned())
            }
        } else { (false, false, None) };
        let tokens = |name| usage.as_ref().and_then(|v| v.get(name)).and_then(Value::as_i64).filter(|n| *n >= 0);
        report(json!({"phase":"diagnostic","variant":variant,"model":model,"httpStatus":status,
            "latencyMs":started.elapsed().as_millis(),"textMatches":has_text,"completed":complete,
            "inputTokens":tokens(if variant=="chat" {"prompt_tokens"} else {"input_tokens"}),
            "outputTokens":tokens(if variant=="chat" {"completion_tokens"} else {"output_tokens"}),
            "error":if status>=400 {diagnostic_error(&value)} else {Value::Null},
            "outputCap":payload.get("max_completion_tokens").or_else(||payload.get("max_output_tokens")),
            "retries":0}));
        if matches!(status, 401 | 403 | 402 | 429) { break; }
    }
    Ok(())
}

struct LiveKey(Zeroizing<String>);
impl ApiKeyProvider for LiveKey {
    fn get(&self, _: &str) -> Result<Option<String>, String> { Ok(Some(self.0.to_string())) }
}
struct Server(tokio::task::JoinHandle<()>);
impl Drop for Server { fn drop(&mut self) { self.0.abort(); } }

#[tokio::test]
#[ignore = "Live HTTPS calls require a one-shot credential pipe and explicit paid confirmation"]
async fn governance_live_acceptance_from_one_shot_pipe() -> Result<(), String> {
    let config = input()?;
    if config.phase == "discover" { return discover(&config).await; }
    if config.phase == "diagnose" { return diagnose(&config).await; }
    if config.phase != "acceptance" || config.confirmation.as_deref() != Some("ALLOW PAID CHECKS") {
        return Err("Explicit paid acceptance confirmation required".into());
    }
    let model = config.model.as_deref().filter(|s| safe_model(s)
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b)))
        .ok_or("Choose a discoverable model compatible with all six client paths")?;
    let db = test_support::database();
    let mut provider = test_support::provider(&config.endpoint);
    provider.id = "live-acceptance".into();
    provider.name = "Isolated live acceptance".into();
    provider.model = model.into();
    provider.advanced.upstream_format = match config.upstream_format.as_deref().unwrap_or("openai_chat") {
        "openai_chat" => "openai_chat",
        "openai_responses" => "openai_responses",
        _ => return Err("Unsupported live upstream format".into()),
    }.into();
    db.save_provider(&provider)?;
    db.set_current_provider(&provider.id)?;
    // Admission prices are test values, NOT a quote or a guarantee about the
    // upstream invoice. The gateway reports extra input beyond the visible prompt; reserve
    // 8,192 input tokens per call. Six calls reserve 49,280 tokens / 49,408 microUSD.
    let policy = budget::BudgetPolicy { enabled:true, daily_tokens:50_000,
        daily_microusd:60_000, input_reservation:8192, max_output_tokens:48 };
    governance::save(&db, budget::KEY, &policy)?;
    {
        let c = db.conn(); let c = c.lock().map_err(|_| "Database lock")?;
        crate::model_pricing::save_on(&c, &crate::model_pricing::PricingInput {
            id:model.into(), input_per_million:1.0, output_per_million:2.0,
            cache_read_per_million:None, cache_creation_per_million:None })?;
    }
    governance::save(&db, access::KEY, &access::AccessPolicy {
        require_token:true, requests_per_minute:12, max_in_flight:1 })?;
    let token = Zeroizing::new(access::issue(&db, "Isolated acceptance", "proxy", 1)?.token);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await
        .map_err(|_| "Cannot bind isolated loopback proxy")?;
    let address = listener.local_addr().map_err(|_| "Cannot get loopback address")?;
    let state = ProxyState { database:db.clone(), client:live_client()?,
        api_keys:Arc::new(LiveKey(Zeroizing::new(config.api_key.to_string()))), local_addr:Some(address) };
    let router = router_with_targets(state, &[]);
    let _server = Server(tokio::spawn(async move { let _ = axum::serve(listener, router).await; }));
    let base = format!("http://{address}");
    let mut passed = 0;
    let mut attempted = 0;
    for mode in ["chat", "stream", "tools", "responses", "anthropic", "gemini"] {
        let monitor_policy = monitor::MonitorPolicy { enabled:false, provider_id:provider.id.clone(),
            model:model.into(), mode:mode.into(), interval_minutes:30, max_runs_per_day:6 };
        let cursor = latest_event_id(&db)?;
        let run = monitor::execute_at(&db, &base, &token, &monitor_policy, false).await?;
        let upstream_events = upstream_events_since(&db, cursor)?;
        attempted += 1;
        passed += u32::from(run.status == "passed");
        let stop = run.evidence.http_status.is_none_or(|status| status >= 400);
        report(json!({"phase":"acceptance","upstreamFormat":provider.advanced.upstream_format,
            "receipt":run,"upstreamEvents":upstream_events}));
        if stop { break; }
    }
    let usage = budget::status(&db)?;
    report(json!({"phase":"summary","attempted":attempted,"passed":passed,
        "admittedAttempts":usage.attempts,"reservedTokens":usage.reserved_tokens,
        "reservedMicroUsdAtTestPrices":usage.reserved_microusd,"totalOutputCap":128,
        "usesProductionDatabase":false,"usesProductionCredentialsStore":false}));
    if passed != 6 { return Err("Live acceptance did not pass all six checks; see redacted receipts".into()); }
    Ok(())
}
