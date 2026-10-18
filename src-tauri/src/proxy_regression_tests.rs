#[test]
fn native_codex_urls_and_chat_routing_follow_the_same_protocol_contract() {
    for (endpoint, expected) in [
        ("https://api.x.ai/v1", "https://api.x.ai/v1"),
        ("https://relay.test/v1/responses", "https://relay.test/v1"),
        (
            "https://relay.test/custom/v4/responses?version=test",
            "https://relay.test/custom/v4?version=test",
        ),
        ("https://relay.test", "https://relay.test/v1"),
    ] {
        let mut p: Provider = serde_json::from_value(json!({"id":"native","name":"Native","appGroup":"Codex","endpoint":endpoint,"model":"test","enabled":true})).unwrap();
        p.advanced.upstream_format = "openai_responses".into();
        assert!(requires_codex_proxy(&p).unwrap());
        assert_eq!(codex_direct_base_url(&p).unwrap(), expected);
        p.advanced.upstream_format = "openai_chat".into();
        assert!(requires_codex_proxy(&p).unwrap());
    }
}

#[tokio::test]
async fn grok_and_kimi_chat_bridge_support_full_tool_sessions_over_http() {
    use std::sync::Mutex;
    async fn mock(
        State(seen): State<Arc<Mutex<Vec<Value>>>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> Response {
        assert_eq!(headers[header::AUTHORIZATION], "Bearer test-api-key");
        assert!(body.get("input").is_none());
        assert!(body.get("previous_response_id").is_none());
        let messages = body["messages"].as_array().unwrap();
        let returned = messages.iter().any(|m| m["role"] == "tool");
        if returned {
            let assistant = messages
                .iter()
                .find(|m| m.get("tool_calls").is_some())
                .unwrap();
            assert_eq!(assistant["tool_calls"][0]["id"], "call_read");
            assert_eq!(assistant["reasoning_content"], "Read workspace.");
            assert!(messages.iter().any(|m| m["role"] == "tool"
                && m["tool_call_id"] == "call_read"
                && m["content"] == "file contents"));
        }
        seen.lock().unwrap().push(body.clone());
        let message = if returned {
            json!({"role":"assistant","content":"File read successfully"})
        } else {
            json!({"role":"assistant","content":null,"reasoning_content":"Read workspace.","tool_calls":[{"index":0,"id":"call_read","type":"function","function":{"name":"read_file","arguments":"{\"path\":\"README.md\"}"}}]})
        };
        let reason = if returned { "stop" } else { "tool_calls" };
        if body["stream"] == true {
            let frames = [
                json!({"id":"chat_fixture","model":body["model"],"choices":[{"delta":message,"finish_reason":null}]}),
                json!({"choices":[{"delta":{},"finish_reason":reason}]}),
                json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5}}),
            ];
            let mut text = frames
                .iter()
                .map(|v| format!("data: {v}\n\n"))
                .collect::<String>();
            text.push_str("data: [DONE]\n\n");
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(text))
                .unwrap()
        } else {
            Json(json!({"id":"chat_fixture","model":body["model"],"choices":[{"message":message,"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}})).into_response()
        }
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let upstream_seen = seen.clone();
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(mock))
                .with_state(upstream_seen),
        )
        .await
    });
    let (db, path) = test_database();
    let state = ProxyState {
        database: db.clone(),
        client: reqwest::Client::new(),
        api_keys: Arc::new(FakeApiKeyProvider),
        local_addr: None,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, router_with_state(state)).await });
    let client = reqwest::Client::new();
    for provider in ["grok", "kimi"] {
        let mut p: Provider = serde_json::from_value(json!({"id":provider,"name":provider,"appGroup":"Codex","endpoint":format!("http://{addr}/v1"),"model":format!("{provider}-fixture"),"enabled":true})).unwrap();
        p.advanced.upstream_format = "openai_chat".into();
        db.save_provider(&p).unwrap();
        db.set_current_provider(provider).unwrap();
        for stream in [false, true] {
            let mut history = vec![json!({"role":"user","content":"Read the file"})];
            for turn in 0..2 {
                let response = client.post(format!("http://{proxy_addr}/v1/responses")).json(&json!({"model":"codex-alias","input":history,"stream":stream,"tools":[{"type":"function","name":"read_file","parameters":{"type":"object","properties":{"path":{"type":"string"}}}}]})).send().await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let body: Value = if stream {
                    let text = response.text().await.unwrap();
                    let events: Vec<Value> = text
                        .lines()
                        .filter_map(|l| l.strip_prefix("data: "))
                        .map(|d| serde_json::from_str(d).unwrap())
                        .collect();
                    assert!(
                        events
                            .iter()
                            .any(|e| e["type"] == "response.output_item.done")
                    );
                    events
                        .iter()
                        .find(|e| e["type"] == "response.completed")
                        .unwrap()["response"]
                        .clone()
                } else {
                    response.json().await.unwrap()
                };
                assert_eq!(body["model"], format!("{provider}-fixture"));
                assert_eq!(body["usage"]["total_tokens"], 15);
                let output = body["output"].as_array().unwrap();
                if turn == 0 {
                    assert!(
                        output
                            .iter()
                            .any(|i| i["type"] == "function_call" && i["call_id"] == "call_read")
                    );
                    history.extend(output.clone());
                    history.push(json!({"type":"function_call_output","call_id":"call_read","output":"file contents"}));
                } else {
                    assert!(output.iter().any(|i| i["type"] == "message"
                        && i["content"][0]["text"] == "File read successfully"));
                }
            }
        }
    }
    assert_eq!(seen.lock().unwrap().len(), 8);
    task.abort();
    let _ = task.await;
    upstream_task.abort();
    let _ = upstream_task.await;
    drop(db);
    std::fs::remove_file(path).unwrap();
}
fn model_audit_fixture(case: &str, stream: bool) -> String {
    let model = if case == "fault" {
        "cheap-model"
    } else {
        "configured-model"
    };
    let output = if matches!(case, "fault" | "reasoning" | "reasoning_missing") {
        100
    } else {
        2
    };
    let mut usage = json!({"prompt_tokens":3,"completion_tokens":output,"total_tokens":3+output,
        "completion_tokens_details":{"reasoning_tokens":if case == "reasoning" { 98 } else { 0 }}});
    if case == "fault" {
        usage["total_tokens"] = json!(900);
    }
    if case == "reasoning_missing" {
        usage
            .as_object_mut()
            .unwrap()
            .remove("completion_tokens_details");
    }
    let mut body = json!({"id":"audit-fixture","object":"chat.completion","model":model,
        "choices":[{"index":0,"message":{"role":"assistant","content":"你好"},"finish_reason":"stop"}],"usage":usage});
    if case == "missing_model" {
        body.as_object_mut().unwrap().remove("model");
    }
    if case == "missing_usage" {
        body.as_object_mut().unwrap().remove("usage");
    }
    if !stream {
        return serde_json::to_string(&body).unwrap();
    }
    let mut first = json!({"id":"audit-fixture","choices":[{"index":0,"delta":{"role":"assistant","content":"你好"}}]});
    if case != "missing_model" {
        first["model"] = json!(model);
    }
    let mut frames = format!("data: {first}\n\n");
    if case == "truncated" {
        return frames;
    }
    if case == "malformed" {
        frames.push_str("data: {broken\n\n");
        return frames;
    }
    if case == "error" {
        frames.push_str("data: {\"error\":{\"message\":\"fixture failure\"}}\n\n");
        return frames;
    }
    frames.push_str(
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
    );
    if case != "missing_usage" {
        frames.push_str(&format!(
            "data: {}\n\n",
            json!({"choices":[],"usage":body["usage"]})
        ));
    }
    frames.push_str("data: [DONE]\n\n");
    frames
}

#[tokio::test]
async fn shadow_audit_real_http_pairs_background_usage_without_persisting_bodies() {
    const ANSWER: &str = "PRIVATE_SHADOW_ASSISTANT";
    fn fixture(body: &Value) -> String {
        let messages = body["messages"].as_array().unwrap();
        let last = messages.last().unwrap()["content"].as_str().unwrap();
        let input = 100
            + messages
                .iter()
                .skip(1)
                .map(|message| message["content"].as_str().unwrap().len() as u64)
                .sum::<u64>()
            + (messages.len() as u64 / 2) * 8;
        let model = if last == "model changed" {
            "other-model"
        } else {
            "shadow-native"
        };
        let mut usage = json!({"prompt_tokens":input,"completion_tokens":5,"total_tokens":input+5,
            "prompt_tokens_details":{"cached_tokens":if last=="cache hit" {5} else {0}}});
        if last == "cache unknown" {
            usage
                .as_object_mut()
                .unwrap()
                .remove("prompt_tokens_details");
        }
        if body["stream"] == true {
            let frames = [
                json!({"model":model,"choices":[{"index":0,"delta":{"role":"assistant","content":ANSWER}}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                json!({"choices":[],"usage":usage}),
            ];
            format!(
                "{}data: [DONE]\n\n",
                frames
                    .iter()
                    .map(|frame| format!("data: {frame}\n\n"))
                    .collect::<String>()
            )
        } else {
            json!({"model":model,"choices":[{"message":{"role":"assistant","content":ANSWER},"finish_reason":"stop"}],"usage":usage}).to_string()
        }
    }
    async fn upstream(Json(body): Json<Value>) -> Response {
        Response::builder()
            .header(
                header::CONTENT_TYPE,
                if body["stream"] == true {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .body(Body::from(fixture(&body)))
            .unwrap()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/chat/completions", post(upstream)),
        )
        .await
    });
    let (db, path) = test_database();
    let mut provider: Provider = serde_json::from_value(
        json!({"id":"shadow","name":"Shadow fixture","appGroup":"Codex",
        "endpoint":format!("http://{upstream_addr}"),"model":"shadow-native","enabled":true}),
    )
    .unwrap();
    provider.advanced.upstream_format = "openai_chat".into();
    db.save_provider(&provider).unwrap();
    db.set_current_provider("shadow").unwrap();
    let tokenizer_path = crate::shadow_audit_runtime::byte_tokenizer_fixture();
    let settings = crate::shadow_audit_runtime::ShadowSettings {
        enabled: true,
        provider_id: "shadow".into(),
        model: "shadow-native".into(),
        tokenizer_path: tokenizer_path.to_string_lossy().into_owned(),
        pretokenizer: Some("cl100k".into()),
    };
    assert!(
        db.shadow
            .configure(settings.clone())
            .unwrap()
            .source_sha256
            .is_some()
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let state = ProxyState {
        database: db.clone(),
        client: reqwest::Client::new(),
        api_keys: Arc::new(FakeApiKeyProvider),
        local_addr: None,
    };
    let proxy_task =
        tokio::spawn(async move { axum::serve(listener, router_with_state(state)).await });
    let client = reqwest::Client::new();
    let mut expected_samples = 0_i64;
    for stream in [false, true] {
        let first =
            vec![json!({"role":"user","content":format!("PRIVATE_SHADOW_INITIAL_{stream}")})];
        let mut next = first.clone();
        next.extend([
            json!({"role":"assistant","content":ANSWER}),
            json!({"role":"user","content":"PRIVATE_SHADOW_NEXT"}),
        ]);
        let mut rewritten = next.clone();
        rewritten.extend([
            json!({"role":"assistant","content":"edited reply"}),
            json!({"role":"user","content":"PRIVATE_SHADOW_REWRITE"}),
        ]);
        let mut cases = vec![
            (first, "baseline", Some("shadow.no_parent")),
            (next.clone(), "paired", None),
            (rewritten, "excluded", Some("shadow.response_mismatch")),
        ];
        for (user, status, reason) in [
            ("model changed", "baseline", Some("shadow.no_parent")),
            ("cache unknown", "excluded", Some("shadow.cache_unknown")),
            ("cache hit", "excluded", Some("shadow.cache_present")),
            ("PRIVATE_SHADOW_FINAL", "paired", None),
        ] {
            let mut history = next.clone();
            history.extend([
                json!({"role":"assistant","content":ANSWER}),
                json!({"role":"user","content":user}),
            ]);
            cases.push((history, status, reason));
        }
        for (history, status, reason) in cases {
            let body = json!({"model":"client-alias","messages":history,"stream":stream});
            let response = client
                .post(format!("http://{proxy_addr}/v1/chat/completions"))
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.text().await.unwrap(), fixture(&body));
            expected_samples += 1;
            let started = Instant::now();
            let outcome = loop {
                let row = {
                    let arc = db.conn();
                    let conn = arc.lock().unwrap();
                    let count: i64 = conn
                        .query_row("SELECT COUNT(*) FROM shadow_audit_samples", [], |row| {
                            row.get(0)
                        })
                        .unwrap();
                    if count == expected_samples {
                        Some(conn.query_row("SELECT result_json FROM shadow_audit_samples ORDER BY id DESC LIMIT 1",[],|row|row.get::<_,String>(0)).unwrap())
                    } else {
                        None
                    }
                };
                if let Some(row) = row {
                    break serde_json::from_str::<Value>(&row).unwrap();
                }
                assert!(
                    started.elapsed() < Duration::from_secs(5),
                    "shadow worker did not persist sample {expected_samples}"
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            };
            assert_eq!(outcome["status"], status, "{outcome}");
            assert_eq!(outcome["reason"].as_str(), reason, "{outcome}");
            if status == "paired" {
                let history = body["messages"].as_array().unwrap();
                let x = ANSWER.len() + history.last().unwrap()["content"].as_str().unwrap().len();
                assert_eq!(outcome["pair"]["xLocalTokens"], x as u64);
                assert_eq!(outcome["pair"]["yInputDelta"], x as u64 + 8);
            }
            assert!(!outcome.to_string().contains("PRIVATE_SHADOW"));
        }
    }
    let report = serde_json::to_value(
        crate::shadow_audit_commands::analyze_database(&db, &crate::usage::UsageFilter::default())
            .unwrap(),
    )
    .unwrap();
    assert_eq!(report["pairCount"], 4);
    assert_eq!(report["sampleCount"], 14);
    assert!(
        report["groups"]
            .as_array()
            .unwrap()
            .iter()
            .all(|group| group["regression"]["status"] == "untested")
    );
    db.shadow
        .configure(crate::shadow_audit_runtime::ShadowSettings {
            enabled: false,
            ..settings
        })
        .unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    upstream_task.abort();
    let _ = upstream_task.await;
    drop(db);
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes
            .windows(b"PRIVATE_SHADOW".len())
            .any(|window| window == b"PRIVATE_SHADOW")
    );
    assert!(
        !bytes
            .windows(b"test-api-key".len())
            .any(|window| window == b"test-api-key")
    );
    std::fs::remove_file(tokenizer_path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn shadow_audit_responses_to_chat_http_uses_client_prefix_and_actual_reply() {
    const ANSWER: &str = "PRIVATE_RESP_REPLY";
    async fn upstream(Json(body): Json<Value>) -> Response {
        assert!(body.get("input").is_none());
        let messages = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|message| message["role"] != "system")
            .collect::<Vec<_>>();
        let input = 100
            + messages
                .iter()
                .skip(1)
                .map(|message| message["content"].as_str().unwrap().len() as u64)
                .sum::<u64>()
            + (messages.len() as u64 / 2) * 8;
        let usage = json!({"prompt_tokens":input,"completion_tokens":5,"total_tokens":input+5,"prompt_tokens_details":{"cached_tokens":0}});
        if body["stream"] == true {
            let frames = [
                json!({"model":"shadow-native","choices":[{"index":0,"delta":{"role":"assistant","content":ANSWER}}]}),
                json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]}),
                json!({"choices":[],"usage":usage}),
            ];
            Response::builder()
                .header(header::CONTENT_TYPE, "text/event-stream")
                .body(Body::from(format!(
                    "{}data: [DONE]\n\n",
                    frames
                        .iter()
                        .map(|frame| format!("data: {frame}\n\n"))
                        .collect::<String>()
                )))
                .unwrap()
        } else {
            Json(json!({"model":"shadow-native","choices":[{"message":{"role":"assistant","content":ANSWER},"finish_reason":"stop"}],"usage":usage})).into_response()
        }
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/chat/completions", post(upstream)),
        )
        .await
    });
    let (db, path) = test_database();
    let mut provider:Provider=serde_json::from_value(json!({"id":"shadow","name":"Shadow","appGroup":"Codex","endpoint":format!("http://{upstream_addr}"),"model":"shadow-native","enabled":true})).unwrap();
    provider.advanced.upstream_format = "openai_chat".into();
    db.save_provider(&provider).unwrap();
    db.set_current_provider("shadow").unwrap();
    let tokenizer_path = crate::shadow_audit_runtime::byte_tokenizer_fixture();
    let settings = crate::shadow_audit_runtime::ShadowSettings {
        enabled: true,
        provider_id: "shadow".into(),
        model: "shadow-native".into(),
        tokenizer_path: tokenizer_path.to_string_lossy().into_owned(),
        pretokenizer: Some("cl100k".into()),
    };
    db.shadow.configure(settings.clone()).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let state = ProxyState {
        database: db.clone(),
        client: reqwest::Client::new(),
        api_keys: Arc::new(FakeApiKeyProvider),
        local_addr: None,
    };
    let proxy_task =
        tokio::spawn(async move { axum::serve(listener, router_with_state(state)).await });
    let client = reqwest::Client::new();
    let mut sample_count = 0_i64;
    for stream in [false, true] {
        let initial = format!("PRIVATE_RESP_INITIAL_{stream}");
        let mut history = vec![json!({"role":"user","content":initial})];
        let mut stable_history = Vec::new();
        for (turn, expected) in ["baseline", "paired", "baseline", "paired"]
            .into_iter()
            .enumerate()
        {
            let body = json!({"model":"client-alias","instructions":"fixed system","input":history,"stream":stream});
            let response = client
                .post(format!("http://{proxy_addr}/v1/responses"))
                .json(&body)
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let text = response.text().await.unwrap();
            let result: Value = if stream {
                text.lines()
                    .filter_map(|line| line.strip_prefix("data: "))
                    .filter_map(|data| serde_json::from_str::<Value>(data).ok())
                    .find(|event| event["type"] == "response.completed")
                    .unwrap()["response"]
                    .clone()
            } else {
                serde_json::from_str(&text).unwrap()
            };
            let output = result["output"].as_array().unwrap();
            assert_eq!(output.len(), 1);
            assert_eq!(output[0]["content"][0]["text"], ANSWER);
            assert!(output[0]["id"].is_string());
            assert_eq!(output[0]["status"], "completed");
            sample_count += 1;
            let started = Instant::now();
            let outcome = loop {
                let saved = {
                    let arc = db.conn();
                    let conn = arc.lock().unwrap();
                    let count: i64 = conn
                        .query_row("SELECT COUNT(*) FROM shadow_audit_samples", [], |row| {
                            row.get(0)
                        })
                        .unwrap();
                    if count == sample_count {
                        Some(conn.query_row("SELECT result_json FROM shadow_audit_samples ORDER BY id DESC LIMIT 1",[],|row|row.get::<_,String>(0)).unwrap())
                    } else {
                        None
                    }
                };
                if let Some(saved) = saved {
                    break serde_json::from_str::<Value>(&saved).unwrap();
                }
                assert!(started.elapsed() < Duration::from_secs(5));
                tokio::time::sleep(Duration::from_millis(10)).await;
            };
            assert_eq!(
                outcome["status"], expected,
                "turn {turn}, stream={stream}: {outcome}"
            );
            if expected == "paired" {
                let user = body["input"].as_array().unwrap().last().unwrap()["content"]
                    .as_str()
                    .unwrap();
                let x = (ANSWER.len() + user.len()) as u64;
                assert_eq!(outcome["pair"]["xLocalTokens"], x);
                assert_eq!(outcome["pair"]["yInputDelta"], x + 8);
            }
            assert!(!outcome.to_string().contains("PRIVATE_RESP"));
            if turn == 2 {
                history = stable_history.clone();
            } else {
                history.extend(output.iter().cloned());
                history.push(json!({"role":"user","content":format!("PRIVATE_RESP_NEXT_{turn}")}));
            }
            if turn == 1 {
                stable_history = history.clone();
                // The adapter flattens these two equivalent containers to the
                // same Chat text. Raw client history still changed, so no pair.
                history[0]["content"] = json!([{"type":"input_text","text":initial}]);
            }
        }
    }
    db.shadow
        .configure(crate::shadow_audit_runtime::ShadowSettings {
            enabled: false,
            ..settings
        })
        .unwrap();
    proxy_task.abort();
    let _ = proxy_task.await;
    upstream_task.abort();
    let _ = upstream_task.await;
    drop(db);
    std::fs::remove_file(tokenizer_path).unwrap();
    std::fs::remove_file(path).unwrap();
}

#[tokio::test]
async fn model_audit_real_http_preserves_native_bytes_and_distinguishes_faults_and_gaps() {
    async fn upstream(Json(body): Json<Value>) -> Response {
        assert_eq!(body["model"], "configured-model");
        let case = body["messages"][0]["content"].as_str().unwrap();
        assert_eq!(
            body["max_tokens"],
            if matches!(case, "reasoning" | "reasoning_missing") {
                200
            } else {
                16
            }
        ); // Effective override.
        let stream = body["stream"] == true;
        Response::builder()
            .header(
                header::CONTENT_TYPE,
                if stream {
                    "text/event-stream"
                } else {
                    "application/json"
                },
            )
            .body(Body::from(model_audit_fixture(case, stream)))
            .unwrap()
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().route("/v1/chat/completions", post(upstream)),
        )
        .await
    });
    let (db, path) = test_database();
    db.verification.set_settings(crate::model_verification::VerificationSettings{enabled:true,..Default::default()}).unwrap();
    let mut provider: Provider = serde_json::from_value(
        json!({"id":"audit","name":"Audit fixture","appGroup":"Codex",
        "endpoint":format!("http://{upstream_addr}"),"model":"configured-model","enabled":true}),
    )
    .unwrap();
    provider.advanced.upstream_format = "openai_chat".into();
    provider
        .advanced
        .body_overrides
        .insert("max_tokens".into(), json!(16));
    db.save_provider(&provider).unwrap();
    db.set_current_provider("audit").unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let state = ProxyState {
        database: db.clone(),
        client: reqwest::Client::new(),
        api_keys: Arc::new(FakeApiKeyProvider),
        local_addr: None,
    };
    let proxy_task =
        tokio::spawn(async move { axum::serve(listener, router_with_state(state)).await });
    let client = reqwest::Client::new();
    for stream in [false, true] {
        for case in [
            "honest",
            "fault",
            "missing_model",
            "missing_usage",
            "reasoning",
            "reasoning_missing",
            "truncated",
            "malformed",
            "error",
        ] {
            if !stream && matches!(case, "truncated" | "malformed" | "error") {
                continue;
            }
            let effective_limit = if matches!(case, "reasoning" | "reasoning_missing") {
                200
            } else {
                16
            };
            provider
                .advanced
                .body_overrides
                .insert("max_tokens".into(), json!(effective_limit));
            db.save_provider(&provider).unwrap();
            let response = client.post(format!("http://{proxy_addr}/v1/chat/completions"))
                .json(&json!({"model":"client-alias","messages":[{"role":"user","content":case}],"stream":stream,"max_tokens":1000}))
                .send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.text().await.unwrap(),
                model_audit_fixture(case, stream),
                "native bytes changed for {case}"
            );
            wait_for_latest_verification(&db).await;
            let connection = rusqlite::Connection::open(&path).unwrap();
            let serialized: String = connection
                .query_row(
                    "SELECT audit_json FROM request_events ORDER BY id DESC LIMIT 1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            let record: crate::model_audit::AuditRecord =
                serde_json::from_str(&serialized).unwrap();
            assert_eq!(
                record.observation.requested_model.as_deref(),
                Some("client-alias")
            );
            assert_eq!(
                record.observation.forwarded_model.as_deref(),
                Some("configured-model")
            );
            assert_eq!(record.observation.max_tokens, Some(effective_limit));
            assert_eq!(record.observation.visible_output_bytes, Some(6));
            assert!(!serialized.contains("你好"));
            assert!(!serialized.contains("test-api-key"));
            match case {
                "fault" => {
                    for expected in [
                        "inv.model_mismatch",
                        "inv.byte_bound",
                        "inv.usage_selfcontradictory",
                        "inv.max_tokens_violated",
                    ] {
                        assert!(
                            record.flags.iter().any(|flag| flag == expected),
                            "missing {expected}: {serialized}"
                        );
                    }
                }
                "reasoning" => assert!(record.flags.is_empty()),
                "reasoning_missing" => {
                    assert!(!record.flags.contains(&"inv.byte_bound".into()));
                    assert!(record.flags.is_empty());
                    assert!(record.gaps.contains(&"byte_bound.reasoning_unknown".into()));
                }
                "missing_model" => {
                    assert_eq!(record.model_status, "unknown");
                    assert!(record.flags.is_empty());
                }
                "missing_usage" => {
                    assert!(record.observation.output_tokens.is_none());
                    assert!(record.flags.is_empty());
                    assert!(!record.gaps.is_empty());
                }
                "truncated" | "malformed" | "error" => {
                    assert!(!record.observation.complete);
                    assert!(record.gaps.contains(&"byte_bound.incomplete".into()));
                }
                _ => {
                    assert_eq!(record.model_status, "matched");
                    assert!(record.flags.is_empty());
                    assert!(record.observation.complete);
                }
            }
            if stream {
                assert_eq!(
                    record.observation.stream.as_ref().unwrap().text_delta_count,
                    1
                );
            }
        }
    }
    proxy_task.abort();
    upstream_task.abort();
    drop(db);
    let _ = std::fs::remove_file(path);
}

#[test]
fn model_audit_collects_native_protocol_accounting_and_all_choices() {
    use crate::model_audit::{Observation, audit};
    let cases = [
        (
            UpstreamFormat::OpenAi,
            json!({"model":"native","choices":[{"message":{"content":"你"},"finish_reason":"stop"},{"message":{"content":"好"},"finish_reason":"stop"}],"usage":{"prompt_tokens":2,"completion_tokens":5,"total_tokens":7,"completion_tokens_details":{"reasoning_tokens":1}}}),
        ),
        (
            UpstreamFormat::Responses,
            json!({"model":"native","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"你好"}]}],"usage":{"input_tokens":2,"output_tokens":5,"total_tokens":7,"output_tokens_details":{"reasoning_tokens":1}}}),
        ),
        (
            UpstreamFormat::Anthropic,
            json!({"model":"native","stop_reason":"end_turn","content":[{"type":"text","text":"你好"}],"usage":{"input_tokens":2,"output_tokens":5,"total_tokens":7,"reasoning_tokens":1}}),
        ),
        (
            UpstreamFormat::Gemini,
            json!({"modelVersion":"native","candidates":[{"content":{"parts":[{"text":"你好"},{"text":"hidden","thought":true}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":2,"candidatesTokenCount":4,"thoughtsTokenCount":1,"totalTokenCount":7}}),
        ),
    ];
    for (format, body) in cases {
        let observed = streaming::audit_capture::Collector::new(
            format,
            Observation {
                requested_model: Some("alias".into()),
                forwarded_model: Some("native".into()),
                max_tokens: Some(32),
                ..Default::default()
            },
        )
        .non_stream(&body);
        assert_eq!(observed.visible_output_bytes, Some(6));
        assert_eq!(observed.output_tokens, Some(5));
        assert_eq!(observed.reasoning_tokens, Some(1));
        assert!(observed.complete);
        assert!(audit(observed).flags.is_empty());
    }
    let tools = json!({"model":"native","choices":[{"message":{"content":null,"tool_calls":[{"type":"function","function":{"name":"lookup","arguments":"{\"city\":\"杭州\"}"}}]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":1,"completion_tokens":20,"total_tokens":21,"completion_tokens_details":{"reasoning_tokens":0}}});
    let observed =
        streaming::audit_capture::Collector::new(UpstreamFormat::OpenAi, Observation::default())
            .non_stream(&tools);
    assert_eq!(
        observed.tool_payload_bytes,
        Some(("lookup".len() + "{\"city\":\"杭州\"}".len()) as u64)
    );
    assert!(!audit(observed).flags.contains(&"inv.byte_bound".into()));
}

#[tokio::test]
async fn model_audit_cancellation_persists_partial_observation() {
    use futures_util::{StreamExt, stream};
    let (db, path) = test_database();
    db.verification.set_settings(crate::model_verification::VerificationSettings{enabled:true,..Default::default()}).unwrap();
    let provider: Provider = serde_json::from_value(
        json!({"id":"audit-cancel","name":"Audit","appGroup":"Codex",
        "endpoint":"http://127.0.0.1:1","model":"native","enabled":true}),
    )
    .unwrap();
    db.save_provider(&provider).unwrap();
    let event_id = record_request(
        &db,
        RequestRecord {
            provider_id: &provider.id,
            protocol: ClientProtocol::Chat,
            status: Some(StatusCode::OK),
            latency_ms: 0,
            retry_count: 0,
            error_kind: None,
            affects_health: true,
        },
    )
    .unwrap()
    .event_id;
    let bytes = Bytes::from_static(b"data: {\"model\":\"native\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"}}]}\n\n");
    let transport =
        stream::iter(vec![Ok::<_, reqwest::Error>(bytes.clone())]).chain(stream::pending());
    let mut observed = streaming::audit_capture::observe_stream(
        transport,
        UpstreamFormat::OpenAi,
        crate::model_audit::Observation {
            requested_model: Some("native".into()),
            verification_capture: db.verification.capture(&provider,"test-api-key","chat","native"),
            ..Default::default()
        },
        "native".into(),
        db.clone(),
        Some(event_id),
        Instant::now(),
    )
    .boxed();
    assert_eq!(observed.next().await.unwrap().unwrap(), bytes);
    drop(observed);
    wait_for_latest_verification(&db).await;
    let connection = rusqlite::Connection::open(&path).unwrap();
    let serialized: String = connection
        .query_row(
            "SELECT audit_json FROM request_events WHERE id=?1",
            [event_id],
            |r| r.get(0),
        )
        .unwrap();
    let record: crate::model_audit::AuditRecord = serde_json::from_str(&serialized).unwrap();
    assert!(!record.observation.complete);
    assert_eq!(record.observation.visible_output_bytes, Some(7));
    assert!(record.gaps.contains(&"byte_bound.incomplete".into()));
    assert!(record.observation.output_tokens.is_none());
    drop(connection);
    drop(db);
    let _ = std::fs::remove_file(path);
}

async fn wait_for_latest_verification(db:&Database) {
    for _ in 0..200 {
        let ready:bool=db.conn().lock().unwrap().query_row("SELECT audit_json IS NOT NULL FROM request_events ORDER BY id DESC LIMIT 1",[],|r|r.get(0)).unwrap_or(false);
        if ready {return;}
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("verification observation was not persisted");
}
