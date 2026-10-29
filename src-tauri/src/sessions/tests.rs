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
