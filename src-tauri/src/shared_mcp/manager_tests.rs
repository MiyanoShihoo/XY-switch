use super::*;
use crate::cancellation::Cancellation;
use std::time::Instant;
use zeroize::Zeroizing;

fn begin(fixture: &Fixture, connection: &SharedMcpConnection, token: &str, request: &str, session: &str, signal: Cancellation) -> JoinHandle<Result<Value, &'static str>> {
    let manager = fixture.manager().clone();
    let connection = connection.clone();
    let token = token.to_owned();
    let request = request.to_owned();
    let session = session.to_owned();
    tokio::spawn(async move { manager.call(connection, session, request, signal, "test.shared-echo.echo".into(), json!({"message":"safe"}), Zeroizing::new(token)).await })
}

#[test]
fn registration_requires_scope_consent_and_keeps_credentials_out_of_status_and_config() {
    let fixture = Fixture::new();
    assert_eq!(fixture.manager().register(fixture.mcp(), "codex", true, false).unwrap_err(), "shared_mcp_confirmation");
    assert_eq!(fixture.manager().register(fixture.mcp(), "claude-desktop", false, true).unwrap_err(), "shared_mcp_confirmation");
    assert_eq!(fixture.manager().register(fixture.mcp(), "unrecognized", true, true).unwrap_err(), "shared_mcp_scope");
    let (connection, token) = fixture.connection("codex");
    assert_eq!(connection.clients, vec!["codex-desktop", "codex-cli"]);
    assert_eq!(fixture.manager().authenticate(&connection.id, &token).unwrap(), connection);
    assert_eq!(fixture.manager().register(fixture.mcp(), "codex", true, true).unwrap_err(), "shared_mcp_limit");
    let status = serde_json::to_string(&fixture.manager().status().unwrap()).unwrap();
    let record = fixture.manager().database.get_mcp_record(&connection.server_id).unwrap().unwrap();
    let config = fs::read_to_string(fixture.root.join("home/.codex/config.toml")).unwrap();
    for text in [status, record.config_json, config] {
        assert!(!text.contains(&token));
        assert!(!text.contains("Bearer"));
    }
}

#[test]
fn credential_and_config_failures_leave_no_published_connection_or_secret() {
    let fixture = Fixture::new();
    let initial_servers = fixture.mcp().list_servers().unwrap();
    fixture.secrets.fail_set.store(true, Ordering::SeqCst);
    assert_eq!(fixture.manager().register(fixture.mcp(), "claude-desktop", true, true).unwrap_err(), "shared_mcp_credentials");
    assert!(fixture.manager().status().unwrap().connections.is_empty());
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
    assert_eq!(fixture.mcp().list_servers().unwrap(), initial_servers);
    fixture.secrets.fail_set.store(false, Ordering::SeqCst);
    fs::create_dir_all(fixture.root.join("home/.codex")).unwrap();
    fs::write(fixture.root.join("home/.codex/config.toml"), "[broken").unwrap();
    assert_eq!(fixture.manager().register(fixture.mcp(), "codex", true, true).unwrap_err(), "shared_mcp_config");
    assert!(fixture.manager().status().unwrap().connections.is_empty());
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
    assert_eq!(fixture.mcp().list_servers().unwrap(), initial_servers);
}

#[test]
fn failed_secret_cleanup_keeps_a_revoked_connection_unusable_and_can_be_retried() {
    let fixture = Fixture::new();
    let (connection, token) = fixture.connection("claude-desktop");
    fixture.secrets.fail_delete.store(true, Ordering::SeqCst);
    assert_eq!(fixture.manager().revoke(fixture.mcp(), &connection.id, true).unwrap_err(), "shared_mcp_credentials_cleanup");
    assert_eq!(fixture.manager().authenticate(&connection.id, &token).unwrap_err(), "shared_mcp_auth");
    assert!(!fixture.manager().status().unwrap().connections[0].active);
    let restored = SharedMcpManager::with_secrets(fixture.manager().database.clone(), fixture.manager().plugins.clone(), fixture.secrets.clone()).unwrap();
    assert!(!restored.status().unwrap().connections[0].active);
    drop(restored);
    fixture.secrets.fail_delete.store(false, Ordering::SeqCst);
    fixture.manager().revoke(fixture.mcp(), &connection.id, true).unwrap();
    assert!(fixture.manager().status().unwrap().connections.is_empty());
    assert!(fixture.secrets.values.lock().unwrap().is_empty());
}

#[tokio::test]
async fn shared_scopes_only_expose_the_intersection_of_current_plugin_grants() {
    let fixture = Fixture::new();
    fixture.install_echo(&["codex-desktop"]);
    let (connection, _) = fixture.connection("codex");
    assert!(fixture.manager().tools(&connection).unwrap().is_empty());
    let preview = fixture.manager().plugins.preview_installed("test.shared-echo").unwrap();
    fixture.manager().plugins.confirm_install(InstallConfirmation { preview_id: preview.preview_id, digest: preview.validation.digest,
        accepted_permissions: preview.validation.manifest.permissions, clients: vec!["codex-desktop".into(), "codex-cli".into()], confirmed: true, enable: true }).unwrap();
    assert_eq!(fixture.manager().tools(&connection).unwrap().len(), 1);
    fixture.manager().plugins.disable_plugin("test.shared-echo", Some("codex-cli")).unwrap();
    assert!(fixture.manager().tools(&connection).unwrap().is_empty());
}

#[tokio::test]
async fn duplicate_requests_are_rejected_and_denied_ids_can_be_reused() {
    let fixture = Fixture::new();
    fixture.install_echo(&["claude-desktop", "claude-desktop-3p"]);
    let (connection, token) = fixture.connection("claude-desktop");
    let first = begin(&fixture, &connection, &token, "1", "", Cancellation::default());
    let review = fixture.pending().await;
    let duplicate = begin(&fixture, &connection, &token, "1", "", Cancellation::default());
    assert_eq!(duplicate.await.unwrap().unwrap_err(), "shared_mcp_limit");
    fixture.manager().decide(&review.id, false, false).unwrap();
    assert_eq!(first.await.unwrap().unwrap_err(), "shared_mcp_denied");
    let second = begin(&fixture, &connection, &token, "1", "", Cancellation::default());
    let review = fixture.pending().await;
    assert_eq!(fixture.manager().decide(&review.id, true, false).unwrap_err(), "shared_mcp_confirmation");
    fixture.manager().decide(&review.id, true, true).unwrap();
    assert_eq!(second.await.unwrap().unwrap()["isError"], false);
    fixture.cleared().await;
}

#[tokio::test]
async fn revoke_disable_and_expiry_cancel_pending_calls_before_execution() {
    for action in ["revoke", "disable", "expire"] {
        let fixture = Fixture::new();
        fixture.install_echo(&["claude-desktop", "claude-desktop-3p"]);
        let (connection, token) = fixture.connection("claude-desktop");
        let call = begin(&fixture, &connection, &token, "1", "", Cancellation::default());
        let review = fixture.pending().await;
        match action {
            "revoke" => fixture.manager().revoke(fixture.mcp(), &connection.id, true).unwrap(),
            "disable" => { fixture.manager().set_enabled(false, true).await.unwrap(); },
            _ => { fixture.manager().expire_test_approval(&review.id); assert!(fixture.manager().pending().unwrap().is_empty()); },
        }
        assert_eq!(tokio::time::timeout(Duration::from_secs(2), call).await.unwrap().unwrap().unwrap_err(), "shared_mcp_cancelled");
        assert_eq!(fixture.manager().decide(&review.id, true, true).unwrap_err(), "shared_mcp_expired");
    }
}

#[tokio::test]
async fn expired_sessions_remove_their_pending_calls_and_approvals() {
    let fixture = Fixture::new();
    fixture.install_echo(&["claude-desktop", "claude-desktop-3p"]);
    let (connection, token) = fixture.connection("claude-desktop");
    let session = fixture.manager().create_session(&connection, "2025-11-25").unwrap();
    let signal = fixture.manager().session(&connection, &session, "2025-11-25", true).unwrap();
    let call = begin(&fixture, &connection, &token, "1", &session, signal);
    let review = fixture.pending().await;
    fixture.manager().state.lock().unwrap().sessions.get_mut(&session).unwrap().last_seen = Instant::now() - Duration::from_secs(1801);
    assert_eq!(fixture.manager().session(&connection, &session, "2025-11-25", false).unwrap_err(), "shared_mcp_session");
    assert_eq!(call.await.unwrap().unwrap_err(), "shared_mcp_cancelled");
    assert_eq!(fixture.manager().decide(&review.id, true, true).unwrap_err(), "shared_mcp_expired");
}

#[tokio::test]
async fn changed_plugin_permissions_after_review_prevent_execution() {
    let fixture = Fixture::new();
    fixture.install_echo(&["codex-desktop", "codex-cli"]);
    let (connection, token) = fixture.connection("codex");
    let call = begin(&fixture, &connection, &token, "1", "", Cancellation::default());
    let review = fixture.pending().await;
    fixture.manager().plugins.disable_plugin("test.shared-echo", Some("codex-cli")).unwrap();
    fixture.manager().decide(&review.id, true, true).unwrap();
    assert_eq!(call.await.unwrap().unwrap_err(), "shared_mcp_plugins");
}

#[tokio::test]
async fn stopping_cancels_runtime_work_without_erasing_the_saved_enable_setting() {
    let fixture = Fixture::new();
    fixture.manager().database.set_setting("shared_mcp_settings_v1", r#"{"enabled":true}"#).unwrap();
    let (connection, token) = fixture.connection("claude-desktop");
    fixture.manager().stop().await;
    assert!(!fixture.manager().status().unwrap().running);
    assert_eq!(fixture.manager().authenticate(&connection.id, &token).unwrap_err(), "shared_mcp_disabled");
    assert_eq!(fixture.manager().database.get_setting("shared_mcp_settings_v1").unwrap().unwrap(), r#"{"enabled":true}"#);
    let restored = SharedMcpManager::with_secrets(fixture.manager().database.clone(), fixture.manager().plugins.clone(), fixture.secrets.clone()).unwrap();
    assert!(restored.status().unwrap().enabled);
    assert!(!restored.status().unwrap().running);
}

#[test]
fn claude_desktop_modes_require_both_scopes_and_share_one_recoverable_config() {
    for (owner, other) in [("claude-desktop", "claude-desktop-3p"), ("claude-desktop-3p", "claude-desktop")] {
        let fixture = Fixture::new();
        let path = crate::client_config::config_directory(&fixture.root.join("home"), "Claude", false).join("claude_desktop_config.json");
        let baseline = json!({"preferences":{"theme":"system"},"mcpServers":{"existing":{"command":"synthetic-existing","args":[]}}});
        let baseline_bytes = format!("{}\n", serde_json::to_string_pretty(&baseline).unwrap());
        fs::write(&path, &baseline_bytes).unwrap();
        assert_eq!(fixture.manager().register(fixture.mcp(), owner, true, false).unwrap_err(), "shared_mcp_confirmation");
        assert_eq!(fs::read_to_string(&path).unwrap(), baseline_bytes);
        assert!(fixture.secrets.values.lock().unwrap().is_empty());
        let (connection, token) = fixture.connection(owner);
        assert_eq!(connection.owner, owner);
        assert_eq!(connection.clients, ["claude-desktop", "claude-desktop-3p"]);
        let saved: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(saved["mcpServers"][&connection.server_id]["args"], json!(["--shared-mcp-stdio", connection.id]));
        assert_eq!(saved["mcpServers"]["existing"], baseline["mcpServers"]["existing"]);
        assert!(!saved.to_string().contains(&token));
        let server = fixture.mcp().list_servers().unwrap().into_iter().find(|server| server.id == connection.server_id).unwrap();
        assert_eq!(server.apps.get("claude-desktop"), Some(&true));
        assert!(!server.apps.contains_key("claude-desktop-3p"));
        assert_eq!(fixture.manager().register(fixture.mcp(), other, true, true).unwrap_err(), "shared_mcp_limit");
        assert_eq!(fixture.secrets.values.lock().unwrap().len(), 1);
        let restored = SharedMcpManager::with_secrets(fixture.manager().database.clone(), fixture.manager().plugins.clone(), fixture.secrets.clone()).unwrap();
        assert_eq!(restored.status().unwrap().connections, vec![connection.clone()]);
        drop(restored);
        fixture.manager().revoke(fixture.mcp(), &connection.id, true).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), baseline_bytes);
        assert!(fixture.secrets.values.lock().unwrap().is_empty());
        let (replacement, _) = fixture.connection(other);
        assert_ne!(replacement.id, connection.id);
        fixture.manager().revoke(fixture.mcp(), &replacement.id, true).unwrap();
    }
}

#[test]
fn legacy_desktop_scope_is_persistently_disabled_without_expanding_authorization() {
    let mut fixture = Fixture::new();
    let (mut legacy, token) = fixture.connection("claude-desktop");
    legacy.clients = vec!["claude-desktop".into()];
    let database = fixture.manager().database.clone();
    let plugins = fixture.manager().plugins.clone();
    database.set_setting("shared_mcp_connections_v1", &serde_json::to_string(&vec![legacy.clone()]).unwrap()).unwrap();
    fixture.shared = Some(SharedMcpManager::with_secrets(database.clone(), plugins.clone(), fixture.secrets.clone()).unwrap());
    {
        let mut state = fixture.manager().state.lock().unwrap();
        state.enabled = true;
        state.running = true;
    }
    let migrated = fixture.manager().status().unwrap().connections.remove(0);
    assert!(!migrated.active);
    assert_eq!(migrated.clients, ["claude-desktop"]);
    assert_eq!(fixture.manager().authenticate(&legacy.id, &token).unwrap_err(), "shared_mcp_auth");
    assert_eq!(fixture.manager().tools(&legacy).unwrap_err(), "shared_mcp_cancelled");
    let saved: Vec<SharedMcpConnection> = serde_json::from_str(&database.get_setting("shared_mcp_connections_v1").unwrap().unwrap()).unwrap();
    assert_eq!(saved, vec![migrated.clone()]);
    let restarted = SharedMcpManager::with_secrets(database, plugins, fixture.secrets.clone()).unwrap();
    assert_eq!(restarted.status().unwrap().connections, vec![migrated]);
    drop(restarted);
    assert_eq!(fixture.manager().register(fixture.mcp(), "claude-desktop-3p", true, true).unwrap_err(), "shared_mcp_cleanup_required");
    assert!(fixture.secrets.get(&legacy.id).unwrap().is_some());
    fixture.manager().revoke(fixture.mcp(), &legacy.id, true).unwrap();
    assert!(fixture.secrets.get(&legacy.id).unwrap().is_none());
    let (replacement, _) = fixture.connection("claude-desktop-3p");
    assert_eq!(replacement.clients, ["claude-desktop", "claude-desktop-3p"]);
    assert_ne!(replacement.id, legacy.id);
    assert!(fixture.manager().pending().unwrap().is_empty());
}

#[test]
fn unsupported_saved_scopes_fail_without_partially_migrating_valid_records() {
    let fixture = Fixture::new();
    let (mut legacy, _) = fixture.connection("claude-desktop");
    let (mut invalid, _) = fixture.connection("codex");
    legacy.clients = vec!["claude-desktop".into()];
    invalid.owner = "claude-desktop-3p".into();
    invalid.clients = vec!["claude-desktop-3p".into()];
    let raw = serde_json::to_string(&vec![legacy, invalid]).unwrap();
    fixture.manager().database.set_setting("shared_mcp_connections_v1", &raw).unwrap();
    let result = SharedMcpManager::with_secrets(fixture.manager().database.clone(), fixture.manager().plugins.clone(), fixture.secrets.clone());
    assert!(matches!(result, Err(error) if error == "shared_mcp_store"));
    assert_eq!(fixture.manager().database.get_setting("shared_mcp_connections_v1").unwrap().unwrap(), raw);
}

#[tokio::test]
async fn claude_desktop_modes_keep_plugin_grants_independent_and_recheck_each_call() {
    for owner in ["claude-desktop", "claude-desktop-3p"] {
        let fixture = Fixture::new();
        fixture.install_echo(&[owner]);
        let (connection, token) = fixture.connection(owner);
        assert_eq!(fixture.manager().plugins.tools_for_client(owner).unwrap().len(), 1);
        assert!(fixture.manager().tools(&connection).unwrap().is_empty());
        assert_eq!(begin(&fixture, &connection, &token, "1", "", Cancellation::default()).await.unwrap().unwrap_err(), "shared_mcp_tool");
        assert!(fixture.manager().pending().unwrap().is_empty());
        let preview = fixture.manager().plugins.preview_installed("test.shared-echo").unwrap();
        fixture.manager().plugins.confirm_install(InstallConfirmation { preview_id: preview.preview_id, digest: preview.validation.digest,
            accepted_permissions: preview.validation.manifest.permissions, clients: vec!["claude-desktop".into(), "claude-desktop-3p".into()], confirmed: true, enable: true }).unwrap();
        let approved = begin(&fixture, &connection, &token, "2", "", Cancellation::default());
        let review = fixture.pending().await;
        assert_eq!(review.owner, owner);
        assert_eq!(review.clients, ["claude-desktop", "claude-desktop-3p"]);
        fixture.manager().decide(&review.id, true, true).unwrap();
        assert_eq!(approved.await.unwrap().unwrap()["isError"], false);
        let changed = begin(&fixture, &connection, &token, "3", "", Cancellation::default());
        let review = fixture.pending().await;
        fixture.manager().plugins.disable_plugin("test.shared-echo", Some(owner)).unwrap();
        fixture.manager().decide(&review.id, true, true).unwrap();
        assert_eq!(changed.await.unwrap().unwrap_err(), "shared_mcp_plugins");
        assert!(fixture.manager().tools(&connection).unwrap().is_empty());
        let other = if owner == "claude-desktop" { "claude-desktop-3p" } else { "claude-desktop" };
        assert_eq!(fixture.manager().plugins.tools_for_client(other).unwrap().len(), 1);
        fixture.cleared().await;
    }
}
