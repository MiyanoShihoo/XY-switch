use super::*;
use serde_json::{Value, json};

struct Fixture {
    root: PathBuf,
    home: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let root =
            std::env::temp_dir().join(format!("xy-switch-sessions-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let home = root.join("isolated home 会话");
        fs::create_dir_all(&home).unwrap();
        Self { root, home }
    }
    fn store(&self) -> SessionStore {
        SessionStore::new(&self.home, &self.home.join(".codex")).unwrap()
    }
    fn write(&self, relative: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let path = self.home.join(relative);
        assert!(path.starts_with(&self.home));
        assert!(
            !Path::new(relative)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
        );
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
    fn codex(&self, id: &str, events: Vec<Value>) -> PathBuf {
        self.write(
            &format!(".codex/sessions/2026/10/27/rollout-date-{id}.jsonl"),
            log(id, events),
        )
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let temp = std::env::temp_dir().canonicalize().unwrap();
        if self.root.parent() == Some(temp.as_path())
            && self
                .root
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("xy-switch-sessions-")
            && self.root.canonicalize().ok().as_ref() == Some(&self.root)
        {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}
fn log(id: &str, events: Vec<Value>) -> String {
    let mut records = vec![
        json!({"type":"session_meta","timestamp":"2026-10-26T16:00:00Z","payload":{"id":id,"cwd":"E:/test project"}}),
    ];
    records.extend(events);
    records.into_iter().map(|v| v.to_string() + "\n").collect()
}
fn pair(id: &str) -> SessionIdPair {
    SessionIdPair {
        provider_id: "codex".into(),
        session_id: id.into(),
    }
}
fn response(role: &str, content: &str) -> Value {
    json!({"type":"response_item","timestamp":"2026-10-26T16:00:01Z","payload":{"type":"message","role":role,"content":[{"type":"output_text","text":content}]}})
}

fn tool_response(payload: Value) -> Value {
    json!({"type":"response_item","payload":payload})
}

fn tool_event(payload: Value) -> Value {
    json!({"type":"event_msg","payload":payload})
}

#[test]
fn codex_tool_search_keeps_native_definitions_and_json_results_without_inventing_success() {
    let fixture = Fixture::new();
    let definitions = json!([{"type":"namespace","name":"receipt","tools":[{"type":"function","name":"record","parameters":{"type":"object"}}]}]);
    let output = " {\n  \"ok\": true, \"receipt\": \"工具结果\"\n} ";
    fixture.codex("search-tools", vec![
        tool_response(json!({"type":"tool_search_call","call_id":"search-a","status":"completed","execution":"client","arguments":{"query":"receipt"}})),
        tool_response(json!({"type":"tool_search_output","call_id":"search-a","status":"completed","execution":"client","tools":definitions})),
        tool_response(json!({"type":"function_call","name":"mcp__receipt__record","call_id":"call-a","arguments":"{}"})),
        tool_response(json!({"type":"function_call_output","call_id":"call-a","output":output})),
    ]);
    let detail = fixture.store().detail("codex", "search-tools").unwrap();
    assert_eq!(detail.messages.len(), 4);
    let search = detail.messages[0].tool.as_ref().unwrap();
    assert_eq!(search.event_kind, SessionToolKind::Search);
    assert_eq!(search.status, SessionToolStatus::Completed);
    assert_eq!(search.call_id.as_deref(), Some("search-a"));
    assert!(detail.messages[0].content.contains("receipt"));
    assert_eq!(serde_json::from_str::<Value>(&detail.messages[1].content).unwrap(), definitions);
    assert_eq!(detail.messages[3].content, output);
    for index in [2, 3] {
        let tool = detail.messages[index].tool.as_ref().unwrap();
        assert_eq!(tool.call_id.as_deref(), Some("call-a"));
        assert_eq!(tool.name.as_deref(), Some("mcp__receipt__record"));
        assert_eq!(tool.status, SessionToolStatus::ResultReceived);
    }
    let wire = serde_json::to_value(&detail.messages[3]).unwrap();
    assert_eq!(wire["tool"]["callId"], "call-a");
    assert_eq!(wire["tool"]["eventKind"], "result");
    assert_eq!(wire["tool"]["status"], "result_received");
    assert!(wire["tool"].get("call_id").is_none());
}

#[test]
fn codex_approval_and_exit_status_enrich_primary_events_and_keep_exact_result_content() {
    let fixture = Fixture::new();
    let output = "{\"native\":\"result\"}";
    fixture.codex("approved-tools", vec![
        tool_response(json!({"type":"function_call","call_id":"exec-a","name":"functions.exec_command","arguments":"{\"cmd\":\"echo test\"}"})),
        tool_event(json!({"type":"exec_approval_request","call_id":"exec-a","reason":"Local test approval"})),
        tool_event(json!({"type":"exec_approval_response","call_id":"exec-a","decision":"approved"})),
        tool_event(json!({"type":"exec_command_begin","call_id":"exec-a","command":["echo","test"]})),
        tool_event(json!({"type":"exec_command_end","call_id":"exec-a","exit_code":0,"aggregated_output":"runtime representation"})),
        tool_response(json!({"type":"function_call_output","call_id":"exec-a","output":output})),
    ]);
    let detail = fixture.store().detail("codex", "approved-tools").unwrap();
    assert_eq!(detail.messages.len(), 4);
    assert_eq!(detail.messages[1].tool.as_ref().unwrap().status, SessionToolStatus::PendingApproval);
    assert_eq!(detail.messages[2].tool.as_ref().unwrap().status, SessionToolStatus::Approved);
    assert_eq!(detail.messages[3].content, output);
    for index in [0, 3] {
        let tool = detail.messages[index].tool.as_ref().unwrap();
        assert_eq!(tool.status, SessionToolStatus::Succeeded);
        assert_eq!(tool.name.as_deref(), Some("functions.exec_command"));
    }
}

#[test]
fn denied_tools_and_empty_outputs_remain_visible_without_execution_success() {
    let fixture = Fixture::new();
    fixture.codex("denied-tools", vec![
        tool_response(json!({"type":"custom_tool_call","call_id":"denied-a","name":"custom","input":"local task"})),
        tool_event(json!({"type":"apply_patch_approval_request","call_id":"denied-a","reason":"Confirm change"})),
        tool_event(json!({"type":"apply_patch_approval_response","call_id":"denied-a","decision":"denied"})),
        tool_response(json!({"type":"custom_tool_call","call_id":"empty-a","name":"custom","input":"empty output"})),
        tool_response(json!({"type":"custom_tool_call_output","call_id":"empty-a","output":""})),
    ]);
    let detail = fixture.store().detail("codex", "denied-tools").unwrap();
    assert_eq!(detail.messages.len(), 5);
    assert_eq!(detail.messages[0].tool.as_ref().unwrap().status, SessionToolStatus::Denied);
    assert_eq!(detail.messages[4].content, "");
    assert_eq!(detail.messages[4].tool.as_ref().unwrap().status, SessionToolStatus::ResultReceived);
    assert!(!detail.messages.iter().any(|message| message.tool.as_ref().is_some_and(|tool| tool.status == SessionToolStatus::Succeeded)));
}

#[test]
fn mcp_runtime_errors_are_evidence_but_arbitrary_result_json_is_not() {
    let fixture = Fixture::new();
    fixture.codex("mcp-tools", vec![
        tool_response(json!({"type":"function_call","call_id":"mcp-a","name":"mcp__local__check","arguments":"{}"})),
        tool_event(json!({"type":"mcp_tool_call_begin","call_id":"mcp-a","invocation":{"server":"local","tool":"check","arguments":{}}})),
        tool_event(json!({"type":"mcp_tool_call_end","call_id":"mcp-a","invocation":{"server":"local","tool":"check"},"result":{"Ok":{"content":[{"type":"text","text":"failed"}],"isError":true}}})),
        tool_response(json!({"type":"function_call_output","call_id":"mcp-a","output":"{\"ok\":true}"})),
        tool_event(json!({"type":"mcp_tool_call_end","call_id":"mcp-b","result":{"Err":"transport failure"}})),
        tool_event(json!({"type":"mcp_tool_call_end","call_id":"mcp-c","result":{"Ok":{"content":[{"type":"text","text":"done"}]}}})),
    ]);
    let detail = fixture.store().detail("codex", "mcp-tools").unwrap();
    assert_eq!(detail.messages.len(), 4);
    assert_eq!(detail.messages[1].content, "{\"ok\":true}");
    for message in &detail.messages[..3] {
        assert_eq!(message.tool.as_ref().unwrap().status, SessionToolStatus::Failed);
    }
    assert_eq!(detail.messages[3].tool.as_ref().unwrap().status, SessionToolStatus::ResultReceived);
}

#[test]
fn identical_actual_tool_calls_survive_primary_and_runtime_occurrence_deduplication() {
    let fixture = Fixture::new();
    let mut events = Vec::new();
    for id in ["repeat-a", "repeat-b", "repeat-a"] {
        events.push(tool_response(json!({"type":"function_call","call_id":id,"name":"exec_command","arguments":"{}"})));
        events.push(tool_event(json!({"type":"exec_command_begin","call_id":id,"command":[]})));
        events.push(tool_event(json!({"type":"exec_command_end","call_id":id,"exit_code":1,"aggregated_output":"done"})));
        events.push(tool_response(json!({"type":"function_call_output","call_id":id,"output":"done"})));
    }
    fixture.codex("repeat-tools", events);
    let detail = fixture.store().detail("codex", "repeat-tools").unwrap();
    assert_eq!(detail.messages.len(), 6);
    assert_eq!(detail.messages.iter().filter(|message| message.tool.as_ref().unwrap().event_kind == SessionToolKind::Call).count(), 3);
    assert_eq!(detail.messages.iter().filter(|message| message.tool.as_ref().unwrap().event_kind == SessionToolKind::Result).count(), 3);
    assert!(detail.messages.iter().all(|message| message.tool.as_ref().unwrap().status == SessionToolStatus::Failed));
}

#[test]
fn claude_mixed_text_calls_and_results_have_separate_ids_and_explicit_outcomes() {
    let fixture = Fixture::new();
    let records = [
        json!({"type":"assistant","sessionId":"claude-trace","message":{"role":"assistant","content":[
            {"type":"text","text":"Before"},{"type":"tool_use","id":"claude-a","name":"read_file","input":{"path":"local.txt"}},{"type":"text","text":"After"}
        ]}}),
        json!({"type":"user","sessionId":"claude-trace","message":{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"claude-a","content":"{\"value\":1}","is_error":false},{"type":"text","text":"Continue"}
        ]}}),
    ];
    fixture.write(".claude/projects/qa/claude-trace.jsonl", records.iter().map(|record| record.to_string()+"\n").collect::<String>());
    let detail = fixture.store().detail("claude", "claude-trace").unwrap();
    assert_eq!(detail.messages.len(), 5);
    assert_eq!(detail.messages[0].content, "Before");
    assert!(detail.messages[0].tool.is_none());
    assert_eq!(detail.messages[2].content, "After");
    assert_eq!(detail.messages[3].content, "{\"value\":1}");
    assert_eq!(detail.messages[3].role, "tool");
    for index in [1, 3] {
        let tool = detail.messages[index].tool.as_ref().unwrap();
        assert_eq!(tool.call_id.as_deref(), Some("claude-a"));
        assert_eq!(tool.name.as_deref(), Some("read_file"));
        assert_eq!(tool.status, SessionToolStatus::Succeeded);
    }
    assert_eq!(fixture.store().catalog().unwrap().sessions[0].title.as_deref(), Some("Continue"));
}

#[test]
fn reused_tool_ids_keep_each_name_outcome_and_partial_runtime_evidence_separate() {
    let fixture = Fixture::new();
    fixture.codex("reused-outcomes", vec![
        tool_response(json!({"type":"function_call","call_id":"reused","name":"first","arguments":"{}"})),
        tool_event(json!({"type":"exec_command_begin","call_id":"reused","command":[]})),
        tool_event(json!({"type":"exec_command_end","call_id":"reused","exit_code":1,"aggregated_output":"runtime first"})),
        tool_response(json!({"type":"function_call_output","call_id":"reused","output":"first result"})),
        tool_response(json!({"type":"function_call","call_id":"reused","name":"second","arguments":"{}"})),
        tool_response(json!({"type":"function_call_output","call_id":"reused","output":"second result"})),
        tool_response(json!({"type":"function_call","call_id":"reused","name":"third","arguments":"{}"})),
        tool_event(json!({"type":"exec_command_begin","call_id":"reused","command":[]})),
        tool_response(json!({"type":"function_call_output","call_id":"reused","output":"third result"})),
        tool_event(json!({"type":"exec_command_end","call_id":"reused","exit_code":0,"aggregated_output":"runtime third"})),
    ]);
    let detail = fixture.store().detail("codex", "reused-outcomes").unwrap();
    assert_eq!(detail.messages.len(), 6);
    for (pair, name, outcome) in [
        (0, "first", SessionToolStatus::Failed),
        (2, "second", SessionToolStatus::ResultReceived),
        (4, "third", SessionToolStatus::Succeeded),
    ] {
        for message in &detail.messages[pair..pair + 2] {
            let tool = message.tool.as_ref().unwrap();
            assert_eq!(tool.name.as_deref(), Some(name));
            assert_eq!(tool.status, outcome);
        }
        assert_eq!(detail.messages[pair + 1].content, format!("{name} result"));
    }
}

#[test]
fn multiple_results_share_a_call_without_sharing_unrelated_outcome_evidence() {
    let fixture = Fixture::new();
    fixture.codex("multiple-results", vec![
        tool_response(json!({"type":"function_call","call_id":"multiple","name":"check","arguments":"{}"})),
        tool_event(json!({"type":"exec_command_begin","call_id":"multiple","command":[]})),
        tool_event(json!({"type":"exec_command_end","call_id":"multiple","exit_code":1,"aggregated_output":"runtime output"})),
        tool_response(json!({"type":"function_call_output","call_id":"multiple","output":"first output"})),
        tool_response(json!({"type":"function_call_output","call_id":"multiple","output":"{\"ok\":true}"})),
    ]);
    let detail = fixture.store().detail("codex", "multiple-results").unwrap();
    assert_eq!(detail.messages.len(), 3);
    assert_eq!(detail.messages[0].tool.as_ref().unwrap().status, SessionToolStatus::Failed);
    assert_eq!(detail.messages[1].tool.as_ref().unwrap().status, SessionToolStatus::Failed);
    assert_eq!(detail.messages[2].tool.as_ref().unwrap().status, SessionToolStatus::ResultReceived);
    assert_eq!(detail.messages[2].content, "{\"ok\":true}");
}

#[test]
fn hosted_mcp_discovery_approvals_and_explicit_errors_keep_distinct_identifiers() {
    let fixture = Fixture::new();
    fixture.codex("hosted-mcp", vec![
        tool_response(json!({"type":"mcp_list_tools","id":"discovery","status":"completed","tools":[{"name":"lookup"}]})),
        tool_response(json!({"type":"mcp_approval_request","id":"approval-a","name":"lookup","arguments":"{}"})),
        tool_response(json!({"type":"mcp_approval_response","id":"response-a","approval_request_id":"approval-a","approve":true})),
        tool_response(json!({"type":"mcp_call","id":"hosted-a","approval_request_id":"approval-a","name":"lookup","arguments":"{}","status":"completed","output":"exact hosted result"})),
        tool_response(json!({"type":"mcp_call","id":"failed-a","name":"lookup","arguments":"{}","status":"completed","error":{"message":"recorded error"}})),
        tool_response(json!({"type":"mcp_call","id":"failed-without-output","name":"lookup","status":"failed"})),
        tool_response(json!({"type":"mcp_approval_request","id":"orphan","name":"lookup","arguments":"{}"})),
        tool_response(json!({"type":"mcp_approval_response","approval_request_id":"orphan","approve":false})),
    ]);
    let detail = fixture.store().detail("codex", "hosted-mcp").unwrap();
    assert_eq!(detail.messages.len(), 10);
    assert_eq!(detail.messages[0].tool.as_ref().unwrap().status, SessionToolStatus::Completed);
    for index in [1, 2, 3, 4] {
        let tool = detail.messages[index].tool.as_ref().unwrap();
        assert_eq!(tool.call_id.as_deref(), Some("hosted-a"));
        assert_eq!(tool.approval_request_id.as_deref(), Some("approval-a"));
        assert_eq!(tool.name.as_deref(), Some("lookup"));
    }
    assert_eq!(detail.messages[1].tool.as_ref().unwrap().status, SessionToolStatus::PendingApproval);
    assert_eq!(detail.messages[2].tool.as_ref().unwrap().status, SessionToolStatus::Approved);
    assert_eq!(detail.messages[4].content, "exact hosted result");
    for message in &detail.messages[5..8] {
        assert_eq!(message.tool.as_ref().unwrap().status, SessionToolStatus::Failed);
    }
    for message in &detail.messages[8..] {
        let tool = message.tool.as_ref().unwrap();
        assert_eq!(tool.call_id, None);
        assert_eq!(tool.approval_request_id.as_deref(), Some("orphan"));
    }
    assert_eq!(detail.messages[9].tool.as_ref().unwrap().status, SessionToolStatus::Denied);
}

#[test]
fn ambiguous_hosted_approval_references_do_not_invent_a_call_association() {
    let fixture = Fixture::new();
    fixture.codex("ambiguous-approval", vec![
        tool_response(json!({"type":"mcp_approval_request","id":"shared","name":"lookup","arguments":"{}"})),
        tool_response(json!({"type":"mcp_call","id":"first","approval_request_id":"shared","name":"lookup"})),
        tool_response(json!({"type":"mcp_call","id":"second","approval_request_id":"shared","name":"lookup"})),
        tool_event(json!({"type":"mcp_tool_call_end","call_id":"explicit-error","result":{"Ok":{"status":"completed","isError":true,"content":[{"type":"text","text":"recorded failure"}]}}})),
    ]);
    let detail = fixture.store().detail("codex", "ambiguous-approval").unwrap();
    assert_eq!(detail.messages.len(), 4);
    assert_eq!(detail.messages[0].tool.as_ref().unwrap().call_id, None);
    assert_eq!(detail.messages[3].tool.as_ref().unwrap().status, SessionToolStatus::Failed);
}

#[test]
fn gemini_tool_ids_use_recorded_status_and_keep_missing_outcome_unknown() {
    let fixture = Fixture::new();
    fixture.write(".gemini/tmp/qa/chats/session-trace.json", json!({"sessionId":"gemini-trace","messages":[
        {"type":"gemini","toolCalls":[
            {"id":"gemini-a","name":"check","args":{},"result":{"ok":true}},
            {"id":"gemini-b","name":"check","args":{},"status":"error","result":"failed"}
        ]}
    ]}).to_string());
    let detail = fixture.store().detail("gemini", "gemini-trace").unwrap();
    assert_eq!(detail.messages.len(), 4);
    assert_eq!(detail.messages[1].content, "{\"ok\":true}");
    assert_eq!(detail.messages[0].tool.as_ref().unwrap().status, SessionToolStatus::ResultReceived);
    assert_eq!(detail.messages[2].tool.as_ref().unwrap().status, SessionToolStatus::Failed);
    assert_eq!(detail.messages[3].tool.as_ref().unwrap().call_id.as_deref(), Some("gemini-b"));
}

#[test]
fn tool_metadata_limits_do_not_create_truncated_id_associations() {
    let fixture = Fixture::new();
    let excessive = "x".repeat(513);
    fixture.codex("bounded-tool-ids", vec![
        tool_response(json!({"type":"function_call","call_id":excessive,"name":"n".repeat(257),"arguments":"{}"})),
        tool_response(json!({"type":"function_call_output","call_id":"x".repeat(513),"output":"ok"})),
    ]);
    let detail = fixture.store().detail("codex", "bounded-tool-ids").unwrap();
    assert_eq!(detail.messages.len(), 2);
    assert!(detail.messages.iter().all(|message| message.tool.as_ref().unwrap().call_id.is_none()));
    assert!(detail.messages[0].tool.as_ref().unwrap().name.is_none());
    assert_eq!(detail.messages[0].tool.as_ref().unwrap().status, SessionToolStatus::Requested);
    assert!(!detail.warnings.is_empty());
}

#[test]
fn tool_metadata_and_associated_names_count_toward_display_and_message_budgets() {
    let first = tool_response(json!({"type":"function_call_output","call_id":"a".repeat(512),"name":"n".repeat(256),"output":"x".repeat(MAX_MESSAGE_BYTES / 2 - 100)}));
    let second = tool_response(json!({"type":"function_call_output","call_id":"b".repeat(512),"name":"n".repeat(256),"output":"x".repeat(MAX_MESSAGE_BYTES / 2 - 100)}));
    let raw = log("tool-byte-budget", vec![first, second]);
    let detail = parsers::parse(Client::Codex, Path::new("rollout-date-tool-byte-budget.jsonl"), raw.as_bytes(), false, true).unwrap().detail;
    assert_eq!(detail.messages.len(), 1);
    assert!(!detail.warnings.is_empty());
    let bytes = detail.messages.iter().map(|message| message.content.len() + serde_json::to_string(message.tool.as_ref().unwrap()).unwrap().len()).sum::<usize>();
    assert!(bytes <= MAX_MESSAGE_BYTES);
    let raw = log("tool-count-budget", (0..=MAX_MESSAGES).map(|index| tool_response(json!({"type":"function_call_output","call_id":format!("call-{index}"),"output":""}))).collect());
    let detail = parsers::parse(Client::Codex, Path::new("rollout-date-tool-count-budget.jsonl"), raw.as_bytes(), false, true).unwrap().detail;
    assert_eq!(detail.messages.len(), MAX_MESSAGES);
    assert!(!detail.warnings.is_empty());
    let legacy: SessionMessage = serde_json::from_value(json!({"role":"tool","content":"old result"})).unwrap();
    assert!(legacy.tool.is_none());
}

#[test]
#[ignore = "reads previously saved native acceptance logs; makes no network or model calls"]
fn saved_native_acceptance_tool_history_reads_back_without_new_api_requests() {
    let root = PathBuf::from(std::env::var("XY_SWITCH_SESSION_ACCEPTANCE_ROOT").expect("saved acceptance root is required"));
    for mode in ["chat", "responses"] {
        let home = root.join(mode).join("home").canonicalize().unwrap();
        let store = SessionStore::new(&home, &home.join(".codex")).unwrap();
        let catalog = store.catalog().unwrap();
        assert_eq!(catalog.sessions.len(), 1);
        let session = &catalog.sessions[0];
        let detail = store.detail("codex", &session.session_id).unwrap();
        assert!(detail.warnings.is_empty());
        let transcript = fs::read_to_string(session.source_path.as_ref().unwrap()).unwrap();
        let mut calls = 0;
        let mut searches = 0;
        let mut results = 0;
        for line in transcript.lines() {
            let value: Value = serde_json::from_str(line).unwrap();
            if value["type"] != "response_item" { continue; }
            let payload = &value["payload"];
            if payload["type"] == "tool_search_call" { searches += 1; }
            if payload["type"] == "function_call" { calls += 1; }
            if payload["type"] != "function_call_output" { continue; }
            results += 1;
            let matching = detail.messages.iter().filter(|message| message.tool.as_ref().is_some_and(|tool| tool.event_kind == SessionToolKind::Result && tool.call_id.as_deref() == payload["call_id"].as_str())).collect::<Vec<_>>();
            assert_eq!(matching.len(), 1);
            let expected = match &payload["output"] {
                Value::String(output) => output.clone(),
                Value::Array(blocks) => blocks.iter().map(|block| block["text"].as_str().unwrap()).collect::<Vec<_>>().join("\n"),
                _ => panic!("unexpected saved native result representation"),
            };
            assert!(matching[0].content == expected);
            assert!(matching[0].tool.as_ref().unwrap().name.is_some());
            assert!(matching[0].tool.as_ref().unwrap().status == SessionToolStatus::ResultReceived);
        }
        assert!(calls >= 2 && results == calls);
        assert_eq!(detail.messages.iter().filter(|message| message.tool.as_ref().is_some_and(|tool| tool.event_kind == SessionToolKind::Search)).count(), searches);
        eprintln!("saved native {mode}: searches={searches}, calls={calls}, exact results={results}");
    }
}

#[test]
fn empty_installation_is_empty_without_production_fallback() {
    let f = Fixture::new();
    let catalog = f.store().catalog().unwrap();
    assert!(catalog.sessions.is_empty());
    assert!(catalog.warnings.is_empty());
    assert!(SessionStore::new(Path::new("relative"), &f.home).is_err());
}

#[test]
fn codex_catalog_detail_and_tools_are_real_and_camel_case() {
    let f = Fixture::new();
    f.codex("id-one", vec![
        response("user", "修复测试问题"), response("assistant", "已修复"),
        json!({"type":"response_item","payload":{"type":"function_call","name":"check","arguments":"{}"}}),
        json!({"type":"response_item","payload":{"type":"function_call_output","output":"passed"}}),
        json!({"type":"response_item","payload":{"type":"reasoning","encrypted_content":"not-for-rendering"}}),
    ]);
    f.write(".codex/auth.json", "not a transcript");
    let catalog = f.store().catalog().unwrap();
    assert_eq!(catalog.sessions.len(), 1);
    let session = &catalog.sessions[0];
    assert_eq!(session.title.as_deref(), Some("修复测试问题"));
    assert_eq!(session.project_dir.as_deref(), Some("E:/test project"));
    assert_eq!(
        session.resume_command.as_deref(),
        Some("codex resume id-one")
    );
    let wire = serde_json::to_value(session).unwrap();
    assert_eq!(wire["providerId"], "codex");
    assert!(wire.get("provider_id").is_none());
    let detail = f.store().detail("codex", "id-one").unwrap();
    assert_eq!(detail.messages.len(), 4);
    assert_eq!(detail.messages[3].role, "tool");
    assert_eq!(detail.messages[3].content, "passed");
    assert!(
        !serde_json::to_string(&detail)
            .unwrap()
            .contains("not-for-rendering")
    );
}

#[test]
fn codex_event_fallback_deduplicates_occurrences_without_losing_repeated_turns() {
    let f = Fixture::new();
    f.codex("repeated", vec![
        json!({"type":"event_msg","payload":{"type":"user_message","message":"again"}}), response("user", "again"),
        json!({"type":"event_msg","payload":{"type":"user_message","message":"again"}}), response("user", "again"),
        json!({"type":"event_msg","payload":{"type":"agent_message","message":"done"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"done"}}),
        json!({"type":"event_msg","payload":{"type":"task_complete","last_agent_message":"final only"}}),
    ]);
    let messages = f.store().detail("codex", "repeated").unwrap().messages;
    assert_eq!(
        messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>(),
        ["again", "again", "done", "final only"]
    );
}

#[test]
fn claude_and_gemini_native_formats_are_read_without_client_config() {
    let f = Fixture::new();
    let claude = [
        json!({"type":"user","sessionId":"claude-one","cwd":"/work/中文","timestamp":"2026-10-27T00:00:00+08:00","message":{"role":"user","content":"hello Claude"}}),
        json!({"type":"assistant","sessionId":"claude-one","message":{"role":"assistant","content":[{"type":"thinking","thinking":"hidden"},{"type":"text","text":"hello"}]}}),
    ].into_iter().map(|v| v.to_string()+"\n").collect::<String>();
    f.write(".claude/projects/project/claude-one.jsonl", claude);
    f.write(".gemini/tmp/project/chats/session-today.json", json!({
        "sessionId":"gemini-one","startTime":"2026-10-26T16:00:00Z","lastUpdated":"2026-10-26T16:01:00Z",
        "messages":[{"type":"user","content":"hello Gemini"},{"type":"gemini","content":[{"text":"hello"}],"thoughts":[{"text":"hidden"}],"toolCalls":[{"name":"test","args":{},"result":{"ok":true}}]}]
    }).to_string());
    let catalog = f.store().catalog().unwrap();
    assert_eq!(catalog.sessions.len(), 2);
    assert!(catalog.warnings.is_empty(), "{:?}", catalog.warnings);
    let claude = f.store().detail("claude", "claude-one").unwrap();
    assert_eq!(claude.messages.len(), 2);
    assert_eq!(claude.messages[1].content, "hello");
    let gemini = f.store().detail("gemini", "gemini-one").unwrap();
    assert_eq!(gemini.messages.len(), 4);
    assert_eq!(gemini.messages[3].role, "tool");
    assert!(!serde_json::to_string(&gemini).unwrap().contains("hidden"));
}

#[test]
fn archived_codex_and_custom_codex_home_are_supported() {
    let f = Fixture::new();
    let custom = f.home.join("custom-codex");
    f.write(
        "custom-codex/archived_sessions/rollout-date-archived.jsonl",
        log("archived", vec![response("user", "archive")]),
    );
    let store = SessionStore::new(&f.home, &custom).unwrap();
    assert_eq!(store.catalog().unwrap().sessions[0].session_id, "archived");
    assert_eq!(
        store.detail("codex", "archived").unwrap().messages[0].content,
        "archive"
    );
    assert!(f.store().catalog().unwrap().sessions.is_empty());
}

#[test]
fn malformed_tail_and_middle_are_reported_without_losing_valid_messages() {
    let f = Fixture::new();
    let data = log("partial", vec![response("user", "first")])
        + "not-json\n"
        + &response("assistant", "last").to_string()
        + "\n{\"partial\":\"";
    f.write(".codex/sessions/rollout-date-partial.jsonl", data);
    let detail = f.store().detail("codex", "partial").unwrap();
    assert_eq!(detail.messages.len(), 2);
    assert_eq!(detail.warnings.len(), 1);
    assert!(detail.warnings[0].contains('2'));
}

#[test]
fn unsafe_ids_unknown_clients_and_path_arguments_are_rejected() {
    let f = Fixture::new();
    for id in [
        "", "..", "../x", "a/b", "a\\b", "-option", "a;calc", "$(calc)", "x\ncmd", "中文",
    ] {
        assert!(f.store().detail("codex", id).is_err(), "{id:?}");
        assert!(f.store().delete(&[pair(id)]).is_err(), "{id:?}");
    }
    assert!(f.store().detail("opencode", "id-one").is_err());
    assert!(
        serde_json::from_value::<SessionIdPair>(
            json!({"providerId":"codex","sessionId":"ok","sourcePath":"outside"})
        )
        .is_err()
    );
    assert!(
        serde_json::from_value::<SessionIdPair>(json!({"providerId":"codex","sessionId":"ok"}))
            .is_ok()
    );
}

#[test]
fn mismatched_and_duplicate_ids_fail_closed() {
    let f = Fixture::new();
    f.write(
        ".codex/sessions/rollout-date-wrong.jsonl",
        log("actual", vec![]),
    );
    assert!(f.store().catalog().unwrap().sessions.is_empty());
    let first = f.codex("duplicate", vec![]);
    let second = f.write(
        ".codex/archived_sessions/rollout-date-duplicate.jsonl",
        log("duplicate", vec![]),
    );
    let catalog = f.store().catalog().unwrap();
    assert_eq!(catalog.sessions.len(), 1);
    assert!(!catalog.warnings.is_empty());
    assert!(f.store().detail("codex", "duplicate").is_err());
    assert!(f.store().delete(&[pair("duplicate")]).is_err());
    assert!(first.exists() && second.exists());
}

#[test]
fn delete_preflights_the_entire_batch_and_counts_unique_files() {
    let f = Fixture::new();
    let first = f.codex("first", vec![]);
    let second = f.codex("second", vec![]);
    let unrelated = f.write(".codex/settings.json", "keep");
    assert!(f.store().delete(&[pair("first"), pair("missing")]).is_err());
    assert!(first.exists() && second.exists());
    assert_eq!(
        f.store().delete(&[pair("first"), pair("first")]).unwrap(),
        1
    );
    assert!(!first.exists());
    assert!(second.exists());
    assert_eq!(fs::read_to_string(unrelated).unwrap(), "keep");
    assert!(f.store().delete(&[]).is_err());
    assert!(
        f.store()
            .delete(&vec![pair("second"); MAX_DELETE_BATCH + 1])
            .is_err()
    );
}

#[test]
fn source_changes_and_outside_paths_are_rejected_before_delete() {
    let f = Fixture::new();
    let path = f.codex("changed", vec![]);
    let store = f.store();
    let (entries, _) = store.scan().unwrap();
    fs::write(&path, log("changed", vec![response("user", "new")])).unwrap();
    assert!(verify_snapshot(&entries[0]).is_err());
    assert!(
        safe_path(
            &f.home.join(".codex/sessions").canonicalize().unwrap(),
            &f.root,
            false
        )
        .is_err()
    );
}

#[test]
fn oversized_logs_remain_discoverable_but_do_not_allocate_unbounded_detail() {
    let f = Fixture::new();
    let path = f.codex("large", vec![]);
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len((MAX_FILE_BYTES + 1) as u64)
        .unwrap();
    assert_eq!(f.store().catalog().unwrap().sessions[0].session_id, "large");
    assert!(
        f.store()
            .detail("codex", "large")
            .unwrap_err()
            .contains("MiB")
    );
}

#[test]
fn display_budget_is_explicit_and_does_not_panic_on_large_content() {
    let bytes = log(
        "bounded",
        vec![
            response("user", "begin"),
            response("assistant", &"x".repeat(MAX_MESSAGE_BYTES)),
        ],
    );
    let result = parsers::parse(
        Client::Codex,
        Path::new("rollout-date-bounded.jsonl"),
        bytes.as_bytes(),
        false,
        true,
    )
    .unwrap();
    assert_eq!(result.detail.messages[0].content, "begin");
    assert!(!result.detail.warnings.is_empty());
}

#[cfg(any(unix, windows))]
#[test]
fn linked_directories_never_extend_the_scan_or_delete_boundary() {
    let f = Fixture::new();
    let outside = f.home.join("outside");
    f.write(
        "outside/rollout-date-outside.jsonl",
        log("outside", vec![response("user", "must not read")]),
    );
    let root = f.home.join(".codex/sessions");
    fs::create_dir_all(&root).unwrap();
    let link = root.join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, &link).unwrap();
    #[cfg(windows)]
    assert!(
        std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(f.store().catalog().unwrap().sessions.is_empty());
    assert!(f.store().delete(&[pair("outside")]).is_err());
    assert!(outside.join("rollout-date-outside.jsonl").exists());
}

#[test]
fn strict_iso_parser_handles_offsets_fractional_seconds_and_bad_unicode() {
    use crate::session_usage_codex::parse_iso_timestamp_ms as parse;
    let utc = parse("2026-10-26T16:00:00Z").unwrap();
    assert_eq!(parse("2026-10-27T00:00:00+08:00"), Some(utc));
    assert_eq!(parse("2026-10-26T21:30:00+0530"), Some(utc));
    assert_eq!(
        parse("2026-10-26T16:00:00.12345678901234567890123456789Z"),
        Some(utc + 123)
    );
    assert_eq!(parse("2026-10-26T16:00:00.1Z"), Some(utc + 100));
    assert!(parse("2024-02-29T00:00:00Z").is_some());
    for value in [
        "2026-02-29T00:00:00Z",
        "2026-13-01T00:00:00Z",
        "2026-01-00T00:00:00Z",
        "2026-01-01T24:00:00Z",
        "2026-01-01T00:60:00Z",
        "2026-01-01T00:00:60Z",
        "2026/01/01T00:00:00Z",
        "2026-01-01T00:00:00oops",
        "2026-01-01T00:00:00.Z",
        "2026-01-01T00:00:00+999999999999999999:00",
        "2026-01-01T00:00:00+24:00",
        "2026-01-01T00:00:00+08:60",
        "2026-01-01T00:00:00+€1",
        "2026-01-01T00:00:00+😀",
        "2026-01-01T00:00:00+08ignored",
    ] {
        assert_eq!(parse(value), None, "{value}");
    }
}

#[test]
fn claude_tool_results_keep_their_role_and_never_become_the_title() {
    let fixture = Fixture::new();
    let records = [
        json!({"type":"user","sessionId":"claude-tools","message":{"role":"user","content":[
            {"type":"tool_result","tool_use_id":"tool-1","content":[{"type":"text","text":"Build succeeded"}]},
            {"type":"text","text":"Now review the output"},
            {"type":"tool_result","tool_use_id":"tool-2","content":"No errors"},
            {"type":"text","text":"Continue"}
        ]}}),
        json!({"type":"assistant","sessionId":"claude-tools","message":{"role":"assistant","content":[
            {"type":"thinking","thinking":"not conversation text"},
            {"type":"tool_use","id":"tool-3","name":"read_file","input":{"path":"test.rs"}}
        ]}}),
    ];
    fixture.write(
        ".claude/projects/qa/claude-tools.jsonl",
        records
            .iter()
            .map(|v| v.to_string() + "\n")
            .collect::<String>(),
    );
    let catalog = fixture.store().catalog().unwrap();
    assert_eq!(
        catalog.sessions[0].title.as_deref(),
        Some("Now review the output")
    );
    let detail = fixture.store().detail("claude", "claude-tools").unwrap();
    let roles: Vec<_> = detail.messages.iter().map(|m| m.role.as_str()).collect();
    assert_eq!(roles, ["tool", "user", "tool", "user", "assistant"]);
    assert_eq!(detail.messages[0].content, "Build succeeded");
    assert!(detail.messages[4].content.contains("read_file"));
    assert!(
        !detail
            .messages
            .iter()
            .any(|m| m.content.contains("not conversation text"))
    );
}

#[test]
fn codex_renamed_titles_use_latest_index_entry_without_discovering_paths() {
    let f = Fixture::new();
    f.codex("renamed", vec![response("user", "original title")]);
    let records = [
        json!({"id":"renamed","thread_name":"最新标题","updated_at":"2026-10-27T03:00:00Z"}),
        json!({"id":"renamed","thread_name":"stale","updated_at":"2026-10-26T03:00:00Z"}),
        json!({"id":"unknown","thread_name":"not a transcript","sourcePath":"../secret.jsonl"}),
        json!({"id":"../escape","thread_name":"invalid"}),
    ].into_iter().map(|v| v.to_string()+"\n").collect::<String>();
    let index = f.write(".codex/session_index.jsonl", &records);
    let catalog = f.store().catalog().unwrap();
    assert_eq!(catalog.sessions.len(), 1);
    assert_eq!(catalog.sessions[0].title.as_deref(), Some("最新标题"));
    assert!(catalog.warnings.is_empty());
    assert_eq!(f.store().detail("codex", "renamed").unwrap().messages[0].content, "original title");
    assert_eq!(f.store().delete(&[pair("renamed")]).unwrap(), 1);
    assert_eq!(fs::read_to_string(index).unwrap(), records);
}

#[test]
fn damaged_and_oversized_title_index_does_not_hide_transcripts() {
    let f = Fixture::new();
    f.codex("safe", vec![response("user", "fallback")]);
    f.write(".codex/session_index.jsonl", "not json\n");
    let catalog = f.store().catalog().unwrap();
    assert_eq!(catalog.sessions.len(), 1);
    assert_eq!(catalog.warnings.len(), 1);
    f.write(".codex/session_index.jsonl", vec![b'x'; MAX_INDEX_BYTES + 1]);
    let catalog = f.store().catalog().unwrap();
    assert_eq!(catalog.sessions.len(), 1);
    assert!(catalog.warnings[0].contains("标题索引不可用"));
    // A title-only index is not part of deletion's transcript preflight.
    assert_eq!(f.store().delete(&[pair("safe")]).unwrap(), 1);
}

#[test]
fn title_index_rejects_control_characters_and_preserves_unicode() {
    let f = Fixture::new();
    f.codex("unicode", vec![response("user", "original")]);
    let records = [json!({"id":"unicode","thread_name":"  安全 emoji 🐈  "}),
        json!({"id":"unicode","thread_name":"bad\u{0000}title"}),
        json!({"id":"unicode","thread_name":"x".repeat(1025)})]
        .into_iter().map(|v| v.to_string()+"\n").collect::<String>();
    f.write(".codex/session_index.jsonl", records);
    assert_eq!(f.store().catalog().unwrap().sessions[0].title.as_deref(), Some("安全 emoji 🐈"));
}

#[test]
fn explicit_claude_and_gemini_roots_never_fall_back_to_default_roots() {
    let f = Fixture::new();
    let claude = f.home.join("custom-claude");
    let gemini = f.home.join("custom-home");
    let record = json!({"type":"user","sessionId":"custom-claude","message":{"role":"user","content":"custom Claude"}}).to_string()+"\n";
    f.write("custom-claude/projects/p/custom-claude.jsonl", record);
    f.write("custom-home/.gemini/tmp/p/chats/session-custom.json", json!({"sessionId":"custom-gemini","messages":[{"type":"user","content":"custom Gemini"}]}).to_string());
    let store = SessionStore::with_overrides(&f.home, &f.home.join(".codex"), Some(&claude), Some(&gemini)).unwrap();
    let catalog = store.catalog().unwrap();
    assert_eq!(catalog.sessions.len(), 2);
    assert!(catalog.warnings.is_empty(), "{:?}", catalog.warnings);
    assert_eq!(store.detail("claude", "custom-claude").unwrap().messages[0].content, "custom Claude");
    assert_eq!(store.detail("gemini", "custom-gemini").unwrap().messages[0].content, "custom Gemini");
    assert!(f.store().catalog().unwrap().sessions.is_empty());
    let missing = f.home.join("not-created");
    assert!(SessionStore::with_overrides(&f.home, &f.home.join(".codex"), Some(&missing), Some(&missing)).unwrap().catalog().unwrap().sessions.is_empty());
}

#[test]
fn custom_roots_reject_relative_paths_and_parent_components() {
    let f = Fixture::new();
    let separator = std::path::MAIN_SEPARATOR;
    let parent = PathBuf::from(format!("{}{separator}custom{separator}..{separator}outside", f.home.display()));
    for bad in [PathBuf::from("relative"), parent] {
        assert!(SessionStore::with_overrides(&f.home, &f.home.join(".codex"), Some(&bad), None).is_err());
        assert!(SessionStore::with_overrides(&f.home, &f.home.join(".codex"), None, Some(&bad)).is_err());
    }
}
