// Opt-in client compatibility test. All model replies are deterministic loopback fixtures.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; uses isolated CODEX_HOME and no cloud API"]
async fn codex_cli_creates_and_resumes_session_through_chat_bridge() {
    run_codex_fixture_session(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires an installed Codex CLI; isolated native Responses loopback fixture"]
async fn codex_cli_native_responses_creates_and_resumes_session() {
    run_codex_fixture_session(true).await;
}

async fn run_codex_fixture_session(native: bool) {
    use std::sync::Mutex;
    async fn mock(State(seen): State<Arc<Mutex<Vec<Value>>>>, Json(body): Json<Value>) -> Response {
        seen.lock().unwrap().push(body.clone());
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
        State(seen): State<Arc<Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> Response {
        seen.lock().unwrap().push(body.clone());
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
        let item = if has_result("call_e2e_patch") {
            json!({"type":"message","id":"msg_native","role":"assistant","status":"completed","content":[{"type":"output_text","text":"XY_SWITCH_E2E_OK","annotations":[]}]})
        } else if has_result("call_e2e_goal") {
            json!({"type":"custom_tool_call","id":"ct_native","status":"completed","call_id":"call_e2e_patch","name":"apply_patch","input":"XY_SWITCH_INTENTIONALLY_INVALID_PATCH"})
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
        .join("artifacts/2026-09-21-regression")
        .join(format!(
            "codex-e2e-{}-{}",
            if native { "native" } else { "chat" },
            uuid::Uuid::new_v4()
        ));
    let home = root.join("home");
    let codex_home = home.join(".codex");
    let work = root.join("workspace");
    std::fs::create_dir_all(&codex_home).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    let database = Database::open(&root.join("test.db")).unwrap();
    for mut seeded in database.list_providers().unwrap() {
        seeded.enabled = false;
        database.save_provider(&seeded).unwrap();
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
                .route("/v1/responses", post(native_mock))
                .with_state(upstream_seen),
        )
        .await
    });
    let mut provider: Provider=serde_json::from_value(json!({"id":"kimi-e2e","name":"Kimi test","appGroup":"Codex","endpoint":format!("http://{addr}/v1"),"model":"kimi-fixture","enabled":true})).unwrap();
    provider.advanced.upstream_format = "openai_chat".into();
    database.save_provider(&provider).unwrap();
    database.set_current_provider(&provider.id).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_addr = listener.local_addr().unwrap();
    let state = ProxyState {
        database: database.clone(),
        client: crate::network::test_client(),
        api_keys: Arc::new(FakeApiKeyProvider),
        local_addr: None,
    };
    let proxy_task =
        tokio::spawn(async move { axum::serve(listener, router_with_state(state)).await });
    let manager = crate::takeover::AppTakeoverManager::new(
        database.clone(),
        crate::proxy_manager::ProxyManager::new(database.clone()),
        home.clone(),
        root.join("data"),
    );
    if native {
        provider.advanced.upstream_format = "openai_responses".into();
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
    assert_eq!(catalog.sessions.len(), 1, "one resumed conversation, not two new ones");
    let session_id = session.as_deref().unwrap();
    assert_eq!(catalog.sessions[0].session_id, session_id);
    let detail = sessions.detail("codex", session_id).unwrap();
    assert!(detail.messages.iter().any(|message| message.role == "tool"), "native tool results must be readable in Sessions");
    assert!(detail.messages.iter().filter(|message| message.role == "assistant" && message.content.contains("XY_SWITCH_E2E_OK")).count() >= 2, "both original and resumed replies must remain visible");
    std::fs::write(root.join("session-readback.json"), serde_json::to_vec_pretty(&detail).unwrap()).unwrap();
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
