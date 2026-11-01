#[tokio::test]
async fn production_native_responses_preserves_hosted_and_deferred_tools() {
    use std::sync::Mutex;

    fn native_frames(response: &Value) -> String {
        let mut frames = vec![json!({"type":"response.created","response":{
            "id":response["id"],"model":response["model"],"status":"in_progress","output":[]
        }})];
        for (index, item) in response["output"].as_array().unwrap().iter().enumerate() {
            let mut added = item.clone();
            added["status"] = json!("in_progress");
            if item["type"] == "custom_tool_call" {
                added["input"] = json!("");
            }
            frames.push(
                json!({"type":"response.output_item.added","output_index":index,"item":added}),
            );
            if item["type"] == "custom_tool_call" {
                for delta in ["line\n", "中文"] {
                    frames.push(json!({"type":"response.custom_tool_call_input.delta",
                        "output_index":index,"item_id":item["id"],"delta":delta}));
                }
            }
            frames
                .push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
        }
        frames.push(json!({"type":"response.provider_extension","metadata":{"opaque":"保留"}}));
        frames.push(json!({"type":"response.completed","response":response}));
        let mut wire = ": native fixture\r\nretry: 1000\r\n\r\n".to_owned();
        for (sequence, mut event) in frames.into_iter().enumerate() {
            event["sequence_number"] = json!(sequence);
            wire.push_str(&format!(
                "id: native-{sequence}\r\nevent: {}\r\ndata: {event}\r\n\r\n",
                event["type"].as_str().unwrap()
            ));
        }
        wire
    }

    let output = json!([
        {"type":"custom_tool_call","id":"ct_native_passthrough","status":"completed",
            "call_id":"call_native_passthrough","namespace":"files","name":"patch","input":"line\n中文"},
        {"type":"tool_search_call","id":"ts_native_passthrough","status":"completed","arguments":{"query":"receipt"}},
        {"type":"reasoning","id":"rs_native_passthrough","summary":[],"encrypted_content":"fixture-encrypted-output"}
    ]);
    let seen = Arc::new(Mutex::new(Vec::<Value>::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_address = listener.local_addr().unwrap();
    let upstream_seen = seen.clone();
    let upstream_output = output.clone();
    let upstream_task = tokio::spawn(async move {
        axum::serve(listener, Router::new().route("/v1/responses", post(move |Json(body): Json<Value>| {
            let seen = upstream_seen.clone();
            let output = upstream_output.clone();
            async move {
                seen.lock().unwrap().push(body.clone());
                let response = json!({"id":"resp_native_passthrough","status":"completed","model":body["model"],
                    "output":output,"metadata":{"opaque":"保留"},
                    "usage":{"input_tokens":20,"input_tokens_details":{"cached_tokens":3},"output_tokens":8,"total_tokens":28}});
                if body["stream"] == true {
                    let wire = native_frames(&response);
                    let chunks = wire.as_bytes().chunks(7)
                        .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
                        .collect::<Vec<_>>();
                    Response::builder().header(header::CONTENT_TYPE,"text/event-stream")
                        .header("x-request-id","req-native-passthrough")
                        .body(Body::from_stream(futures_util::stream::iter(chunks))).unwrap()
                } else {
                    Json(response).into_response()
                }
            }
        }))).await.unwrap();
    });
    let database = crate::governance::test_support::database();
    let mut provider =
        crate::governance::test_support::provider(&format!("http://{upstream_address}"));
    provider.advanced.upstream_format = "openai_responses".into();
    database.save_provider(&provider).unwrap();
    database.set_current_provider(&provider.id).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_address = listener.local_addr().unwrap();
    let state = ProxyState {
        database: database.clone(),
        client: crate::network::test_client(),
        api_keys: Arc::new(FakeApiKeyProvider),
        local_addr: Some(proxy_address),
    };
    let proxy_task = tokio::spawn(async move {
        axum::serve(listener, router_with_targets(state, &["codex".into()]))
            .await
            .unwrap();
    });
    for stream in [false, true] {
        let payload = json!({"model":provider.model,"stream":stream,
            "input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"receipt"}]},
                {"type":"additional_tools","role":"developer","tools":[{"type":"namespace","name":"joint_probe",
                    "tools":[{"type":"function","name":"receipt","parameters":{"type":"object"},"defer_loading":true}]}]},
                {"type":"reasoning","id":"rs_native_passthrough","summary":[],"encrypted_content":"fixture-encrypted-content"}],
            "tools":[{"type":"tool_search"},{"type":"custom","name":"patch","format":{"type":"text"}},
                {"type":"namespace","name":"files","tools":[{"type":"function","name":"read","parameters":{"type":"object"}}]}],
            "client_metadata":{"fixture":"native-passthrough"}});
        let response = crate::network::test_client()
            .post(format!("http://{proxy_address}/v1/responses"))
            .json(&payload)
            .send()
            .await
            .unwrap();
        let status = response.status();
        if stream {
            assert_eq!(response.headers()["x-request-id"], "req-native-passthrough");
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/event-stream"
            );
        }
        let body = response.text().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(seen.lock().unwrap().last(), Some(&payload));
        let expected = json!({"id":"resp_native_passthrough","status":"completed","model":provider.model,
            "output":output,"metadata":{"opaque":"保留"},
            "usage":{"input_tokens":20,"input_tokens_details":{"cached_tokens":3},"output_tokens":8,"total_tokens":28}});
        let returned = if stream {
            assert_eq!(body, native_frames(&expected));
            body.lines()
                .filter_map(|line| line.strip_prefix("data: "))
                .filter_map(|data| serde_json::from_str::<Value>(data).ok())
                .find(|event| event["type"] == "response.completed")
                .unwrap()["response"]
                .clone()
        } else {
            serde_json::from_str::<Value>(&body).unwrap()
        };
        assert_eq!(returned, expected);
    }
    assert_eq!(seen.lock().unwrap().len(), 2);
    let connection = database.conn();
    let connection = connection.lock().unwrap();
    let mut statement = connection
        .prepare(
            "SELECT input_tokens,output_tokens,cache_read_tokens FROM request_events ORDER BY id",
        )
        .unwrap();
    let usage = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(usage, vec![(20, 8, 3), (20, 8, 3)]);
    proxy_task.abort();
    upstream_task.abort();
}

// Opt-in client compatibility test. All model replies are deterministic loopback fixtures.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; uses isolated CODEX_HOME and no cloud API"]
async fn codex_cli_creates_and_resumes_session_through_chat_bridge() {
    run_codex_fixture_session(CodexFixtureRoute::ChatBridge).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; isolated native Responses loopback fixture"]
async fn codex_cli_native_responses_creates_and_resumes_session() {
    run_codex_fixture_session(CodexFixtureRoute::NativeDirect).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; native Responses through the production budget proxy and loopback upstream"]
async fn codex_cli_native_responses_creates_and_resumes_session_through_budget_proxy() {
    run_codex_fixture_session(CodexFixtureRoute::NativeBudgetProxy).await;
}

#[derive(Clone, Copy)]
enum CodexFixtureRoute {
    ChatBridge,
    NativeDirect,
    NativeBudgetProxy,
}

async fn run_codex_fixture_session(route: CodexFixtureRoute) {
    use std::sync::Mutex;
    let native = !matches!(route, CodexFixtureRoute::ChatBridge);
    let hard_budget = matches!(route, CodexFixtureRoute::NativeBudgetProxy);
    #[derive(Clone)]
    struct FixtureUpstream {
        seen: Arc<Mutex<Vec<Value>>>,
        client_search: bool,
    }
    async fn mock(State(state): State<FixtureUpstream>, Json(body): Json<Value>) -> Response {
        state.seen.lock().unwrap().push(body.clone());
        let messages = body["messages"].as_array().unwrap();
        let has_result = |id: &str| {
            messages
                .iter()
                .any(|m| m["role"] == "tool" && m["tool_call_id"] == id)
        };
        let returned = has_result("call_e2e_patch");
        let message = if returned {
            json!({"role":"assistant","content":"XY_SWITCH_E2E_OK"})
        } else if has_result("call_e2e_goal") {
            assert!(
                body["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["function"]["name"] == "apply_patch")
            );
            // An invalid patch exercises the actual custom-tool parser without filesystem writes.
            let patch = "XY_SWITCH_INTENTIONALLY_INVALID_PATCH";
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_e2e_patch","type":"function","function":{"name":"apply_patch","arguments":json!({"input":patch}).to_string()}}]})
        } else {
            assert!(
                body["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|t| t["function"]["name"] == "get_goal")
            );
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"call_e2e_goal","type":"function","function":{"name":"get_goal","arguments":"{}"}}]})
        };
        let frames = [
            json!({"id":"chat_e2e","model":body["model"],"choices":[{"delta":message}]}),
            json!({"choices":[{"delta":{},"finish_reason":if returned {"stop"} else {"tool_calls"}}]}),
            json!({"choices":[],"usage":{"prompt_tokens":20,"completion_tokens":8}}),
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
    }
    async fn native_mock(
        State(state): State<FixtureUpstream>,
        Json(body): Json<Value>,
    ) -> Response {
        state.seen.lock().unwrap().push(body.clone());
        let input = body["input"].as_array().unwrap();
        let has_result = |id: &str| {
            input.iter().any(|i| {
                i["call_id"] == id
                    && matches!(
                        i["type"].as_str(),
                        Some("function_call_output" | "custom_tool_call_output")
                    )
            })
        };
        let searched = input
            .iter()
            .any(|item| item["type"] == "tool_search_output" && item["execution"] == "client");
        let item = if has_result("call_e2e_patch") {
            json!({"type":"message","id":"msg_native","role":"assistant","status":"completed","content":[{"type":"output_text","text":"XY_SWITCH_E2E_OK","annotations":[]}]})
        } else if has_result("call_e2e_goal") {
            json!({"type":"custom_tool_call","id":"ct_native","status":"completed","call_id":"call_e2e_patch","name":"apply_patch","input":"XY_SWITCH_INTENTIONALLY_INVALID_PATCH"})
        } else if state.client_search && !searched {
            assert!(
                body["tools"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|tool| tool["type"] == "tool_search" && tool["execution"] == "client")
            );
            json!({"type":"tool_search_call","id":"ts_native","status":"completed","call_id":"call_e2e_search","execution":"client","arguments":{"query":"spawn_agent","limit":1}})
        } else {
            json!({"type":"function_call","id":"fc_native","status":"completed","call_id":"call_e2e_goal","name":"get_goal","arguments":"{}"})
        };
        let id = format!("resp_{}", uuid::Uuid::new_v4().simple());
        let frames = [
            json!({"type":"response.created","response":{"id":id,"status":"in_progress","output":[]}}),
            json!({"type":"response.output_item.done","output_index":0,"item":item}),
            json!({"type":"response.completed","response":{"id":id,"status":"completed","model":body["model"],"output":[item],"usage":{"input_tokens":20,"output_tokens":8,"total_tokens":28}}}),
        ];
        let text = frames
            .iter()
            .enumerate()
            .map(|(n, v)| {
                let mut v = v.clone();
                v["sequence_number"] = json!(n);
                format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap())
            })
            .collect::<String>();
        Response::builder()
            .header(header::CONTENT_TYPE, "text/event-stream")
            .body(Body::from(text))
            .unwrap()
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(if hard_budget {
            "artifacts/responses-budget-20261102"
        } else {
            "artifacts/2026-09-21-regression"
        })
        .join(format!(
            "codex-e2e-{}-{}",
            if hard_budget {
                "native-budget"
            } else if native {
                "native"
            } else {
                "chat"
            },
            uuid::Uuid::new_v4()
        ));
    let home = root.join("home");
    let codex_home = home.join(".codex");
    let work = root.join("workspace");
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let database = Database::open(&root.join("test.db")).unwrap();
    for seeded in database.list_providers().unwrap() {
        database.delete_provider(&seeded.id).unwrap();
    }
    let seen = Arc::new(Mutex::new(Vec::new()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let upstream_seen = FixtureUpstream {
        seen: seen.clone(),
        client_search: hard_budget,
    };
    let upstream_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new()
                .route("/v1/chat/completions", post(mock))
                .route("/v1/responses", post(native_mock))
                .with_state(upstream_seen),
        )
        .await
    });
    let mut provider: Provider=serde_json::from_value(json!({"id":"kimi-e2e","name":"Kimi test","appGroup":"Codex","endpoint":format!("http://{addr}/v1"),"model":"kimi-fixture","enabled":true})).unwrap();
    provider.advanced.upstream_format = if native {
        "openai_responses"
    } else {
        "openai_chat"
    }
    .into();
    database.save_provider(&provider).unwrap();
    database.set_current_provider(&provider.id).unwrap();
    if hard_budget {
        crate::governance::test_support::native_budget(&database, &provider.model, 128_000, 512);
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let state = ProxyState {
        database: database.clone(),
        client: crate::network::test_client(),
        api_keys: Arc::new(FakeApiKeyProvider),
        local_addr: Some(proxy_addr),
    };
    let proxy_task =
        tokio::spawn(async move { axum::serve(listener, router_with_state(state)).await });
    let manager = crate::takeover::AppTakeoverManager::new(
        database.clone(),
        crate::proxy_manager::ProxyManager::new(database.clone()),
        home.clone(),
        root.join("data"),
    );
    if matches!(route, CodexFixtureRoute::NativeDirect) {
        let base = codex_direct_base_url(&provider).unwrap();
        // Same production config/catalog generators, fixture bearer token instead of OS credentials.
        let config = crate::takeover::managed_codex_config_internal(
            "",
            &base,
            "e2e-placeholder",
            &provider,
            &codex_home,
        )
        .unwrap();
        std::fs::write(codex_home.join("config.toml"), config).unwrap();
        crate::takeover::write_codex_model_catalog(&codex_home, &provider).unwrap();
        proxy_task.abort(); // Native mode must work with the bridge stopped.
    } else {
        manager
            .sync_codex_provider(&provider, Some(&format!("http://{proxy_addr}")))
            .unwrap();
    }
    if hard_budget {
        let config = std::fs::read_to_string(codex_home.join("config.toml"))
            .unwrap()
            .parse::<toml_edit::DocumentMut>()
            .unwrap();
        let model_provider = config["model_provider"].as_str().unwrap();
        assert_eq!(
            config["model_providers"][model_provider]["base_url"]
                .as_str()
                .unwrap(),
            format!("http://{proxy_addr}/v1")
        );
        assert_eq!(
            database
                .get_provider(&provider.id)
                .unwrap()
                .unwrap()
                .advanced
                .upstream_format,
            "openai_responses"
        );
    }
    let exe = std::env::var_os("XY_SWITCH_CODEX_EXE").unwrap_or_else(|| "codex".into());
    let mut session: Option<String> = None;
    for turn in 0..2 {
        let root = root.clone();
        let home = home.clone();
        let codex_home = codex_home.clone();
        let work = work.clone();
        let exe = exe.clone();
        let previous = session.clone();
        let output=tokio::task::spawn_blocking(move || {
            use std::process::{Command,Stdio};
            let stdout=root.join(format!("turn-{turn}.jsonl"));let stderr=root.join(format!("turn-{turn}.stderr"));
            let mut command=Command::new(exe);
            command.env_clear();
            for name in ["SystemRoot", "WINDIR", "COMSPEC", "PATH", "PATHEXT", "TEMP", "TMP"] {
                if let Some(value) = std::env::var_os(name) { command.env(name, value); }
            }
            let appdata = home.join("appdata");
            let localappdata = home.join("localappdata");
            std::fs::create_dir_all(&appdata).unwrap();
            std::fs::create_dir_all(&localappdata).unwrap();
            command.env("APPDATA", appdata).env("LOCALAPPDATA", localappdata);
            // Keep deterministic loopback traffic out of system/PAC proxies.
            command.env("NO_PROXY", "127.0.0.1,localhost,::1");
            command.env("CODEX_HOME",codex_home).env("HOME",&home).env("USERPROFILE",&home).env("OPENAI_API_KEY","e2e-placeholder").current_dir(&work);
            command.arg("exec");
            if let Some(id)=previous {command.arg("resume").arg(id);}
            command.args(["--skip-git-repo-check","--json"]);
            if turn == 0 { command.args(["--color","never"]); }
            if turn == 0 {command.args(["--sandbox","read-only"]);}
            command.args(["-c","approval_policy=\"never\"","-c","web_search=\"disabled\""]);
            command.arg(if turn == 0 {"Read the current goal using get_goal. Pass XY_SWITCH_INTENTIONALLY_INVALID_PATCH to apply_patch to verify tool input validation (do not write files). Then say XY_SWITCH_E2E_OK."} else {"Confirm the previous tool result and say XY_SWITCH_E2E_OK."});
            command.stdin(Stdio::null()).stdout(std::fs::File::create(&stdout).unwrap()).stderr(std::fs::File::create(&stderr).unwrap());
            let mut child=command.spawn().unwrap();let start=Instant::now();
            let status=loop {
                if let Some(status)=child.try_wait().unwrap(){break status;}
                if start.elapsed()>Duration::from_secs(45){let _=child.kill();let _=child.wait();panic!("Codex timed out; logs: {}",root.display());}
                std::thread::sleep(Duration::from_millis(100));
            };
            let output=std::fs::read_to_string(&stdout).unwrap();let errors=std::fs::read_to_string(&stderr).unwrap();
            assert!(status.success(),"Codex failed: {output}\n{errors}\nlogs: {}",root.display());
            output
        }).await.unwrap();
        assert!(output.contains("XY_SWITCH_E2E_OK"), "{output}");
        if let Some(expected) = &session {
            assert!(
                output
                    .lines()
                    .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                    .any(|v| v["thread_id"].as_str() == Some(expected.as_str())),
                "resume changed session identity"
            );
        }
        if turn == 0 {
            session = output
                .lines()
                .filter_map(|l| serde_json::from_str::<Value>(l).ok())
                .find_map(|v| {
                    v.get("thread_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                });
            assert!(
                session.is_some(),
                "client did not create a session: {output}"
            );
        }
    }
    let sessions = crate::sessions::SessionStore::new(&home, &codex_home).unwrap();
    let catalog = sessions.catalog().unwrap();
    assert_eq!(
        catalog.sessions.len(),
        1,
        "one resumed conversation, not two new ones"
    );
    let session_id = session.as_deref().unwrap();
    assert_eq!(catalog.sessions[0].session_id, session_id);
    let detail = sessions.detail("codex", session_id).unwrap();
    assert!(
        detail.messages.iter().any(|message| message.role == "tool"),
        "native tool results must be readable in Sessions"
    );
    assert!(
        detail
            .messages
            .iter()
            .filter(|message| message.role == "assistant"
                && message.content.contains("XY_SWITCH_E2E_OK"))
            .count()
            >= 2,
        "both original and resumed replies must remain visible"
    );
    std::fs::write(
        root.join("session-readback.json"),
        serde_json::to_vec_pretty(&detail).unwrap(),
    )
    .unwrap();
    let seen = seen.lock().unwrap();
    let replies: Vec<&Value> = seen
        .iter()
        .flat_map(|v| {
            v[if native { "input" } else { "messages" }]
                .as_array()
                .unwrap()
        })
        .collect();
    let tool_reply = |id: &str| {
        replies.iter().find(|m| {
            if native {
                m["call_id"] == id
                    && matches!(
                        m["type"].as_str(),
                        Some("function_call_output" | "custom_tool_call_output")
                    )
            } else {
                m["role"] == "tool" && m["tool_call_id"] == id
            }
        })
    };
    assert!(
        tool_reply("call_e2e_goal").is_some(),
        "no actual tool result returned"
    );
    let patch = tool_reply("call_e2e_patch").expect("custom tool result missing")
        [if native { "output" } else { "content" }]
    .as_str()
    .unwrap();
    assert!(
        patch.contains("patch") && !patch.contains("unsupported"),
        "custom tool parser did not execute: {patch}"
    );
    if hard_budget {
        let search_output = replies
            .iter()
            .find(|item| item["type"] == "tool_search_output" && item["execution"] == "client")
            .expect("actual client tool discovery result missing");
        let discovered = search_output["tools"].as_array().unwrap();
        assert!(
            discovered.iter().any(|tool| tool["type"] == "namespace"),
            "tool discovery must return native namespace definitions: {search_output}"
        );
        assert!(
            serde_json::to_string(discovered)
                .unwrap()
                .contains("spawn_agent")
        );
        assert!(
            !replies.iter().any(|item| item["type"] == "function_call"
                && item["name"]
                    .as_str()
                    .is_some_and(|name| name.contains("spawn_agent"))),
            "discovery must not execute a subagent"
        );
        let resumed = seen.last().unwrap();
        let resumed_input = resumed["input"].as_array().unwrap();
        assert!(
            resumed_input.iter().any(|item| item["role"] == "assistant"
                && item["content"].to_string().contains("XY_SWITCH_E2E_OK")),
            "resumed request must retain the original assistant reply"
        );
        assert!(
            resumed_input
                .iter()
                .any(|item| item["type"] == "custom_tool_call_output"
                    && item["call_id"] == "call_e2e_patch"),
            "resumed request must retain the custom-tool result"
        );
        assert!(
            resumed_input
                .iter()
                .any(|item| item["type"] == "tool_search_output"),
            "resumed request must retain the discovered schemas"
        );
        for request in &*seen {
            assert_eq!(request["max_output_tokens"], 512);
            assert!(request.get("previous_response_id").is_none());
            assert!(request.get("conversation").is_none());
        }
        let budget = crate::governance::budget::status(&database).unwrap();
        assert!(budget.policy.enabled);
        assert_eq!(budget.attempts, seen.len() as i64);
        assert_eq!(budget.reserved_tokens, seen.len() as i64 * 128_512);
        assert_eq!(budget.reserved_microusd, seen.len() as i64 * 129_024);
        let logs = crate::usage::query_usage_logs(
            &database.conn().lock().unwrap(),
            &crate::usage::UsageFilter::default(),
            100,
            0,
        )
        .unwrap();
        assert_eq!(
            logs.len(),
            seen.len(),
            "every CLI request must be recorded by the application proxy"
        );
        for log in &logs {
            assert!(log.success, "{log:?}");
            assert_eq!((log.input_tokens, log.output_tokens), (20, 8));
            let diagnostic = log.diagnostics.as_ref().unwrap();
            assert_eq!(diagnostic.send_attempts, 1);
            assert!(diagnostic.upstream_response_received);
            assert_eq!(diagnostic.failure_stage, None);
        }
        let searches = detail
            .messages
            .iter()
            .filter(|message| {
                message
                    .tool
                    .as_ref()
                    .is_some_and(|tool| tool.event_kind == crate::sessions::SessionToolKind::Search)
            })
            .count();
        assert!(
            searches > 0,
            "tool discovery must remain readable in the restored session"
        );
        let evidence = json!({"mode":"native-budget-proxy","sessionId":session_id,"upstreamRequests":seen.len(),
            "sameSessionResumed":true,"clientToolSearchReturned":true,"customToolParserExecuted":true,
            "toolSearchReadbackEvents":searches,"resumedInputItems":resumed_input.len(),"budget":budget,
            "requestLogs":logs,"newPaidApiRequests":0});
        std::fs::write(
            root.join("budget-proxy-evidence.json"),
            serde_json::to_vec_pretty(&evidence).unwrap(),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("mock-requests.json"),
        serde_json::to_vec_pretty(&*seen).unwrap(),
    )
    .unwrap();
    eprintln!(
        "Codex client session/tool/resume evidence: {}",
        root.display()
    );
    proxy_task.abort();
    upstream_task.abort();
}
