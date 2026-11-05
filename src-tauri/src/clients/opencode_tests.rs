use super::*;
use std::{io::{Read, Write}, net::TcpListener, sync::Mutex};

struct Reply { status: u16, value: Value, delay: Duration, size: Option<usize> }

fn reply(value: Value) -> Reply { Reply { status: 200, value, delay: Duration::ZERO, size: None } }
fn empty() -> Reply { Reply { status: 204, value: Value::Null, delay: Duration::ZERO, size: None } }

#[derive(Debug)]
struct Request { method: String, path: String, body: Value, protected: bool }

fn server(replies: Vec<Reply>) -> (LocalApi, Arc<Mutex<Vec<Request>>>, JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let address = listener.local_addr().unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let recorded = requests.clone();
    let worker = std::thread::spawn(move || {
        for reply in replies {
            let deadline = Instant::now() + Duration::from_secs(5);
            let mut stream = loop {
                match listener.accept() {
                    Ok((stream, _)) => break stream,
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline => std::thread::sleep(Duration::from_millis(2)),
                    _ => panic!("synthetic loopback request did not arrive"),
                }
            };
            stream.set_nonblocking(false).unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new(); reader.read_line(&mut first).unwrap();
            let mut parts = first.split_whitespace();
            let method = parts.next().unwrap().to_owned();
            let path = parts.next().unwrap().to_owned();
            let mut size = 0usize;
            let mut protected = false;
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0, "fixture request ended before its headers");
                if line == "\r\n" { break; }
                let line = line.to_ascii_lowercase();
                if let Some(value) = line.strip_prefix("content-length:") { size = value.trim().parse().unwrap(); }
                if line.starts_with("authorization: basic ") { protected = true; }
            }
            let mut bytes = vec![0; size]; reader.read_exact(&mut bytes).unwrap();
            let body = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
            recorded.lock().unwrap().push(Request { method, path, body, protected });
            std::thread::sleep(reply.delay);
            let body = if reply.status == 204 { String::new() } else { reply.value.to_string() };
            let header = format!("HTTP/1.1 {} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", reply.status, reply.size.unwrap_or(body.len()));
            let _ = stream.write_all(header.as_bytes()); let _ = stream.write_all(body.as_bytes());
        }
    });
    (LocalApi::new(endpoint(&format!("http://{address}/")).unwrap(), "synthetic-only".into()).unwrap(), requests, worker)
}

fn driver(api: LocalApi, schema: OpenCodeSchema) -> OpenCodeDriver {
    OpenCodeDriver { child: None, api, schema, login: None, failed: false }
}

fn integration(connections: Value) -> Value {
    json!({"location":{"directory":"PRIVATE_DIRECTORY"},"data":[{"id":"openai","name":"OpenAI","methods":[{"id":"oauth/native","type":"oauth","label":"Native OAuth"}],"connections":connections}]})
}

fn attempt(mode: &str) -> Value {
    json!({"location":{"directory":"PRIVATE_DIRECTORY"},"data":{"attemptID":"con_test","url":"https://auth.example.test/authorize?state=temporary-only","instructions":"Continue in the native authorization page","mode":mode,"time":{"created":1,"expires":600001}}})
}

#[test]
fn loopback_and_authorization_urls_reject_credentials_and_redirect_targets() {
    for address in ["https://127.0.0.1:1234", "http://localhost:1234", "http://127.0.0.1", "http://user:pass@127.0.0.1:1234", "http://127.0.0.1:1234/path", "http://127.0.0.1:1234/?key=secret", "http://127.0.0.1:1234/#fragment", "http://127.0.0.1:0"] {
        assert!(endpoint(address).is_err());
    }
    assert!(endpoint("http://127.0.0.1:4444/").is_ok());
    for address in ["javascript:alert(1)", "http://auth.example.test", "https://user:secret@auth.example.test", "https://auth.example.test:4444"] {
        assert!(authorization_url(&json!(address)).is_err());
    }
    assert!(authorization_url(&json!("https://auth.example.test/?state=native")).is_ok());
}

#[test]
fn startup_output_is_bounded_and_never_returns_raw_diagnostics() {
    let mut source = std::io::Cursor::new(b"PRIVATE_TOKEN\nopencode server listening on http://127.0.0.1:4000\n".to_vec());
    assert_eq!(read_endpoint(&mut source).unwrap().port(), Some(4000));
    let mut source = std::io::Cursor::new(vec![b'x'; MAX_LINE + 1]);
    assert!(read_endpoint(&mut source).is_err());
    let mut source = std::io::Cursor::new(b"PRIVATE_TOKEN".to_vec());
    assert!(!read_endpoint(&mut source).unwrap_err().contains("PRIVATE_TOKEN"));
}

#[test]
fn oauth_method_indices_and_conditional_prompts_are_preserved() {
    let methods = parse_methods(&json!({"openai":[{"type":"api","label":"API Key"},{"type":"oauth","label":"OAuth","prompts":[{"type":"select","key":"mode","message":"Choose","options":[{"label":"Enterprise","value":"enterprise"},{"label":"Personal","value":"personal"}]},{"type":"text","key":"tenant","message":"Tenant","when":{"key":"mode","op":"eq","value":"enterprise"}}]}]}), OpenCodeSchema::V1).unwrap();
    assert_eq!(methods[0].method_id, "1");
    assert!(validate_inputs(&methods[0], &BTreeMap::from([("mode".into(), "personal".into())])).is_ok());
    for inputs in [BTreeMap::from([("mode".into(), "bad".into())]), BTreeMap::from([("mode".into(), "enterprise".into())]), BTreeMap::from([("mode".into(), "personal".into()), ("tenant".into(), "hidden".into())])] {
        assert!(validate_inputs(&methods[0], &inputs).is_err());
    }
    assert!(prompts(Some(&json!([{"type":"text","key":"__proto__","message":"Unsafe"}]))).is_err());
}

#[test]
fn native_snapshot_excludes_locations_account_labels_and_environment_names() {
    let (api, requests, worker) = server(vec![reply(integration(json!([{"type":"credential","id":"cred_owned","label":"PRIVATE_EMAIL@example.test"},{"type":"env","name":"PRIVATE_ENV_KEY"}])))]);
    let snapshot = driver(api, OpenCodeSchema::V2).account("opencode-desktop").unwrap();
    assert_eq!(snapshot.providers.len(), 2);
    assert!(snapshot.providers[0].disconnectable);
    assert!(!snapshot.providers[1].disconnectable);
    assert!(snapshot.quota.is_none());
    assert!(!serde_json::to_string(&snapshot).unwrap().contains("PRIVATE"));
    worker.join().unwrap();
    assert!(requests.lock().unwrap()[0].protected);
}

#[test]
fn v2_auto_authorization_only_polls_and_clears_temporary_links() {
    let (api, requests, worker) = server(vec![reply(integration(json!([]))), reply(attempt("auto")), reply(json!({"data":{"status":"pending"}})), reply(json!({"data":{"status":"complete"}}))]);
    let mut driver = driver(api, OpenCodeSchema::V2);
    assert_eq!(driver.start_login("opencode-cli", "openai", "oauth/native", BTreeMap::new()).unwrap().status, "pending");
    assert!(driver.complete_login("opencode-cli", "unneeded").is_err());
    assert_eq!(driver.login_progress("opencode-cli").unwrap().status, "pending");
    let finished = driver.login_progress("opencode-cli").unwrap();
    assert_eq!(finished.status, "completed");
    assert!(finished.verification_url.is_none() && finished.instructions.is_none());
    worker.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.iter().filter(|item| item.method == "POST").count(), 1);
    assert_eq!(requests[1].path, "/api/integration/openai/connect/oauth");
    assert_eq!(requests[1].body["methodID"], "oauth/native");
}

#[test]
fn v2_code_authorization_submits_only_to_its_pending_attempt() {
    let (api, requests, worker) = server(vec![reply(integration(json!([]))), reply(attempt("code")), empty(), reply(json!({"data":{"status":"complete"}}))]);
    let mut driver = driver(api, OpenCodeSchema::V2);
    assert!(driver.start_login("opencode-cli", "openai", "oauth/native", BTreeMap::new()).unwrap().requires_code);
    assert!(driver.complete_login("opencode-cli", "\n").is_err());
    assert_eq!(driver.complete_login("opencode-cli", "synthetic-code").unwrap().status, "completed");
    assert!(driver.complete_login("opencode-cli", "second-code").is_err());
    worker.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests[2].path, "/api/integration/attempt/con_test/complete");
    assert_eq!(requests[2].body, json!({"code":"synthetic-code"}));
}

fn v1_methods() -> Value { json!({"openai":[{"type":"api","label":"API Key"},{"type":"oauth","label":"Native OAuth"}]}) }
fn v1_attempt(mode: &str) -> Value { json!({"url":"https://auth.example.test/native","method":mode,"instructions":"Continue in the authorization page"}) }

#[test]
fn v1_auto_callback_is_started_once_and_is_not_restarted_by_polling() {
    let mut callback = reply(Value::Bool(true)); callback.delay = Duration::from_millis(100);
    let (api, requests, worker) = server(vec![reply(v1_methods()), reply(v1_attempt("auto")), callback]);
    let mut driver = driver(api, OpenCodeSchema::V1);
    assert_eq!(driver.start_login("opencode-cli", "openai", "1", BTreeMap::new()).unwrap().status, "pending");
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if driver.login_progress("opencode-cli").unwrap().status == "completed" { break; }
        assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(5));
    }
    worker.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[1].body["method"], 1);
    assert_eq!(requests[2].path, "/provider/openai/oauth/callback");
    assert_eq!(requests[2].body, json!({"method":1}));
}

#[test]
fn v1_cancel_stops_the_local_callback_wait_without_repeating_authentication() {
    let mut callback = reply(Value::Bool(true)); callback.delay = Duration::from_millis(800);
    let (api, requests, worker) = server(vec![reply(v1_methods()), reply(v1_attempt("auto")), callback]);
    let mut driver = driver(api, OpenCodeSchema::V1);
    driver.start_login("opencode-cli", "openai", "1", BTreeMap::new()).unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    while requests.lock().unwrap().len() != 3 {
        assert!(Instant::now() < deadline); std::thread::sleep(Duration::from_millis(5));
    }
    let started = Instant::now(); driver.cancel_login().unwrap();
    assert!(started.elapsed() < Duration::from_millis(500));
    assert!(!driver.login_pending());
    assert!(driver.login_progress("opencode-cli").unwrap().verification_url.is_none());
    worker.join().unwrap();
    assert_eq!(requests.lock().unwrap().len(), 3);
}

#[test]
fn v1_code_callback_keeps_the_original_oauth_method_index() {
    let (api, requests, worker) = server(vec![reply(v1_methods()), reply(v1_attempt("code")), reply(Value::Bool(true))]);
    let mut driver = driver(api, OpenCodeSchema::V1);
    assert!(driver.start_login("opencode-desktop", "openai", "1", BTreeMap::new()).unwrap().requires_code);
    assert_eq!(driver.complete_login("opencode-desktop", "synthetic-only").unwrap().status, "completed");
    worker.join().unwrap();
    assert_eq!(requests.lock().unwrap()[2].body, json!({"method":1,"code":"synthetic-only"}));
}

#[test]
fn v2_cancel_only_targets_the_owned_attempt() {
    let (api, requests, worker) = server(vec![reply(integration(json!([]))), reply(attempt("auto")), empty()]);
    let mut driver = driver(api, OpenCodeSchema::V2);
    driver.start_login("opencode-cli", "openai", "oauth/native", BTreeMap::new()).unwrap();
    driver.cancel_login().unwrap();
    worker.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests[2].method, "DELETE");
    assert_eq!(requests[2].path, "/api/integration/attempt/con_test");
    assert!(!driver.login_pending());
}

#[test]
fn disconnect_rechecks_credential_ownership_and_refuses_environment_connections() {
    for connection in [Some("cred_other"), None] {
        let (api, requests, worker) = server(vec![reply(integration(json!([{"type":"credential","id":"cred_owned","label":"PRIVATE_LABEL"},{"type":"env","name":"PRIVATE_ENV"}])))]);
        assert!(driver(api, OpenCodeSchema::V2).disconnect("openai", connection).is_err());
        worker.join().unwrap();
        assert_eq!(requests.lock().unwrap().len(), 1);
    }
    let (api, requests, worker) = server(vec![reply(integration(json!([{"type":"credential","id":"cred_owned","label":"PRIVATE_LABEL"}]))), empty()]);
    driver(api, OpenCodeSchema::V2).disconnect("openai", Some("cred_owned")).unwrap();
    worker.join().unwrap();
    let requests = requests.lock().unwrap();
    assert_eq!(requests[1].method, "DELETE");
    assert_eq!(requests[1].path, "/api/credential/cred_owned");
}

#[test]
fn loopback_transport_rejects_oversized_bodies_and_never_echoes_errors() {
    for response in [Reply { status: 200, value: json!({"PRIVATE":"secret"}), delay: Duration::ZERO, size: Some(MAX_RESPONSE + 1) }, Reply { status: 307, value: json!({"PRIVATE":"secret"}), delay: Duration::ZERO, size: None }, Reply { status: 401, value: json!({"PRIVATE":"secret"}), delay: Duration::ZERO, size: None }] {
        let (api, _, worker) = server(vec![response]);
        let error = match api.call(Method::GET, &["provider"], None) { Err(error) => error.message(), Ok(_) => panic!("unsafe response accepted") };
        assert!(!error.contains("PRIVATE") && !error.contains("secret"));
        worker.join().unwrap();
    }
    let (api, _, worker) = server(vec![reply(integration(json!([]))), reply(attempt("auto")), reply(json!({"data":{"status":"failed","message":"PRIVATE_PASSWORD=secret"}}))]);
    let mut driver = driver(api, OpenCodeSchema::V2);
    driver.start_login("opencode-cli", "openai", "oauth/native", BTreeMap::new()).unwrap();
    let failed = driver.login_progress("opencode-cli").unwrap();
    assert_eq!(failed.status, "failed");
    assert!(failed.instructions.is_none() && failed.verification_url.is_none());
    worker.join().unwrap();
}

#[test]
fn nested_sensitive_payload_strings_are_cleared() {
    let mut value = json!({"code":"synthetic-secret","inputs":[{"token":"synthetic-token"}]});
    wipe(&mut value);
    assert_eq!(value, json!({"code":"","inputs":[{"token":""}]}));
    let mut inputs = Inputs(BTreeMap::from([("tenant".into(), "synthetic-private-input".into())]));
    inputs.clear();
    assert!(inputs.0.values().all(String::is_empty));
}

#[test]
fn startup_reader_discards_following_output_without_closing_the_pipe_early() {
    let mut bytes = b"opencode server listening on http://127.0.0.1:4000\n".to_vec();
    bytes.extend(vec![b'x'; 24576]);
    let expected = bytes.len() as u64;
    let mut reader = BufReader::with_capacity(512, std::io::Cursor::new(bytes));
    assert_eq!(read_endpoint(&mut reader).unwrap().port(), Some(4000));
    assert!(reader.get_ref().position() < expected);
    discard_output(&mut reader);
    assert_eq!(reader.get_ref().position(), expected);
    assert!(reader.buffer().is_empty());
}
