use super::*;
use axum::{http::Request, middleware::Next};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use toml_edit::{DocumentMut, value};

const CLIENT_REQUEST_LIMIT: usize = 4;
const OUTPUT_CAP: u64 = 256;
const INPUT_BYTE_CAP: usize = 65_536;

#[derive(Clone)]
struct Capture {
    provider: Provider,
    requests: Arc<Mutex<Vec<Value>>>,
    admitted: Arc<AtomicUsize>,
    failed: Arc<AtomicBool>,
}

fn tool_outputs(payload: &Value, field: &str) -> Vec<Value> {
    payload[field]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| {
            item["role"] == "tool"
                || matches!(
                    item["type"].as_str(),
                    Some("function_call_output" | "custom_tool_call_output")
                )
        })
        .cloned()
        .collect()
}

fn denied(code: &'static str) -> Response {
    (
        StatusCode::PAYMENT_REQUIRED,
        Json(json!({"error":{"type":code,"message":code}})),
    )
        .into_response()
}

async fn capture_request(
    State(capture): State<Capture>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if request.method() != axum::http::Method::POST {
        return next.run(request).await;
    }
    if capture.failed.load(Ordering::SeqCst)
        || capture.admitted.fetch_add(1, Ordering::SeqCst) >= CLIENT_REQUEST_LIMIT
    {
        return denied("joint_client_request_limit");
    }
    let (parts, body) = request.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, INPUT_BYTE_CAP).await else {
        return denied("joint_input_byte_limit");
    };
    let Ok(payload) = serde_json::from_slice::<Value>(&bytes) else {
        return denied("joint_invalid_json");
    };
    let Ok(prepared) =
        prepare_upstream_request(&capture.provider, ClientProtocol::Responses, &bytes)
    else {
        return denied("joint_prepare_failed");
    };
    let cap = prepared
        .payload
        .get("max_completion_tokens")
        .or_else(|| prepared.payload.get("max_output_tokens"))
        .and_then(Value::as_u64);
    if cap != Some(OUTPUT_CAP) || prepared.url.scheme() != "https" {
        return denied("joint_output_or_destination_limit");
    }
    let observation = json!({
        "clientPath":parts.uri.path(),"clientModel":payload["model"],"stream":payload["stream"],
        "inputBytes":bytes.len(),"toolTypes":payload["tools"].as_array().into_iter().flatten().map(|tool|tool["type"].clone()).collect::<Vec<_>>(),
        "upstreamUrl":prepared.url.as_str(),"outputCap":cap,
        "clientToolOutputs":tool_outputs(&payload,"input"),
        "preparedToolOutputs":tool_outputs(&prepared.payload,if capture.provider.advanced.upstream_format=="openai_chat" {"messages"} else {"input"}),
    });
    let index = {
        let Ok(mut requests) = capture.requests.lock() else {
            return denied("joint_capture_lock");
        };
        let index = requests.len();
        requests.push(observation);
        index
    };
    let response = next
        .run(Request::from_parts(parts, Body::from(bytes)))
        .await;
    if let Ok(mut requests) = capture.requests.lock() {
        requests[index]["clientStatus"] = json!(response.status().as_u16());
    }
    if !response.status().is_success() {
        capture.failed.store(true, Ordering::SeqCst);
    }
    response
}

fn write_json(path: &Path, contents: &Value) -> Result<(), String> {
    std::fs::write(
        path,
        serde_json::to_vec_pretty(contents).map_err(|_| "joint_json_encode")?,
    )
    .map_err(|_| "joint_artifact_write".into())
}

fn request_events(db: &Database) -> Result<Vec<Value>, String> {
    let connection = db.conn();
    let connection = connection.lock().map_err(|_| "joint_database_lock")?;
    let mut statement = connection.prepare("SELECT id,status_code,error_kind,retry_count,model,input_tokens,output_tokens FROM request_events ORDER BY id LIMIT 16")
        .map_err(|_| "joint_event_query")?;
    statement.query_map([], |row| {
        let kind: Option<String> = row.get(2)?;
        Ok(json!({"id":row.get::<_,i64>(0)?,"statusCode":row.get::<_,Option<i64>>(1)?,
            "errorKind":error_category(kind.as_deref()),"retryCount":row.get::<_,i64>(3)?,
            "model":row.get::<_,Option<String>>(4)?.filter(|model|safe_model(model)),
            "inputTokens":row.get::<_,Option<i64>>(5)?,"outputTokens":row.get::<_,Option<i64>>(6)?}))
    }).map_err(|_| "joint_event_read")?.collect::<Result<Vec<_>,_>>()
        .map_err(|_| "joint_event_decode".into())
}

async fn client_turn(
    root: &Path,
    turn: usize,
    previous: Option<&str>,
    prompt: String,
) -> Result<Vec<Value>, String> {
    let root = root.to_path_buf();
    let previous = previous.map(str::to_owned);
    let executable =
        std::env::var_os("XY_SWITCH_CODEX_EXE").ok_or("joint_cli_executable_required")?;
    tokio::task::spawn_blocking(move || {
        let home = root.join("home");
        let stdout = root.join(format!("turn-{turn}.jsonl"));
        let stderr = root.join(format!("turn-{turn}.stderr"));
        let mut command = Command::new(executable);
        command.env_clear();
        for name in [
            "SystemRoot",
            "WINDIR",
            "COMSPEC",
            "PATH",
            "PATHEXT",
            "TEMP",
            "TMP",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .env("APPDATA", home.join("appdata"))
            .env("LOCALAPPDATA", home.join("localappdata"))
            .env("HOME", &home)
            .env("USERPROFILE", &home)
            .env("CODEX_HOME", home.join(".codex"))
            .env("OPENAI_API_KEY", "joint-local-placeholder")
            .env("NO_PROXY", "127.0.0.1,localhost,::1")
            .current_dir(root.join("workspace"))
            .arg("exec");
        if let Some(previous) = previous {
            command.arg("resume").arg(previous);
        }
        command.args(["--skip-git-repo-check", "--json"]);
        if turn == 0 {
            command.args(["--color", "never", "--sandbox", "read-only"]);
        }
        command
            .arg(prompt)
            .stdin(Stdio::null())
            .stdout(std::fs::File::create(&stdout).map_err(|_| "joint_stdout_create")?)
            .stderr(std::fs::File::create(&stderr).map_err(|_| "joint_stderr_create")?);
        let mut child = command.spawn().map_err(|_| "joint_cli_spawn")?;
        let started = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().map_err(|_| "joint_cli_wait")? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(240) {
                let _ = child.kill();
                let _ = child.wait();
                return Err("joint_cli_timeout".to_string());
            }
            std::thread::sleep(Duration::from_millis(100));
        };
        if !status.success() {
            return Err("joint_cli_failed".into());
        }
        let output = std::fs::read_to_string(stdout).map_err(|_| "joint_stdout_read")?;
        let events: Vec<Value> = output
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        if !events.iter().any(|event| event["type"] == "turn.completed") {
            return Err("joint_turn_incomplete".into());
        }
        Ok(events)
    })
    .await
    .map_err(|_| "joint_cli_task")?
}

async fn verify_client(root: &Path, nonce: &str, capture: &Capture) -> Result<Value, String> {
    let prompt = format!(
        "Verify this live forwarding chain. Call get_goal once and the joint_probe receipt MCP tool once with a=17, b=25, nonce={nonce}. Use tool discovery first if the MCP tool is deferred. After both tool results, answer with only the exact receipt generated locally by that MCP tool. Do not run shell commands, edit files, or use web search."
    );
    let first = client_turn(root, 0, None, prompt).await?;
    let session = first
        .iter()
        .find_map(|event| event["thread_id"].as_str())
        .ok_or("joint_session_missing")?;
    let receipt: Value = serde_json::from_slice(
        &std::fs::read(root.join("receipt.json")).map_err(|_| "joint_receipt_missing")?,
    )
    .map_err(|_| "joint_receipt_invalid")?;
    let marker = receipt["receipt"]
        .as_str()
        .filter(|marker| marker.starts_with("XY_RECEIPT_") && marker.len() == 43)
        .ok_or("joint_receipt_format")?;
    if receipt["sum"] != 42 || receipt["nonce"] != nonce {
        return Err("joint_receipt_arguments".into());
    }
    let final_text = |events: &[Value]| {
        events
            .iter()
            .rev()
            .find(|event| {
                event["type"] == "item.completed" && event["item"]["type"] == "agent_message"
            })
            .and_then(|event| event["item"]["text"].as_str())
            .map(str::trim)
            .map(str::to_owned)
    };
    if final_text(&first).as_deref() != Some(marker) {
        return Err("joint_first_receipt_mismatch".into());
    }
    let resumed = client_turn(root,1,Some(session),"Return only the exact receipt that the MCP tool produced in the previous turn, using the existing conversation history. Do not call any tools.".into()).await?;
    if !resumed.iter().any(|event| event["thread_id"] == session)
        || final_text(&resumed).as_deref() != Some(marker)
    {
        return Err("joint_resume_identity_or_receipt".into());
    }
    let calls = std::fs::read_to_string(root.join("mcp-calls.jsonl"))
        .map_err(|_| "joint_mcp_calls_missing")?;
    if calls.lines().count() != 1 {
        return Err("joint_mcp_repeated".into());
    }
    let home = root.join("home");
    let store = crate::sessions::SessionStore::new(&home, &home.join(".codex"))?;
    let catalog = store.catalog()?;
    if catalog.sessions.len() != 1 || catalog.sessions[0].session_id != session {
        return Err("joint_catalog_identity".into());
    }
    let detail = store.detail("codex", session)?;
    let goal = detail
        .messages
        .iter()
        .filter(|message| message.role == "tool")
        .any(|message| {
            serde_json::from_str::<Value>(&message.content)
                .is_ok_and(|value| value.get("goal").is_some_and(Value::is_null))
        });
    if !goal
        || !detail
            .messages
            .iter()
            .any(|message| message.role == "tool" && message.content.contains(marker))
        || detail
            .messages
            .iter()
            .filter(|message| message.role == "assistant" && message.content.trim() == marker)
            .count()
            < 2
    {
        return Err("joint_session_readback_failed".into());
    }
    write_json(
        &root.join("session-readback.json"),
        &serde_json::to_value(&detail).map_err(|_| "joint_session_encode")?,
    )?;
    let requests = capture.requests.lock().map_err(|_| "joint_capture_lock")?;
    let receipt_returned = requests.iter().any(|request| {
        request["preparedToolOutputs"]
            .as_array()
            .is_some_and(|outputs| {
                outputs
                    .iter()
                    .any(|output| output.to_string().contains(marker))
            })
    });
    if !receipt_returned {
        return Err("joint_forwarded_tool_result_missing".into());
    }
    Ok(
        json!({"sessionId":session,"sameSessionAfterResume":true,"sessionCount":catalog.sessions.len(),
        "getGoalExecuted":goal,"mcpCalls":1,"sum":42,"nonceMatched":true,"receipt":marker,
        "firstExactReceipt":true,"resumedExactReceipt":true,"toolResultPreparedForUpstream":true,
        "applicationSessionReadback":true}),
    )
}

async fn run_path(config: &LiveInput, format: &str, root: &Path) -> Result<Value, String> {
    let model = config
        .model
        .as_deref()
        .filter(|model| safe_model(model))
        .ok_or("joint_model_required")?;
    let node = std::env::var("XY_SWITCH_NODE_EXE").map_err(|_| "joint_node_required")?;
    for directory in [
        "home/.codex",
        "home/appdata",
        "home/localappdata",
        "workspace",
    ] {
        std::fs::create_dir_all(root.join(directory)).map_err(|_| "joint_directory_create")?;
    }
    let script = root.join("receipt-mcp.mjs");
    std::fs::write(&script, include_str!("live_receipt_mcp.mjs"))
        .map_err(|_| "joint_mcp_script_write")?;
    let nonce = uuid::Uuid::new_v4().to_string();
    let db = test_support::database();
    let mut provider = test_support::provider(&config.endpoint);
    provider.id = "joint-live".into();
    provider.name = "Isolated live forwarding".into();
    provider.model = model.into();
    provider.advanced.upstream_format = format.into();
    provider.advanced.body_overrides.insert(
        if format == "openai_chat" {
            "max_completion_tokens"
        } else {
            "max_output_tokens"
        }
        .into(),
        json!(OUTPUT_CAP),
    );
    db.save_provider(&provider)?;
    db.set_current_provider(&provider.id)?;
    governance::save(
        &db,
        access::KEY,
        &access::AccessPolicy {
            require_token: true,
            requests_per_minute: 20,
            max_in_flight: 1,
        },
    )?;
    let token = Zeroizing::new(access::issue(&db, "Joint live local access", "proxy", 1)?.token);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "joint_loopback_bind")?;
    let address = listener
        .local_addr()
        .map_err(|_| "joint_loopback_address")?;
    let capture = Capture {
        provider: provider.clone(),
        requests: Arc::new(Mutex::new(Vec::new())),
        admitted: Arc::new(AtomicUsize::new(0)),
        failed: Arc::new(AtomicBool::new(false)),
    };
    let state = ProxyState {
        database: db.clone(),
        client: live_client()?,
        api_keys: Arc::new(LiveKey(Zeroizing::new(config.api_key.to_string()))),
        local_addr: Some(address),
    };
    let router = router_with_targets(state, &["codex".into()]).layer(
        axum::middleware::from_fn_with_state(capture.clone(), capture_request),
    );
    let _server = Server(tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    }));
    let local_client = crate::network::test_client();
    let base = format!("http://{address}/v1");
    let unauthorized = local_client
        .get(format!("{base}/models"))
        .send()
        .await
        .map_err(|_| "joint_local_auth_request")?;
    if unauthorized.status() != StatusCode::UNAUTHORIZED {
        return Err("joint_local_auth_missing".into());
    }
    let authorized = local_client
        .get(format!("{base}/models"))
        .bearer_auth(token.as_str())
        .send()
        .await
        .map_err(|_| "joint_local_catalog_request")?;
    if !authorized.status().is_success() {
        return Err("joint_local_catalog_failed".into());
    }
    let codex_home = root.join("home/.codex");
    let config_path = codex_home.join("config.toml");
    let config_bytes = crate::takeover::managed_codex_config_internal(
        "",
        &base,
        token.as_str(),
        &provider,
        &codex_home,
    )?;
    std::fs::write(&config_path, config_bytes).map_err(|_| "joint_config_write")?;
    crate::takeover::write_codex_model_catalog(&codex_home, &provider)?;
    let mcp = crate::mcp::McpManager::new(db.clone(), root.join("home"));
    let server = serde_json::from_value(
        json!({"id":"joint_probe","name":"Joint receipt probe","transport":"stdio",
        "command":node,"args":[script,root,nonce],"apps":{"codex":true}}),
    )
    .map_err(|_| "joint_mcp_config")?;
    mcp.save_server(server)?;
    let text = std::fs::read_to_string(&config_path).map_err(|_| "joint_config_read")?;
    let mut document = text
        .parse::<DocumentMut>()
        .map_err(|_| "joint_config_parse")?;
    document["approval_policy"] = value("never");
    document["web_search"] = value("disabled");
    document["model_reasoning_effort"] = value("low");
    document["features"]["unbounded_connection_retries"] = value(false);
    document["model_providers"]["xy_switch"]["request_max_retries"] = value(0_i64);
    document["model_providers"]["xy_switch"]["stream_max_retries"] = value(0_i64);
    document["model_providers"]["xy_switch"]["stream_idle_timeout_ms"] = value(65_000_i64);
    document["mcp_servers"]["joint_probe"]["startup_timeout_sec"] = value(15_i64);
    document["mcp_servers"]["joint_probe"]["tool_timeout_sec"] = value(10_i64);
    document["mcp_servers"]["joint_probe"]["tools"]["receipt"]["approval_mode"] = value("approve");
    std::fs::write(&config_path, document.to_string()).map_err(|_| "joint_config_limits_write")?;
    let verified = verify_client(root, &nonce, &capture).await;
    let requests = capture
        .requests
        .lock()
        .map_err(|_| "joint_capture_lock")?
        .clone();
    let events = request_events(&db)?;
    write_json(&root.join("proxy-requests.json"), &json!(requests))?;
    write_json(&root.join("request-events.json"), &json!(events))?;
    let attempts: u64 = events
        .iter()
        .map(|event| 1 + event["retryCount"].as_u64().unwrap_or(0))
        .sum();
    let events_ok = !events.is_empty()
        && events.len() == requests.len()
        && events
            .iter()
            .all(|event| event["statusCode"] == 200 && event["retryCount"] == 0);
    let (checks, error) = match verified {
        Ok(checks) => (Some(checks), None),
        Err(error) => (None, Some(error)),
    };
    let result = json!({"upstreamFormat":format,"throughApplicationProxy":true,"passed":checks.is_some() && events_ok,
        "clientRequests":requests.len(),"upstreamAttempts":attempts,"allStatus200NoRetries":events_ok,
        "localAuthenticationChecked":true,"mcpConfigurationWrittenByApplication":true,
        "checks":checks,"error":error,"events":events});
    write_json(&root.join("result.json"), &result)?;
    report(result.clone());
    Ok(result)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "Real CLI and paid HTTPS require a one-shot credential pipe and isolated output directory"]
async fn codex_cli_tools_and_resume_through_real_application_proxy() -> Result<(), String> {
    let config = input()?;
    if config.phase != "client" || config.confirmation.as_deref() != Some("ALLOW PAID CHECKS") {
        return Err("joint_paid_confirmation_required".into());
    }
    let root =
        PathBuf::from(std::env::var_os("XY_JOINT_LIVE_OUTPUT").ok_or("joint_output_required")?);
    let artifacts = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or("joint_workspace_path")?
        .join("artifacts");
    let canonical = root.canonicalize().map_err(|_| "joint_output_must_exist")?;
    if !canonical.starts_with(
        artifacts
            .canonicalize()
            .map_err(|_| "joint_artifacts_path")?,
    ) {
        return Err("joint_output_outside_workspace_artifacts".into());
    }
    let mut results = Vec::new();
    for (format, name) in [("openai_chat", "chat"), ("openai_responses", "responses")] {
        let result = match run_path(&config, format, &canonical.join(name)).await {
            Ok(result) => result,
            Err(error) => json!({"upstreamFormat":format,"passed":false,"error":error}),
        };
        let passed = result["passed"] == true;
        results.push(result);
        if !passed {
            break;
        }
    }
    let passed = results.len() == 2 && results.iter().all(|result| result["passed"] == true);
    let summary = json!({"origin":config.endpoint,"model":config.model,"passed":passed,"paths":results,
        "limits":{"clientRequestsPerPath":CLIENT_REQUEST_LIMIT,"applicationAttemptsPerRequest":3,
            "potentialUpstreamAttemptsTotal":CLIENT_REQUEST_LIMIT*2*3,"outputTokensPerAttempt":OUTPUT_CAP,
            "inputBytesPerRequest":INPUT_BYTE_CAP,"clientRetries":0,"cliTurnTimeoutSeconds":240},
        "throughApplicationProxy":true,"usesProductionDatabase":false,"usesProductionCredentialsStore":false});
    write_json(&canonical.join("summary.json"), &summary)?;
    report(summary);
    if !passed {
        return Err("joint_live_client_verification_failed".into());
    }
    Ok(())
}
