#[tokio::test]
async fn selection_without_takeover_never_starts_proxy_or_creates_client_files() {
    let (manager, proxy, root) = manager().await;
    proxy.set_enabled(false).await.unwrap();
    let mut p = manager.database.get_provider("openai").unwrap().unwrap();
    p.id = "chat-selection".into();
    p.model = "chat-fixture".into();
    p.advanced.upstream_format = "openai_chat".into();
    manager.database.save_provider(&p).unwrap();
    crate::commands::providers::select_provider(&manager.database, &manager, &p.id)
        .await
        .unwrap();
    assert!(!proxy.status().await.unwrap().running);
    assert!(!manager.has_active());
    assert!(!root.join("home").exists());
    assert_eq!(
        manager.database.current_provider().unwrap().unwrap().id,
        p.id
    );

    // Both route startup and client takeover remain explicit user actions.
    assert!(manager.set_enabled("codex", true).await.is_err());
    assert!(!root.join("home").exists());
    proxy.set_enabled(true).await.unwrap();
    manager.set_enabled("codex", true).await.unwrap();
    let config = fs::read_to_string(root.join("home/.codex/config.toml"))
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
    assert_eq!(config["model"].as_str(), Some("chat-fixture"));
    assert!(proxy.status().await.unwrap().running);
    manager.set_enabled("codex", false).await.unwrap();
    assert!(!root.join("home/.codex/config.toml").exists());
    proxy.stop().await;
    drop(manager);
    drop(proxy);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn global_selection_syncs_only_active_clients_and_rejects_external_edits() {
    let (manager, proxy, root) = manager().await;
    let codex = root.join("home/.codex/config.toml");
    let claude = root.join("home/.claude/settings.json");
    fs::create_dir_all(codex.parent().unwrap()).unwrap();
    fs::create_dir_all(claude.parent().unwrap()).unwrap();
    let original_codex = b"approval_policy = \"on-request\"\n";
    let original_claude = br#"{"theme":"dark","env":{"CUSTOM":"keep"}}"#;
    fs::write(&codex, original_codex).unwrap();
    fs::write(&claude, original_claude).unwrap();
    manager.set_enabled("codex", true).await.unwrap();
    manager.set_enabled("claude", true).await.unwrap();
    let mut p = manager.database.get_provider("openai").unwrap().unwrap();
    p.id = "global-selection".into();
    p.model = "global-model".into();
    manager.database.save_provider(&p).unwrap();
    crate::commands::providers::select_provider(&manager.database, &manager, &p.id)
        .await
        .unwrap();
    assert!(fs::read_to_string(&codex).unwrap().contains("global-model"));
    let claude_config: Value = serde_json::from_slice(&fs::read(&claude).unwrap()).unwrap();
    assert_eq!(claude_config["env"]["ANTHROPIC_BASE_URL"], proxy.status().await.unwrap().address);
    assert_eq!(claude_config["env"]["CUSTOM"], "keep");
    assert!(!root.join("home/.gemini").exists());
    let managed_codex = fs::read(&codex).unwrap();
    let managed_claude = fs::read(&claude).unwrap();
    let mut external: Value = serde_json::from_slice(&managed_claude).unwrap();
    external["external"] = json!(true);
    fs::write(&claude, serde_json::to_vec(&external).unwrap()).unwrap();
    let external_bytes = fs::read(&claude).unwrap();
    let mut next = p.clone();
    next.id = "next-selection".into();
    next.model = "must-not-apply".into();
    manager.database.save_provider(&next).unwrap();
    let error = crate::commands::providers::select_provider(&manager.database, &manager, &next.id)
        .await
        .unwrap_err();
    assert!(error.contains("外部修改"), "{error}");
    assert_eq!(fs::read(&codex).unwrap(), managed_codex);
    assert_eq!(fs::read(&claude).unwrap(), external_bytes);
    assert_eq!(
        manager.database.current_provider().unwrap().unwrap().id,
        p.id
    );
    fs::write(&claude, managed_claude).unwrap();
    manager.set_enabled("claude", false).await.unwrap();
    manager.set_enabled("codex", false).await.unwrap();
    assert_eq!(fs::read(&codex).unwrap(), original_codex);
    assert_eq!(fs::read(&claude).unwrap(), original_claude);
    proxy.stop().await;
    drop(manager);
    drop(proxy);
    fs::remove_dir_all(root).unwrap();
}

#[tokio::test]
async fn switching_during_takeover_keeps_original_backup_restorable() {
    let (manager, proxy, root) = manager().await;
    let directory = root.join("home/.codex");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("config.toml"),
        "approval_policy = \"on-request\"\n",
    )
    .unwrap();
    fs::write(
        directory.join("auth.json"),
        "{\"tokens\":{\"original\":true}}",
    )
    .unwrap();
    let original = fs::read(directory.join("config.toml")).unwrap();
    manager.set_enabled("codex", true).await.unwrap();
    let mut p = manager.database.current_provider().unwrap().unwrap();
    p.advanced.upstream_format = "openai_chat".into();
    p.model = "kimi-fixture".into();
    manager
        .sync_codex_provider(&p, Some(&proxy.status().await.unwrap().address))
        .unwrap();
    assert!(
        fs::read_to_string(directory.join("config.toml"))
            .unwrap()
            .contains("kimi-fixture")
    );
    manager.set_enabled("codex", false).await.unwrap();
    assert_eq!(fs::read(directory.join("config.toml")).unwrap(), original);
    proxy.stop().await;
    drop(manager);
    drop(proxy);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_config_preserves_direct_endpoint_and_clears_stale_auth_overrides() {
    let mut p: Provider=serde_json::from_value(json!({"id":"grok-native","name":"Grok","appGroup":"Codex","endpoint":"https://api.x.ai/v1/responses","model":"grok-fixture","enabled":true})).unwrap();
    p.advanced.upstream_format = "openai_responses".into();
    let original = "model_provider = \"xy_switch\"\n[model_providers.xy_switch]\nenv_key = \"OLD_KEY\"\nhttp_headers = { Authorization = \"Bearer stale\" }\n";
    let updated = managed_codex_config_internal(
        original,
        &crate::proxy::codex_direct_base_url(&p).unwrap(),
        "fake-native-key",
        &p,
        &temp_root(),
    )
    .unwrap();
    let doc = String::from_utf8(updated)
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
    assert_eq!(
        doc["model_providers"]["xy_switch"]["base_url"].as_str(),
        Some("https://api.x.ai/v1")
    );
    assert_eq!(
        doc["model_providers"]["xy_switch"]["wire_api"].as_str(),
        Some("responses")
    );
    assert!(!doc.to_string().contains("OLD_KEY"));
    assert!(!doc.to_string().contains("Bearer stale"));
}

#[test]
fn codex_activation_owns_only_xy_provider_and_updates_active_profile() {
    let p: Provider = serde_json::from_value(json!({"id":"xy-fixture","name":"XY fixture","appGroup":"Codex","endpoint":"http://127.0.0.1:12345/v1","model":"xy-model","enabled":true})).unwrap();
    let original = r#"model_provider = "custom"
model = "cc-model"
profile = "cc-profile"
model_catalog_json = "cc-switch-model-catalog.json"
[model_providers.custom]
name = "CC Switch fixture"
base_url = "http://127.0.0.1:54321/v1"
wire_api = "responses"
[profiles.cc-profile]
model_provider = "custom"
model = "cc-model"
model_catalog_json = "cc-switch-model-catalog.json"
model_reasoning_effort = "low"
[profiles.unrelated]
model = "keep-me"
"#;
    let bytes = managed_codex_config_internal(
        original,
        "http://127.0.0.1:12345/v1",
        "fixture-placeholder",
        &p,
        &temp_root(),
    )
    .unwrap();
    let doc = String::from_utf8(bytes)
        .unwrap()
        .parse::<DocumentMut>()
        .unwrap();
    assert_eq!(doc["model_provider"].as_str(), Some("xy_switch"));
    assert_eq!(
        doc["model_providers"]["custom"]["name"].as_str(),
        Some("CC Switch fixture")
    );
    assert_eq!(
        doc["model_providers"]["custom"]["base_url"].as_str(),
        Some("http://127.0.0.1:54321/v1")
    );
    assert_eq!(
        doc["profiles"]["cc-profile"]["model_provider"].as_str(),
        Some("custom")
    );
    assert_eq!(
        doc["profiles"]["cc-profile"]["model"].as_str(),
        Some("cc-model")
    );
    assert_eq!(
        doc["profiles"]["cc-profile"]["model_catalog_json"].as_str(),
        Some("cc-switch-model-catalog.json")
    );
    assert_eq!(
        doc["profiles"]["cc-profile"]["model_reasoning_effort"].as_str(),
        Some("low")
    );
    assert!(doc.get("profile").is_none());
    assert_eq!(doc["model_reasoning_effort"].as_str(), Some("low"));
    assert_eq!(
        doc["profiles"]["unrelated"]["model"].as_str(),
        Some("keep-me")
    );
}

#[test]
fn catalog_does_not_overwrite_cc_switch_file() {
    let dir = temp_root();
    fs::create_dir_all(&dir).unwrap();
    let cc_path = dir.join("cc-switch-model-catalog.json");
    let original = b"{\"models\":[{\"slug\":\"cc-fixture\"}]}\n";
    fs::write(&cc_path, original).unwrap();
    let p: Provider = serde_json::from_value(json!({"id":"xy-fixture","name":"XY fixture","appGroup":"Codex","endpoint":"http://127.0.0.1:12345/v1","model":"xy-model","enabled":true})).unwrap();
    write_codex_model_catalog(&dir, &p).unwrap();
    assert_eq!(fs::read(&cc_path).unwrap(), original);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn primary_model_mapping_keeps_its_display_name_and_context() {
    let dir = temp_root();
    let mut p: Provider = serde_json::from_value(json!({"id":"xy-fixture","name":"XY fixture","appGroup":"Codex","endpoint":"http://127.0.0.1:12345/v1","model":"xy-model","enabled":true})).unwrap();
    p.advanced
        .model_mappings
        .push(crate::database::ProviderModelMapping {
            model: "xy-model".into(),
            display_name: "XY model display".into(),
            context_window: Some(256_000),
        });
    write_codex_model_catalog(&dir, &p).unwrap();
    let catalog: Value =
        serde_json::from_slice(&fs::read(dir.join("xy-switch-model-catalog.json")).unwrap())
            .unwrap();
    assert_eq!(catalog["models"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["models"][0]["display_name"], "XY model display");
    assert_eq!(catalog["models"][0]["context_window"], 256_000);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn codex_home_resolution_is_explicit_and_rejects_relative_override() {
    let home = temp_root();
    assert_eq!(
        resolve_codex_home(&home, None).unwrap(),
        home.join(".codex")
    );
    let custom = home.join("isolated-codex-home");
    assert_eq!(
        resolve_codex_home(&home, Some(custom.clone().into_os_string())).unwrap(),
        custom
    );
    assert!(resolve_codex_home(&home, Some("relative-home".into())).is_err());
}

#[tokio::test]
async fn custom_codex_home_exports_consistent_absolute_catalog_and_preserves_legacy_files() {
    let (manager, proxy, root) = manager().await;
    let directory = root.join("自定义 Codex 目录");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("config.toml"), "profile = \"old\"\n[profiles.old]\nmodel = \"cc-old\"\nmodel_provider = \"custom\"\nmodel_catalog_json = \"cc-switch-model-catalog.json\"\n").unwrap();
    fs::write(
        directory.join("cc-switch-model-catalog.json"),
        b"legacy fixture",
    )
    .unwrap();
    let manager = manager.with_codex_home(directory.clone());
    let p: Provider = serde_json::from_value(json!({"id":"xy-custom-home","name":"XY fixture","appGroup":"Codex","endpoint":"http://127.0.0.1:12345/v1","model":"xy-model","enabled":true,"advanced":{"upstreamFormat":"openai_chat"}})).unwrap();
    manager
        .sync_codex_provider(&p, Some("http://127.0.0.1:15721"))
        .unwrap();
    let config: DocumentMut = fs::read_to_string(directory.join("config.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let path = config["model_catalog_json"].as_str().unwrap();
    assert!(Path::new(path).is_absolute());
    assert_eq!(
        Path::new(path),
        directory.join("xy-switch-model-catalog.json")
    );
    assert_eq!(
        config["profiles"]["old"]["model_catalog_json"].as_str(),
        Some("cc-switch-model-catalog.json")
    );
    assert_eq!(
        fs::read(directory.join("cc-switch-model-catalog.json")).unwrap(),
        b"legacy fixture"
    );
    assert!(!root.join("home/.codex/config.toml").exists());
    assert!(
        manager
            .status_for(ManagedApp::Codex)
            .config_paths
            .iter()
            .all(|path| Path::new(path).starts_with(&directory))
    );
    proxy.stop().await;
    drop(manager);
    drop(proxy);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn exports_model_source_fixture_for_isolated_app_server_verification() {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("artifacts/2026-09-21-model-source-fix")
        .join(format!("model-list-fixture-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let mut p: Provider = serde_json::from_value(json!({"id":"xy-model-list","name":"XY fixture","appGroup":"Codex","endpoint":"http://127.0.0.1:1/v1","model":"xy-model-a","enabled":true,"advanced":{"upstreamFormat":"openai_chat"}})).unwrap();
    for (id, display, cw) in [
        ("xy-model-a", "XY 独立模型 A", 256_000),
        ("xy-model-b", "XY 独立模型 B", 128_000),
    ] {
        p.advanced
            .model_mappings
            .push(crate::database::ProviderModelMapping {
                model: id.into(),
                display_name: display.into(),
                context_window: Some(cw),
            });
    }
    let old = "profile = \"legacy\"\nmodel_provider = \"custom\"\nmodel = \"cc-old\"\n[model_providers.custom]\nname = \"CC Switch fixture\"\nbase_url = \"http://127.0.0.1:2/v1\"\nwire_api = \"responses\"\n[profiles.legacy]\nmodel = \"cc-old\"\nmodel_provider = \"custom\"\nmodel_catalog_json = \"cc-switch-model-catalog.json\"\n";
    let config = managed_codex_config_internal(
        old,
        "http://127.0.0.1:1/v1",
        "fixture-placeholder",
        &p,
        &directory,
    )
    .unwrap();
    fs::write(directory.join("config.toml"), config).unwrap();
    write_codex_model_catalog(&directory, &p).unwrap();
    println!("Model source fixture: {}", directory.display());
}

#[test]
fn invalid_legacy_profile_is_not_silently_ignored() {
    let p: Provider = serde_json::from_value(json!({"id":"xy-fixture","name":"XY fixture","appGroup":"Codex","endpoint":"http://127.0.0.1:1/v1","model":"xy-model","enabled":true})).unwrap();
    for original in ["profile = \"missing\"\n", "profile = 12\n"] {
        assert!(
            managed_codex_config_internal(
                original,
                "http://127.0.0.1:1/v1",
                "fixture",
                &p,
                &temp_root()
            )
            .is_err()
        );
    }
}

#[test]
fn malformed_owned_provider_is_not_reported_as_success() {
    let p: Provider = serde_json::from_value(json!({"id":"xy-fixture","name":"XY fixture","appGroup":"Codex","endpoint":"http://127.0.0.1:1/v1","model":"xy-model","enabled":true})).unwrap();
    assert!(
        managed_codex_config_internal(
            "[model_providers]\nxy_switch = false\n",
            "http://127.0.0.1:1/v1",
            "fixture",
            &p,
            &temp_root()
        )
        .is_err()
    );
}
