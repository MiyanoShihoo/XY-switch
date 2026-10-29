use super::*;
use crate::{
    database::{Provider, ProviderAdvancedConfig},
    model_pricing::PricingInput,
};
use model::SyncProvider;
use store::Choice;
const PASS: &str = "correct horse battery staple";
fn db() -> Database {
    crate::governance::test_support::database()
}
fn provider(id: &str) -> Provider {
    Provider {
        id: id.into(),
        name: "Sync test".into(),
        provider_type: "openai_compatible".into(),
        endpoint: "https://example.invalid/v1".into(),
        model: "test-model".into(),
        app_group: "Codex".into(),
        enabled: true,
        sort_order: 0,
        advanced: ProviderAdvancedConfig::default(),
    }
}
fn favorite(id: &str) -> Favorite {
    Favorite {
        provider_id: "codex".into(),
        session_id: id.into(),
    }
}
fn price(rate: f64) -> PricingInput {
    PricingInput {
        id: "test-model".into(),
        input_per_million: rate,
        output_per_million: 2.0,
        cache_read_per_million: None,
        cache_creation_per_million: None,
    }
}
fn incoming() -> Payload {
    Payload {
        schema: 1,
        providers: vec![SyncProvider::from_local(
            &provider("never-use-as-credential-id"),
            &uuid::Uuid::new_v4().to_string(),
        )],
        favorites: vec![favorite("session-1")],
        prices: vec![price(1.0)],
    }
}
fn choose(rows: &[store::Change], use_incoming: bool) -> Vec<Choice> {
    rows.iter()
        .filter(|r| r.status != "unchanged")
        .map(|r| Choice {
            key: r.key.clone(),
            use_incoming,
        })
        .collect()
}
fn apply_all(db: &Database, payload: &Payload) -> store::Applied {
    let (rows, hash) = store::import_preview(db, payload).unwrap();
    store::apply(db, payload, &hash, &choose(&rows, true)).unwrap()
}
fn count(db: &Database, table: &str) -> i64 {
    let c = db.conn();
    let c = c.lock().unwrap();
    c.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn encryption_roundtrip_is_randomized_and_does_not_contain_plaintext() {
    let plain = encode(&incoming()).unwrap();
    let first = crypto::encrypt(&plain, PASS).unwrap();
    let second = crypto::encrypt(&plain, PASS).unwrap();
    assert_ne!(first, second);
    assert_eq!(*crypto::decrypt(&first, PASS).unwrap(), *plain);
    assert!(
        !first
            .windows(b"test-model".len())
            .any(|w| w == b"test-model")
    );
    assert_eq!(
        crypto::decrypt(&first, "wrong passphrase long enough").unwrap_err(),
        "Wrong passphrase or damaged encrypted file"
    );
}
#[test]
fn salt_nonce_ciphertext_and_tag_tampering_is_rejected() {
    let file = crypto::encrypt(b"private configuration", PASS).unwrap();
    for at in [0, 8, 39, 40, 51, 52, file.len() - 1] {
        let mut bad = file.clone();
        bad[at] ^= 1;
        assert!(crypto::decrypt(&bad, PASS).is_err(), "byte {at}");
    }
    for len in [0, 7, 8, 51, 52, 67, file.len() - 1] {
        assert!(crypto::decrypt(&file[..len], PASS).is_err());
    }
}
#[test]
fn encryption_limits_and_passphrase_policy_are_enforced_before_kdf() {
    assert!(crypto::validate_passphrase("too short").is_err());
    assert!(crypto::validate_passphrase(&"a".repeat(1025)).is_err());
    assert!(crypto::validate_passphrase("测试口令至少十二个汉字字符").is_ok());
    assert!(crypto::encrypt(&vec![0; crypto::MAX_PLAINTEXT + 1], PASS).is_err());
    assert!(crypto::decrypt(&vec![0; crypto::MAX_FILE + 1], PASS).is_err());
}
#[test]
fn payload_rejects_unknown_fields_paths_duplicates_and_embedded_url_credentials() {
    let payload = incoming();
    for (path, key) in [
        ("root", "apiKey"),
        ("provider", "headers"),
        ("price", "token"),
        ("favorite", "title"),
    ] {
        let mut raw = serde_json::to_value(&payload).unwrap();
        let node = match path {
            "provider" => &mut raw["providers"][0],
            "price" => &mut raw["prices"][0],
            "favorite" => &mut raw["favorites"][0],
            _ => &mut raw,
        };
        node[key] = Value::String("not allowed".into());
        assert!(parse(&serde_json::to_vec(&raw).unwrap()).is_err());
    }
    for url in [
        "https://u:p@example.invalid/",
        "https://example.invalid/?token=x",
        "https://example.invalid/#secret",
        "file:///tmp/x",
        "https://example.invalid/\n",
    ] {
        let mut p = payload.clone();
        p.providers[0].endpoint = url.into();
        assert!(p.validate().is_err(), "{url}");
    }
    for id in ["../session", "C:\\session", "line\nbreak", " "] {
        assert!(favorite(id).validate().is_err());
    }
    let mut dup = payload.clone();
    dup.providers.push(dup.providers[0].clone());
    assert!(dup.validate().is_err());
    let mut next = payload;
    next.schema = 2;
    assert!(next.validate().is_err());
}
#[test]
fn export_excludes_credentials_advanced_config_prompts_usage_and_local_ids() {
    let db = db();
    let mut p = provider("local-credential-identity");
    p.advanced.codex.config_toml = "# secret-only-in-local-advanced".into();
    p.advanced.body_overrides.insert(
        "metadata".into(),
        Value::String("private-body-override".into()),
    );
    db.save_provider(&p).unwrap();
    db.set_setting("access-policy-secret", "do-not-export")
        .unwrap();
    let (payload, _) = store::export_snapshot(&db).unwrap();
    let text = serde_json::to_string(&payload).unwrap();
    for secret in [
        "local-credential-identity",
        "secret-only-in-local-advanced",
        "private-body-override",
        "do-not-export",
        "headerOverrides",
        "configToml",
        "currentProvider",
    ] {
        assert!(!text.contains(secret), "{secret}");
    }
    assert_eq!(payload.providers.len(), 1);
    assert_ne!(payload.providers[0].id, p.id);
    assert_eq!(store::export_snapshot(&db).unwrap().0, payload);
}
#[test]
fn favorites_migration_merges_once_preserves_original_and_does_not_resurrect_deletions() {
    let db = db();
    let raw = serde_json::json!({"version":1,"keys":[favorite("old").key(),favorite("old").key()]})
        .to_string();
    store::set_favorite(&db, &favorite("existing"), true).unwrap();
    assert_eq!(store::load_favorites(&db, Some(&raw)).unwrap().len(), 2);
    assert_eq!(
        db.get_setting("sync_favorites_legacy_backup_v1")
            .unwrap()
            .as_deref(),
        Some(raw.as_str())
    );
    store::set_favorite(&db, &favorite("old"), false).unwrap();
    assert_eq!(
        store::load_favorites(&db, Some(&raw)).unwrap(),
        vec![favorite("existing")]
    );
}
#[test]
fn invalid_legacy_favorites_never_overwrite_or_mark_migration_complete() {
    let db = db();
    store::set_favorite(&db, &favorite("keep"), true).unwrap();
    for raw in [
        "broken",
        r#"{"version":2,"keys":[]}"#,
        r#"{"version":1,"keys":[],"secret":"x"}"#,
        r#"{"version":1,"keys":["[\"codex\",\"../secret\"]"]}"#,
    ] {
        assert!(store::load_favorites(&db, Some(raw)).is_err());
        assert_eq!(count(&db, "session_favorites"), 1);
        assert!(
            db.get_setting("sync_favorites_migrated_v1")
                .unwrap()
                .is_none()
        );
    }
    assert_eq!(
        store::load_favorites(&db, None).unwrap(),
        vec![favorite("keep")]
    );
}
#[test]
fn preview_does_not_apply_incoming_and_every_changed_item_needs_a_choice() {
    let db = db();
    let p = incoming();
    let (rows, hash) = store::import_preview(&db, &p).unwrap();
    assert_eq!(count(&db, "providers"), 0);
    assert_eq!(count(&db, "model_pricing"), 0);
    assert_eq!(count(&db, "session_favorites"), 0);
    assert_eq!(count(&db, "sync_backups"), 0);
    assert!(store::apply(&db, &p, &hash, &[]).is_err());
    let mut choices = choose(&rows, true);
    choices.push(choices[0].clone());
    assert!(store::apply(&db, &p, &hash, &choices).is_err());
    let mut choices = choose(&rows, true);
    choices[0].key = "unknown".into();
    assert!(store::apply(&db, &p, &hash, &choices).is_err());
    assert!(
        store::apply(&db, &p, &hash, &choose(&rows, false))
            .unwrap_err()
            .contains("No incoming")
    );
    assert_eq!(count(&db, "sync_backups"), 0);
}
#[test]
fn import_uses_new_disabled_local_ids_even_if_incoming_uuid_is_an_orphan_credential_id() {
    let db = db();
    let p = incoming();
    let orphan = p.providers[0].id.clone();
    let receipt = apply_all(&db, &p);
    assert_eq!(receipt.applied, 3);
    assert_eq!(receipt.disabled_copies, 1);
    let providers = db.list_providers().unwrap();
    assert_eq!(providers.len(), 1);
    assert_ne!(providers[0].id, orphan);
    assert!(!providers[0].enabled);
    assert_eq!(store::export_snapshot(&db).unwrap().0, p);
    assert_eq!(count(&db, "governance_events"), 1);
    let (rows, hash) = store::import_preview(&db, &p).unwrap();
    assert!(rows.iter().all(|r| r.status == "unchanged"));
    assert!(store::apply(&db, &p, &hash, &[]).is_err());
}
#[test]
fn destination_changes_preserve_old_provider_and_advanced_secrets_and_create_disabled_copy() {
    let db = db();
    let mut local = provider("old-local");
    local.advanced.codex.config_toml = "# local-only-token".into();
    db.save_provider(&local).unwrap();
    let mut payload = store::export_snapshot(&db).unwrap().0;
    let identity = payload.providers[0].id.clone();
    payload.providers[0].endpoint = "https://new.example.invalid/v1".into();
    let (rows, _) = store::import_preview(&db, &payload).unwrap();
    assert_eq!(rows[0].effect, "keep_old_target_create_disabled_copy");
    let receipt = apply_all(&db, &payload);
    assert_eq!(receipt.disabled_copies, 1);
    assert_eq!(
        db.get_provider("old-local").unwrap().unwrap().endpoint,
        local.endpoint
    );
    let new = db
        .list_providers()
        .unwrap()
        .into_iter()
        .find(|p| p.id != "old-local")
        .unwrap();
    assert!(!new.enabled);
    assert_ne!(new.id, identity);
    assert!(new.advanced.codex.config_toml.is_empty());
    assert_eq!(
        db.get_provider("old-local")
            .unwrap()
            .unwrap()
            .advanced
            .codex
            .config_toml,
        "# local-only-token"
    );
    let exported = store::export_snapshot(&db).unwrap().0;
    assert_eq!(
        exported
            .providers
            .iter()
            .filter(|p| p.id == identity)
            .count(),
        1
    );
}
#[test]
fn same_destination_updates_only_whitelisted_fields_and_can_keep_price_conflicts_local() {
    let db = db();
    let mut local = provider("local");
    local.advanced.codex.config_toml = "# retain-local".into();
    db.save_provider(&local).unwrap();
    crate::model_pricing::save(&db, &price(3.0)).unwrap();
    let mut p = store::export_snapshot(&db).unwrap().0;
    p.providers[0].name = "Renamed".into();
    p.prices[0] = price(5.0);
    let (rows, hash) = store::import_preview(&db, &p).unwrap();
    let mut choices = choose(&rows, true);
    for c in &mut choices {
        if c.key.starts_with("price:") {
            c.use_incoming = false;
        }
    }
    store::apply(&db, &p, &hash, &choices).unwrap();
    let now = db.get_provider("local").unwrap().unwrap();
    assert_eq!(now.name, "Renamed");
    assert!(now.enabled);
    assert_eq!(now.advanced.codex.config_toml, "# retain-local");
    assert_eq!(
        store::export_snapshot(&db).unwrap().0.prices[0].input_per_million,
        3.0
    );
}
#[test]
fn stale_preview_is_rejected_after_provider_favorite_or_reference_price_change() {
    for change in 0..3 {
        let db = db();
        let p = incoming();
        let (rows, hash) = store::import_preview(&db, &p).unwrap();
        match change {
            0 => db.save_provider(&provider("new-local")).unwrap(),
            1 => store::set_favorite(&db, &favorite("new"), true).unwrap(),
            _ => crate::model_pricing::save(&db, &price(4.0)).unwrap(),
        }
        assert!(
            store::apply(&db, &p, &hash, &choose(&rows, true))
                .unwrap_err()
                .contains("changed after preview")
        );
        assert_eq!(count(&db, "sync_backups"), 0);
    }
}
#[test]
fn apply_failure_rolls_back_all_changes_backup_and_audit() {
    let db = db();
    let p = incoming();
    let (rows, hash) = store::import_preview(&db, &p).unwrap();
    {
        let c = db.conn();
        c.lock().unwrap().execute_batch("CREATE TRIGGER fail_audit BEFORE INSERT ON governance_events BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    }
    assert!(store::apply(&db, &p, &hash, &choose(&rows, true)).is_err());
    for table in [
        "providers",
        "model_pricing",
        "session_favorites",
        "sync_backups",
        "sync_provider_ids",
        "governance_events",
    ] {
        assert_eq!(count(&db, table), 0, "{table}");
    }
}
#[test]
fn rollback_restores_reference_price_and_favorites_but_retains_new_provider_and_request_history() {
    let db = db();
    store::set_favorite(&db, &favorite("before"), true).unwrap();
    crate::model_pricing::save(&db, &price(8.0)).unwrap();
    {
        let c = db.conn();
        c.lock()
            .unwrap()
            .execute(
                "UPDATE model_pricing SET source='models.dev',updated_at=123",
                [],
            )
            .unwrap();
    }
    let receipt = apply_all(&db, &incoming());
    let new = db.list_providers().unwrap()[0].clone();
    {
        let c = db.conn();
        c.lock().unwrap().execute("INSERT INTO request_events(provider_id,route,status_code,latency_ms,success,created_at) VALUES(?1,'sync-test',200,1,1,123)",[&new.id]).unwrap();
    }
    let (_, after, hash) = store::rollback_preview(&db, &receipt.backup_id).unwrap();
    assert_eq!(
        after["retainedDisabledProviders"].as_array().unwrap().len(),
        1
    );
    store::rollback(&db, &receipt.backup_id, &hash).unwrap();
    assert_eq!(count(&db, "request_events"), 1);
    assert!(!db.get_provider(&new.id).unwrap().unwrap().enabled);
    assert_eq!(
        store::load_favorites(&db, None).unwrap(),
        vec![favorite("before")]
    );
    let prices = crate::model_pricing::list(&db).unwrap();
    assert_eq!(prices[0].source, "models.dev");
    assert_eq!(prices[0].updated_at, 123);
    assert_eq!(prices[0].pricing.input_per_million, 8.0);
    assert!(store::rollback_preview(&db, &receipt.backup_id).is_err());
    assert!(store::backups(&db).unwrap()[0].used);
}
#[test]
fn rollback_cannot_overwrite_newer_changes_and_failure_is_atomic() {
    let db = db();
    let receipt = apply_all(&db, &incoming());
    let (_, _, hash) = store::rollback_preview(&db, &receipt.backup_id).unwrap();
    store::set_favorite(&db, &favorite("later"), true).unwrap();
    assert!(store::rollback(&db, &receipt.backup_id, &hash).is_err());
    assert!(store::rollback_preview(&db, &receipt.backup_id).is_err());
    store::set_favorite(&db, &favorite("later"), false).unwrap();
    let (_, _, hash) = store::rollback_preview(&db, &receipt.backup_id).unwrap();
    {
        let c = db.conn();
        c.lock().unwrap().execute_batch("CREATE TRIGGER fail_rollback BEFORE DELETE ON session_favorites BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    }
    assert!(store::rollback(&db, &receipt.backup_id, &hash).is_err());
    assert_eq!(count(&db, "session_favorites"), 1);
    assert!(!store::backups(&db).unwrap()[0].used);
    assert_eq!(count(&db, "governance_events"), 1);
}
#[test]
fn backups_are_bounded_and_integrity_checked() {
    let db = db();
    let mut p = incoming();
    let mut last = String::new();
    for n in 0..12 {
        p.providers[0].name = format!("Name {n}");
        last = apply_all(&db, &p).backup_id;
    }
    assert_eq!(store::backups(&db).unwrap().len(), 10);
    {
        let c = db.conn();
        c.lock()
            .unwrap()
            .execute(
                "UPDATE sync_backups SET content=x'7b7d' WHERE id=?1",
                [&last],
            )
            .unwrap();
    }
    assert!(
        store::rollback_preview(&db, &last)
            .unwrap_err()
            .contains("Invalid sync backup")
    );
}
#[test]
fn tickets_are_bounded_expiring_single_use_action_bound_and_cancellable() {
    let manager = SyncManager::default();
    let issue = || manager.issue(Action::Import, "hash".into(), Zeroizing::new(vec![]), ());
    let first = issue().unwrap();
    assert!(manager.take(&first.ticket_id, Action::Export).is_err());
    assert!(manager.take(&first.ticket_id, Action::Import).is_err());
    let first = issue().unwrap();
    manager.cancel(&first.ticket_id).unwrap();
    assert!(manager.take(&first.ticket_id, Action::Import).is_err());
    let first = issue().unwrap();
    manager
        .pending
        .lock()
        .unwrap()
        .get_mut(&first.ticket_id)
        .unwrap()
        .deadline = Instant::now() - Duration::from_secs(1);
    assert!(manager.take(&first.ticket_id, Action::Import).is_err());
    for _ in 0..MAX_TICKETS {
        issue().unwrap();
    }
    assert!(issue().is_err());
    assert!(confirm("IMPORT ", "IMPORT").is_err());
    assert!(confirm("ROLLBACK", "ROLLBACK SYNC").is_err());
}
#[test]
fn file_exchange_never_overwrites_and_rejects_directories_and_oversized_inputs() {
    let root =
        std::env::temp_dir().join(format!("xy-encrypted-sync-test-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let file = root.join("test.xysync");
    files::write_new(&file, b"ciphertext").unwrap();
    assert!(files::write_new(&file, b"replacement").is_err());
    assert_eq!(files::read(&file).unwrap(), b"ciphertext");
    assert!(files::read(&root).is_err());
    assert!(files::write_new(&root.join("wrong.json"), b"x").is_err());
    let large = root.join("large.xysync");
    std::fs::File::create(&large)
        .unwrap()
        .set_len((crypto::MAX_FILE + 1) as u64)
        .unwrap();
    assert!(files::read(&large).is_err());
    #[cfg(unix)]
    {
        let link = root.join("link.xysync");
        std::os::unix::fs::symlink(&file, &link).unwrap();
        assert!(files::read(&link).is_err());
        assert!(files::write_new(&link, b"x").is_err());
    }
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn favorite_and_payload_record_limits_are_enforced() {
    let db = db();
    {
        let c = db.conn();
        let mut c = c.lock().unwrap();
        let tx = c.transaction().unwrap();
        for n in 0..10000 {
            tx.execute(
                "INSERT INTO session_favorites VALUES('codex',?1)",
                [n.to_string()],
            )
            .unwrap();
        }
        tx.commit().unwrap();
    }
    assert!(store::set_favorite(&db, &favorite("overflow"), true).is_err());
    store::set_favorite(&db, &favorite("1"), true).unwrap();
    let mut p = incoming();
    p.favorites = vec![favorite("x"); 10001];
    assert!(p.validate().is_err());
}

#[test]
fn two_devices_exchange_real_encrypted_files_resolve_conflicts_and_rollback() {
    let device_a = db();
    let device_b = db();
    let mut original = provider("device-a-credential-id");
    original.app_group = "Claude".into();
    original.advanced.codex.config_toml = "# device-a-private-advanced".into();
    device_a.save_provider(&original).unwrap();
    let legacy =
        serde_json::json!({"version": 1, "keys": [favorite("session-a").key()]}).to_string();
    store::load_favorites(&device_a, Some(&legacy)).unwrap();
    store::load_favorites(&device_b, None).unwrap();
    store::set_favorite(&device_b, &favorite("session-b"), true).unwrap();
    crate::model_pricing::save(&device_a, &price(1.0)).unwrap();
    crate::model_pricing::save(&device_b, &price(9.0)).unwrap();

    let root = std::env::temp_dir().join(format!("xy-encrypted-sync-e2e-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir(&root).unwrap();
    let (before_a, _) = store::export_snapshot(&device_a).unwrap();
    let plaintext = encode(&before_a).unwrap();
    let text = std::str::from_utf8(&plaintext).unwrap();
    assert!(!text.contains("device-a-credential-id"));
    assert!(!text.contains("device-a-private-advanced"));
    let first = root.join("device-a.xysync");
    files::write_new(&first, &crypto::encrypt(&plaintext, PASS).unwrap()).unwrap();
    let from_a = parse(&crypto::decrypt(&files::read(&first).unwrap(), PASS).unwrap()).unwrap();
    assert_eq!(from_a, before_a);
    assert!(crypto::decrypt(&files::read(&first).unwrap(), "incorrect long passphrase").is_err());
    assert_eq!(count(&device_b, "providers"), 0);
    let (rows, hash) = store::import_preview(&device_b, &from_a).unwrap();
    assert_eq!(
        count(&device_b, "providers"),
        0,
        "preview must not apply incoming configuration"
    );
    assert_eq!(count(&device_b, "sync_backups"), 0);
    assert!(
        rows.iter()
            .any(|r| r.kind == "price" && r.status == "conflict")
    );
    let merged = store::apply(&device_b, &from_a, &hash, &choose(&rows, true)).unwrap();
    assert_eq!((merged.applied, merged.disabled_copies), (3, 1));
    let mut copied = device_b.list_providers().unwrap().remove(0);
    assert_ne!(copied.id, original.id);
    assert_ne!(copied.id, before_a.providers[0].id);
    assert!(!copied.enabled);
    assert_eq!(copied.app_group, "Claude");
    assert!(copied.advanced.codex.config_toml.is_empty());

    copied.name = "Renamed on device B".into();
    copied.enabled = true;
    copied.advanced.codex.config_toml = "# device-b-private-advanced".into();
    device_b.save_provider(&copied).unwrap();
    crate::model_pricing::save(&device_b, &price(2.0)).unwrap();
    let (snapshot_b, _) = store::export_snapshot(&device_b).unwrap();
    assert_eq!(snapshot_b.providers[0].id, before_a.providers[0].id);
    let second = root.join("device-b.xysync");
    files::write_new(
        &second,
        &crypto::encrypt(&encode(&snapshot_b).unwrap(), PASS).unwrap(),
    )
    .unwrap();
    let from_b = parse(&crypto::decrypt(&files::read(&second).unwrap(), PASS).unwrap()).unwrap();
    assert!(
        !std::str::from_utf8(&encode(&from_b).unwrap())
            .unwrap()
            .contains("device-b-private-advanced")
    );
    let (rows, hash) = store::import_preview(&device_a, &from_b).unwrap();
    assert!(
        rows.iter()
            .any(|r| r.kind == "provider" && r.status == "conflict")
    );
    let choices: Vec<_> = rows
        .iter()
        .filter(|r| r.status != "unchanged")
        .map(|r| Choice {
            key: r.key.clone(),
            use_incoming: r.kind != "provider",
        })
        .collect();
    let merged = store::apply(&device_a, &from_b, &hash, &choices).unwrap();
    assert_eq!((merged.applied, merged.disabled_copies), (2, 0));
    assert_eq!(
        serde_json::to_value(device_a.get_provider(&original.id).unwrap().unwrap()).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(
        store::load_favorites(&device_a, None).unwrap(),
        vec![favorite("session-a"), favorite("session-b")]
    );
    assert_eq!(
        store::export_snapshot(&device_a).unwrap().0.prices,
        vec![price(2.0)]
    );
    let (_, _, hash) = store::rollback_preview(&device_a, &merged.backup_id).unwrap();
    store::rollback(&device_a, &merged.backup_id, &hash).unwrap();
    assert_eq!(store::export_snapshot(&device_a).unwrap().0, before_a);
    assert_eq!(
        serde_json::to_value(device_a.get_provider(&original.id).unwrap().unwrap()).unwrap(),
        serde_json::to_value(&original).unwrap()
    );
    assert_eq!(
        serde_json::to_value(device_b.get_provider(&copied.id).unwrap().unwrap()).unwrap(),
        serde_json::to_value(&copied).unwrap()
    );
    assert!(store::backups(&device_a).unwrap()[0].used);
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}
