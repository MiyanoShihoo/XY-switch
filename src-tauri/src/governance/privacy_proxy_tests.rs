use super::*;
use crate::privacy::{self, PrivacySettings};
use crate::request_diagnostics::RequestDiagnostics;

fn latest_diagnostics(fixture: &Fixture) -> (String, RequestDiagnostics) {
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let serialized: String = connection.query_row(
        "SELECT diagnostics_json FROM request_events ORDER BY id DESC LIMIT 1", [], |row| row.get(0),
    ).unwrap();
    let diagnostic = serde_json::from_str(&serialized).unwrap();
    (serialized, diagnostic)
}

async fn interrupted_response(request: reqwest::RequestBuilder) -> String {
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => { assert!(error.is_request()); return String::new(); }
    };
    assert_eq!(response.status(), 200);
    let mut output = String::new();
    let mut stream = response.bytes_stream();
    let mut interrupted = false;
    while let Some(result) = stream.next().await {
        match result {
            Ok(bytes) => output.push_str(&String::from_utf8_lossy(&bytes)),
            Err(_) => { interrupted = true; break; }
        }
    }
    assert!(interrupted);
    output
}

async fn reject_without_dispatch(fixture: &Fixture, payload: &Value, code: &str, status: u16) -> RequestDiagnostics {
    let calls = fixture.calls();
    let before = budget::status(&fixture.db).unwrap();
    let health = serde_json::to_value(fixture.db.provider_health("gov-test").unwrap()).unwrap();
    let response = fixture.post().json(payload).send().await.unwrap();
    assert_eq!(response.status().as_u16(), status);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["type"], code);
    let after = budget::status(&fixture.db).unwrap();
    assert_eq!(fixture.calls(), calls);
    assert_eq!((after.attempts, after.reserved_tokens, after.reserved_microusd), (before.attempts, before.reserved_tokens, before.reserved_microusd));
    assert_eq!(serde_json::to_value(fixture.db.provider_health("gov-test").unwrap()).unwrap(), health);
    let (_, diagnostic) = latest_diagnostics(fixture);
    assert_eq!(diagnostic.failure_stage.as_deref(), Some("privacy"));
    assert_eq!(diagnostic.send_attempts, 0);
    assert!(!diagnostic.upstream_response_received);
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let local: (bool, i64, i64, f64) = connection.query_row(
        "SELECT affects_health,input_tokens,output_tokens,cost FROM request_events ORDER BY id DESC LIMIT 1", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!(local, (false, 0, 0, 0.0));
    diagnostic
}

#[tokio::test]
async fn privacy_outbound_text_and_opaque_provider_secret_are_redacted_before_real_dispatch() {
    let fixture = Fixture::new(vec![Reply::Error(503), Reply::Good], strict()).await;
    let secret = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
    let mut payload = body();
    payload["messages"][0]["content"] = json!(format!("你好 {secret} unit-upstream-key private@example.test 13712345678"));
    let response = fixture.post().json(&payload).send().await.unwrap();
    assert_eq!(response.status(), 200);
    response.bytes().await.unwrap();
    assert_eq!(fixture.calls(), 2);
    for captured in fixture.mock.captured.lock().unwrap().iter() {
        assert_eq!(captured.authorization.as_deref(), Some("Bearer unit-upstream-key"));
        let sent = captured.body.to_string();
        for private in [secret, "unit-upstream-key", "private@example.test", "13712345678"] { assert!(!sent.contains(private)); }
        assert!(sent.contains("你好"));
        assert!(sent.contains("[REDACTED:SECRET]"));
    }
    let (receipt, diagnostic) = latest_diagnostics(&fixture);
    let report = diagnostic.privacy.unwrap();
    assert_eq!((report.secret_redactions, report.personal_redactions), (2, 2));
    assert_eq!(diagnostic.send_attempts, 2);
    for private in [secret, "unit-upstream-key", "private@example.test", "13712345678", "你好"] { assert!(!receipt.contains(private)); }
    assert_eq!(budget::status(&fixture.db).unwrap().attempts, 2);
}

#[tokio::test]
async fn privacy_safe_tools_keep_names_identifiers_schemas_and_arguments() {
    let fixture = Fixture::new(vec![], strict()).await;
    let tools = json!([{"type":"function","function":{"name":"xy_health_echo","parameters":{"type":"object","properties":{"value":{"type":"string"},"password":{"type":"string"}}}}}]);
    let tool_calls = json!([{"id":"safe-call-1","type":"function","function":{"name":"xy_health_echo","arguments":"{\"value\":\"ordinary output\"}"}}]);
    let mut payload = body();
    payload["tools"] = tools.clone();
    payload["messages"] = json!([
        {"role":"user","content":"Check ordinary output"},
        {"role":"assistant","content":null,"tool_calls":tool_calls.clone()},
        {"role":"tool","tool_call_id":"safe-call-1","content":"ordinary output"},
    ]);
    let response = fixture.post().json(&payload).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let received = response.json::<Value>().await.unwrap();
    assert_eq!(received["choices"][0]["message"]["tool_calls"][0]["function"]["name"], "xy_health_echo");
    let captured = fixture.mock.captured.lock().unwrap();
    assert_eq!(captured[0].body["tools"], tools);
    assert_eq!(captured[0].body["messages"][1]["tool_calls"], tool_calls);
    assert_eq!(captured[0].body["messages"][2]["tool_call_id"], "safe-call-1");
    assert_eq!(latest_diagnostics(&fixture).1.privacy.unwrap(), privacy::PrivacyReport::default());
}

#[tokio::test]
async fn privacy_sensitive_tools_and_attachments_block_before_budget_or_provider_health_changes() {
    let fixture = Fixture::new(vec![], strict()).await;
    let tool = json!({"model":"gov-model","messages":[{"role":"assistant","tool_calls":[{"id":"private-call","type":"function","function":{"name":"send_mail","arguments":"{\"to\":\"private@example.test\"}"}}]}]});
    let diagnostic = reject_without_dispatch(&fixture, &tool, "privacy_sensitive_structure", 422).await;
    assert_eq!(diagnostic.privacy.unwrap().protected_fields, 1);
    let image = json!({"model":"gov-model","messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"data:image/png;base64,private-image"}}]}]});
    let diagnostic = reject_without_dispatch(&fixture, &image, "privacy_attachment_uninspected", 422).await;
    assert_eq!(diagnostic.privacy.unwrap().uninspected_attachments, 1);
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let diagnostic_text: String = connection.query_row("SELECT group_concat(diagnostics_json) FROM request_events", [], |row| row.get(0)).unwrap();
    assert!(!diagnostic_text.contains("private@example.test"));
    assert!(!diagnostic_text.contains("private-image"));
    assert_eq!(fixture.calls(), 0);
}

#[tokio::test]
async fn privacy_corrupt_policies_and_scan_limits_fail_closed_without_dispatch() {
    let fixture = Fixture::new(vec![], strict()).await;
    fixture.db.set_setting("privacy_settings", "{invalid}").unwrap();
    reject_without_dispatch(&fixture, &body(), "privacy_policy_unavailable", 503).await;
    privacy::save(&fixture.db, &PrivacySettings::default()).unwrap();
    let mut payload = body();
    payload["messages"][0]["content"] = json!("a@b.example ".repeat(4097));
    reject_without_dispatch(&fixture, &payload, "privacy_scan_limit", 422).await;
    assert_eq!(fixture.calls(), 0);
}

#[tokio::test]
async fn privacy_provider_body_overrides_cannot_reintroduce_secrets_or_sensitive_structures() {
    let fixture = Fixture::new(vec![], strict()).await;
    let mut provider = fixture.db.get_provider("gov-test").unwrap().unwrap();
    provider.advanced.body_overrides.insert("messages".into(), json!([{"role":"user","content":"unit-upstream-key private@example.test"}]));
    fixture.db.save_provider(&provider).unwrap();
    let response = fixture.post().send().await.unwrap();
    assert_eq!(response.status(), 200);
    response.bytes().await.unwrap();
    let sent = fixture.mock.captured.lock().unwrap()[0].body.to_string();
    assert!(!sent.contains("unit-upstream-key"));
    assert!(!sent.contains("private@example.test"));
    let report = latest_diagnostics(&fixture).1.privacy.unwrap();
    assert_eq!((report.secret_redactions, report.personal_redactions), (1, 1));
    provider.advanced.body_overrides.insert("metadata".into(), json!({"access_token":["opaque-overridden-secret"]}));
    let stored = serde_json::to_string(&provider.advanced).unwrap();
    fixture.db.conn().lock().unwrap().execute("UPDATE providers SET advanced_json=?1 WHERE id='gov-test'", [&stored]).unwrap();
    reject_without_dispatch(&fixture, &body(), "privacy_sensitive_structure", 422).await;
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn privacy_explicit_personal_and_attachment_choices_never_disable_credential_protection() {
    let fixture = Fixture::new(vec![], strict()).await;
    privacy::save(&fixture.db, &PrivacySettings { redact_personal_information: false, block_uninspected_attachments: false }).unwrap();
    let payload = json!({"model":"gov-model","messages":[{"role":"user","content":[{"type":"text","text":"unit-upstream-key private@example.test"},{"type":"image_url","image_url":{"url":"data:image/png;base64,allowed-image"}}]}],"max_tokens":16});
    let blocked = fixture.post().json(&payload).send().await.unwrap();
    assert_eq!(blocked.status(), 402);
    assert!(blocked.text().await.unwrap().contains("budget_unbounded_input"));
    assert_eq!(fixture.calls(), 0);
    let mut policy = fixture::budget(&fixture.db);
    policy.enabled = false;
    governance::save(&fixture.db, budget::KEY, &policy).unwrap();
    let response = fixture.post().json(&payload).send().await.unwrap();
    assert_eq!(response.status(), 200);
    response.bytes().await.unwrap();
    let captured = fixture.mock.captured.lock().unwrap();
    let sent = captured[0].body.to_string();
    assert!(!sent.contains("unit-upstream-key"));
    assert!(sent.contains("private@example.test"));
    assert!(sent.contains("allowed-image"));
    let report = latest_diagnostics(&fixture).1.privacy.unwrap();
    assert_eq!((report.secret_redactions, report.personal_redactions, report.uninspected_attachments), (1, 0, 1));
}

#[tokio::test]
async fn privacy_outbound_redaction_preserves_stream_framing_and_usage_accounting() {
    let fixture = Fixture::new(vec![], strict()).await;
    let mut payload = body();
    payload["stream"] = json!(true);
    payload["messages"][0]["content"] = json!("unit-upstream-key private@example.test");
    let response = fixture.post().json(&payload).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let stream = response.text().await.unwrap();
    assert!(stream.contains("data: [DONE]"));
    assert!(stream.contains("OK"));
    assert!(stream.contains("prompt_tokens"));
    let sent = fixture.mock.captured.lock().unwrap()[0].body.to_string();
    assert!(!sent.contains("unit-upstream-key"));
    assert!(!sent.contains("private@example.test"));
    let report = latest_diagnostics(&fixture).1.privacy.unwrap();
    assert_eq!((report.secret_redactions, report.personal_redactions), (1, 1));
    assert_eq!(budget::status(&fixture.db).unwrap().attempts, 1);
}

fn json_reply(status: u16, value: Value, request_id: Option<&'static str>) -> Reply {
    Reply::Raw { status, content_type: "application/json", chunks: vec![serde_json::to_vec(&value).unwrap()], request_id }
}

fn completion(message: Value) -> Value {
    json!({"id":"gov-completion","object":"chat.completion","model":"gov-model","choices":[{"index":0,"message":message,"finish_reason":"stop"}],
        "usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}})
}

fn stream_reply(chunks: Vec<Vec<u8>>) -> Reply {
    Reply::Raw { status: 200, content_type: "text/event-stream", chunks, request_id: None }
}

fn response_record(fixture: &Fixture) -> (bool, i64, i64, String) {
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    connection.query_row("SELECT affects_health,input_tokens,output_tokens,COALESCE(model,'') FROM request_events ORDER BY id DESC LIMIT 1", [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))).unwrap()
}

#[tokio::test]
async fn privacy_response_text_and_sensitive_headers_are_protected_over_real_sockets() {
    let value = completion(json!({"role":"assistant","content":"你好 unit-upstream-key private@example.test 13712345678"}));
    let fixture = Fixture::new(vec![json_reply(200, value, Some("unit-upstream-key"))], strict()).await;
    let response = fixture.post().send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers().get("x-request-id").is_none());
    let value = response.json::<Value>().await.unwrap();
    let text = value["choices"][0]["message"]["content"].as_str().unwrap();
    assert_eq!(text, "你好 [REDACTED:SECRET] [REDACTED:EMAIL] [REDACTED:PHONE]");
    assert_eq!(value["usage"]["total_tokens"], 5);
    let (receipt, diagnostic) = latest_diagnostics(&fixture);
    assert_eq!(diagnostic.privacy.unwrap().secret_redactions, 1);
    for private in ["unit-upstream-key", "private@example.test", "13712345678", "你好"] { assert!(!receipt.contains(private)); }
    assert_eq!(response_record(&fixture), (true, 4, 1, "gov-model".into()));
    assert_eq!(budget::status(&fixture.db).unwrap().attempts, 1);
}

#[tokio::test]
async fn privacy_response_error_json_and_plain_text_never_return_credentials() {
    for reply in [json_reply(400, json!({"error":{"message":"unit-upstream-key private@example.test"}}), Some("request-ok")),
        Reply::Raw { status: 400, content_type: "text/html", chunks: vec![b"unit-upstream-key private@example.test".to_vec()], request_id: Some("unit-upstream-key") }] {
        let fixture = Fixture::new(vec![reply], strict()).await;
        let response = fixture.post().send().await.unwrap();
        assert_eq!(response.status(), 400);
        assert_ne!(response.headers().get("x-request-id").and_then(|value| value.to_str().ok()), Some("unit-upstream-key"));
        assert_ne!(response.headers()[header::CONTENT_TYPE], "text/html");
        let text = response.text().await.unwrap();
        assert!(!text.contains("unit-upstream-key"));
        assert!(!text.contains("private@example.test"));
        assert!(text.contains("[REDACTED:SECRET]"));
        assert_eq!(fixture.calls(), 1);
        assert!(latest_diagnostics(&fixture).1.privacy.unwrap().secret_redactions > 0);
    }
}

#[tokio::test]
async fn privacy_response_sensitive_tool_arguments_keep_known_usage_and_neutral_health() {
    let value = completion(json!({"role":"assistant","content":null,"tool_calls":[{"id":"call-1","type":"function","function":{"name":"inspect","arguments":"{\"pass\\u0077ord\":\"opaque-private\"}"}}]}));
    let fixture = Fixture::new(vec![json_reply(200, value, None)], strict()).await;
    let response = fixture.post().send().await.unwrap();
    assert_eq!(response.status(), 502);
    let value = response.json::<Value>().await.unwrap();
    assert_eq!(value["error"]["type"], "privacy_response_sensitive_structure");
    assert!(!value.to_string().contains("opaque-private"));
    let (receipt, diagnostic) = latest_diagnostics(&fixture);
    assert_eq!(diagnostic.failure_stage.as_deref(), Some("response_privacy"));
    assert!(diagnostic.privacy.unwrap().protected_fields > 0);
    assert!(!receipt.contains("opaque-private"));
    assert_eq!(response_record(&fixture), (false, 4, 1, "gov-model".into()));
    assert_eq!(fixture.calls(), 1);
    assert_eq!(budget::status(&fixture.db).unwrap().attempts, 1);
}

#[tokio::test]
async fn privacy_response_stream_text_is_scanned_across_events_and_network_chunks() {
    let bytes = [chunk("unit-up").into_bytes(), chunk("stream-key private@").into_bytes(), chunk("example.test ").into_bytes(), terminal().into_bytes()].concat();
    let fixture = Fixture::new(vec![stream_reply(bytes.chunks(7).map(<[u8]>::to_vec).collect())], strict()).await;
    let mut payload = body(); payload["stream"] = json!(true);
    let response = fixture.post().json(&payload).send().await.unwrap();
    assert_eq!(response.status(), 200);
    let text = response.text().await.unwrap();
    assert!(!text.contains("unit-upstream-key"));
    assert!(!text.contains("private@example.test"));
    assert!(text.contains("[REDACTED:SECRET]"));
    assert!(text.contains("[REDACTED:EMAIL]"));
    assert!(text.contains("data: [DONE]"));
    assert_eq!(response_record(&fixture), (true, 4, 1, "gov-model".into()));
    assert!(latest_diagnostics(&fixture).1.privacy.unwrap().secret_redactions > 0);
}

#[tokio::test]
async fn privacy_response_stream_tool_parameters_are_checked_before_delivery() {
    let mut bytes = Vec::new();
    for arguments in ["{\"pass\\u0077", "ord\":\"opaque-private\"}"] {
        bytes.extend(format!("data: {}\n\n", json!({"model":"gov-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":arguments}}]},"finish_reason":null}],
            "usage":{"prompt_tokens":4,"completion_tokens":1}})).into_bytes());
    }
    bytes.extend(terminal().into_bytes());
    let fixture = Fixture::new(vec![stream_reply(vec![bytes])], strict()).await;
    let mut payload = body(); payload["stream"] = json!(true);
    let output = interrupted_response(fixture.post().json(&payload)).await;
    assert!(!output.contains("opaque-private"));
    assert!(!output.contains("pass\\u0077"));
    let (receipt, diagnostic) = latest_diagnostics(&fixture);
    assert_eq!(diagnostic.failure_stage.as_deref(), Some("response_privacy"));
    assert!(!receipt.contains("opaque-private"));
    assert_eq!(response_record(&fixture), (false, 4, 1, "gov-model".into()));
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn privacy_response_stream_sensitive_model_metadata_cannot_enter_audit_storage() {
    let bytes = format!("data: {}\n\n{}", json!({"id":"unit-upstream-key","model":"unit-upstream-key","choices":[{"index":0,"delta":{"content":"ordinary text"},"finish_reason":null}],
        "usage":{"prompt_tokens":4,"completion_tokens":1}}), terminal());
    let fixture = Fixture::new(vec![stream_reply(vec![bytes.into_bytes()])], strict()).await;
    let mut payload = body(); payload["stream"] = json!(true);
    let output = interrupted_response(fixture.post().json(&payload)).await;
    assert!(!output.contains("unit-upstream-key"));
    let (receipt, diagnostic) = latest_diagnostics(&fixture);
    assert_eq!(diagnostic.failure_stage.as_deref(), Some("response_privacy"));
    assert!(!receipt.contains("unit-upstream-key"));
    let record = response_record(&fixture);
    assert!(!record.0);
    assert_eq!((record.1, record.2), (4, 1));
    assert!(!record.3.contains("unit-upstream-key"));
}

#[tokio::test]
async fn privacy_response_enabled_audits_never_store_sensitive_models_or_diagnostic_ids() {
    for streaming in [false, true] {
        let reply = if streaming {
            Reply::Raw { status: 200, content_type: "text/event-stream", request_id: Some("unit-upstream-key"),
                chunks: vec![format!("data: {}\n\n{}", json!({"id":"unit-upstream-key","model":"unit-upstream-key",
                    "choices":[{"index":0,"delta":{"content":"ordinary text"},"finish_reason":null}],
                    "usage":{"prompt_tokens":4,"completion_tokens":1}}), terminal()).into_bytes()] }
        } else {
            let mut value = completion(json!({"role":"assistant","content":"ordinary text"}));
            value["id"] = json!("unit-upstream-key"); value["model"] = json!("unit-upstream-key");
            json_reply(200, value, Some("unit-upstream-key"))
        };
        let fixture = Fixture::new(vec![reply], strict()).await;
        fixture.db.verification.set_settings(crate::model_verification::VerificationSettings {
            enabled: true, store_diagnostic_ids: true, ..Default::default()
        }).unwrap();
        let mut payload = body(); payload["stream"] = json!(streaming);
        if streaming {
            assert!(!interrupted_response(fixture.post().json(&payload)).await.contains("unit-upstream-key"));
        } else {
            let response = fixture.post().json(&payload).send().await.unwrap();
            assert_eq!(response.status(), 502);
            assert_eq!(response.json::<Value>().await.unwrap()["error"]["type"], "privacy_response_sensitive_structure");
        }
        assert_eq!(latest_diagnostics(&fixture).1.failure_stage.as_deref(), Some("response_privacy"));
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let complete: bool = fixture.db.conn().lock().unwrap().query_row(
                    "SELECT EXISTS(SELECT 1 FROM verification_samples)", [], |row| row.get(0),
                ).unwrap();
                if complete { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("the real verification worker must persist the audited response");
        let connection = fixture.db.conn(); let connection = connection.lock().unwrap();
        let audit: String = connection.query_row("SELECT audit_json FROM request_events ORDER BY id DESC LIMIT 1", [], |row| row.get(0)).unwrap();
        let evidence: String = connection.query_row("SELECT evidence_json FROM verification_samples ORDER BY event_id DESC LIMIT 1", [], |row| row.get(0)).unwrap();
        for receipt in [audit, evidence] {
            for sensitive in ["unit-upstream-key", "ordinary text", "verificationCapture"] { assert!(!receipt.contains(sensitive)); }
        }
        let usage: (i64, i64) = connection.query_row("SELECT input_tokens,output_tokens FROM request_events ORDER BY id DESC LIMIT 1", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(usage, (4, 1));
    }
}

#[tokio::test]
async fn privacy_response_conversion_to_responses_keeps_redaction_and_usage() {
    for streaming in [false, true] {
        let reply = if streaming { stream_reply(vec![chunk("unit-up").into_bytes(), chunk("stream-key ").into_bytes(), terminal().into_bytes()]) }
            else { json_reply(200, completion(json!({"role":"assistant","content":"unit-upstream-key"})), None) };
        let fixture = Fixture::new(vec![reply], strict()).await;
        let response = client().post(format!("{}/v1/responses", fixture.base)).bearer_auth(&fixture.token)
            .header("x-xy-task-id", uuid::Uuid::new_v4().to_string())
            .json(&json!({"model":"gov-model","input":"OK","max_output_tokens":16,"stream":streaming})).send().await.unwrap();
        assert_eq!(response.status(), 200);
        let output = response.text().await.unwrap();
        assert!(!output.contains("unit-upstream-key"));
        assert!(output.contains("[REDACTED:SECRET]"));
        if streaming { assert!(output.contains("response.completed")); }
        assert_eq!(response_record(&fixture), (true, 4, 1, "gov-model".into()));
        assert_eq!(budget::status(&fixture.db).unwrap().attempts, 1);
    }
}

#[tokio::test]
async fn privacy_response_read_limits_block_oversized_real_upstream_bodies() {
    let reply = Reply::Raw { status: 200, content_type: "text/plain", chunks: vec![vec![b'x'; privacy::MAX_RESPONSE_BYTES + 1]], request_id: None };
    let fixture = Fixture::new(vec![reply], strict()).await;
    let response = fixture.post().send().await.unwrap();
    assert_eq!(response.status(), 502);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["type"], "privacy_response_scan_limit");
    assert_eq!(latest_diagnostics(&fixture).1.failure_stage.as_deref(), Some("response_privacy"));
    assert_eq!(fixture.calls(), 1);
}
