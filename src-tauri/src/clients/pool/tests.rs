use super::*;
use crate::accounts::{
    AccountCancellation, AccountKind, AccountOutcome, AccountSelection, AccountSource,
    AuthorizationStatus, SelectionContext,
};
use crate::clients::NativeQuota;
use rusqlite::params;
use serde_json::{Value, json};
use std::sync::{Arc, Barrier, mpsc};
use std::thread;
use std::time::Duration;

const PROVIDER: &str = "bridge-provider";
const DESTINATION: &str = "https://provider.example.test/v1";
const OTHER_PROVIDER: &str = "bridge-other-provider";
const OTHER_DESTINATION: &str = "https://other.example.test/v1";
const WATCHDOG: Duration = Duration::from_secs(10);
const CONTENTION_WINDOW: Duration = Duration::from_millis(100);

struct Fixture {
    database: Database,
    pool: AccountPool,
    bridge: Arc<NativePoolBridge>,
}

impl Fixture {
    fn new() -> Self {
        let database = Database::open(Path::new(":memory:")).unwrap();
        for (provider_id, endpoint) in
            [(PROVIDER, DESTINATION), (OTHER_PROVIDER, OTHER_DESTINATION)]
        {
            database
                .save_provider(
                    &serde_json::from_value(json!({
                        "id": provider_id, "name": "Synthetic native provider",
                        "providerType": "custom", "endpoint": endpoint,
                        "model": "synthetic-model", "appGroup": "Codex",
                        "enabled": true, "sortOrder": 0
                    }))
                    .unwrap(),
                )
                .unwrap();
        }
        let pool = AccountPool::test_pool(database.clone()).unwrap();
        let bridge = Arc::new(NativePoolBridge::new(database.clone(), pool.clone()).unwrap());
        Self {
            database,
            pool,
            bridge,
        }
    }

    fn scope(&self, owner: &str, directory: &str) -> String {
        let path = std::env::current_dir().unwrap().join(directory);
        self.bridge.scope(owner, &[&path]).unwrap()
    }

    fn candidate(&self, scope: &str, snapshot: &NativeAccountSnapshot) -> NativePoolCandidate {
        let mut observed = self
            .bridge
            .observe(
                owner(&snapshot.client_id).unwrap(),
                scope,
                snapshot.clone(),
                false,
            )
            .unwrap();
        assert_eq!(observed.pool_candidates.len(), 1);
        observed.pool_candidates.remove(0)
    }

    fn bind(&self, scope: &str, snapshot: &NativeAccountSnapshot) -> AccountInfo {
        let candidate = self.candidate(scope, snapshot);
        self.bridge
            .bind(scope, snapshot, request(&snapshot.client_id, &candidate.id))
            .unwrap()
    }

    fn bind_connections(
        &self,
        scope: &str,
        connections: &[NativeProviderConnection],
    ) -> Vec<AccountInfo> {
        let candidates: Vec<_> = connections
            .iter()
            .map(|connection| {
                self.candidate(scope, &opencode_snapshot(std::slice::from_ref(connection)))
            })
            .collect();
        let snapshot = opencode_snapshot(connections);
        candidates
            .iter()
            .map(|candidate| {
                self.bridge
                    .bind(
                        scope,
                        &snapshot,
                        request(&snapshot.client_id, &candidate.id),
                    )
                    .unwrap()
            })
            .collect()
    }

    fn admit(&self) -> SelectionContext {
        let task_id = Uuid::new_v4().to_string();
        let attempt_id = Uuid::new_v4().to_string();
        let connection = self.database.conn();
        let connection = connection.lock().unwrap();
        connection.execute(
            "INSERT INTO proxy_tasks(id,identity_source,max_attempts,max_microusd,attempts,reserved_microusd,created_at,updated_at) VALUES(?1,'synthetic',4,1000,1,10,?2,?2)",
            params![task_id, crate::governance::now()],
        ).unwrap();
        connection.execute(
            "INSERT INTO proxy_attempts(id,task_id,request_id,ordinal,provider_id,model,route,reserved_tokens,reserved_microusd,status,started_at) VALUES(?1,?2,?1,1,?3,'synthetic-model','responses',32,10,'sending',?4)",
            params![attempt_id, task_id, PROVIDER, crate::governance::now()],
        ).unwrap();
        connection.execute(
            "INSERT INTO budget_ledger(id,day,provider_id,model,tokens,cost_microusd,source,created_at) VALUES(?1,1,?2,'synthetic-model',32,10,'synthetic',?3)",
            params![attempt_id, PROVIDER, crate::governance::now()],
        ).unwrap();
        SelectionContext {
            provider_id: PROVIDER.into(),
            destination: None,
            http_only: false,
            model: "synthetic-model".into(),
            task_id,
            attempt_id,
            affinity_key: None,
            cancellation: AccountCancellation::default(),
        }
    }

    fn select(&self) -> AccountSelection {
        self.pool.select(&self.admit()).unwrap()
    }

    fn metadata(&self, id: &str) -> Value {
        let text: String = self
            .database
            .conn()
            .lock()
            .unwrap()
            .query_row(
                "SELECT metadata FROM account_pool_accounts WHERE id=?1",
                [id],
                |row| row.get(0),
            )
            .unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn reload_bridge(&mut self) {
        self.bridge = Arc::new(
            NativePoolBridge::new(
                self.database.clone(),
                AccountPool::test_pool(self.database.clone()).unwrap(),
            )
            .unwrap(),
        );
    }
}

fn request(client: &str, candidate: &str) -> BindNativeAccountRequest {
    BindNativeAccountRequest {
        client_id: client.into(),
        candidate_id: candidate.into(),
        alias: "Synthetic native account".into(),
        provider_id: PROVIDER.into(),
        approved_destination: DESTINATION.into(),
        confirmed: true,
    }
}

fn snapshot(client: &str, subject: &str) -> NativeAccountSnapshot {
    NativeAccountSnapshot {
        client_id: client.into(),
        auth_mode: if client.starts_with("codex-") {
            "chatgpt"
        } else {
            "native"
        }
        .into(),
        plan: None,
        checked_at: crate::governance::now(),
        quota: None,
        quotas: Vec::new(),
        providers: Vec::new(),
        pool_candidates: Vec::new(),
        identities: vec![NativeIdentity {
            subject: Zeroizing::new(subject.into()),
            label: "Synthetic subscription".into(),
        }],
    }
}

fn connection(provider: &str, id: Option<&str>, disconnectable: bool) -> NativeProviderConnection {
    NativeProviderConnection {
        provider_id: provider.into(),
        label: "Synthetic provider".into(),
        connection_id: id.map(str::to_owned),
        disconnectable,
    }
}

fn opencode_snapshot(connections: &[NativeProviderConnection]) -> NativeAccountSnapshot {
    let mut snapshot = snapshot("opencode-cli", "unused");
    snapshot.identities = connections
        .iter()
        .filter_map(NativeIdentity::opencode)
        .collect();
    snapshot.providers = connections.to_vec();
    snapshot
}

fn observed_value(result: Result<NativeAccountSnapshot, String>) -> Result<Value, String> {
    result.map(|native| {
        let identities: Vec<_> = native
            .identities
            .iter()
            .map(|identity| (identity.subject.as_str(), identity.label.as_str()))
            .collect();
        json!({ "public": &native, "privateIdentities": identities })
    })
}

fn overlapping_refreshes(
    operations: &NativeOperations,
    first_key: &str,
    first_result: Result<NativeAccountSnapshot, String>,
    second_key: &str,
    second_result: Result<NativeAccountSnapshot, String>,
) -> (Vec<&'static str>, [Result<Value, String>; 2]) {
    let (entered, entries) = mpsc::channel();
    let first_entered = entered.clone();
    let (release, released) = mpsc::channel();
    let ready = Barrier::new(2);
    thread::scope(|threads| {
        let first = threads.spawn(move || {
            observed_value(operations.observe_for(first_key, true, || {
                first_entered.send("first").unwrap();
                released
                    .recv_timeout(WATCHDOG)
                    .expect("first refresh was not released");
                first_result
            }))
        });
        assert_eq!(entries.recv_timeout(WATCHDOG).unwrap(), "first");
        let ready = &ready;
        let second = threads.spawn(move || {
            ready.wait();
            observed_value(operations.observe_for(second_key, true, || {
                entered.send("second").unwrap();
                second_result
            }))
        });
        ready.wait();
        assert_eq!(
            entries.recv_timeout(CONTENTION_WINDOW),
            Err(mpsc::RecvTimeoutError::Timeout),
            "the second native callback must wait for the in-flight callback"
        );
        release.send(()).unwrap();
        let results = [first.join().unwrap(), second.join().unwrap()];
        let mut callbacks = vec!["first"];
        callbacks.extend(entries.try_iter());
        (callbacks, results)
    })
}

#[test]
fn binding_requires_explicit_confirmation_before_consuming_the_candidate() {
    let fixture = Fixture::new();
    let scope = fixture.scope("codex", "native-home");
    let native = snapshot("codex-desktop", "synthetic-subject");
    let candidate = fixture.candidate(&scope, &native);
    let mut unconfirmed = request(&native.client_id, &candidate.id);
    unconfirmed.confirmed = false;
    assert_eq!(
        fixture
            .bridge
            .bind(&scope, &native, unconfirmed)
            .unwrap_err(),
        AccountError::AuthorizationRequired.to_string()
    );
    assert!(fixture.pool.list().unwrap().is_empty());

    let account = fixture
        .bridge
        .bind(&scope, &native, request(&native.client_id, &candidate.id))
        .unwrap();
    assert_eq!(account.status, AccountStatus::Available);
    assert_eq!(account.kind, AccountKind::NativeManaged);
    assert_eq!(account.source, AccountSource::NativeClient);
    assert_eq!(account.client, Some(AccountClient::CodexDesktop));
    assert_eq!(account.provider_id, PROVIDER);
    assert!(account.destination_approved);
    assert!(!account.allow_refresh);
    assert!(!account.can_refresh);
    assert_eq!(fixture.pool.list().unwrap().len(), 1);
}

#[test]
fn binding_requires_the_exact_destination_of_the_selected_provider() {
    let fixture = Fixture::new();
    let scope = fixture.scope("codex", "native-home");
    let native = snapshot("codex-cli", "synthetic-subject");
    for (provider, destination) in [
        (PROVIDER, OTHER_DESTINATION),
        (PROVIDER, "https://provider.example.test/v1/"),
        (PROVIDER, "https://provider.example.test/v2"),
        (PROVIDER, ""),
        (OTHER_PROVIDER, DESTINATION),
    ] {
        let candidate = fixture.candidate(&scope, &native);
        let mut bind = request(&native.client_id, &candidate.id);
        bind.provider_id = provider.into();
        bind.approved_destination = destination.into();
        assert_eq!(
            fixture.bridge.bind(&scope, &native, bind).unwrap_err(),
            AccountError::DestinationApprovalRequired.to_string(),
            "provider={provider}, destination={destination}"
        );
        assert!(fixture.pool.list().unwrap().is_empty());
    }
    assert!(fixture.bind(&scope, &native).destination_approved);
}

#[test]
fn candidates_reject_expiry_owner_identity_and_configuration_scope_changes() {
    for changed in ["expiry", "owner", "identity", "scope"] {
        let fixture = Fixture::new();
        let scope = fixture.scope("codex", "native-home");
        let native = snapshot("codex-cli", "original-subject");
        let candidate = fixture.candidate(&scope, &native);
        let mut bind = request(&native.client_id, &candidate.id);
        let mut current = native.clone();
        let mut current_scope = scope.clone();
        match changed {
            "expiry" => {
                fixture
                    .bridge
                    .state
                    .lock()
                    .unwrap()
                    .candidates
                    .get_mut(&candidate.id)
                    .unwrap()
                    .expires_at = crate::governance::now() - 1;
            }
            "owner" => bind.client_id = "opencode-cli".into(),
            "identity" => current = snapshot("codex-cli", "replacement-subject"),
            "scope" => current_scope = fixture.scope("codex", "changed-native-home"),
            _ => unreachable!(),
        }
        assert_eq!(
            fixture
                .bridge
                .bind(&current_scope, &current, bind)
                .unwrap_err(),
            BINDING_ERROR,
            "changed {changed}"
        );
        assert!(fixture.pool.list().unwrap().is_empty());
        assert!(
            fixture
                .bridge
                .bind(&scope, &native, request(&native.client_id, &candidate.id),)
                .is_err(),
            "a rejected candidate cannot be replayed after restoring {changed}"
        );
    }
}

#[test]
fn unknown_and_consumed_candidates_cannot_create_another_account() {
    let fixture = Fixture::new();
    let scope = fixture.scope("codex", "native-home");
    let native = snapshot("codex-cli", "synthetic-subject");
    assert_eq!(
        fixture
            .bridge
            .bind(
                &scope,
                &native,
                request(&native.client_id, &Uuid::new_v4().to_string()),
            )
            .unwrap_err(),
        BINDING_ERROR
    );
    assert!(fixture.pool.list().unwrap().is_empty());
    let candidate = fixture.candidate(&scope, &native);
    let account = fixture
        .bridge
        .bind(&scope, &native, request(&native.client_id, &candidate.id))
        .unwrap();
    assert_eq!(
        fixture
            .bridge
            .bind(&scope, &native, request(&native.client_id, &candidate.id),)
            .unwrap_err(),
        BINDING_ERROR
    );
    assert_eq!(fixture.pool.list().unwrap().len(), 1);
    assert_eq!(
        fixture.pool.get(&account.id).unwrap().status,
        AccountStatus::Available
    );
}

#[test]
fn one_native_identity_has_one_binding_across_desktop_cli_scopes_and_reload() {
    for (native_owner, desktop, cli) in [
        ("codex", "codex-desktop", "codex-cli"),
        ("opencode", "opencode-desktop", "opencode-cli"),
    ] {
        let mut fixture = Fixture::new();
        let scope = fixture.scope(native_owner, "native-home");
        let other_scope = fixture.scope(native_owner, "other-native-home");
        let account = fixture.bind(&scope, &snapshot(desktop, "same-native-subject"));
        fixture.reload_bridge();
        assert_eq!(fixture.scope(native_owner, "native-home"), scope);
        for (client, current_scope) in [(cli, &scope), (desktop, &other_scope), (cli, &other_scope)]
        {
            let native = snapshot(client, "same-native-subject");
            let candidate = fixture.candidate(current_scope, &native);
            assert_eq!(
                candidate.bound_account_id.as_deref(),
                Some(account.id.as_str())
            );
            let mut duplicate = request(client, &candidate.id);
            duplicate.provider_id = OTHER_PROVIDER.into();
            duplicate.approved_destination = OTHER_DESTINATION.into();
            assert!(
                fixture
                    .bridge
                    .bind(current_scope, &native, duplicate)
                    .is_err()
            );
            assert_eq!(fixture.pool.list().unwrap().len(), 1);
        }
    }
}

#[test]
fn bound_lookup_rechecks_native_identity_scope_and_current_destination() {
    let fixture = Fixture::new();
    let scope = fixture.scope("codex", "native-home");
    let other_scope = fixture.scope("codex", "other-native-home");
    let native = snapshot("codex-cli", "original-subject");
    let account = fixture.bind(&scope, &native);
    assert_eq!(
        fixture
            .bridge
            .bound_client(&account.id, Some(&scope), Some(&native))
            .unwrap(),
        "codex-cli"
    );
    assert!(
        fixture
            .bridge
            .bound_client(&account.id, Some(&other_scope), Some(&native))
            .is_err()
    );
    assert!(
        fixture
            .bridge
            .bound_client(
                &account.id,
                Some(&scope),
                Some(&snapshot("codex-cli", "other-subject")),
            )
            .is_err()
    );
    fixture
        .database
        .conn()
        .lock()
        .unwrap()
        .execute(
            "UPDATE providers SET endpoint=?1 WHERE id=?2",
            params![OTHER_DESTINATION, PROVIDER],
        )
        .unwrap();
    assert!(
        fixture
            .bridge
            .bound_client(&account.id, Some(&scope), Some(&native))
            .is_err()
    );
    assert_eq!(
        fixture.pool.get(&account.id).unwrap().status,
        AccountStatus::DestinationApprovalRequired
    );
}

#[test]
fn snapshots_and_persisted_metadata_keep_native_subjects_and_tokens_private() {
    const EMAIL: &str = "bridge-private-person@example.test";
    const ACCOUNT_ID: &str = "bridge-private-account-id";
    const SUBJECT: &str = "bridge-private-connection-subject";
    const TOKEN: &str = "bridge-private-access-token";
    let mut fixture = Fixture::new();
    let identities = [
        NativeIdentity::codex(&json!({
            "type": "chatgpt", "accountId": ACCOUNT_ID, "email": EMAIL, "accessToken": TOKEN
        }))
        .unwrap(),
        NativeIdentity::codex(&json!({"type": "chatgpt", "email": EMAIL})).unwrap(),
        NativeIdentity {
            subject: Zeroizing::new(format!("{SUBJECT}:{TOKEN}")),
            label: "Synthetic connection".into(),
        },
    ];
    let mut accounts = Vec::new();
    let mut serialized = Vec::new();
    for (index, identity) in identities.into_iter().enumerate() {
        let client = if index == 2 {
            "opencode-cli"
        } else {
            "codex-cli"
        };
        let scope = fixture.scope(owner(client).unwrap(), &format!("private-home-{index}"));
        let mut native = snapshot(client, "unused");
        native.identities = vec![identity];
        let raw_json = serde_json::to_value(&native).unwrap();
        assert!(raw_json.get("identities").is_none());
        serialized.push(raw_json.to_string());
        let observed = fixture
            .bridge
            .observe(owner(client).unwrap(), &scope, native.clone(), false)
            .unwrap();
        assert!(observed.identities.is_empty());
        serialized.push(serde_json::to_string(&observed).unwrap());
        let account = fixture
            .bridge
            .bind(
                &scope,
                &native,
                request(client, &observed.pool_candidates[0].id),
            )
            .unwrap();
        serialized.push(serde_json::to_string(&account).unwrap());
        let metadata = fixture.metadata(&account.id);
        assert_eq!(metadata["kind"], "nativeManaged");
        assert_eq!(metadata["hasRefreshToken"], false);
        assert!(Uuid::parse_str(metadata["nativeReference"].as_str().unwrap()).is_ok());
        serialized.push(metadata.to_string());
        accounts.push((account, scope, native));
    }
    serialized.push(fixture.database.get_setting(STORAGE_KEY).unwrap().unwrap());
    for text in &serialized {
        for secret in [EMAIL, ACCOUNT_ID, SUBJECT, TOKEN] {
            assert!(
                !text.contains(secret),
                "native private data escaped into a serialized surface"
            );
        }
    }
    let references: i64 = fixture
        .database
        .conn()
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM account_pool_secret_refs", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(
        references, 0,
        "nativeManaged must not stage credential-store references"
    );
    fixture.reload_bridge();
    for (account, scope, native) in accounts {
        assert_eq!(
            fixture
                .bridge
                .bound_client(&account.id, Some(&scope), Some(&native))
                .unwrap(),
            native.client_id
        );
    }
}

#[test]
fn observation_preserves_401_until_confirmed_renew_and_old_feedback_cannot_undo_it() {
    let fixture = Fixture::new();
    let scope = fixture.scope("codex", "native-home");
    let native = snapshot("codex-cli", "synthetic-subject");
    let account = fixture.bind(&scope, &native);
    let rejected = fixture.select();
    let old_success = fixture.select();
    let old_failure = fixture.select();
    fixture
        .pool
        .record_outcome(&rejected, AccountOutcome::from_http(401, None))
        .unwrap();
    let reference = fixture.metadata(&account.id)["nativeReference"].clone();
    fixture
        .bridge
        .observe("codex", &scope, native.clone(), false)
        .unwrap();
    let observed = fixture
        .pool
        .record_outcome(&old_success, AccountOutcome::from_http(200, None))
        .unwrap();
    assert_eq!(observed.status, AccountStatus::LoginRequired);
    assert_eq!(
        observed.authorization_status,
        AuthorizationStatus::LoginRequired
    );
    assert_eq!(observed.last_validated_at, None);
    assert_eq!(
        observed.credential_generation,
        account.credential_generation
    );
    assert_eq!(fixture.metadata(&account.id)["nativeReference"], reference);
    assert_eq!(
        fixture.pool.select(&fixture.admit()).unwrap_err(),
        AccountError::NoEligibleAccount
    );

    fixture
        .bridge
        .observe("codex", &scope, native, true)
        .unwrap();
    let renewed = fixture.pool.get(&account.id).unwrap();
    assert_eq!(renewed.status, AccountStatus::Available);
    assert_eq!(
        renewed.authorization_status,
        AuthorizationStatus::Authorized
    );
    assert!(renewed.credential_generation > account.credential_generation);
    assert_ne!(fixture.metadata(&account.id)["nativeReference"], reference);
    let after_old_failure = fixture
        .pool
        .record_outcome(&old_failure, AccountOutcome::from_http(401, None))
        .unwrap();
    assert_eq!(after_old_failure.status, AccountStatus::Available);
    assert_eq!(
        after_old_failure.credential_generation,
        renewed.credential_generation
    );
    assert_eq!(fixture.select().account_id(), account.id);
}

#[test]
fn failed_renewal_persistence_cannot_authorize_the_next_ordinary_observation() {
    let fixture = Fixture::new();
    let scope = fixture.scope("codex", "native-home");
    let native = snapshot("codex-cli", "synthetic-subject");
    let account = fixture.bind(&scope, &native);
    fixture
        .pool
        .record_outcome(&fixture.select(), AccountOutcome::from_http(401, None))
        .unwrap();
    let reference = fixture.metadata(&account.id)["nativeReference"].clone();
    let saved = fixture.database.get_setting(STORAGE_KEY).unwrap();
    fixture
        .database
        .conn()
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TEMP TRIGGER reject_native_renew BEFORE UPDATE ON app_settings
         WHEN NEW.key='native_account_bindings_v1'
         BEGIN SELECT RAISE(ABORT,'synthetic persistence failure'); END;",
        )
        .unwrap();
    let failed = fixture
        .bridge
        .observe("codex", &scope, native.clone(), true);
    assert!(matches!(failed, Err(ref error) if error == STORE_ERROR));
    assert_eq!(fixture.database.get_setting(STORAGE_KEY).unwrap(), saved);
    fixture
        .database
        .conn()
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER reject_native_renew;")
        .unwrap();
    fixture
        .bridge
        .observe("codex", &scope, native, false)
        .unwrap();
    let current = fixture.pool.get(&account.id).unwrap();
    assert_eq!(current.status, AccountStatus::LoginRequired);
    assert_eq!(current.credential_generation, account.credential_generation);
    assert_eq!(fixture.metadata(&account.id)["nativeReference"], reference);
}

#[test]
fn same_reference_observation_preserves_failure_count_quota_and_active_cooldown() {
    for status in [429, 503] {
        let fixture = Fixture::new();
        let scope = fixture.scope("codex", "native-home");
        let mut native = snapshot("codex-desktop", "synthetic-subject");
        native.quotas = vec![NativeQuota {
            source: "five-hour".into(),
            used_percent: Some(35.0),
            reset_at: Some(crate::governance::now() + 3600),
            window_minutes: Some(300),
        }];
        let account = fixture.bind(&scope, &native);
        let failed = fixture.select();
        let old_success = fixture.select();
        let cooling = fixture
            .pool
            .record_outcome(&failed, AccountOutcome::from_http(status, Some(120)))
            .unwrap();
        let failures = fixture.metadata(&account.id)["consecutiveFailures"].clone();
        assert!(failures.as_u64().unwrap() > 0);
        assert!(cooling.cooldown_until.unwrap() > crate::governance::now());
        fixture
            .bridge
            .observe("codex", &scope, native.clone(), false)
            .unwrap();
        let current = fixture
            .pool
            .record_outcome(&old_success, AccountOutcome::from_http(200, None))
            .unwrap();
        assert_eq!(current.status, AccountStatus::CoolingDown, "HTTP {status}");
        assert_eq!(current.credential_generation, account.credential_generation);
        assert_eq!(current.cooldown_until, cooling.cooldown_until);
        assert_eq!(current.quota, cooling.quota);
        assert_eq!(
            fixture.metadata(&account.id)["consecutiveFailures"],
            failures
        );
        assert_eq!(
            fixture.pool.select(&fixture.admit()).unwrap_err(),
            AccountError::NoEligibleAccount
        );
        fixture
            .bridge
            .observe("codex", &scope, native, true)
            .unwrap();
        let renewed = fixture.pool.get(&account.id).unwrap();
        assert_eq!(renewed.status, AccountStatus::CoolingDown);
        assert_eq!(renewed.cooldown_until, cooling.cooldown_until);
    }
}

#[test]
fn local_revocation_survives_observation_renewal_reload_and_late_success() {
    let mut fixture = Fixture::new();
    let scope = fixture.scope("codex", "native-home");
    let native = snapshot("codex-cli", "synthetic-subject");
    let account = fixture.bind(&scope, &native);
    let late_success = fixture.select();
    let revoked = fixture.pool.revoke(&account.id, true).unwrap();
    assert!(revoked.local_reference_revoked);
    fixture.reload_bridge();
    for renew in [false, true] {
        let observed = fixture
            .bridge
            .observe("codex", &scope, native.clone(), renew)
            .unwrap();
        assert_eq!(observed.pool_candidates[0].bound_account_id, None);
        let current = fixture.pool.get(&account.id).unwrap();
        assert_eq!(current.status, AccountStatus::Revoked);
        assert_eq!(
            current.credential_generation,
            revoked.account.credential_generation
        );
        assert_eq!(
            fixture.metadata(&account.id)["nativeReference"],
            Value::Null
        );
    }
    assert!(
        fixture
            .bridge
            .bound_client(&account.id, Some(&scope), Some(&native))
            .is_err()
    );
    assert_eq!(
        fixture
            .pool
            .record_outcome(&late_success, AccountOutcome::from_http(200, None))
            .unwrap()
            .status,
        AccountStatus::Revoked
    );
    assert_eq!(fixture.pool.list().unwrap().len(), 1);
    assert_eq!(
        fixture.pool.select(&fixture.admit()).unwrap_err(),
        AccountError::NoEligibleAccount
    );
}

#[test]
fn opencode_v1_and_environment_connections_never_offer_bindable_candidates() {
    let fixture = Fixture::new();
    let scope = fixture.scope("opencode", "native-home");
    for unsupported in [
        connection("openai", None, true),
        connection("openai", None, false),
        connection("openai", Some("environment-entry"), false),
        connection("openai", Some(""), true),
    ] {
        assert!(NativeIdentity::opencode(&unsupported).is_none());
        let native = opencode_snapshot(&[unsupported]);
        let observed = fixture
            .bridge
            .observe("opencode", &scope, native.clone(), false)
            .unwrap();
        assert!(observed.pool_candidates.is_empty());
        assert!(
            fixture
                .bridge
                .bind(&scope, &native, request("opencode-cli", "forged-candidate"))
                .is_err()
        );
        assert!(fixture.pool.list().unwrap().is_empty());
    }
    let native = opencode_snapshot(&[connection("openai", Some("credential-a"), true)]);
    assert_eq!(
        fixture.bind(&scope, &native).status,
        AccountStatus::Available
    );
}

#[test]
fn provider_only_opencode_disconnect_does_not_sign_out_other_v2_connections() {
    let fixture = Fixture::new();
    let scope = fixture.scope("opencode", "native-home");
    let connections = [
        connection("openai", Some("credential-a"), true),
        connection("openai", Some("credential-b"), true),
        connection("openai", Some("unbound-credential"), true),
    ];
    let accounts = fixture.bind_connections(&scope, &connections[..2]);
    let native = opencode_snapshot(&connections);
    let observed = fixture
        .bridge
        .observe("opencode", &scope, native.clone(), false)
        .unwrap();
    let candidate = observed
        .pool_candidates
        .iter()
        .find(|candidate| candidate.bound_account_id.is_none())
        .unwrap();
    fixture
        .bridge
        .signed_out(&scope, Some(&connection("openai", None, true)))
        .unwrap();
    for account in accounts {
        let current = fixture.pool.get(&account.id).unwrap();
        assert_eq!(current.status, AccountStatus::Available);
        assert_eq!(current.credential_generation, account.credential_generation);
    }
    assert_eq!(
        fixture
            .bridge
            .bind(&scope, &native, request("opencode-cli", &candidate.id),)
            .unwrap_err(),
        BINDING_ERROR
    );
    assert_eq!(fixture.pool.list().unwrap().len(), 2);
}

#[test]
fn opencode_disconnect_matches_both_provider_and_connection_in_the_current_scope() {
    let fixture = Fixture::new();
    let scope = fixture.scope("opencode", "native-home");
    let other_scope = fixture.scope("opencode", "other-native-home");
    let connections = [
        connection("openai", Some("credential-a"), true),
        connection("openai", Some("credential-b"), true),
        connection("other-provider", Some("credential-a"), true),
    ];
    let accounts = fixture.bind_connections(&scope, &connections);
    fixture
        .bridge
        .signed_out(&other_scope, Some(&connections[0]))
        .unwrap();
    for account in &accounts {
        assert_eq!(
            fixture.pool.get(&account.id).unwrap().status,
            AccountStatus::Available
        );
    }
    fixture
        .bridge
        .signed_out(&scope, Some(&connections[0]))
        .unwrap();
    assert_eq!(
        fixture.pool.get(&accounts[0].id).unwrap().status,
        AccountStatus::LoginRequired
    );
    for account in &accounts[1..] {
        let current = fixture.pool.get(&account.id).unwrap();
        assert_eq!(current.status, AccountStatus::Available);
        assert_eq!(current.credential_generation, account.credential_generation);
    }
}

#[test]
fn invalidating_one_scope_does_not_invalidate_another_native_account() {
    let fixture = Fixture::new();
    let scope = fixture.scope("codex", "native-home");
    let other_scope = fixture.scope("codex", "other-native-home");
    let native = snapshot("codex-cli", "subject-a");
    let account = fixture.bind(&scope, &native);
    let other = fixture.bind(&other_scope, &snapshot("codex-cli", "subject-b"));
    fixture.bridge.invalidate(&scope).unwrap();
    assert_eq!(
        fixture.pool.get(&account.id).unwrap().status,
        AccountStatus::NativeCheckRequired
    );
    assert_eq!(
        fixture.pool.get(&other.id).unwrap().status,
        AccountStatus::Available
    );
    fixture
        .bridge
        .observe("codex", &scope, native, false)
        .unwrap();
    assert_eq!(
        fixture.pool.get(&account.id).unwrap().status,
        AccountStatus::Available
    );
}

#[test]
fn concurrent_refreshes_of_one_binding_share_one_success_but_a_later_refresh_runs_again() {
    let operations = NativeOperations::default();
    let native = snapshot("codex-cli", "first-native-subject");
    let expected = observed_value(Ok(native.clone()));
    let (callbacks, results) = overlapping_refreshes(
        &operations,
        "binding-a",
        Ok(native),
        "binding-a",
        Ok(snapshot("codex-cli", "unexpected-second-refresh")),
    );
    assert_eq!(callbacks, ["first"]);
    for result in results {
        assert_eq!(result, expected);
    }
    let fresh = snapshot("codex-cli", "later-native-subject");
    let expected = observed_value(Ok(fresh.clone()));
    let mut called = false;
    let result = operations.observe_for("binding-a", true, || {
        called = true;
        Ok(fresh)
    });
    assert!(
        called,
        "completed refreshes must not become a permanent cache"
    );
    assert_eq!(observed_value(result), expected);
}

#[test]
fn concurrent_failed_refreshes_share_the_error_and_a_later_call_can_retry() {
    let operations = NativeOperations::default();
    let failure = "synthetic native refresh failure".to_owned();
    let (callbacks, results) = overlapping_refreshes(
        &operations,
        "binding-a",
        Err(failure.clone()),
        "binding-a",
        Ok(snapshot("codex-cli", "must-not-retry-in-the-same-flight")),
    );
    assert_eq!(callbacks, ["first"]);
    for result in results {
        assert_eq!(result, Err(failure.clone()));
    }
    let fresh = snapshot("codex-cli", "successful-retry");
    let expected = observed_value(Ok(fresh.clone()));
    let mut called = false;
    let result = operations.observe_for("binding-a", true, || {
        called = true;
        Ok(fresh)
    });
    assert!(called, "a failed refresh must remain retryable");
    assert_eq!(observed_value(result), expected);
}

#[test]
fn concurrent_refreshes_with_different_keys_never_share_successes_or_failures() {
    for first_result in [
        Ok(snapshot("codex-cli", "binding-a-private-subject")),
        Err("binding-a-refresh-failed".into()),
    ] {
        let operations = NativeOperations::default();
        let second_result = Ok(snapshot("codex-cli", "binding-b-private-subject"));
        let expected_first = observed_value(first_result.clone());
        let expected_second = observed_value(second_result.clone());
        let (callbacks, [first, second]) = overlapping_refreshes(
            &operations,
            "scope-a/binding-a",
            first_result,
            "scope-b/binding-b",
            second_result,
        );
        assert_eq!(callbacks, ["first", "second"]);
        assert_eq!(first, expected_first);
        assert_eq!(second, expected_second);
    }
}

#[test]
fn logout_waits_for_reads_and_refreshes_and_the_old_result_cannot_revive_the_account() {
    for refresh in [false, true] {
        let fixture = Fixture::new();
        let scope = fixture.scope("codex", "native-home");
        let native = snapshot("codex-cli", "synthetic-subject");
        let account = fixture.bind(&scope, &native);
        let late_success = fixture.select();
        let operations = NativeOperations::default();
        let events = Mutex::new(Vec::new());
        let (entered, entries) = mpsc::channel();
        let read_entered = entered.clone();
        let (release, released) = mpsc::channel();
        let ready = Barrier::new(2);
        let stale_candidate = thread::scope(|threads| {
            let operations = &operations;
            let events = &events;
            let bridge = &fixture.bridge;
            let scope = &scope;
            let native = &native;
            let read = threads.spawn(move || {
                operations.observe(refresh, || {
                    events.lock().unwrap().push("read-started");
                    read_entered.send("read").unwrap();
                    released
                        .recv_timeout(WATCHDOG)
                        .expect("native read was not released");
                    let observed = bridge.observe("codex", scope, native.clone(), false)?;
                    events.lock().unwrap().push("read-applied");
                    Ok(observed)
                })
            });
            assert_eq!(entries.recv_timeout(WATCHDOG).unwrap(), "read");
            let ready = &ready;
            let logout = threads.spawn(move || {
                ready.wait();
                operations.mutate(|| {
                    events.lock().unwrap().push("logout-started");
                    entered.send("logout").unwrap();
                    bridge.signed_out(scope, None)?;
                    events.lock().unwrap().push("logout-applied");
                    Ok(())
                })
            });
            ready.wait();
            assert_eq!(
                entries.recv_timeout(CONTENTION_WINDOW),
                Err(mpsc::RecvTimeoutError::Timeout),
                "logout must wait until the native observation has been applied"
            );
            release.send(()).unwrap();
            let observed = read.join().unwrap().unwrap();
            logout.join().unwrap().unwrap();
            assert_eq!(entries.recv_timeout(WATCHDOG).unwrap(), "logout");
            assert_eq!(
                observed.pool_candidates[0].bound_account_id.as_deref(),
                Some(account.id.as_str())
            );
            observed.pool_candidates[0].id.clone()
        });
        assert!(
            operations.gate.lock().unwrap().is_none(),
            "logout must discard the pre-logout cached refresh result"
        );
        assert_eq!(
            *events.lock().unwrap(),
            [
                "read-started",
                "read-applied",
                "logout-started",
                "logout-applied",
            ]
        );
        let signed_out = fixture.pool.get(&account.id).unwrap();
        assert_eq!(signed_out.status, AccountStatus::LoginRequired);
        assert_eq!(
            signed_out.authorization_status,
            AuthorizationStatus::LoginRequired
        );
        assert!(signed_out.credential_generation > account.credential_generation);
        assert_eq!(
            fixture.metadata(&account.id)["nativeReference"],
            Value::Null
        );
        assert_eq!(
            fixture
                .bridge
                .bind(
                    &scope,
                    &native,
                    request(&native.client_id, &stale_candidate),
                )
                .unwrap_err(),
            BINDING_ERROR
        );
        let after_success = fixture
            .pool
            .record_outcome(&late_success, AccountOutcome::from_http(200, None))
            .unwrap();
        assert_eq!(after_success.status, AccountStatus::LoginRequired);
        assert_eq!(
            after_success.credential_generation,
            signed_out.credential_generation
        );

        let mut checked_after_logout = false;
        let observed = operations
            .observe(true, || {
                checked_after_logout = true;
                let mut logged_out = native.clone();
                logged_out.auth_mode = "none".into();
                logged_out.identities.clear();
                fixture.bridge.observe("codex", &scope, logged_out, false)
            })
            .unwrap();
        assert!(checked_after_logout);
        assert!(observed.pool_candidates.is_empty());
        assert_eq!(
            fixture.pool.get(&account.id).unwrap().status,
            AccountStatus::LoginRequired
        );
        assert_eq!(
            fixture.pool.select(&fixture.admit()).unwrap_err(),
            AccountError::NoEligibleAccount
        );
    }
}
