use ring::{rand::SystemRandom, signature::{Ed25519KeyPair, KeyPair}};
use rusqlite::Connection;
use serde_json::json;
use std::{collections::BTreeMap, io::Write, net::TcpListener, path::PathBuf, process::{Command, Output, Stdio}, sync::OnceLock};
use xy_switch_lib::marketplace::{Marketplace, MarketplaceConfig, PasswordHash, ReviewerConfig, hash_password_for_deployment};

const SYNTHETIC_PASSWORD: &str = "synthetic CLI review phrase 2026";

struct CliFixture {
    directory: PathBuf,
    private_key: Vec<u8>,
    reviewer: ReviewerConfig,
    occupied_listener: TcpListener,
}

impl CliFixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("xy-marketplace-cli-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory).unwrap();
        let private_key = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap().as_ref().to_vec();
        static REVIEWER_HASH: OnceLock<PasswordHash> = OnceLock::new();
        let reviewer = ReviewerConfig {
            username: "cli_reviewer".to_owned(), display_name: "Independent CLI Reviewer".to_owned(),
            password_hash: REVIEWER_HASH.get_or_init(|| hash_password_for_deployment(SYNTHETIC_PASSWORD).unwrap()).clone(),
        };
        let fixture = Self { directory, private_key, reviewer, occupied_listener: TcpListener::bind("127.0.0.1:0").unwrap() };
        std::fs::write(fixture.directory.join("signing-key.pk8"), &fixture.private_key).unwrap();
        fixture.write_reviewers(&fixture.reviewer);
        fixture
    }

    fn database_path(&self) -> PathBuf { self.directory.join("data/marketplace.sqlite3") }

    fn write_reviewers(&self, reviewer: &ReviewerConfig) {
        std::fs::write(self.directory.join("reviewers.json"), serde_json::to_vec(&[reviewer]).unwrap()).unwrap();
    }

    fn config(&self) -> MarketplaceConfig {
        MarketplaceConfig {
            database_path: self.database_path(), listen: self.occupied_listener.local_addr().unwrap(),
            public_origin: "https://market.example.invalid".to_owned(),
            allowed_origins: vec!["https://market.example.invalid".to_owned()],
            reviewers: vec![self.reviewer.clone()], signing_key_id: "cli.publisher".to_owned(),
            signing_private_key: self.private_key.clone(), verification_keys: BTreeMap::new(),
            session_ttl_seconds: 86400, allow_development_http: false, allow_non_loopback: false,
        }
    }

    fn command(&self, argument: &str) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_xy-marketplace"));
        command.env_clear().arg(argument)
            .env("XY_MARKETPLACE_DATABASE", self.database_path())
            .env("XY_MARKETPLACE_LISTEN", self.occupied_listener.local_addr().unwrap().to_string())
            .env("XY_MARKETPLACE_PUBLIC_ORIGIN", "https://market.example.invalid")
            .env("XY_MARKETPLACE_REVIEWERS_FILE", self.directory.join("reviewers.json"))
            .env("XY_MARKETPLACE_SIGNING_KEY_FILE", self.directory.join("signing-key.pk8"))
            .env("XY_MARKETPLACE_SIGNING_KEY_ID", "cli.publisher");
        command
    }
}

impl Drop for CliFixture {
    fn drop(&mut self) {
        if self.directory.parent() == Some(std::env::temp_dir().as_path())
            && self.directory.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("xy-marketplace-cli-test-")) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

fn assert_safe_failure(output: Output, code: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    assert_eq!(String::from_utf8(output.stderr).unwrap(), format!("xy-marketplace: {code}\n"));
}

#[test]
fn configuration_preflight_validates_real_signing_keys_without_creating_data_or_binding() {
    let fixture = CliFixture::new();
    let output = fixture.command("--check-config").output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8(output.stdout).unwrap().contains("no listener started"));
    assert!(!fixture.database_path().exists());
    assert!(!fixture.directory.join("data").exists());
    std::fs::write(fixture.directory.join("signing-key.pk8"), b"synthetic private bytes must not be echoed").unwrap();
    assert_safe_failure(fixture.command("--check-config").output().unwrap(), "invalid_configuration");
    assert!(!fixture.database_path().exists());
}

#[test]
fn offline_verification_requires_existing_state_and_preserves_reviewer_keys_and_audit() {
    let fixture = CliFixture::new();
    assert_safe_failure(fixture.command("--verify-state").output().unwrap(), "invalid_configuration");
    assert!(!fixture.database_path().exists());
    Marketplace::open(fixture.config()).unwrap();
    let connection = Connection::open(fixture.database_path()).unwrap();
    let before: (String, i64, i64) = connection.query_row(
        "SELECT display_name,(SELECT count(*) FROM audit),(SELECT count(*) FROM signing_keys) FROM users WHERE username='cli_reviewer'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    let mut reviewer = fixture.reviewer.clone();
    reviewer.display_name = "Another Configured Reviewer Name".to_owned();
    fixture.write_reviewers(&reviewer);
    let historic = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let public = Ed25519KeyPair::from_pkcs8(historic.as_ref()).unwrap().public_key().as_ref().to_vec();
    let encoded: String = public.iter().map(|byte| format!("{byte:02x}")).collect();
    let verification_file = fixture.directory.join("verification-keys.json");
    std::fs::write(&verification_file, serde_json::to_vec(&json!([{"id":"cli.historic","publicKeyHex":encoded}])).unwrap()).unwrap();
    let output = fixture.command("--verify-state").env("XY_MARKETPLACE_VERIFICATION_KEYS_FILE", verification_file).output().unwrap();
    assert!(output.status.success());
    assert!(output.stderr.is_empty());
    assert!(String::from_utf8(output.stdout).unwrap().contains("no listener started"));
    let after: (String, i64, i64) = connection.query_row(
        "SELECT display_name,(SELECT count(*) FROM audit),(SELECT count(*) FROM signing_keys) FROM users WHERE username='cli_reviewer'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).unwrap();
    assert_eq!(before, after);
}

#[test]
fn offline_verification_rejects_empty_foreign_and_tampered_schema_without_initializing() {
    let fixture = CliFixture::new();
    std::fs::create_dir(fixture.directory.join("data")).unwrap();
    std::fs::write(fixture.database_path(), []).unwrap();
    assert_safe_failure(fixture.command("--verify-state").output().unwrap(), "service_unavailable");
    assert_eq!(std::fs::metadata(fixture.database_path()).unwrap().len(), 0);
    Marketplace::open(fixture.config()).unwrap();
    let connection = Connection::open(fixture.database_path()).unwrap();
    connection.execute_batch("DROP TRIGGER audit_no_update;").unwrap();
    assert_safe_failure(fixture.command("--verify-state").output().unwrap(), "service_unavailable");
    connection.execute_batch("CREATE TRIGGER audit_no_update BEFORE UPDATE ON audit BEGIN SELECT 1; END;").unwrap();
    assert_safe_failure(fixture.command("--verify-state").output().unwrap(), "service_unavailable");
    connection.execute_batch("CREATE TABLE foreign_data(value TEXT);").unwrap();
    assert_safe_failure(fixture.command("--verify-state").output().unwrap(), "service_unavailable");
}

#[test]
fn password_hash_command_accepts_only_stdin_and_emits_salted_hashes_without_passwords() {
    let mut salts = Vec::new();
    for _ in 0..2 {
        let mut child = Command::new(env!("CARGO_BIN_EXE_xy-marketplace"))
            .env_clear().arg("--hash-password-stdin").stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(SYNTHETIC_PASSWORD.as_bytes()).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success());
        assert!(output.stderr.is_empty());
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(!stdout.contains(SYNTHETIC_PASSWORD));
        let hash: PasswordHash = serde_json::from_str(&stdout).unwrap();
        hash.validate().unwrap();
        assert!(hash.verify(SYNTHETIC_PASSWORD).unwrap());
        salts.push(hash.salt_hex);
    }
    assert_ne!(salts[0], salts[1]);
    let output = Command::new(env!("CARGO_BIN_EXE_xy-marketplace"))
        .env_clear().args(["--hash-password-stdin", SYNTHETIC_PASSWORD]).output().unwrap();
    assert_safe_failure(output, "invalid_request");
}
