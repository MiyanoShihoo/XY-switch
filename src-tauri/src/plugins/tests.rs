use super::{runtime::public_address, store::PackageStore, validation::*, *};
use crate::database::Database;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Poll, Waker},
    time::Duration,
};

const CLIENT: &str = "codex-desktop";
const OTHER_CLIENT: &str = "claude-desktop";
const KEY_ID: &str = "test.publisher";

fn signing_key(seed: u8) -> Ed25519KeyPair {
    Ed25519KeyPair::from_seed_unchecked(&[seed; 32]).expect("synthetic signing seed")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn trusted_key(pair: &Ed25519KeyPair) -> TrustedSigningKey {
    TrustedSigningKey {
        id: KEY_ID.into(),
        label: "Synthetic publisher".into(),
        public_key: hex(pair.public_key().as_ref()),
        fingerprint: sha256_hex(pair.public_key().as_ref()),
        enabled: true,
    }
}

fn echo_package(version: &str) -> PluginPackage {
    let entry = json!({
        "schemaVersion": 1,
        "tools": [{"name": "echo", "description": "Return reviewed JSON.",
            "inputSchema": {"type": "object", "properties": {"message": {"type": "string", "maxLength": 1024}},
                "required": ["message"], "additionalProperties": false},
            "operation": {"type": "json_passthrough"}}]
    });
    PluginPackage {
        manifest: PluginManifest {
            schema_version: 1,
            id: "test.echo".into(),
            name: "Synthetic echo".into(),
            description: "A local test fixture.".into(),
            author: "Synthetic publisher".into(),
            version: version.into(),
            minimum_host_version: "0.0.0".into(),
            kind: PluginKind::Mcp,
            entry: "tools.json".into(),
            clients: vec![CLIENT.into(), OTHER_CLIENT.into()],
            permissions: PluginPermissions::default(),
            dependencies: vec![],
        },
        files: BTreeMap::from([("tools.json".into(), entry.to_string())]),
    }
}

fn signed(package: PluginPackage, pair: &Ed25519KeyPair) -> PublishedPlugin {
    let digest = package_digest(&package).expect("bounded synthetic package");
    let payload =
        signing_payload(&package.manifest.id, &package.manifest.version, &digest).unwrap();
    PublishedPlugin {
        package,
        digest,
        signature: hex(pair.sign(&payload).as_ref()),
        signing_key_id: KEY_ID.into(),
    }
}

fn reader_package(version: &str) -> PluginPackage {
    let mut package = echo_package(version);
    package.manifest.permissions.read_paths = vec!["package:docs/".into()];
    package
        .files
        .insert("docs/readme.txt".into(), "Reviewed package text.".into());
    package.files.insert("tools.json".into(), json!({
        "schemaVersion": 1,
        "tools": [{"name": "read", "description": "Read approved package text.",
            "inputSchema": {"type": "object", "properties": {"path": {"type": "string", "maxLength": 240}},
                "required": ["path"], "additionalProperties": false},
            "operation": {"type": "read_text", "pathArgument": "path"}}]
    }).to_string());
    package
}

fn withdrawal(published: &PublishedPlugin, pair: &Ed25519KeyPair) -> PluginWithdrawal {
    let mut notice = PluginWithdrawal {
        id: published.package.manifest.id.clone(),
        version: published.package.manifest.version.clone(),
        digest: published.digest.clone(),
        reason: WithdrawalReason::Security,
        signing_key_id: KEY_ID.into(),
        signature: String::new(),
    };
    notice.signature = hex(pair.sign(&withdrawal_payload(&notice).unwrap()).as_ref());
    notice
}

fn write_fixture_object(host: &TestHost, published: &PublishedPlugin) {
    let object = host.package_root.join(format!("{}.json", published.digest));
    if let Ok(metadata) = fs::symlink_metadata(&object) {
        assert!(metadata.is_file() && !metadata.file_type().is_symlink());
        make_writable(&object);
    }
    fs::write(&object, serde_json::to_vec(published).unwrap()).unwrap();
    let mut permissions = fs::metadata(&object).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(object, permissions).unwrap();
}

fn install_approval(
    preview: &InstallPreview,
    clients: &[&str],
    enable: bool,
) -> InstallConfirmation {
    InstallConfirmation {
        preview_id: preview.preview_id.clone(),
        digest: preview.validation.digest.clone(),
        accepted_permissions: preview.validation.manifest.permissions.clone(),
        clients: clients.iter().map(|client| (*client).into()).collect(),
        confirmed: true,
        enable,
    }
}

fn tool_approval(preview: &ToolCallPreview) -> ToolCallConfirmation {
    ToolCallConfirmation {
        preview_id: preview.preview_id.clone(),
        plugin_digest: preview.plugin_digest.clone(),
        arguments_digest: preview.arguments_digest.clone(),
        accepted_permissions: preview.required_permissions.clone(),
        confirmed: true,
    }
}

fn expect_error<Success>(result: PluginResult<Success>, code: &str) -> PluginError {
    match result {
        Err(failure) => {
            assert_eq!(failure.code, code);
            failure
        }
        Ok(_) => panic!("expected a controlled rejection: {code}"),
    }
}

struct TestHost {
    directory: PathBuf,
    package_root: PathBuf,
    manager: Option<PluginManager>,
}

impl TestHost {
    fn new() -> Self {
        let base = std::env::temp_dir();
        #[cfg(unix)]
        let base = fs::canonicalize(base).expect("real test temporary directory");
        let directory = base.join(format!("xy-plugin-tests-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&directory).expect("new isolated fixture directory");
        let package_root = directory.join("packages");
        let database =
            Database::open(&directory.join("metadata.sqlite")).expect("isolated fixture database");
        let manager = PluginManager::new(database, package_root.clone()).unwrap();
        Self {
            directory,
            package_root,
            manager: Some(manager),
        }
    }

    fn manager(&self) -> &PluginManager {
        self.manager.as_ref().unwrap()
    }

    fn trust(&self, pair: &Ed25519KeyPair) {
        let input = TrustedKeyInput {
            id: KEY_ID.into(),
            label: "Synthetic publisher".into(),
            public_key: hex(pair.public_key().as_ref()),
        };
        let preview = self.manager().preview_trusted_key(&input).unwrap();
        self.manager()
            .configure_trusted_key(input, &preview.fingerprint, true)
            .unwrap();
    }

    fn install(
        &self,
        published: &PublishedPlugin,
        clients: &[&str],
        enable: bool,
    ) -> InstalledPlugin {
        let preview = self.manager().preview_install(published.clone()).unwrap();
        self.manager()
            .confirm_install(install_approval(&preview, clients, enable))
            .unwrap()
    }
}

fn make_writable(path: &Path) {
    let mut permissions = fs::metadata(path).unwrap().permissions();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        permissions.set_mode(permissions.mode() | 0o200);
    }
    #[cfg(windows)]
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).unwrap();
}

fn cleanup_fixture(root: &Path, path: &Path) {
    if !path.starts_with(root) {
        return;
    }
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return;
    };
    #[cfg(windows)]
    let linked = {
        use std::os::windows::fs::MetadataExt;
        metadata.file_attributes() & 0x400 != 0
    };
    #[cfg(not(windows))]
    let linked = metadata.file_type().is_symlink();
    if linked {
        #[cfg(windows)]
        if metadata.is_dir() {
            let _ = fs::remove_dir(path);
            return;
        }
        let _ = fs::remove_file(path);
    } else if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                cleanup_fixture(root, &entry.path());
            }
        }
        let _ = fs::remove_dir(path);
    } else if metadata.is_file() {
        if metadata.permissions().readonly() {
            make_writable(path);
        }
        let _ = fs::remove_file(path);
    }
}

impl Drop for TestHost {
    fn drop(&mut self) {
        drop(self.manager.take());
        cleanup_fixture(&self.directory, &self.directory);
    }
}

#[test]
fn canonical_typed_package_and_signing_domain_are_stable() {
    let package = PluginPackage {
        manifest: PluginManifest {
            schema_version: 1,
            id: "example.fixture".into(),
            name: "Fixture".into(),
            description: "Approved text.".into(),
            author: "Test publisher".into(),
            version: "1.0.0".into(),
            minimum_host_version: "0.0.0".into(),
            kind: PluginKind::Skill,
            entry: "SKILL.md".into(),
            clients: vec![CLIENT.into()],
            permissions: PluginPermissions::default(),
            dependencies: vec![],
        },
        files: BTreeMap::from([
            ("b.txt".into(), "B".into()),
            ("SKILL.md".into(), "# Approved\n".into()),
        ]),
    };
    let expected = concat!(
        r##"{"manifest":{"schemaVersion":1,"id":"example.fixture","name":"Fixture","description":"Approved text.","author":"Test publisher","version":"1.0.0","minimumHostVersion":"0.0.0","kind":"skill","entry":"SKILL.md","clients":["codex-desktop"],"permissions":{"network":[],"readPaths":[],"writePaths":[],"process":false,"credentials":[]},"dependencies":[]},"files":{"SKILL.md":"# Approved\n","b.txt":"B"}}"##
    );
    assert_eq!(
        canonical_package_bytes(&package).unwrap(),
        expected.as_bytes()
    );
    assert_eq!(
        package_digest(&package).unwrap(),
        sha256_hex(expected.as_bytes())
    );
    let digest = "a".repeat(64);
    assert_eq!(
        signing_payload("example.fixture", "1.0.0", &digest).unwrap(),
        format!("xy-switch-plugin-v1\nexample.fixture\n1.0.0\n{digest}").as_bytes()
    );
    validate_package(&package).unwrap();
}

#[test]
fn synthetic_ed25519_signature_requires_the_exact_payload() {
    let pair = signing_key(7);
    let published = signed(echo_package("1.0.0"), &pair);
    let key = trusted_key(&pair);
    assert_eq!(
        verify_published(&published, &key).unwrap().digest,
        published.digest
    );
    let mut wrong = published.clone();
    wrong.signature = "00".repeat(64);
    expect_error(verify_published(&wrong, &key), "plugin_signature");
    let mut trailing_line =
        signing_payload(&published.package.manifest.id, "1.0.0", &published.digest).unwrap();
    trailing_line.push(b'\n');
    wrong.signature = hex(pair.sign(&trailing_line).as_ref());
    expect_error(verify_published(&wrong, &key), "plugin_signature");
    wrong.signature = published.signature.to_uppercase();
    expect_error(verify_published(&wrong, &key), "plugin_signature");
    expect_error(
        verify_published(&published, &trusted_key(&signing_key(8))),
        "plugin_signature",
    );
}

#[test]
fn an_embedded_public_key_cannot_establish_local_trust() {
    let host = TestHost::new();
    let pair = signing_key(7);
    let mut package = echo_package("1.0.0");
    package
        .files
        .insert("publisher.txt".into(), hex(pair.public_key().as_ref()));
    let published = signed(package, &pair);
    expect_error(
        host.manager().preview_install(published.clone()),
        "plugin_untrusted_key",
    );
    assert!(host.manager().list_trusted_keys().unwrap().is_empty());
    let mut envelope = serde_json::to_value(&published).unwrap();
    envelope["publicKey"] = json!(hex(pair.public_key().as_ref()));
    expect_error(parse_published_json(&envelope.to_string()), "plugin_schema");
    let mut key = trusted_key(&pair);
    key.enabled = false;
    expect_error(verify_published(&published, &key), "plugin_untrusted_key");
    key.enabled = true;
    key.fingerprint = "00".repeat(32);
    expect_error(verify_published(&published, &key), "plugin_untrusted_key");
}

#[test]
fn trust_configuration_requires_explicit_fingerprint_and_prevents_key_replacement() {
    let host = TestHost::new();
    let pair = signing_key(7);
    let input = TrustedKeyInput {
        id: KEY_ID.into(),
        label: "Synthetic publisher".into(),
        public_key: hex(pair.public_key().as_ref()),
    };
    let preview = host.manager().preview_trusted_key(&input).unwrap();
    expect_error(
        host.manager()
            .configure_trusted_key(input.clone(), &preview.fingerprint, false),
        "plugin_confirmation",
    );
    expect_error(
        host.manager()
            .configure_trusted_key(input.clone(), &"0".repeat(64), true),
        "plugin_confirmation",
    );
    host.manager()
        .configure_trusted_key(input.clone(), &preview.fingerprint, true)
        .unwrap();
    let replacement = TrustedKeyInput {
        public_key: hex(signing_key(8).public_key().as_ref()),
        ..input
    };
    let replacement_preview = host.manager().preview_trusted_key(&replacement).unwrap();
    expect_error(
        host.manager()
            .configure_trusted_key(replacement, &replacement_preview.fingerprint, true),
        "plugin_trust_conflict",
    );
}

#[test]
fn digest_and_signature_bind_the_entire_typed_package() {
    let pair = signing_key(7);
    let published = signed(echo_package("1.0.0"), &pair);
    for change in 0..3 {
        let mut replaced = published.clone();
        match change {
            0 => replaced.package.manifest.description = "Changed reviewed description.".into(),
            1 => {
                replaced
                    .package
                    .files
                    .insert("extra.txt".into(), "Changed package content.".into());
            }
            _ => replaced.package.manifest.clients = vec![CLIENT.into()],
        }
        expect_error(
            verify_published(&replaced, &trusted_key(&pair)),
            "plugin_digest",
        );
        replaced.digest = package_digest(&replaced.package).unwrap();
        expect_error(
            verify_published(&replaced, &trusted_key(&pair)),
            "plugin_signature",
        );
    }
    let mut wrong = published;
    wrong.digest = wrong.digest.to_uppercase();
    expect_error(
        verify_published(&wrong, &trusted_key(&pair)),
        "plugin_digest",
    );
}

#[test]
fn schema_rejects_unknown_missing_duplicate_and_non_text_fields() {
    let package = echo_package("1.0.0");
    let canonical = String::from_utf8(canonical_package_bytes(&package).unwrap()).unwrap();
    for input in [
        canonical.replacen(
            "\"schemaVersion\":1",
            "\"schemaVersion\":1,\"schemaVersion\":1",
            1,
        ),
        canonical.replacen(
            "\"schemaVersion\":1",
            "\"schemaVersion\":1,\"schema\\u0056ersion\":1",
            1,
        ),
    ] {
        expect_error(parse_package_json(&input), "plugin_schema");
    }
    for change in 0..4 {
        let mut value = serde_json::to_value(&package).unwrap();
        match change {
            0 => value["manifest"]["publicKey"] = json!("not-a-trust-source"),
            1 => {
                value["manifest"]
                    .as_object_mut()
                    .unwrap()
                    .remove("permissions");
            }
            2 => value["files"]["tools.json"] = json!([0, 1, 2]),
            _ => value["manifest"]["schemaVersion"] = json!(2),
        }
        expect_error(parse_package_json(&value.to_string()), "plugin_schema");
    }
    let mut duplicate_file = package;
    duplicate_file
        .files
        .insert("metadata.json".into(), r#"{"item":1,"item":2}"#.into());
    expect_error(validate_package(&duplicate_file), "plugin_schema");
    expect_error(
        strict_json(&"[".repeat(150), MAX_PACKAGE_BYTES),
        "plugin_schema",
    );
}

#[test]
fn malicious_paths_and_windows_file_aliases_are_rejected() {
    let unsafe_paths = [
        "../readme.txt",
        "docs/../readme.txt",
        "/absolute.txt",
        "C:/absolute.txt",
        "C:relative.txt",
        "docs\\readme.txt",
        "docs//readme.txt",
        "docs/./readme.txt",
        "readme.txt:stream",
        "readme.txt::$DATA",
        "CON.txt",
        "nul.md",
        "AUX.json",
        "COM1.txt",
        "lpt9.md",
        "PRN/readme.txt",
        "CONIN$.txt",
        "readme.txt.",
        "readme.txt ",
        ".hidden.txt",
        "docs/.hidden.txt",
        "..txt",
        "文档.txt",
        "docs~1/readme.txt",
    ];
    for path in unsafe_paths {
        let mut package = echo_package("1.0.0");
        package
            .files
            .insert(path.into(), "Harmless fixture content.".into());
        expect_error(validate_package(&package), "plugin_path");
    }
    for paths in [
        ["readme.txt", "README.txt"],
        ["docs/readme.txt", "DOCS/other.txt"],
        ["docs.txt", "docs.txt/readme.txt"],
    ] {
        let mut package = echo_package("1.0.0");
        for path in paths {
            package
                .files
                .insert(path.into(), "Harmless fixture content.".into());
        }
        expect_error(validate_package(&package), "plugin_path");
    }
    let mut executable = echo_package("1.0.0");
    executable
        .files
        .insert("payload.js".into(), "throw 0;".into());
    expect_error(validate_package(&executable), "plugin_entry");
}

#[test]
fn bounded_text_limits_are_checked_before_installation() {
    let mut package = echo_package("1.0.0");
    package
        .files
        .insert("large.txt".into(), "x".repeat(MAX_FILE_BYTES + 1));
    expect_error(validate_package(&package), "plugin_package_size");
    package.files.remove("large.txt");
    for index in 0..MAX_PACKAGE_FILES {
        package
            .files
            .insert(format!("file-{index}.txt"), "x".into());
    }
    expect_error(validate_package(&package), "plugin_package_size");
    let mut package = echo_package("1.0.0");
    for index in 0..9 {
        package
            .files
            .insert(format!("file-{index}.txt"), "x".repeat(MAX_FILE_BYTES));
    }
    expect_error(validate_package(&package), "plugin_package_size");
    expect_error(
        parse_published_json(&" ".repeat(MAX_PACKAGE_BYTES + 2049)),
        "plugin_package_size",
    );
}

#[test]
fn semantic_versions_dependencies_clients_and_permissions_are_strict() {
    for version in [
        "1.0",
        "01.0.0",
        "1.00.0",
        "1.0.0-01",
        "1.0.0+",
        "v1.0.0",
        "18446744073709551616.0.0",
    ] {
        expect_error(parse_version(version), "plugin_semver");
    }
    let precedence = [
        "1.0.0-alpha",
        "1.0.0-alpha.1",
        "1.0.0-alpha.beta",
        "1.0.0-beta",
        "1.0.0-beta.2",
        "1.0.0-beta.11",
        "1.0.0-rc.1",
        "1.0.0",
    ];
    for pair in precedence.windows(2) {
        assert!(parse_version(pair[0]).unwrap() < parse_version(pair[1]).unwrap());
    }
    assert_eq!(
        parse_version("1.0.0+first").unwrap(),
        parse_version("1.0.0+second").unwrap()
    );
    for dependency in [
        "test.other",
        "test.other@^1.0.0",
        "test.other@1.0.0@2.0.0",
        "Test.other@1.0.0",
    ] {
        expect_error(parse_dependency(dependency), "plugin_dependency");
    }
    for dependencies in [
        vec!["test.echo@1.0.0"],
        vec!["test.other@1.0.0", "test.other@2.0.0"],
    ] {
        let mut package = echo_package("1.0.0");
        package.manifest.dependencies = dependencies.into_iter().map(str::to_owned).collect();
        expect_error(validate_package(&package), "plugin_dependency");
    }
    let mut package = echo_package("1.0.0");
    package.manifest.minimum_host_version = "999.0.0".into();
    expect_error(validate_package(&package), "plugin_host_version");
    for clients in [
        vec![],
        vec![CLIENT, CLIENT],
        vec!["desktop"],
        vec!["claude-desktop", "unknown"],
    ] {
        let mut package = echo_package("1.0.0");
        package.manifest.clients = clients.into_iter().map(str::to_owned).collect();
        expect_error(validate_package(&package), "plugin_client");
    }
    for origin in [
        "*",
        "http://example.test",
        "https://127.0.0.1",
        "https://example.local",
        "https://example.test/path",
        "https://example.test/",
        "https://example.test:8443",
        "https://a@example.test",
    ] {
        let permissions = PluginPermissions {
            network: vec![origin.into()],
            ..Default::default()
        };
        expect_error(validate_permissions(&permissions), "plugin_permissions");
    }
    assert!(package_scope_allows("package:docs/", "docs/readme.txt"));
    assert!(!package_scope_allows(
        "package:docs/",
        "docs-other/readme.txt"
    ));
    assert!(!package_scope_allows(
        "package:docs/readme.txt",
        "docs/readme.txt.extra"
    ));
    expect_error(
        validate_permissions(&PluginPermissions {
            read_paths: vec!["package:docs/../".into()],
            ..Default::default()
        }),
        "plugin_permissions",
    );
    expect_error(
        validate_permissions(&PluginPermissions {
            network: vec!["https://example.test".into(); 2],
            ..Default::default()
        }),
        "plugin_permissions",
    );
}

#[test]
fn privacy_inspects_manifest_unused_files_and_decoded_json_without_echoing_secrets() {
    let synthetic_secret = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
    for change in 0..5 {
        let mut package = echo_package("1.0.0");
        match change {
            0 => {
                package
                    .files
                    .insert("unused.txt".into(), synthetic_secret.into());
            }
            1 => package.manifest.author = "fixture@example.test".into(),
            2 => package.manifest.description = "Phone 13800138000".into(),
            3 => {
                package.files.insert(
                    "unused.json".into(),
                    r#"{"pass\u0077ord":"opaque-fixture-secret"}"#.into(),
                );
            }
            _ => {
                package.files.insert(
                    "unused.json".into(),
                    r#"{"type":"input_file","file_data":"data:text/plain;base64,ZmFrZQ=="}"#.into(),
                );
            }
        }
        let failure = expect_error(validate_package(&package), "plugin_privacy");
        let diagnostic = serde_json::to_string(&failure).unwrap();
        for private in [
            synthetic_secret,
            "fixture@example.test",
            "13800138000",
            "opaque-fixture-secret",
            "ZmFrZQ==",
        ] {
            assert!(!diagnostic.contains(private));
        }
    }
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let mut package = echo_package("1.0.0");
    package
        .files
        .insert("unused.txt".into(), synthetic_secret.into());
    expect_error(
        host.manager().preview_install(signed(package, &pair)),
        "plugin_privacy",
    );
    assert!(host.manager().list_installed().unwrap().is_empty());
    assert_eq!(fs::read_dir(&host.package_root).unwrap().count(), 0);
}

#[test]
fn validated_dependency_components_do_not_disable_full_package_privacy() {
    let mut package = echo_package("1.0.0");
    package.manifest.id = "test.dependent".into();
    package.manifest.dependencies = vec!["test.echo@1.0.0".into()];
    let original = canonical_package_bytes(&package).unwrap();
    let validated = validate_package(&package).unwrap();
    assert_eq!(validated.digest, sha256_hex(&original));
    assert_eq!(canonical_package_bytes(&package).unwrap(), original);
    for dependency in [
        "test.sk-proj-abcdefghijklmnopqrstuvwxyz0123456789@1.0.0",
        "test.echo@1.0.0-sk-proj-abcdefghijklmnopqrstuvwxyz0123456789",
    ] {
        package.manifest.dependencies = vec![dependency.into()];
        let failure = expect_error(validate_package(&package), "plugin_privacy");
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("abcdefghijklmnopqrstuvwxyz")
        );
    }
    package.manifest.dependencies = vec!["test.echo@1.0.0".into()];
    for (name, value) in [
        ("unused.txt", "fixture@example.test"),
        (
            "unused.json",
            r##"{"dependencies":["fixture@example.test"],"note":"Approved"}"##,
        ),
    ] {
        package.files.insert(name.into(), value.into());
        expect_error(validate_package(&package), "plugin_privacy");
        package.files.remove(name);
    }
    package.manifest.author = "fixture@example.test".into();
    expect_error(validate_package(&package), "plugin_privacy");
}

#[test]
fn encoded_json_in_all_text_files_cannot_hide_secrets_or_duplicate_fields() {
    let private_json = r#"{"pass\u0077ord":"opaque-fixture-secret"}"#;
    let duplicate_json = r#"{"pass\u0077ord":"opaque-fixture-secret","password":""}"#;
    for text in [
        private_json.to_owned(),
        duplicate_json.to_owned(),
        json!(private_json).to_string(),
        json!(duplicate_json).to_string(),
    ] {
        let mut package = echo_package("1.0.0");
        package.files.insert("unused.txt".into(), text.clone());
        let failure = expect_error(validate_package(&package), "plugin_privacy");
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("opaque-fixture-secret")
        );
        package.files.remove("unused.txt");
        package.manifest.description = text;
        expect_error(validate_package(&package), "plugin_privacy");
    }
    let mut package = echo_package("1.0.0");
    package.files.insert(
        "notes.md".into(),
        "[TOC]\n\n{placeholder}\n\nA harmless Markdown example.".into(),
    );
    validate_package(&package).unwrap();
}

#[tokio::test]
async fn cancel_tool_preview_drops_only_the_requested_in_memory_approval() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    host.install(&signed(echo_package("1.0.0"), &pair), &[CLIENT], true);
    let cancelled = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "echo",
            json!({"message": "Cancelled review."}),
        )
        .unwrap();
    let retained = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "echo",
            json!({"message": "Retained review."}),
        )
        .unwrap();
    host.manager()
        .cancel_tool_preview(&cancelled.preview_id)
        .unwrap();
    {
        let state = host.manager().state.lock().unwrap();
        assert!(!state.tool_previews.contains_key(&cancelled.preview_id));
        assert!(state.tool_previews.contains_key(&retained.preview_id));
        assert_eq!(state.tool_previews.len(), 1);
    }
    host.manager()
        .cancel_tool_preview(&cancelled.preview_id)
        .unwrap();
    host.manager()
        .cancel_tool_preview(&uuid::Uuid::new_v4().to_string())
        .unwrap();
    expect_error(
        host.manager()
            .execute_tool(CLIENT, tool_approval(&cancelled))
            .await,
        "plugin_preview_expired",
    );
    let result = host
        .manager()
        .execute_tool(CLIENT, tool_approval(&retained))
        .await
        .unwrap();
    assert_eq!(result.value, json!({"message": "Retained review."}));
    host.manager()
        .cancel_tool_preview(&retained.preview_id)
        .unwrap();
    assert!(
        host.manager()
            .state
            .lock()
            .unwrap()
            .tool_previews
            .is_empty()
    );
}

#[test]
fn cancel_tool_preview_rejects_invalid_ids_with_static_errors() {
    let host = TestHost::new();
    let valid = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    for invalid in [
        "",
        "../preview",
        "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789",
        &valid.to_uppercase(),
        &valid.replace('-', ""),
    ] {
        let failure = expect_error(
            host.manager().cancel_tool_preview(invalid),
            "plugin_confirmation",
        );
        assert_eq!(failure, PluginError::new("plugin_confirmation"));
        assert!(!failure.message.contains(invalid) || invalid.is_empty());
    }
    assert!(
        host.manager()
            .state
            .lock()
            .unwrap()
            .tool_previews
            .is_empty()
    );
}

#[test]
fn cancel_tool_preview_never_waits_for_or_modifies_database_metadata() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    host.install(&signed(echo_package("1.0.0"), &pair), &[CLIENT], true);
    let preview = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "echo",
            json!({"message": "Discard this review."}),
        )
        .unwrap();
    let connection = host.manager().database.conn();
    let guard = connection.lock().unwrap();
    let before = super::manager::revision(&guard).unwrap();
    let manager = host.manager().clone();
    let preview_id = preview.preview_id.clone();
    let (sender, receiver) = std::sync::mpsc::channel();
    let cancellation = std::thread::spawn(move || {
        sender
            .send(manager.cancel_tool_preview(&preview_id))
            .unwrap()
    });
    let result = receiver.recv_timeout(Duration::from_secs(2));
    assert_eq!(super::manager::revision(&guard).unwrap(), before);
    drop(guard);
    cancellation.join().unwrap();
    result
        .expect("cancellation must not acquire the database lock")
        .unwrap();
    assert!(
        host.manager()
            .state
            .lock()
            .unwrap()
            .tool_previews
            .is_empty()
    );
}

#[test]
fn cancel_tool_preview_reports_a_poisoned_state_without_echoing_input() {
    let host = TestHost::new();
    let state = host.manager().state.clone();
    let poisoned = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = state.lock().unwrap();
        panic!("synthetic state lock failure");
    }));
    assert!(poisoned.is_err());
    let failure = expect_error(
        host.manager()
            .cancel_tool_preview("aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee"),
        "plugin_database",
    );
    assert_eq!(failure, PluginError::new("plugin_database"));
}

#[test]
fn install_confirmation_is_exact_one_use_and_versions_cannot_be_replaced() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let published = signed(echo_package("1.0.0"), &pair);
    for change in 0..6 {
        let preview = host.manager().preview_install(published.clone()).unwrap();
        let mut confirmation = install_approval(&preview, &[CLIENT], true);
        let expected = match change {
            0 => {
                confirmation.confirmed = false;
                "plugin_confirmation"
            }
            1 => {
                confirmation.digest = "0".repeat(64);
                "plugin_confirmation"
            }
            2 => {
                confirmation
                    .accepted_permissions
                    .network
                    .push("https://example.test".into());
                "plugin_confirmation"
            }
            3 => {
                confirmation.clients.clear();
                "plugin_confirmation"
            }
            4 => {
                confirmation.clients.push(CLIENT.into());
                "plugin_confirmation"
            }
            _ => {
                confirmation.clients = vec!["desktop".into()];
                "plugin_client"
            }
        };
        expect_error(host.manager().confirm_install(confirmation), expected);
        expect_error(
            host.manager()
                .confirm_install(install_approval(&preview, &[CLIENT], true)),
            "plugin_preview_expired",
        );
        assert!(host.manager().list_installed().unwrap().is_empty());
        assert_eq!(fs::read_dir(&host.package_root).unwrap().count(), 0);
    }
    let original_preview = host.manager().preview_install(published.clone()).unwrap();
    let mut replacement = published.package.clone();
    replacement.manifest.description = "Replaced package description.".into();
    let replacement = signed(replacement, &pair);
    let replacement_preview = host.manager().preview_install(replacement.clone()).unwrap();
    let installed = host
        .manager()
        .confirm_install(install_approval(&original_preview, &[CLIENT], true))
        .unwrap();
    assert_eq!(installed.digest.as_deref(), Some(published.digest.as_str()));
    assert_eq!(installed.enabled_clients, vec![CLIENT]);
    expect_error(
        host.manager()
            .confirm_install(install_approval(&replacement_preview, &[CLIENT], true)),
        "plugin_preview_expired",
    );
    expect_error(
        host.manager().preview_install(replacement),
        "plugin_immutable_version",
    );
    assert_eq!(fs::read_dir(&host.package_root).unwrap().count(), 1);
    assert!(
        fs::metadata(host.package_root.join(format!("{}.json", published.digest)))
            .unwrap()
            .permissions()
            .readonly()
    );
    let connection = host.manager().database.conn();
    assert!(
        connection
            .lock()
            .unwrap()
            .execute("UPDATE xy_plugin_versions SET digest=?1", ["0".repeat(64)])
            .is_err()
    );
}

#[tokio::test]
async fn an_upgrade_with_added_permissions_and_new_clients_requires_fresh_full_approval() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let old = signed(echo_package("1.0.0"), &pair);
    host.install(&old, &[CLIENT, OTHER_CLIENT], true);
    let pending = host
        .manager()
        .preview_tool_call(
            "test.echo",
            OTHER_CLIENT,
            "echo",
            json!({"message": "Old approval."}),
        )
        .unwrap();
    let upgraded = signed(reader_package("2.0.0"), &pair);
    let preview = host.manager().preview_install(upgraded.clone()).unwrap();
    assert_eq!(preview.previous_version.as_deref(), Some("1.0.0"));
    assert!(
        preview
            .permission_changes
            .iter()
            .all(|change| change.added.read_paths == vec!["package:docs/"])
    );
    let mut insufficient = install_approval(&preview, &[CLIENT], true);
    insufficient.accepted_permissions = PluginPermissions::default();
    expect_error(
        host.manager().confirm_install(insufficient),
        "plugin_confirmation",
    );
    assert_eq!(
        host.manager()
            .installed("test.echo")
            .unwrap()
            .active_version
            .as_deref(),
        Some("1.0.0")
    );
    let installed = host.install(&upgraded, &[CLIENT], true);
    assert_eq!(installed.enabled_clients, vec![CLIENT]);
    assert!(
        host.manager()
            .state
            .lock()
            .unwrap()
            .tool_previews
            .is_empty()
    );
    expect_error(
        host.manager()
            .execute_tool(OTHER_CLIENT, tool_approval(&pending))
            .await,
        "plugin_preview_expired",
    );
    assert!(
        host.manager()
            .tools_for_client(OTHER_CLIENT)
            .unwrap()
            .is_empty()
    );
    expect_error(
        host.manager()
            .enable_plugin_client("test.echo", OTHER_CLIENT, &upgraded.digest, true),
        "plugin_confirmation",
    );
    let review = host.manager().preview_installed("test.echo").unwrap();
    assert!(
        review
            .permission_changes
            .iter()
            .find(|change| change.client == CLIENT)
            .unwrap()
            .added
            .is_empty()
    );
    assert_eq!(
        review
            .permission_changes
            .iter()
            .find(|change| change.client == OTHER_CLIENT)
            .unwrap()
            .added
            .read_paths,
        vec!["package:docs/"]
    );
    host.manager()
        .confirm_install(install_approval(&review, &[CLIENT, OTHER_CLIENT], true))
        .unwrap();
    assert_eq!(
        host.manager().tools_for_client(OTHER_CLIENT).unwrap()[0]
            .tool
            .name,
        "read"
    );
    host.manager()
        .disable_plugin("test.echo", Some(CLIENT))
        .unwrap();
    assert!(host.manager().tools_for_client(CLIENT).unwrap().is_empty());
    expect_error(
        host.manager()
            .enable_plugin_client("test.echo", CLIENT, &old.digest, true),
        "plugin_confirmation",
    );
    assert_eq!(
        host.manager()
            .enable_plugin_client("test.echo", CLIENT, &upgraded.digest, true)
            .unwrap()
            .enabled_clients,
        vec![CLIENT, OTHER_CLIENT]
    );
}

#[tokio::test]
async fn rollback_and_reinstall_use_immutable_history_and_reauthorize_permissions() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let original = signed(reader_package("1.0.0"), &pair);
    let upgraded = signed(echo_package("2.0.0"), &pair);
    host.install(&original, &[CLIENT, OTHER_CLIENT], true);
    host.install(&upgraded, &[CLIENT, OTHER_CLIENT], true);
    let pending = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "echo",
            json!({"message": "Before rollback."}),
        )
        .unwrap();
    expect_error(
        host.manager().preview_install(original.clone()),
        "plugin_immutable_version",
    );
    let preview = host
        .manager()
        .preview_rollback("test.echo", "1.0.0")
        .unwrap();
    assert_eq!(preview.validation.digest, original.digest);
    assert!(
        preview
            .permission_changes
            .iter()
            .all(|change| change.added.read_paths == vec!["package:docs/"])
    );
    let restored = host
        .manager()
        .confirm_install(install_approval(&preview, &[OTHER_CLIENT], true))
        .unwrap();
    assert_eq!(restored.active_version.as_deref(), Some("1.0.0"));
    assert_eq!(restored.enabled_clients, vec![OTHER_CLIENT]);
    assert!(host.manager().tools_for_client(CLIENT).unwrap().is_empty());
    expect_error(
        host.manager()
            .execute_tool(CLIENT, tool_approval(&pending))
            .await,
        "plugin_preview_expired",
    );
    expect_error(
        host.manager().preview_rollback("test.echo", "2.0.0"),
        "plugin_immutable_version",
    );
    expect_error(
        host.manager().uninstall_plugin("test.echo", false),
        "plugin_confirmation",
    );
    let removed = host.manager().uninstall_plugin("test.echo", true).unwrap();
    assert!(!removed.installed);
    assert!(removed.active_version.is_none());
    assert!(removed.enabled_clients.is_empty());
    assert_eq!(removed.versions.len(), 2);
    assert_eq!(fs::read_dir(&host.package_root).unwrap().count(), 2);
    let review = host
        .manager()
        .preview_rollback("test.echo", "1.0.0")
        .unwrap();
    let restored = host
        .manager()
        .confirm_install(install_approval(&review, &[CLIENT], true))
        .unwrap();
    assert_eq!(restored.enabled_clients, vec![CLIENT]);
    assert!(
        host.manager()
            .tools_for_client(OTHER_CLIENT)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn signed_withdrawals_bind_reason_version_digest_and_original_publisher() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let original = signed(echo_package("1.0.0"), &pair);
    let upgraded = signed(echo_package("2.0.0"), &pair);
    host.install(&original, &[CLIENT], true);
    host.install(&upgraded, &[CLIENT], true);
    let notice = withdrawal(&original, &pair);
    assert_eq!(
        withdrawal_payload(&notice).unwrap(),
        format!(
            "xy-switch-plugin-revocation-v1\ntest.echo\n1.0.0\n{}\nsecurity",
            original.digest
        )
        .as_bytes()
    );
    for change in 0..4 {
        let mut wrong = notice.clone();
        match change {
            0 => wrong.signature = "0".repeat(128),
            1 => wrong.reason = WithdrawalReason::Privacy,
            2 => wrong.digest = "0".repeat(64),
            _ => wrong.signing_key_id = "test.attacker".into(),
        }
        expect_error(host.manager().apply_withdrawal(wrong), "plugin_signature");
        assert_eq!(
            host.manager()
                .installed("test.echo")
                .unwrap()
                .enabled_clients,
            vec![CLIENT]
        );
    }
    let mut extra = serde_json::to_value(&notice).unwrap();
    extra["publicKey"] = json!(hex(pair.public_key().as_ref()));
    expect_error(
        host.manager().apply_withdrawal_json(&extra.to_string()),
        "plugin_schema",
    );
    let installed = host.manager().apply_withdrawal(notice.clone()).unwrap();
    assert_eq!(installed.active_version.as_deref(), Some("2.0.0"));
    assert_eq!(installed.enabled_clients, vec![CLIENT]);
    assert!(
        installed
            .versions
            .iter()
            .find(|version| version.version == "1.0.0")
            .unwrap()
            .withdrawn
    );
    expect_error(
        host.manager().preview_rollback("test.echo", "1.0.0"),
        "plugin_withdrawn",
    );
    host.manager().apply_withdrawal(notice).unwrap();
    let pending = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "echo",
            json!({"message": "Before withdrawal."}),
        )
        .unwrap();
    let installed = host
        .manager()
        .apply_withdrawal(withdrawal(&upgraded, &pair))
        .unwrap();
    assert_eq!(installed.verification, "plugin_withdrawn");
    assert!(installed.enabled_clients.is_empty());
    assert!(
        host.manager()
            .state
            .lock()
            .unwrap()
            .tool_previews
            .get(&pending.preview_id)
            .is_none()
    );
    assert!(host.manager().tools_for_client(CLIENT).unwrap().is_empty());
    expect_error(host.manager().preview_install(upgraded), "plugin_withdrawn");
    host.manager().uninstall_plugin("test.echo", true).unwrap();
}

#[tokio::test]
async fn successful_host_mutations_immediately_drop_all_pending_reviews() {
    for change in 0..7 {
        let host = TestHost::new();
        let pair = signing_key(7);
        host.trust(&pair);
        let published = signed(echo_package("1.0.0"), &pair);
        host.install(&published, &[CLIENT, OTHER_CLIENT], true);
        if change == 6 {
            host.manager()
                .disable_plugin("test.echo", Some(CLIENT))
                .unwrap();
        }
        let tool = host
            .manager()
            .preview_tool_call(
                "test.echo",
                OTHER_CLIENT,
                "echo",
                json!({"message": "Pending review."}),
            )
            .unwrap();
        let install = host.manager().preview_installed("test.echo").unwrap();
        match change {
            0 => {
                host.manager().disable_plugin("test.echo", None).unwrap();
            }
            1 => {
                host.manager().uninstall_plugin("test.echo", true).unwrap();
            }
            2 => host.manager().revoke_trusted_key(KEY_ID).unwrap(),
            3 => {
                let key = TrustedKeyInput {
                    id: "test.second".into(),
                    label: "Second synthetic publisher".into(),
                    public_key: hex(signing_key(9).public_key().as_ref()),
                };
                let review = host.manager().preview_trusted_key(&key).unwrap();
                host.manager()
                    .configure_trusted_key(key, &review.fingerprint, true)
                    .unwrap();
            }
            4 => {
                host.install(&published, &[CLIENT], true);
            }
            5 => {
                host.manager()
                    .apply_withdrawal(withdrawal(&published, &pair))
                    .unwrap();
            }
            _ => {
                host.manager()
                    .enable_plugin_client("test.echo", CLIENT, &published.digest, true)
                    .unwrap();
            }
        }
        assert!(
            !host
                .manager()
                .state
                .lock()
                .unwrap()
                .tool_previews
                .contains_key(&tool.preview_id)
        );
        assert!(
            host.manager()
                .state
                .lock()
                .unwrap()
                .tool_previews
                .is_empty()
        );
        expect_error(
            host.manager()
                .confirm_install(install_approval(&install, &[CLIENT], true)),
            "plugin_preview_expired",
        );
        expect_error(
            host.manager()
                .execute_tool(OTHER_CLIENT, tool_approval(&tool))
                .await,
            "plugin_preview_expired",
        );
    }
}

#[tokio::test]
async fn tool_execution_requires_exact_client_arguments_digest_and_permissions_once() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let published = signed(echo_package("1.0.0"), &pair);
    host.install(&published, &[CLIENT, OTHER_CLIENT], true);
    let arguments = json!({"message": "Reviewed input."});
    for change in 0..5 {
        let preview = host
            .manager()
            .preview_tool_call("test.echo", CLIENT, "echo", arguments.clone())
            .unwrap();
        assert_eq!(
            preview.arguments_digest,
            sha256_hex(&serde_json::to_vec(&arguments).unwrap())
        );
        assert!(preview.required_permissions.is_empty());
        assert!(
            !serde_json::to_string(&preview)
                .unwrap()
                .contains("Reviewed input.")
        );
        let mut confirmation = tool_approval(&preview);
        let client = match change {
            0 => {
                confirmation.confirmed = false;
                CLIENT
            }
            1 => {
                confirmation.plugin_digest = "0".repeat(64);
                CLIENT
            }
            2 => {
                confirmation.arguments_digest = sha256_hex(br#"{"message":"Changed input."}"#);
                CLIENT
            }
            3 => {
                confirmation.accepted_permissions.process = true;
                CLIENT
            }
            _ => OTHER_CLIENT,
        };
        expect_error(
            host.manager().execute_tool(client, confirmation).await,
            "plugin_confirmation",
        );
        expect_error(
            host.manager()
                .execute_tool(CLIENT, tool_approval(&preview))
                .await,
            "plugin_preview_expired",
        );
    }
    let preview = host
        .manager()
        .preview_tool_call("test.echo", CLIENT, "echo", arguments.clone())
        .unwrap();
    let result = host
        .manager()
        .execute_tool(CLIENT, tool_approval(&preview))
        .await
        .unwrap();
    assert_eq!(result.value, arguments);
    assert_eq!(result.digest, published.digest);
    assert!(!result.redacted);
    expect_error(
        host.manager()
            .execute_tool(CLIENT, tool_approval(&preview))
            .await,
        "plugin_preview_expired",
    );
    assert_eq!(fs::read_dir(&host.package_root).unwrap().count(), 1);
}

#[tokio::test]
async fn scoped_execution_requires_unique_clients_and_the_preview_client_in_scope() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    host.install(
        &signed(echo_package("1.0.0"), &pair),
        &[CLIENT, OTHER_CLIENT],
        true,
    );
    let scope = vec![OTHER_CLIENT.to_owned(), CLIENT.to_owned()];
    let preview = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "echo",
            json!({"message": "Shared reviewed input."}),
        )
        .unwrap();
    let result = host
        .manager()
        .execute_tool_for_clients(&scope, tool_approval(&preview))
        .await
        .unwrap();
    assert_eq!(result.client, CLIENT);
    assert_eq!(result.value, json!({"message": "Shared reviewed input."}));
    for invalid in [
        vec![],
        vec![CLIENT.to_owned(), CLIENT.to_owned()],
        vec![CLIENT.to_owned(), "unknown-client".into()],
    ] {
        let preview = host
            .manager()
            .preview_tool_call(
                "test.echo",
                CLIENT,
                "echo",
                json!({"message": "Invalid scope."}),
            )
            .unwrap();
        expect_error(
            host.manager()
                .execute_tool_for_clients(&invalid, tool_approval(&preview))
                .await,
            "plugin_client",
        );
        host.manager()
            .cancel_tool_preview(&preview.preview_id)
            .unwrap();
    }
    let preview = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "echo",
            json!({"message": "Wrong scope anchor."}),
        )
        .unwrap();
    expect_error(
        host.manager()
            .execute_tool_for_clients(&[OTHER_CLIENT.into()], tool_approval(&preview))
            .await,
        "plugin_confirmation",
    );
    expect_error(
        host.manager()
            .execute_tool(CLIENT, tool_approval(&preview))
            .await,
        "plugin_preview_expired",
    );
}

#[tokio::test]
async fn scoped_execution_checks_secondary_grants_even_without_a_revision_change() {
    for change in 0..3 {
        let host = TestHost::new();
        let pair = signing_key(7);
        host.trust(&pair);
        host.install(
            &signed(echo_package("1.0.0"), &pair),
            &[CLIENT, OTHER_CLIENT],
            true,
        );
        let preview = host
            .manager()
            .preview_tool_call(
                "test.echo",
                CLIENT,
                "echo",
                json!({"message": "Secondary grant check."}),
            )
            .unwrap();
        {
            let _state = host.manager().state.lock().unwrap();
            let connection = host.manager().database.conn();
            let connection = connection.lock().unwrap();
            let sql = match change {
                0 => "UPDATE xy_plugin_grants SET enabled=0 WHERE client=?1",
                1 => "UPDATE xy_plugin_grants SET digest='invalid' WHERE client=?1",
                _ => "UPDATE xy_plugin_grants SET permissions_json='{}' WHERE client=?1",
            };
            connection.execute(sql, [OTHER_CLIENT]).unwrap();
        }
        let code = if change == 2 {
            "plugin_confirmation"
        } else {
            "plugin_disabled"
        };
        expect_error(
            host.manager()
                .execute_tool_for_clients(
                    &[CLIENT.into(), OTHER_CLIENT.into()],
                    tool_approval(&preview),
                )
                .await,
            code,
        );
        assert_eq!(host.manager().state.lock().unwrap().in_flight, 0);
        let single = host
            .manager()
            .preview_tool_call(
                "test.echo",
                CLIENT,
                "echo",
                json!({"message": "Independent client."}),
            )
            .unwrap();
        host.manager()
            .execute_tool(CLIENT, tool_approval(&single))
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn desktop_cli_scopes_recheck_valid_permission_reductions_without_revision_changes() {
    for (desktop, cli) in [
        ("codex-desktop", "codex-cli"),
        ("opencode-desktop", "opencode-cli"),
    ] {
        for changed_dependency in [false, true] {
            let host = TestHost::new();
            let pair = signing_key(7);
            host.trust(&pair);
            let clients = [desktop, cli];
            let mut package = reader_package("1.0.0");
            package.manifest.clients = clients.iter().map(|client| (*client).into()).collect();
            if changed_dependency {
                let mut dependency = package.clone();
                dependency.manifest.id = "test.reader-dependency".into();
                host.install(&signed(dependency, &pair), &clients, true);
                package.manifest.dependencies = vec!["test.reader-dependency@1.0.0".into()];
            }
            host.install(&signed(package, &pair), &clients, true);
            let preview = host
                .manager()
                .preview_tool_call(
                    "test.echo",
                    desktop,
                    "read",
                    json!({"path": "docs/readme.txt"}),
                )
                .unwrap();
            let reduced = PluginPermissions::default();
            validate_permissions(&reduced).unwrap();
            {
                let _state = host.manager().state.lock().unwrap();
                let connection = host.manager().database.conn();
                let connection = connection.lock().unwrap();
                let before = super::manager::revision(&connection).unwrap();
                connection
                    .execute(
                        "UPDATE xy_plugin_grants SET permissions_json=?1 WHERE plugin_id=?2 AND client=?3",
                        rusqlite::params![
                            serde_json::to_string(&reduced).unwrap(),
                            if changed_dependency {
                                "test.reader-dependency"
                            } else {
                                "test.echo"
                            },
                            cli
                        ],
                    )
                    .unwrap();
                assert_eq!(super::manager::revision(&connection).unwrap(), before);
            }
            expect_error(
                host.manager()
                    .execute_tool_for_clients(
                        &[desktop.to_owned(), cli.to_owned()],
                        tool_approval(&preview),
                    )
                    .await,
                if changed_dependency {
                    "plugin_dependency"
                } else {
                    "plugin_disabled"
                },
            );
            assert_eq!(host.manager().state.lock().unwrap().in_flight, 0);
            let single = host
                .manager()
                .preview_tool_call(
                    "test.echo",
                    desktop,
                    "read",
                    json!({"path": "docs/readme.txt"}),
                )
                .unwrap();
            let result = host
                .manager()
                .execute_tool(desktop, tool_approval(&single))
                .await
                .unwrap();
            assert_eq!(result.client, desktop);
            assert_eq!(result.value, json!("Reviewed package text."));
        }
    }
}

#[tokio::test]
async fn scoped_execution_checks_secondary_dependency_grants() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    host.install(
        &signed(echo_package("1.0.0"), &pair),
        &[CLIENT, OTHER_CLIENT],
        true,
    );
    let mut dependent = echo_package("1.0.0");
    dependent.manifest.id = "test.dependent".into();
    dependent.manifest.dependencies = vec!["test.echo@1.0.0".into()];
    host.install(&signed(dependent, &pair), &[CLIENT, OTHER_CLIENT], true);
    let preview = host
        .manager()
        .preview_tool_call(
            "test.dependent",
            CLIENT,
            "echo",
            json!({"message": "Pinned dependency check."}),
        )
        .unwrap();
    {
        let _state = host.manager().state.lock().unwrap();
        let connection = host.manager().database.conn();
        connection
            .lock()
            .unwrap()
            .execute(
                "UPDATE xy_plugin_grants SET enabled=0 WHERE plugin_id='test.echo' AND client=?1",
                [OTHER_CLIENT],
            )
            .unwrap();
    }
    expect_error(
        host.manager()
            .execute_tool_for_clients(
                &[CLIENT.into(), OTHER_CLIENT.into()],
                tool_approval(&preview),
            )
            .await,
        "plugin_dependency",
    );
}

#[tokio::test]
async fn scoped_authorization_blocks_revocation_before_the_first_effect_poll() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    host.install(
        &signed(echo_package("1.0.0"), &pair),
        &[CLIENT, OTHER_CLIENT],
        true,
    );
    let preview = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "echo",
            json!({"message": "Prepared network effect."}),
        )
        .unwrap();
    let pending = host
        .manager()
        .state
        .lock()
        .unwrap()
        .tool_previews
        .remove(&preview.preview_id)
        .unwrap();
    let manager = host.manager().clone();
    std::thread::spawn(move || {
        manager
            .disable_plugin("test.echo", Some(OTHER_CLIENT))
            .unwrap()
    })
    .join()
    .unwrap();
    let effects = AtomicUsize::new(0);
    let effect = std::future::poll_fn(|_| {
        effects.fetch_add(1, Ordering::SeqCst);
        Poll::Ready(Ok(json!({"status": 200})))
    });
    expect_error(
        host.manager()
            .poll_tool_authorized(&pending, &[CLIENT.into(), OTHER_CLIENT.into()], effect)
            .await,
        "plugin_preview_expired",
    );
    assert_eq!(effects.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_pending_effect_releases_locks_and_cannot_resume_after_secondary_revocation() {
    for changed_dependency in [false, true] {
        let host = TestHost::new();
        let pair = signing_key(7);
        host.trust(&pair);
        host.install(
            &signed(echo_package("1.0.0"), &pair),
            &[CLIENT, OTHER_CLIENT],
            true,
        );
        let mut package = echo_package("1.0.0");
        package.manifest.id = "test.shared".into();
        if changed_dependency {
            package.manifest.dependencies = vec!["test.echo@1.0.0".into()];
        }
        host.install(&signed(package, &pair), &[CLIENT, OTHER_CLIENT], true);
        let preview = host
            .manager()
            .preview_tool_call(
                "test.shared",
                CLIENT,
                "echo",
                json!({"message": "Waiting effect."}),
            )
            .unwrap();
        let pending = host
            .manager()
            .state
            .lock()
            .unwrap()
            .tool_previews
            .remove(&preview.preview_id)
            .unwrap();
        let first_poll = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(Mutex::new(None::<Waker>));
        let polls = AtomicUsize::new(0);
        let effects = AtomicUsize::new(0);
        let connection = host.manager().database.conn();
        let effect = std::future::poll_fn(|context| {
            assert!(host.manager().state.try_lock().is_err());
            assert!(connection.try_lock().is_err());
            if polls.fetch_add(1, Ordering::SeqCst) == 0 {
                *resume.lock().unwrap() = Some(context.waker().clone());
                first_poll.notify_one();
                Poll::Pending
            } else {
                effects.fetch_add(1, Ordering::SeqCst);
                Poll::Ready(Ok(json!({"status": 200})))
            }
        });
        let scope = vec![CLIENT.into(), OTHER_CLIENT.into()];
        let revoke = async {
            first_poll.notified().await;
            assert!(host.manager().state.try_lock().is_ok());
            assert!(connection.try_lock().is_ok());
            let _state = host.manager().state.lock().unwrap();
            connection
                .lock()
                .unwrap()
                .execute(
                    "UPDATE xy_plugin_grants SET enabled=0 WHERE plugin_id=?1 AND client=?2",
                    rusqlite::params![
                        if changed_dependency {
                            "test.echo"
                        } else {
                            "test.shared"
                        },
                        OTHER_CLIENT
                    ],
                )
                .unwrap();
            drop(_state);
            resume.lock().unwrap().take().unwrap().wake();
        };
        let (result, ()) = tokio::time::timeout(Duration::from_secs(2), async {
            tokio::join!(
                host.manager()
                    .poll_tool_authorized(&pending, &scope, effect),
                revoke
            )
        })
        .await
        .expect("synthetic pending effect must wake after revocation");
        expect_error(
            result,
            if changed_dependency {
                "plugin_dependency"
            } else {
                "plugin_disabled"
            },
        );
        assert_eq!(polls.load(Ordering::SeqCst), 1);
        assert_eq!(effects.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn invalid_and_private_tool_arguments_never_enter_the_pending_state() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    host.install(&signed(echo_package("1.0.0"), &pair), &[CLIENT], true);
    for arguments in [
        json!({}),
        json!({"message": 1}),
        json!({"message": "Reviewed", "extra": true}),
        json!({"message": "x".repeat(1025)}),
    ] {
        expect_error(
            host.manager()
                .preview_tool_call("test.echo", CLIENT, "echo", arguments),
            "plugin_tool_arguments",
        );
    }
    let private = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
    let failure = expect_error(
        host.manager()
            .preview_tool_call("test.echo", CLIENT, "echo", json!({"message": private})),
        "plugin_privacy",
    );
    assert!(!serde_json::to_string(&failure).unwrap().contains(private));
    expect_error(
        host.manager().preview_tool_call_json(
            "test.echo",
            CLIENT,
            "echo",
            r#"{"message":"A","message":"B"}"#,
        ),
        "plugin_schema",
    );
    assert!(
        host.manager()
            .state
            .lock()
            .unwrap()
            .tool_previews
            .is_empty()
    );
}

#[test]
fn nested_private_tool_arguments_are_rejected_before_preview_storage() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    host.install(&signed(echo_package("1.0.0"), &pair), &[CLIENT], true);
    let private_json = r#"{"pass\u0077ord":"opaque-fixture-secret"}"#;
    let duplicate_json = r#"{"pass\u0077ord":"opaque-fixture-secret","password":""}"#;
    for text in [
        private_json.to_owned(),
        duplicate_json.to_owned(),
        json!(private_json).to_string(),
        json!(duplicate_json).to_string(),
    ] {
        let failure = expect_error(
            host.manager()
                .preview_tool_call("test.echo", CLIENT, "echo", json!({"message": text})),
            "plugin_privacy",
        );
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("opaque-fixture-secret")
        );
        assert!(
            host.manager()
                .state
                .lock()
                .unwrap()
                .tool_previews
                .is_empty()
        );
    }
}

#[test]
fn output_redacts_plain_text_and_blocks_sensitive_or_ambiguous_json_structures() {
    let private = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
    let (clean, redacted) = super::runtime::inspect_output(json!(format!(
        "Result {private}. Contact fixture@example.test."
    )))
    .unwrap();
    assert!(redacted);
    let encoded = serde_json::to_string(&clean).unwrap();
    assert!(!encoded.contains(private));
    assert!(!encoded.contains("fixture@example.test"));
    assert!(encoded.contains("[REDACTED:SECRET]"));
    assert!(encoded.contains("[REDACTED:EMAIL]"));
    inspect_package_privacy(clean.clone()).unwrap();
    let (same, twice) = super::runtime::inspect_output(clean).unwrap();
    assert!(!twice);
    assert!(same.is_string());
    let private_json = r#"{"pass\u0077ord":"opaque-fixture-secret"}"#;
    let duplicate_json = r#"{"pass\u0077ord":"opaque-fixture-secret","password":""}"#;
    for value in [
        json!({"password": "opaque-fixture-secret"}),
        json!(private_json),
        json!(duplicate_json),
        json!({"text": json!(duplicate_json).to_string()}),
        json!({"type": "input_file", "file_data": "data:text/plain;base64,ZmFrZQ=="}),
    ] {
        let failure = expect_error(super::runtime::inspect_output(value), "plugin_privacy");
        assert!(
            !serde_json::to_string(&failure)
                .unwrap()
                .contains("opaque-fixture-secret")
        );
    }
    expect_error(
        super::runtime::inspect_output(json!("x".repeat(MAX_TOOL_BYTES))),
        "plugin_tool_arguments",
    );
}

#[tokio::test]
async fn read_text_uses_only_verified_package_content_and_exact_file_approval() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let mut package = reader_package("1.0.0");
    package
        .files
        .insert("notes.txt".into(), "Not in the declared read scope.".into());
    host.install(&signed(package, &pair), &[CLIENT], true);
    for path in [
        "../metadata.sqlite",
        "C:/fixture/private.txt",
        "docs/readme.txt:extra",
    ] {
        expect_error(
            host.manager()
                .preview_tool_call("test.echo", CLIENT, "read", json!({"path": path})),
            "plugin_path",
        );
    }
    expect_error(
        host.manager()
            .preview_tool_call("test.echo", CLIENT, "read", json!({"path": "notes.txt"})),
        "plugin_permissions",
    );
    let preview = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "read",
            json!({"path": "docs/readme.txt"}),
        )
        .unwrap();
    assert_eq!(
        preview.required_permissions.read_paths,
        vec!["package:docs/readme.txt"]
    );
    let mut broad = tool_approval(&preview);
    broad.accepted_permissions.read_paths = vec!["package:docs/".into()];
    expect_error(
        host.manager().execute_tool(CLIENT, broad).await,
        "plugin_confirmation",
    );
    let preview = host
        .manager()
        .preview_tool_call(
            "test.echo",
            CLIENT,
            "read",
            json!({"path": "docs/readme.txt"}),
        )
        .unwrap();
    let result = host
        .manager()
        .execute_tool(CLIENT, tool_approval(&preview))
        .await
        .unwrap();
    assert_eq!(result.value, json!("Reviewed package text."));
    assert!(
        host.manager()
            .tools_for_client(OTHER_CLIENT)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn process_credentials_external_reads_and_writes_are_never_enabled() {
    for change in 0..4 {
        let host = TestHost::new();
        let pair = signing_key(7);
        host.trust(&pair);
        let mut package = echo_package("1.0.0");
        let expected = match change {
            0 => {
                package.manifest.permissions.process = true;
                "plugin_process_unsupported"
            }
            1 => {
                package.manifest.permissions.credentials = vec!["provider.fixture".into()];
                "plugin_credentials_unsupported"
            }
            2 => {
                package.manifest.permissions.write_paths = vec!["package:docs/".into()];
                "plugin_write_unsupported"
            }
            _ => {
                package.manifest.permissions.read_paths = vec!["C:/fixture/text/".into()];
                "plugin_external_read_unsupported"
            }
        };
        let published = signed(package, &pair);
        let preview = host.manager().preview_install(published.clone()).unwrap();
        assert!(!preview.validation.runtime_supported);
        assert_eq!(preview.validation.warnings, vec![expected]);
        expect_error(
            host.manager()
                .confirm_install(install_approval(&preview, &[CLIENT], true)),
            expected,
        );
        assert!(host.manager().list_installed().unwrap().is_empty());
        let stored = host.install(&published, &[CLIENT], false);
        assert!(stored.enabled_clients.is_empty());
        assert!(stored.warnings.contains(&expected.to_owned()));
        expect_error(
            host.manager()
                .enable_plugin_client("test.echo", CLIENT, &published.digest, true),
            expected,
        );
        expect_error(
            host.manager().preview_tool_call(
                "test.echo",
                CLIENT,
                "echo",
                json!({"message": "Reviewed"}),
            ),
            expected,
        );
        assert!(host.manager().tools_for_client(CLIENT).unwrap().is_empty());
    }
}

#[test]
fn dependencies_are_pinned_verified_and_approved_for_the_same_client() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let dependency = signed(echo_package("1.0.0"), &pair);
    let mut package = echo_package("1.0.0");
    package.manifest.id = "test.dependent".into();
    package.manifest.dependencies = vec!["test.echo@1.0.0".into()];
    let dependent = signed(package, &pair);
    expect_error(
        host.manager().preview_install(dependent.clone()),
        "plugin_dependency",
    );
    host.install(&dependency, &[CLIENT], true);
    let preview = host.manager().preview_install(dependent.clone()).unwrap();
    expect_error(
        host.manager()
            .confirm_install(install_approval(&preview, &[OTHER_CLIENT], true)),
        "plugin_dependency",
    );
    host.install(&dependent, &[OTHER_CLIENT], false);
    expect_error(
        host.manager().enable_plugin_client(
            "test.dependent",
            OTHER_CLIENT,
            &dependent.digest,
            true,
        ),
        "plugin_dependency",
    );
    let review = host.manager().preview_installed("test.echo").unwrap();
    host.manager()
        .confirm_install(install_approval(&review, &[CLIENT, OTHER_CLIENT], true))
        .unwrap();
    host.manager()
        .enable_plugin_client("test.dependent", OTHER_CLIENT, &dependent.digest, true)
        .unwrap();
    assert_eq!(
        host.manager().tools_for_client(OTHER_CLIENT).unwrap().len(),
        2
    );
    host.manager()
        .disable_plugin("test.echo", Some(OTHER_CLIENT))
        .unwrap();
    assert!(
        host.manager()
            .tools_for_client(OTHER_CLIENT)
            .unwrap()
            .is_empty()
    );
    host.manager()
        .enable_plugin_client("test.echo", OTHER_CLIENT, &dependency.digest, true)
        .unwrap();
    host.install(
        &signed(echo_package("2.0.0"), &pair),
        &[CLIENT, OTHER_CLIENT],
        true,
    );
    let tools = host.manager().tools_for_client(OTHER_CLIENT).unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].plugin_id, "test.echo");
    expect_error(
        host.manager().preview_installed("test.dependent"),
        "plugin_dependency",
    );
}

#[test]
fn cached_package_tampering_and_hard_links_fail_closed() {
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let original = signed(echo_package("1.0.0"), &pair);
    host.install(&original, &[CLIENT], true);
    let mut tampered = original.clone();
    tampered.package.manifest.description = "Changed cached content.".into();
    write_fixture_object(&host, &tampered);
    expect_error(
        host.manager().preview_installed("test.echo"),
        "plugin_digest",
    );
    assert!(host.manager().tools_for_client(CLIENT).unwrap().is_empty());
    write_fixture_object(&host, &original);
    let object = host.package_root.join(format!("{}.json", original.digest));
    let alias = host.package_root.join("hard-link.json");
    fs::hard_link(&object, &alias).unwrap();
    expect_error(
        host.manager().preview_installed("test.echo"),
        "plugin_storage_boundary",
    );
    assert!(host.manager().tools_for_client(CLIENT).unwrap().is_empty());
    make_writable(&alias);
    fs::remove_file(alias).unwrap();
    write_fixture_object(&host, &original);
    assert_eq!(host.manager().tools_for_client(CLIENT).unwrap().len(), 1);
    expect_error(
        PluginManager::new(
            host.manager().database.clone(),
            host.directory.join("different-root"),
        ),
        "plugin_storage_boundary",
    );
}

#[cfg(unix)]
#[test]
fn symlinks_and_replaced_package_directories_are_rejected() {
    use std::os::unix::fs::symlink;
    let host = TestHost::new();
    let pair = signing_key(7);
    host.trust(&pair);
    let published = signed(echo_package("1.0.0"), &pair);
    host.install(&published, &[CLIENT], true);
    let linked_root = host.directory.join("linked-root");
    symlink(&host.package_root, &linked_root).unwrap();
    expect_error(PackageStore::open(linked_root), "plugin_storage_boundary");
    let object = host.package_root.join(format!("{}.json", published.digest));
    let outside = host.directory.join("outside.json");
    fs::write(&outside, serde_json::to_vec(&published).unwrap()).unwrap();
    fs::remove_file(&object).unwrap();
    symlink(&outside, &object).unwrap();
    expect_error(
        host.manager().preview_installed("test.echo"),
        "plugin_storage",
    );
    fs::remove_file(&object).unwrap();
    write_fixture_object(&host, &published);
    let moved = host.directory.join("original-root");
    fs::rename(&host.package_root, &moved).unwrap();
    fs::create_dir(&host.package_root).unwrap();
    expect_error(
        host.manager().store.check_boundary(),
        "plugin_storage_boundary",
    );
    assert!(host.manager().tools_for_client(CLIENT).unwrap().is_empty());
}

#[cfg(windows)]
#[test]
fn open_windows_directory_handles_prevent_root_replacement() {
    let host = TestHost::new();
    assert!(fs::rename(&host.package_root, host.directory.join("replaced-root")).is_err());
    host.manager().store.check_boundary().unwrap();
}

#[test]
fn network_address_filter_rejects_private_metadata_and_transition_ranges() {
    for address in [
        "0.0.0.0",
        "10.0.0.1",
        "100.64.0.1",
        "127.0.0.1",
        "169.254.169.254",
        "172.16.0.1",
        "192.168.0.1",
        "198.18.0.1",
        "198.51.100.1",
        "203.0.113.1",
        "224.0.0.1",
        "::1",
        "::ffff:127.0.0.1",
        "fc00::1",
        "fe80::1",
        "2001:db8::1",
        "2002:7f00:1::1",
        "2001::1",
    ] {
        assert!(
            !public_address(address.parse().unwrap()),
            "synthetic nonpublic address {address}"
        );
    }
    for address in ["8.8.8.8", "1.1.1.1", "2606:4700:4700::1111"] {
        assert!(public_address(address.parse().unwrap()));
    }
}
