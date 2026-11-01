fn assert_codex_advanced_settings(path: &Path, id: &str, enabled: bool) -> DocumentMut {
    let text = fs::read_to_string(path).unwrap();
    assert!(text.contains("# keep per-tool approval"));
    let document = text.parse::<DocumentMut>().unwrap();
    let entry = &document["mcp_servers"][id];
    assert_eq!(entry["enabled"].as_bool(), Some(enabled));
    assert_eq!(entry["startup_timeout_sec"].as_integer(), Some(35));
    assert_eq!(entry["tool_timeout_sec"].as_integer(), Some(75));
    assert_eq!(entry["enabled_tools"].as_array().unwrap().get(0).unwrap().as_str(), Some("receipt"));
    assert_eq!(entry["disabled_tools"].as_array().unwrap().get(0).unwrap().as_str(), Some("unsafe_tool"));
    assert_eq!(entry["tools"]["receipt"]["approval_mode"].as_str(), Some("approve"));
    assert_eq!(entry["future_setting"]["version"].as_integer(), Some(2));
    assert_eq!(document["model"].as_str(), Some("keep-model"));
    document
}

#[test]
fn codex_advanced_settings_survive_edit_unmount_restart_remount_and_delete() {
    let root = temp_root();
    fs::create_dir_all(root.join(".codex")).unwrap();
    let path = root.join(".codex/config.toml");
    fs::write(&path, r#"model = "keep-model"
[mcp_servers.reliable]
command = "cmd"
args = ["/c", "echo"]
env = { QA_TOKEN = "fixture-env-secret" }
startup_timeout_sec = 35
tool_timeout_sec = 75
enabled_tools = ["receipt"]
disabled_tools = ["unsafe_tool"]
[mcp_servers.reliable.tools.receipt]
# keep per-tool approval
approval_mode = "approve"
[mcp_servers.reliable.future_setting]
version = 2
[mcp_servers.other]
url = "https://example.com/other"
"#).unwrap();
    let (manager, database, credentials) = test_manager(&root);
    assert_eq!(manager.import_from_apps().unwrap().imported, 2);
    let mut server = manager.list_servers().unwrap().into_iter().find(|server| server.id == "reliable").unwrap();
    assert!(server.apps["codex"]);
    assert_eq!(server.env.get("QA_TOKEN").map(String::as_str), Some(""));
    server.name = "Edited MCP".into();
    server.args.push("edited".into());
    manager.save_server(server).unwrap();
    let document = assert_codex_advanced_settings(&path, "reliable", true);
    assert_eq!(document["mcp_servers"]["reliable"]["args"].as_array().unwrap().len(), 3);
    assert_eq!(toml_string_table(document["mcp_servers"]["reliable"].get("env")).get("QA_TOKEN").map(String::as_str), Some("fixture-env-secret"));
    assert!(!database.get_mcp_record("reliable").unwrap().unwrap().config_json.contains("fixture-env-secret"));
    manager.set_app_enabled("reliable", "codex", false).unwrap();
    assert_codex_advanced_settings(&path, "reliable", false);
    drop(manager);
    let restarted = McpManager::with_credentials(database.clone(), root.clone(), credentials);
    assert!(!restarted.list_servers().unwrap().into_iter().find(|server| server.id == "reliable").unwrap().apps["codex"]);
    let before = fs::read(&path).unwrap();
    assert_eq!(restarted.import_from_apps().unwrap().imported, 0);
    assert_eq!(fs::read(&path).unwrap(), before);
    restarted.set_app_enabled("reliable", "codex", true).unwrap();
    assert_codex_advanced_settings(&path, "reliable", true);
    restarted.set_app_enabled("reliable", "codex", false).unwrap();
    restarted.delete_server("reliable").unwrap();
    let document = fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
    assert!(document["mcp_servers"].get("reliable").is_none());
    assert!(document["mcp_servers"].get("other").is_some());
    assert_eq!(document["model"].as_str(), Some("keep-model"));
    drop(restarted);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn disabled_inline_codex_entries_import_without_enabling_or_losing_settings() {
    let root = temp_root();
    fs::create_dir_all(root.join(".codex")).unwrap();
    let path = root.join(".codex/config.toml");
    let original = b"mcp_servers = { disabled = { url = 'https://example.com/mcp', enabled = false, tool_timeout_sec = 42, http_headers = { 'X-QA-Key' = 'fixture-header-secret' }, tools = { receipt = { approval_mode = 'approve' } } } }\n";
    fs::write(&path, original).unwrap();
    let (manager, database, _) = test_manager(&root);
    assert_eq!(manager.import_from_apps().unwrap().imported, 1);
    let saved = manager.list_servers().unwrap().into_iter().find(|server| server.id == "disabled").unwrap();
    assert!(!saved.apps["codex"]);
    assert_eq!(saved.headers.get("X-QA-Key").map(String::as_str), Some(""));
    assert_eq!(fs::read(&path).unwrap(), original);
    manager.set_app_enabled("disabled", "codex", true).unwrap();
    let document = fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
    let entry = &document["mcp_servers"]["disabled"];
    assert_eq!(entry["enabled"].as_bool(), Some(true));
    assert_eq!(entry["tool_timeout_sec"].as_integer(), Some(42));
    assert_eq!(entry["tools"]["receipt"]["approval_mode"].as_str(), Some("approve"));
    assert_eq!(toml_string_table(entry.get("http_headers")).get("X-QA-Key").map(String::as_str), Some("fixture-header-secret"));
    assert!(!database.get_mcp_record("disabled").unwrap().unwrap().config_json.contains("fixture-header-secret"));
    drop(manager);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_transport_changes_preserve_common_settings_and_remove_incompatible_fields() {
    let root = temp_root();
    fs::create_dir_all(root.join(".codex")).unwrap();
    let path = root.join(".codex/config.toml");
    fs::write(&path, "[mcp_servers.transport]\nurl = 'https://example.com/mcp'\nbearer_token_env_var = 'QA_BEARER'\nenv_http_headers = { 'X-Key' = 'QA_KEY' }\ntool_timeout_sec = 65\n").unwrap();
    let (manager, database, _) = test_manager(&root);
    manager.import_from_apps().unwrap();
    let mut server = manager.list_servers().unwrap().into_iter().find(|server| server.id == "transport").unwrap();
    server.transport = "stdio".into();
    server.command = Some("cmd".into());
    server.args = vec!["/c".into(), "echo".into()];
    server.url = None;
    manager.save_server(server).unwrap();
    let mut document = fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
    let entry = &document["mcp_servers"]["transport"];
    for key in ["url", "http_headers", "bearer_token_env_var", "env_http_headers"] { assert!(entry.get(key).is_none()); }
    assert_eq!(entry["tool_timeout_sec"].as_integer(), Some(65));
    document["mcp_servers"]["transport"]["cwd"] = value("fixture-workdir");
    let mut env_vars = Array::new();
    env_vars.push("QA_ENV");
    document["mcp_servers"]["transport"]["env_vars"] = Item::Value(TomlValue::Array(env_vars));
    fs::write(&path, document.to_string()).unwrap();
    let mut server = manager.list_servers().unwrap().into_iter().find(|server| server.id == "transport").unwrap();
    server.transport = "http".into();
    server.command = None;
    server.args.clear();
    server.url = Some("https://example.com/new-mcp".into());
    manager.save_server(server).unwrap();
    let document = fs::read_to_string(&path).unwrap().parse::<DocumentMut>().unwrap();
    let entry = &document["mcp_servers"]["transport"];
    for key in ["command", "args", "env", "cwd", "env_vars"] { assert!(entry.get(key).is_none()); }
    assert_eq!(entry["url"].as_str(), Some("https://example.com/new-mcp"));
    assert_eq!(entry["tool_timeout_sec"].as_integer(), Some(65));
    drop(manager);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn mcp_uses_the_same_resolved_codex_home_as_takeover() {
    let root = temp_root();
    let custom = root.join("custom client home 中文");
    fs::create_dir_all(root.join(".codex")).unwrap();
    fs::create_dir_all(&custom).unwrap();
    let default_path = root.join(".codex/config.toml");
    fs::write(&default_path, b"model = 'default-home-must-stay'\n").unwrap();
    fs::write(custom.join("config.toml"), b"[mcp_servers.custom]\nurl = 'https://example.com/mcp'\n").unwrap();
    let resolved = crate::takeover::resolve_codex_home(&root, Some(custom.clone().into_os_string())).unwrap();
    let (manager, database, _) = test_manager(&root);
    let manager = manager.with_codex_home(resolved);
    assert_eq!(manager.config_path(McpApp::Codex), custom.join("config.toml"));
    assert_eq!(manager.app_statuses().into_iter().find(|status| status.app == "codex").unwrap().config_path, custom.join("config.toml").display().to_string());
    assert_eq!(manager.import_from_apps().unwrap().imported, 1);
    assert!(manager.list_servers().unwrap().iter().any(|server| server.id == "custom"));
    manager.set_app_enabled("custom", "codex", false).unwrap();
    assert!(fs::read_to_string(custom.join("config.toml")).unwrap().contains("enabled = false"));
    assert_eq!(fs::read(&default_path).unwrap(), b"model = 'default-home-must-stay'\n");
    drop(manager);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_never_mounted_server_does_not_read_unrelated_codex_config() {
    let root = temp_root();
    fs::create_dir_all(&root).unwrap();
    let (manager, database, _) = test_manager(&root);
    manager.save_server(test_server("unmounted")).unwrap();
    fs::create_dir_all(root.join(".codex")).unwrap();
    let path = root.join(".codex/config.toml");
    let original = b"unrelated = [broken TOML\n";
    fs::write(&path, original).unwrap();
    manager.delete_server("unmounted").unwrap();
    assert!(database.get_mcp_record("unmounted").unwrap().is_none());
    assert_eq!(fs::read(&path).unwrap(), original);
    drop(manager);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}
