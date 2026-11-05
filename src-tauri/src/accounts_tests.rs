use super::*;
use serde_json::{Value, json};
use std::collections::{BTreeMap, VecDeque};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use tokio::sync::{Barrier, Notify, Semaphore};

const TEST_DESTINATION: &str = "https://provider.example.test/v1";

#[derive(Default)]
struct MemoryCredentials {
    values: Mutex<HashMap<String, String>>,
    reads: AtomicUsize,
    fail_refresh_write: AtomicBool,
    fail_delete: AtomicBool,
}

impl CredentialStore for MemoryCredentials {
    fn set(&self, id: &str, value: &str) -> Result<(), AccountError> {
        if id.ends_with(".refresh") && self.fail_refresh_write.load(Ordering::SeqCst) {
            return Err(AccountError::CredentialStoreUnavailable);
        }
        self.values
            .lock()
            .unwrap()
            .insert(id.to_owned(), value.to_owned());
        Ok(())
    }
    fn get(&self, id: &str) -> Result<Option<AccountSecret>, AccountError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.values
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .map(AccountSecret::new)
            .transpose()
    }
    fn delete(&self, id: &str) -> Result<(), AccountError> {
        if self.fail_delete.load(Ordering::SeqCst) {
            return Err(AccountError::CredentialStoreUnavailable);
        }
        self.values.lock().unwrap().remove(id);
        Ok(())
    }
}

struct Fixture {
    db: Database,
    pool: AccountPool,
    store: Arc<MemoryCredentials>,
}

impl Fixture {
    fn new() -> Self {
        let db = Database::open(Path::new(":memory:")).unwrap();
        for id in ["accounts-provider", "other-provider"] {
            let provider = serde_json::from_value(json!({
                "id": id, "name": "Synthetic provider", "providerType": "custom",
                "endpoint": "https://provider.example.test/v1", "model": "synthetic-model",
                "appGroup": "Codex", "enabled": true, "sortOrder": 0
            }))
            .unwrap();
            db.save_provider(&provider).unwrap();
        }
        let store = Arc::new(MemoryCredentials::default());
        let pool = AccountPool::with_store(db.clone(), store.clone()).unwrap();
        Self { db, pool, store }
    }
    fn api(&self, alias: &str) -> AccountInfo {
        self.pool
            .add(AddAccountRequest {
                alias: alias.into(),
                provider_id: "accounts-provider".into(),
                approved_destination: Some(TEST_DESTINATION.into()),
                credentials: AccountCredentialInput::ApiKey {
                    api_key: secret(&format!("synthetic-key-{alias}")),
                },
                available_models: vec![],
                allow_refresh: false,
                confirmed: true,
            })
            .unwrap()
    }
    fn oauth(&self, alias: &str, expired: bool) -> AccountInfo {
        self.pool
            .add(AddAccountRequest {
                alias: alias.into(),
                provider_id: "accounts-provider".into(),
                approved_destination: Some(TEST_DESTINATION.into()),
                credentials: oauth_input(
                    "synthetic-access-initial",
                    "synthetic-refresh-initial",
                    expired,
                ),
                available_models: vec![],
                allow_refresh: true,
                confirmed: true,
            })
            .unwrap()
    }
    fn native(&self, client: AccountClient) -> (AccountInfo, String) {
        let account = self
            .pool
            .add(AddAccountRequest {
                alias: "native".into(),
                provider_id: "accounts-provider".into(),
                approved_destination: Some(TEST_DESTINATION.into()),
                credentials: AccountCredentialInput::NativeManaged { client },
                available_models: vec![],
                allow_refresh: false,
                confirmed: true,
            })
            .unwrap();
        let reference = Uuid::new_v4().to_string();
        let authorized = self
            .pool
            .update_native_state(
                &account.id,
                account.credential_generation,
                native_state(client, &reference, true),
            )
            .unwrap();
        (authorized, reference)
    }
    fn admit(&self, affinity: Option<&str>) -> SelectionContext {
        let task_id = Uuid::new_v4().to_string();
        let attempt_id = Uuid::new_v4().to_string();
        let connection = self.db.conn();
        let connection = connection.lock().unwrap();
        connection.execute("INSERT INTO proxy_tasks(id,identity_source,max_attempts,max_microusd,attempts,reserved_microusd,created_at,updated_at) VALUES(?1,'synthetic',4,1000,1,10,?2,?2)", params![task_id, now()]).unwrap();
        connection.execute("INSERT INTO proxy_attempts(id,task_id,request_id,ordinal,provider_id,model,route,reserved_tokens,reserved_microusd,status,started_at) VALUES(?1,?2,?1,1,'accounts-provider','synthetic-model','responses',32,10,'sending',?3)", params![attempt_id, task_id, now()]).unwrap();
        connection.execute("INSERT INTO budget_ledger(id,day,provider_id,model,tokens,cost_microusd,source,created_at) VALUES(?1,1,'accounts-provider','synthetic-model',32,10,'synthetic',?2)", params![attempt_id, now()]).unwrap();
        SelectionContext {
            provider_id: "accounts-provider".into(),
            destination: Some(TEST_DESTINATION.into()),
            http_only: false,
            model: "synthetic-model".into(),
            task_id,
            attempt_id,
            affinity_key: affinity.map(str::to_owned),
            cancellation: AccountCancellation::default(),
        }
    }
    fn budgets(&self) -> Vec<(String, i64, i64)> {
        let connection = self.db.conn();
        let connection = connection.lock().unwrap();
        let mut query = connection.prepare("SELECT id,attempts,reserved_microusd FROM proxy_tasks UNION ALL SELECT id,tokens,cost_microusd FROM budget_ledger ORDER BY id").unwrap();
        query
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }
    fn dispatched(&self, context: &SelectionContext) -> bool {
        let connection = self.db.conn();
        let connection = connection.lock().unwrap();
        selection_on(&connection, &context.attempt_id)
            .unwrap()
            .unwrap()
            .1
    }
    fn destination(&self, destination: &str) {
        self.db
            .conn()
            .lock()
            .unwrap()
            .execute(
                "UPDATE providers SET endpoint=?1 WHERE id='accounts-provider'",
                [destination],
            )
            .unwrap();
    }
    fn replacement(&self, account: &AccountInfo, access: &str, refresh: &str) -> AccountInfo {
        self.pool
            .replace_credentials(ReplaceCredentialRequest {
                id: account.id.clone(),
                expected_revision: account.revision,
                approved_destination: Some(TEST_DESTINATION.into()),
                credentials: oauth_input(access, refresh, false),
                allow_refresh: true,
                confirmed: true,
            })
            .unwrap()
    }
}

fn secret(value: &str) -> AccountSecret {
    AccountSecret::new(value.to_owned()).unwrap()
}
fn oauth_input(access: &str, refresh: &str, expired: bool) -> AccountCredentialInput {
    AccountCredentialInput::OAuth {
        access_token: secret(access),
        refresh_token: Some(secret(refresh)),
        expires_at: Some(now() + if expired { -30 } else { 3600 }),
    }
}
fn quota(remaining: f64) -> QuotaSnapshot {
    QuotaSnapshot {
        source: QuotaSource::ProviderResponse,
        observed_at: now(),
        valid_until: now() + 600,
        windows: vec![QuotaWindow {
            id: "five-hour".into(),
            unit: QuotaUnit::Requests,
            remaining: Some(remaining),
            limit: Some(100.0),
            resets_at: Some(now() + 600),
        }],
    }
}
fn outcome(kind: OutcomeKind) -> AccountOutcome {
    AccountOutcome {
        kind,
        retry_after_seconds: None,
        quota: None,
    }
}
fn native_state(client: AccountClient, reference: &str, authorized: bool) -> NativeAccountState {
    NativeAccountState {
        client,
        reference_id: reference.into(),
        authorized,
        expires_at: None,
        available_models: vec!["synthetic-model".into()],
    }
}

fn updated(account: &AccountInfo, enabled: bool) -> UpdateAccountRequest {
    UpdateAccountRequest {
        id: account.id.clone(),
        expected_revision: account.revision,
        alias: account.alias.clone(),
        provider_id: account.provider_id.clone(),
        approved_destination: None,
        enabled,
        available_models: account.available_models.clone(),
        confirmed: true,
    }
}

#[derive(Clone)]
struct ControlledRefresher {
    calls: Arc<AtomicUsize>,
    seen: Arc<Mutex<Vec<String>>>,
    started: Arc<Notify>,
    permits: Arc<Semaphore>,
    results: Arc<Mutex<VecDeque<Result<RefreshedCredential, RefreshError>>>>,
    dropped: Arc<AtomicUsize>,
}

impl ControlledRefresher {
    fn new() -> Self {
        Self {
            calls: Arc::new(AtomicUsize::new(0)),
            seen: Arc::new(Mutex::new(vec![])),
            started: Arc::new(Notify::new()),
            permits: Arc::new(Semaphore::new(0)),
            results: Arc::new(Mutex::new(VecDeque::new())),
            dropped: Arc::new(AtomicUsize::new(0)),
        }
    }
    fn register(&self, pool: &AccountPool, id: &str) {
        pool.register_refresher(id, Arc::new(self.clone()), Duration::from_secs(2))
            .unwrap();
    }
    fn result(&self, access: &str, refresh: Option<&str>) {
        self.results
            .lock()
            .unwrap()
            .push_back(Ok(RefreshedCredential {
                access_token: secret(access),
                refresh_token: refresh.map(secret),
                expires_at: Some(now() + 3600),
            }));
    }
}

struct DropCounter(Arc<AtomicUsize>);
impl Drop for DropCounter {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Clone)]
struct GuardedRefresher {
    runtime: Arc<PoolRuntime>,
    database: Arc<Mutex<Connection>>,
    created: Arc<AtomicUsize>,
    polled: Arc<AtomicUsize>,
    dropped: Arc<AtomicUsize>,
}

impl GuardedRefresher {
    fn new(pool: &AccountPool) -> Self {
        Self {
            runtime: pool.runtime.clone(),
            database: pool.database.conn(),
            created: Arc::new(AtomicUsize::new(0)),
            polled: Arc::new(AtomicUsize::new(0)),
            dropped: Arc::new(AtomicUsize::new(0)),
        }
    }
}

impl AccountRefresher for GuardedRefresher {
    fn refresh(
        &self,
        request: RefreshRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RefreshedCredential, RefreshError>> + Send>> {
        self.created.fetch_add(1, Ordering::SeqCst);
        assert!(self.runtime.gate.try_lock().is_err());
        assert!(self.database.try_lock().is_err());
        assert!(request.cancellation.0.gate.try_lock().is_err());
        let shared = self.clone();
        let cancellation = request.cancellation;
        let guard = DropCounter(shared.dropped.clone());
        Box::pin(std::future::poll_fn(move |_task| {
            let _keep_alive = &guard;
            if shared.polled.fetch_add(1, Ordering::SeqCst) == 0 {
                assert!(shared.runtime.gate.try_lock().is_err());
                assert!(shared.database.try_lock().is_err());
                assert!(cancellation.0.gate.try_lock().is_err());
            }
            Poll::Pending
        }))
    }
}

fn synthetic_refresh(account: &AccountInfo) -> (RefreshFlight, RefreshRequest) {
    let flight = RefreshFlight {
        generation: account.credential_generation,
        completed: watch::channel(None).0,
        cancellation: AccountCancellation::default(),
    };
    let request = RefreshRequest {
        account_id: account.id.clone(),
        provider_id: account.provider_id.clone(),
        expected_generation: account.credential_generation,
        refresh_token: secret("synthetic-refresh-initial"),
        cancellation: flight.cancellation.clone(),
    };
    (flight, request)
}

impl AccountRefresher for ControlledRefresher {
    fn refresh(
        &self,
        request: RefreshRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RefreshedCredential, RefreshError>> + Send>> {
        let shared = self.clone();
        Box::pin(async move {
            let _drop = DropCounter(shared.dropped.clone());
            let count = shared.calls.fetch_add(1, Ordering::SeqCst) + 1;
            shared
                .seen
                .lock()
                .unwrap()
                .push(request.refresh_token.expose().to_owned());
            shared.started.notify_one();
            shared.permits.acquire().await.unwrap().forget();
            shared
                .results
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| {
                    Ok(RefreshedCredential {
                        access_token: secret(&format!("synthetic-refreshed-{count}")),
                        refresh_token: None,
                        expires_at: Some(now() + 3600),
                    })
                })
        })
    }
}

async fn receivers(pool: &AccountPool, id: &str, count: usize) {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let actual = pool
                .runtime
                .flights
                .lock()
                .unwrap()
                .get(id)
                .map(|flight| flight.completed.receiver_count())
                .unwrap_or(0);
            if actual >= count {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
}
fn start_refresh(
    pool: &AccountPool,
    id: &str,
    cancellation: AccountCancellation,
) -> tokio::task::JoinHandle<Result<AccountInfo, AccountError>> {
    let pool = pool.clone();
    let id = id.to_owned();
    tokio::spawn(async move { pool.refresh(&id, &cancellation).await })
}

#[test]
fn explicit_authorization_and_redacted_secrets_are_enforced() {
    let fixture = Fixture::new();
    let request = AddAccountRequest {
        alias: "private".into(),
        provider_id: "accounts-provider".into(),
        approved_destination: Some(TEST_DESTINATION.into()),
        credentials: oauth_input(
            "synthetic-access-initial",
            "synthetic-refresh-initial",
            false,
        ),
        available_models: vec![],
        allow_refresh: true,
        confirmed: false,
    };
    let debug = format!("{request:?}");
    assert!(!debug.contains("synthetic-access-initial"));
    assert!(!debug.contains("synthetic-refresh-initial"));
    assert_eq!(
        fixture.pool.add(request).unwrap_err(),
        AccountError::AuthorizationRequired
    );
    assert!(fixture.store.values.lock().unwrap().is_empty());
    let info = fixture.oauth("private", false);
    assert_eq!(info.authorization_status, AuthorizationStatus::Unverified);
    let snapshot = serde_json::to_string(&info).unwrap();
    assert!(!snapshot.contains("synthetic-access-initial"));
    assert!(!snapshot.contains("refreshToken"));
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let metadata: String = connection
        .query_row("SELECT metadata FROM account_pool_accounts", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(!metadata.contains("synthetic-access-initial"));
    assert!(!metadata.contains("synthetic-refresh-initial"));
    assert!(
        !format!(
            "{:?}",
            AccountAuthorization::OAuth(secret("synthetic-only"))
        )
        .contains("synthetic-only")
    );
    assert!(
        !format!(
            "{:?}",
            RefreshedCredential {
                access_token: secret("synthetic-only"),
                refresh_token: None,
                expires_at: None
            }
        )
        .contains("synthetic-only")
    );
    let update = UpdateAccountRequest {
        id: "synthetic-private-id".into(),
        expected_revision: 1,
        alias: "synthetic-private-alias".into(),
        provider_id: "synthetic-private-provider".into(),
        approved_destination: None,
        enabled: true,
        available_models: vec!["synthetic-private-model".into()],
        confirmed: true,
    };
    assert_eq!(format!("{update:?}"), "UpdateAccountRequest([REDACTED])");
    let import = ImportAccountRequest {
        source_id: "synthetic-private-source".into(),
        alias: "synthetic-private-alias".into(),
        provider_id: "synthetic-private-provider".into(),
        approved_destination: Some(TEST_DESTINATION.into()),
        allow_refresh: false,
        confirmed: true,
    };
    assert_eq!(format!("{import:?}"), "ImportAccountRequest([REDACTED])");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_refreshes_share_one_provider_callback_and_preserve_quota() {
    let fixture = Fixture::new();
    let account = fixture.oauth("parallel", true);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &account.id);
    fixture
        .pool
        .update_quota(&account.id, account.credential_generation, quota(0.0))
        .unwrap();
    let mut tasks = vec![];
    for _ in 0..12 {
        tasks.push(start_refresh(
            &fixture.pool,
            &account.id,
            AccountCancellation::default(),
        ));
    }
    receivers(&fixture.pool, &account.id, 12).await;
    tokio::time::timeout(Duration::from_secs(2), callback.started.notified())
        .await
        .unwrap();
    assert_eq!(callback.calls.load(Ordering::SeqCst), 1);
    callback.permits.add_permits(1);
    for task in tasks {
        assert_eq!(task.await.unwrap().unwrap().credential_generation, 2);
    }
    assert_eq!(callback.calls.load(Ordering::SeqCst), 1);
    let current = fixture.pool.get(&account.id).unwrap();
    assert_eq!(current.status, AccountStatus::QuotaLimited);
    assert_eq!(current.quota.unwrap().windows[0].remaining, Some(0.0));
}

#[tokio::test]
async fn sequential_refreshes_rotate_and_retain_the_refresh_token() {
    let fixture = Fixture::new();
    let account = fixture.oauth("serial", true);
    let callback = ControlledRefresher::new();
    callback.result("synthetic-access-next", Some("synthetic-refresh-rotated"));
    callback.result("synthetic-access-last", None);
    callback.permits.add_permits(2);
    callback.register(&fixture.pool, &account.id);
    fixture
        .pool
        .refresh(&account.id, &AccountCancellation::default())
        .await
        .unwrap();
    let last = fixture
        .pool
        .refresh(&account.id, &AccountCancellation::default())
        .await
        .unwrap();
    assert_eq!(
        *callback.seen.lock().unwrap(),
        ["synthetic-refresh-initial", "synthetic-refresh-rotated"]
    );
    assert_eq!(last.credential_generation, 3);
    let credential = fixture
        .store
        .get(&secret_id(&account.id, 3, "refresh"))
        .unwrap()
        .unwrap();
    assert_eq!(credential.expose(), "synthetic-refresh-rotated");
    assert_eq!(fixture.store.values.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn automatic_refresh_skips_fresh_credentials_and_obeys_admitted_candidates() {
    let fixture = Fixture::new();
    let account = fixture.oauth("automatic", true);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &account.id);
    callback.permits.add_permits(1);
    let context = fixture.admit(None);
    let before = fixture.budgets();
    assert!(fixture.pool.eligible(&context).unwrap().is_empty());
    assert_eq!(
        fixture.pool.refresh_candidates(&context).unwrap()[0].id,
        account.id
    );
    let first = fixture
        .pool
        .refresh_if_needed(&account.id, &context.cancellation)
        .await
        .unwrap();
    for _ in 0..8 {
        assert_eq!(
            fixture
                .pool
                .refresh_if_needed(&account.id, &context.cancellation)
                .await
                .unwrap()
                .credential_generation,
            first.credential_generation
        );
    }
    let selection = fixture.pool.select(&context).unwrap();
    assert_eq!(
        selection.credential_generation(),
        first.credential_generation
    );
    assert_eq!(callback.calls.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.budgets(), before);
}

#[tokio::test]
async fn cancelling_one_waiter_does_not_cancel_shared_refresh_or_dispatch_it() {
    let fixture = Fixture::new();
    let account = fixture.oauth("waiters", true);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &account.id);
    let context = fixture.admit(None);
    let cancelled = start_refresh(&fixture.pool, &account.id, context.cancellation.clone());
    let active = start_refresh(&fixture.pool, &account.id, AccountCancellation::default());
    receivers(&fixture.pool, &account.id, 2).await;
    context.cancellation.cancel();
    assert_eq!(
        cancelled.await.unwrap().unwrap_err(),
        AccountError::Cancelled
    );
    callback.permits.add_permits(1);
    active.await.unwrap().unwrap();
    assert_eq!(
        fixture.pool.select(&context).unwrap_err(),
        AccountError::Cancelled
    );
    assert_eq!(callback.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn refreshes_for_different_accounts_can_be_in_flight_together() {
    let fixture = Fixture::new();
    let first = fixture.oauth("first", true);
    let second = fixture.oauth("second", true);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &first.id);
    callback.register(&fixture.pool, &second.id);
    let left = start_refresh(&fixture.pool, &first.id, AccountCancellation::default());
    let right = start_refresh(&fixture.pool, &second.id, AccountCancellation::default());
    receivers(&fixture.pool, &first.id, 1).await;
    receivers(&fixture.pool, &second.id, 1).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while callback.calls.load(Ordering::SeqCst) != 2 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    callback.permits.add_permits(2);
    left.await.unwrap().unwrap();
    right.await.unwrap().unwrap();
}

#[tokio::test]
async fn replacement_cancels_old_refresh_and_stale_results_cannot_overwrite_it() {
    let fixture = Fixture::new();
    let account = fixture.oauth("replace", true);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &account.id);
    let task = start_refresh(&fixture.pool, &account.id, AccountCancellation::default());
    receivers(&fixture.pool, &account.id, 1).await;
    let flight = fixture.pool.runtime.flights.lock().unwrap()[&account.id].clone();
    let replacement = fixture.replacement(
        &account,
        "synthetic-replaced-access",
        "synthetic-replaced-refresh",
    );
    assert_eq!(
        task.await.unwrap().unwrap_err(),
        AccountError::StaleSelection
    );
    assert_eq!(
        fixture
            .pool
            .finish_refresh(
                &flight,
                &account.id,
                Ok(RefreshedCredential {
                    access_token: secret("synthetic-late-access"),
                    refresh_token: None,
                    expires_at: Some(now() + 3600)
                })
            )
            .unwrap_err(),
        AccountError::StaleSelection
    );
    assert_eq!(
        fixture
            .store
            .get(&secret_id(
                &account.id,
                replacement.credential_generation,
                "access"
            ))
            .unwrap()
            .unwrap()
            .expose(),
        "synthetic-replaced-access"
    );
}

#[tokio::test]
async fn disabling_and_revoking_cancel_refresh_and_do_not_resurrect_accounts() {
    let fixture = Fixture::new();
    let account = fixture.oauth("revoke", true);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &account.id);
    let task = start_refresh(&fixture.pool, &account.id, AccountCancellation::default());
    receivers(&fixture.pool, &account.id, 1).await;
    fixture.pool.update(updated(&account, false)).unwrap();
    assert_eq!(task.await.unwrap().unwrap_err(), AccountError::Cancelled);
    let disabled = fixture.pool.get(&account.id).unwrap();
    fixture.pool.update(updated(&disabled, true)).unwrap();
    let task = start_refresh(&fixture.pool, &account.id, AccountCancellation::default());
    receivers(&fixture.pool, &account.id, 1).await;
    let flight = fixture.pool.runtime.flights.lock().unwrap()[&account.id].clone();
    let receipt = fixture.pool.revoke(&account.id, true).unwrap();
    assert!(receipt.local_reference_revoked && receipt.credentials_removed);
    assert_eq!(task.await.unwrap().unwrap_err(), AccountError::Revoked);
    assert_eq!(
        fixture
            .pool
            .finish_refresh(
                &flight,
                &account.id,
                Ok(RefreshedCredential {
                    access_token: secret("synthetic-late-access"),
                    refresh_token: None,
                    expires_at: Some(now() + 3600)
                })
            )
            .unwrap_err(),
        AccountError::Revoked
    );
    assert_eq!(
        fixture.pool.get(&account.id).unwrap().status,
        AccountStatus::Revoked
    );
    assert!(fixture.store.values.lock().unwrap().is_empty());
}

#[tokio::test]
async fn refresh_rate_limits_and_unauthorized_results_block_retry_storms() {
    let fixture = Fixture::new();
    let account = fixture.oauth("limited", true);
    let callback = ControlledRefresher::new();
    callback
        .results
        .lock()
        .unwrap()
        .push_back(Err(RefreshError::RateLimited {
            retry_after_seconds: 99_999,
        }));
    callback.permits.add_permits(1);
    callback.register(&fixture.pool, &account.id);
    assert_eq!(
        fixture
            .pool
            .refresh(&account.id, &AccountCancellation::default())
            .await
            .unwrap_err(),
        AccountError::RefreshRateLimited
    );
    let cooling = fixture.pool.get(&account.id).unwrap();
    assert!(cooling.cooldown_until.unwrap() <= now() + 3600);
    assert_eq!(
        fixture
            .pool
            .refresh(&account.id, &AccountCancellation::default())
            .await
            .unwrap_err(),
        AccountError::RefreshRateLimited
    );
    assert_eq!(callback.calls.load(Ordering::SeqCst), 1);
    let fresh = fixture.replacement(
        &cooling,
        "synthetic-reset-access",
        "synthetic-reset-refresh",
    );
    callback
        .results
        .lock()
        .unwrap()
        .push_back(Err(RefreshError::Unauthorized));
    callback.permits.add_permits(1);
    assert_eq!(
        fixture
            .pool
            .refresh(&fresh.id, &AccountCancellation::default())
            .await
            .unwrap_err(),
        AccountError::LoginRequired
    );
    assert_eq!(
        fixture.pool.get(&fresh.id).unwrap().status,
        AccountStatus::LoginRequired
    );
    assert_eq!(
        fixture
            .pool
            .refresh(&fresh.id, &AccountCancellation::default())
            .await
            .unwrap_err(),
        AccountError::LoginRequired
    );
    assert_eq!(callback.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn expired_refresh_results_persist_login_required_without_overwriting_credentials() {
    let fixture = Fixture::new();
    let account = fixture.oauth("expired-result", true);
    let callback = ControlledRefresher::new();
    callback
        .results
        .lock()
        .unwrap()
        .push_back(Ok(RefreshedCredential {
            access_token: secret("synthetic-expired-result"),
            refresh_token: Some(secret("synthetic-unused-refresh")),
            expires_at: Some(now() - 1),
        }));
    callback.register(&fixture.pool, &account.id);
    let before = fixture.store.values.lock().unwrap().clone();
    let first = start_refresh(&fixture.pool, &account.id, AccountCancellation::default());
    let second = start_refresh(&fixture.pool, &account.id, AccountCancellation::default());
    receivers(&fixture.pool, &account.id, 2).await;
    tokio::time::timeout(Duration::from_secs(2), callback.started.notified())
        .await
        .unwrap();
    callback.permits.add_permits(1);
    assert_eq!(
        first.await.unwrap().unwrap_err(),
        AccountError::LoginRequired
    );
    assert_eq!(
        second.await.unwrap().unwrap_err(),
        AccountError::LoginRequired
    );
    let current = fixture.pool.get(&account.id).unwrap();
    assert_eq!(current.status, AccountStatus::LoginRequired);
    assert_eq!(
        current.authorization_status,
        AuthorizationStatus::LoginRequired
    );
    assert_eq!(current.credential_generation, account.credential_generation);
    assert_eq!(*fixture.store.values.lock().unwrap(), before);
    let connection = fixture.db.conn();
    let metadata: String = connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT metadata FROM account_pool_accounts WHERE id=?1",
            [&account.id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        serde_json::from_str::<AccountRecord>(&metadata)
            .unwrap()
            .authorization_status,
        AuthorizationStatus::LoginRequired
    );
    assert!(
        !metadata.contains("synthetic-expired-result")
            && !metadata.contains("synthetic-unused-refresh")
    );
    let context = fixture.admit(None);
    assert!(fixture.pool.eligible(&context).unwrap().is_empty());
    assert!(
        fixture
            .pool
            .refresh_candidates(&context)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .pool
            .refresh_if_needed(&account.id, &context.cancellation)
            .await
            .unwrap_err(),
        AccountError::LoginRequired
    );
    assert_eq!(callback.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn refresh_rejection_survives_runtime_recreation_until_credentials_are_replaced() {
    let fixture = Fixture::new();
    let account = fixture.oauth("persisted-rejection", true);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &account.id);
    callback
        .results
        .lock()
        .unwrap()
        .push_back(Err(RefreshError::Unauthorized));
    callback.permits.add_permits(1);
    assert_eq!(
        fixture
            .pool
            .refresh(&account.id, &AccountCancellation::default())
            .await
            .unwrap_err(),
        AccountError::LoginRequired
    );
    let source_connection = fixture.db.conn();
    let source_connection = source_connection.lock().unwrap();
    let metadata: String = source_connection
        .query_row(
            "SELECT metadata FROM account_pool_accounts WHERE id=?1",
            [&account.id],
            |row| row.get(0),
        )
        .unwrap();
    let record: AccountRecord = serde_json::from_str(&metadata).unwrap();
    assert!(record.refresh_login_required);
    let mut query = source_connection
        .prepare("SELECT item_id FROM account_pool_secret_refs WHERE account_id=?1")
        .unwrap();
    let references = query
        .query_map([&account.id], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    drop(query);
    drop(source_connection);
    let restored = Fixture::new();
    *restored.store.values.lock().unwrap() = fixture.store.values.lock().unwrap().clone();
    let restored_connection = restored.db.conn();
    let restored_connection = restored_connection.lock().unwrap();
    restored_connection
        .execute(
            "INSERT INTO account_pool_accounts(id,metadata) VALUES(?1,?2)",
            params![account.id, metadata],
        )
        .unwrap();
    for item_id in references {
        restored_connection.execute("INSERT INTO account_pool_secret_refs(item_id,account_id,generation) VALUES(?1,?2,?3)",
            params![item_id, account.id, account.credential_generation]).unwrap();
    }
    drop(restored_connection);
    let new_callback = ControlledRefresher::new();
    new_callback.register(&restored.pool, &account.id);
    let current = restored.pool.get(&account.id).unwrap();
    assert_eq!(current.status, AccountStatus::LoginRequired);
    assert!(!current.can_refresh);
    let context = restored.admit(None);
    assert!(restored.pool.eligible(&context).unwrap().is_empty());
    assert!(
        restored
            .pool
            .refresh_candidates(&context)
            .unwrap()
            .is_empty()
    );
    for force in [false, true] {
        let result = if force {
            restored
                .pool
                .refresh(&account.id, &context.cancellation)
                .await
        } else {
            restored
                .pool
                .refresh_if_needed(&account.id, &context.cancellation)
                .await
        };
        assert_eq!(result.unwrap_err(), AccountError::LoginRequired);
    }
    restored.pool.unregister_refresher(&account.id).unwrap();
    new_callback.register(&restored.pool, &account.id);
    assert_eq!(
        restored
            .pool
            .refresh(&account.id, &context.cancellation)
            .await
            .unwrap_err(),
        AccountError::LoginRequired
    );
    assert_eq!(new_callback.calls.load(Ordering::SeqCst), 0);
    let replaced = restored.replacement(
        &current,
        "synthetic-reauthorized-access",
        "synthetic-reauthorized-refresh",
    );
    assert!(replaced.can_refresh);
    assert_eq!(
        replaced.authorization_status,
        AuthorizationStatus::Unverified
    );
    new_callback.result("synthetic-refreshed-after-reauthorization", None);
    new_callback.permits.add_permits(1);
    let refreshed = restored
        .pool
        .refresh(&account.id, &context.cancellation)
        .await
        .unwrap();
    assert_eq!(
        refreshed.credential_generation,
        replaced.credential_generation + 1
    );
    assert_eq!(new_callback.calls.load(Ordering::SeqCst), 1);
    assert_eq!(restored.store.values.lock().unwrap().len(), 2);
}

#[tokio::test]
async fn refresh_factory_and_first_poll_share_pool_database_and_flight_gates() {
    let fixture = Fixture::new();
    let account = fixture.oauth("refresh-start", true);
    let callback = GuardedRefresher::new(&fixture.pool);
    let (flight, request) = synthetic_refresh(&account);
    let before = fixture.store.values.lock().unwrap().clone();
    let mut invocation = Box::pin(fixture.pool.invoke_refresher(
        &flight,
        &account.id,
        Arc::new(callback.clone()),
        request,
        Duration::from_secs(2),
    ));
    std::future::poll_fn(|task| {
        assert!(invocation.as_mut().poll(task).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(callback.created.load(Ordering::SeqCst), 1);
    assert!(callback.polled.load(Ordering::SeqCst) >= 1);
    assert!(fixture.pool.runtime.gate.try_lock().is_ok());
    assert!(fixture.db.conn().try_lock().is_ok());
    assert!(flight.cancellation.0.gate.try_lock().is_ok());
    let previous_polls = callback.polled.load(Ordering::SeqCst);
    flight.cancellation.cancel();
    assert!(matches!(
        invocation.await.unwrap(),
        Err(RefreshError::Cancelled)
    ));
    assert_eq!(callback.polled.load(Ordering::SeqCst), previous_polls);
    assert_eq!(callback.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(*fixture.store.values.lock().unwrap(), before);
    assert_eq!(
        fixture.pool.get(&account.id).unwrap().credential_generation,
        account.credential_generation
    );
}

#[tokio::test]
async fn expired_refresh_start_deadline_blocks_the_factory_and_persists_cooldown() {
    let fixture = Fixture::new();
    let account = fixture.oauth("refresh-deadline", true);
    let callback = GuardedRefresher::new(&fixture.pool);
    let (flight, request) = synthetic_refresh(&account);
    let before = fixture.store.values.lock().unwrap().clone();
    let result = fixture
        .pool
        .invoke_refresher(
            &flight,
            &account.id,
            Arc::new(callback.clone()),
            request,
            Duration::ZERO,
        )
        .await
        .unwrap();
    assert!(matches!(&result, Err(RefreshError::Unavailable)));
    assert_eq!(callback.created.load(Ordering::SeqCst), 0);
    assert_eq!(callback.polled.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture
            .pool
            .finish_refresh(&flight, &account.id, result)
            .unwrap_err(),
        AccountError::RefreshFailed
    );
    let after = fixture.pool.get(&account.id).unwrap();
    assert_eq!(after.status, AccountStatus::CoolingDown);
    assert_eq!(after.credential_generation, account.credential_generation);
    assert_eq!(*fixture.store.values.lock().unwrap(), before);
}

#[tokio::test]
async fn refresh_start_revalidates_revocation_generation_destination_and_cancellation() {
    for mutation in [
        "revoked",
        "replaced",
        "disabled",
        "cancelled",
        "destination",
    ] {
        let fixture = Fixture::new();
        let account = fixture.oauth("refresh-revalidation", true);
        let callback = GuardedRefresher::new(&fixture.pool);
        let (flight, request) = synthetic_refresh(&account);
        let expected = match mutation {
            "revoked" => {
                fixture.pool.revoke(&account.id, true).unwrap();
                AccountError::Revoked
            }
            "replaced" => {
                fixture.replacement(
                    &account,
                    "synthetic-replaced-access",
                    "synthetic-replaced-refresh",
                );
                AccountError::StaleSelection
            }
            "disabled" => {
                fixture.pool.update(updated(&account, false)).unwrap();
                AccountError::Cancelled
            }
            "cancelled" => {
                flight.cancellation.cancel();
                AccountError::Cancelled
            }
            "destination" => {
                fixture.destination("https://changed.example.test/v1");
                AccountError::DestinationApprovalRequired
            }
            _ => unreachable!(),
        };
        let before = fixture.store.values.lock().unwrap().clone();
        assert_eq!(
            fixture
                .pool
                .invoke_refresher(
                    &flight,
                    &account.id,
                    Arc::new(callback.clone()),
                    request,
                    Duration::from_secs(2),
                )
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(callback.created.load(Ordering::SeqCst), 0);
        assert_eq!(callback.polled.load(Ordering::SeqCst), 0);
        assert_eq!(*fixture.store.values.lock().unwrap(), before);
    }
}

#[tokio::test]
async fn timed_out_refresh_is_dropped_and_persistently_cooled_down() {
    let fixture = Fixture::new();
    let account = fixture.oauth("timeout", true);
    let callback = ControlledRefresher::new();
    fixture
        .pool
        .register_refresher(
            &account.id,
            Arc::new(callback.clone()),
            Duration::from_millis(30),
        )
        .unwrap();
    assert_eq!(
        fixture
            .pool
            .refresh(&account.id, &AccountCancellation::default())
            .await
            .unwrap_err(),
        AccountError::RefreshFailed
    );
    assert_eq!(callback.dropped.load(Ordering::SeqCst), 1);
    assert_eq!(
        fixture.pool.get(&account.id).unwrap().status,
        AccountStatus::CoolingDown
    );
    assert_eq!(
        fixture
            .pool
            .refresh(&account.id, &AccountCancellation::default())
            .await
            .unwrap_err(),
        AccountError::RefreshFailed
    );
    assert_eq!(callback.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn failed_staging_retains_cleanup_references_and_can_be_retried() {
    let fixture = Fixture::new();
    let account = fixture.oauth("cleanup", false);
    fixture
        .store
        .fail_refresh_write
        .store(true, Ordering::SeqCst);
    fixture.store.fail_delete.store(true, Ordering::SeqCst);
    let failed = fixture.pool.replace_credentials(ReplaceCredentialRequest {
        id: account.id.clone(),
        expected_revision: account.revision,
        approved_destination: Some(TEST_DESTINATION.into()),
        credentials: oauth_input("synthetic-staged-access", "synthetic-staged-refresh", false),
        allow_refresh: true,
        confirmed: true,
    });
    assert_eq!(
        failed.unwrap_err(),
        AccountError::CredentialStoreUnavailable
    );
    assert_eq!(
        fixture.pool.get(&account.id).unwrap().credential_generation,
        1
    );
    assert!(fixture.pool.get(&account.id).unwrap().cleanup_pending);
    fixture
        .store
        .fail_refresh_write
        .store(false, Ordering::SeqCst);
    let replacement = fixture.replacement(
        &account,
        "synthetic-working-access",
        "synthetic-working-refresh",
    );
    assert_eq!(replacement.credential_generation, 3);
    let revoked = fixture.pool.revoke(&account.id, true).unwrap();
    assert!(!revoked.credentials_removed && revoked.account.cleanup_pending);
    fixture.store.fail_delete.store(false, Ordering::SeqCst);
    assert!(fixture.pool.cleanup_secrets().unwrap());
    assert!(!fixture.pool.get(&account.id).unwrap().cleanup_pending);
    assert!(fixture.store.values.lock().unwrap().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_selection_is_fair_and_does_not_create_another_budget() {
    let fixture = Fixture::new();
    for alias in ["one", "two", "three"] {
        fixture.api(alias);
    }
    fixture
        .pool
        .save_policy(
            "accounts-provider",
            AccountPolicy {
                affinity: false,
                ..Default::default()
            },
            true,
        )
        .unwrap();
    let contexts: Vec<_> = (0..30).map(|_| fixture.admit(None)).collect();
    let before = fixture.budgets();
    let barrier = Arc::new(Barrier::new(31));
    let mut tasks = vec![];
    for context in contexts {
        let pool = fixture.pool.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            pool.select(&context).unwrap()
        }));
    }
    barrier.wait().await;
    let mut selected = BTreeMap::new();
    for task in tasks {
        *selected.entry(task.await.unwrap().account_id).or_insert(0) += 1;
    }
    assert_eq!(selected.len(), 3);
    assert!(selected.values().all(|count| *count == 10));
    assert_eq!(fixture.budgets(), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn same_attempt_is_idempotently_selected_but_dispatched_only_once() {
    let fixture = Fixture::new();
    fixture.api("only");
    let context = fixture.admit(None);
    let before = fixture.budgets();
    let selection = fixture.pool.select(&context).unwrap();
    for _ in 0..8 {
        assert_eq!(
            fixture.pool.select(&context).unwrap().account_id(),
            selection.account_id()
        );
    }
    let sent = Arc::new(AtomicUsize::new(0));
    let mut tasks = vec![];
    for _ in 0..12 {
        let pool = fixture.pool.clone();
        let context = context.clone();
        let selection = selection.clone();
        let sent = sent.clone();
        tasks.push(tokio::spawn(async move {
            pool.dispatch(&selection, &context, move |authorization| async move {
                assert!(matches!(authorization, AccountAuthorization::ApiKey(_)));
                sent.fetch_add(1, Ordering::SeqCst);
                7
            })
            .await
        }));
    }
    let mut successes = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(7) => successes += 1,
            Err(AccountError::AlreadyDispatched) => {}
            other => panic!("unexpected result: {other:?}"),
        }
    }
    assert_eq!(successes, 1);
    assert_eq!(sent.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.budgets(), before);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_parallel_rate_limit_allows_concurrent_reselection_before_dispatch_without_another_budget()
 {
    let fixture = Fixture::new();
    let first = fixture.api("waiting-first");
    let context = fixture.admit(Some("waiting-session"));
    let original = fixture.pool.select(&context).unwrap();
    let parallel_context = fixture.admit(Some("waiting-session"));
    let parallel = fixture.pool.select(&parallel_context).unwrap();
    let replacement = fixture.api("waiting-replacement");
    let before = fixture.budgets();
    assert_eq!(original.account_id(), first.id);
    assert_eq!(parallel.account_id(), first.id);
    fixture
        .pool
        .dispatch(&parallel, &parallel_context, |authorization| async move {
            assert!(matches!(authorization, AccountAuthorization::ApiKey(_)));
        })
        .await
        .unwrap();
    let limited = fixture
        .pool
        .record_outcome(&parallel, AccountOutcome::from_http(429, Some(3600)))
        .unwrap();
    assert_eq!(limited.status, AccountStatus::CoolingDown);
    let started = AtomicUsize::new(0);
    assert_eq!(
        fixture
            .pool
            .dispatch(&original, &context, |_| async {
                started.fetch_add(1, Ordering::SeqCst);
            })
            .await
            .unwrap_err(),
        AccountError::NoEligibleAccount
    );
    let barrier = Arc::new(Barrier::new(13));
    let mut tasks = Vec::new();
    for _ in 0..12 {
        let pool = fixture.pool.clone();
        let context = context.clone();
        let barrier = barrier.clone();
        tasks.push(tokio::spawn(async move {
            barrier.wait().await;
            pool.select(&context).unwrap()
        }));
    }
    barrier.wait().await;
    let mut selections = Vec::new();
    for task in tasks {
        let selection = task.await.unwrap();
        assert_eq!(selection.account_id(), replacement.id);
        assert_eq!(
            selection.credential_generation(),
            replacement.credential_generation
        );
        selections.push(selection);
    }
    assert_eq!(
        selections
            .iter()
            .filter(|selection| selection.reason == SelectionReason::RoundRobin)
            .count(),
        1
    );
    assert_eq!(
        fixture
            .pool
            .dispatch(&original, &context, |_| async {
                started.fetch_add(1, Ordering::SeqCst);
            })
            .await
            .unwrap_err(),
        AccountError::StaleSelection
    );
    assert_eq!(started.load(Ordering::SeqCst), 0);
    fixture
        .pool
        .dispatch(&selections[0], &context, |authorization| {
            assert!(matches!(authorization, AccountAuthorization::ApiKey(_)));
            async {
                started.fetch_add(1, Ordering::SeqCst);
            }
        })
        .await
        .unwrap();
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.budgets(), before);
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let count: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM account_pool_attempts WHERE attempt_id=?1",
            [&context.attempt_id],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 1);
}

#[test]
fn undispatched_reselection_requires_an_available_fixed_binding_after_local_revocation() {
    let fixture = Fixture::new();
    let first = fixture.api("fixed-first");
    fixture
        .pool
        .save_policy(
            "accounts-provider",
            AccountPolicy {
                mode: SelectionMode::Fixed,
                fixed_account_id: Some(first.id.clone()),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    let context = fixture.admit(None);
    let original = fixture.pool.select(&context).unwrap();
    let replacement = fixture.api("fixed-replacement");
    let before = fixture.budgets();
    let revoked = fixture.pool.revoke(&first.id, true).unwrap();
    assert!(revoked.local_reference_revoked);
    assert!(revoked.credentials_removed);
    assert_eq!(
        fixture.pool.select(&context).unwrap_err(),
        AccountError::NoEligibleAccount
    );
    assert_eq!(
        fixture
            .pool
            .policy("accounts-provider")
            .unwrap()
            .fixed_account_id,
        Some(first.id.clone())
    );
    fixture
        .pool
        .save_policy(
            "accounts-provider",
            AccountPolicy {
                mode: SelectionMode::Fixed,
                fixed_account_id: Some(replacement.id.clone()),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    let selected = fixture.pool.select(&context).unwrap();
    assert_eq!(original.account_id(), first.id);
    assert_eq!(selected.account_id(), replacement.id);
    assert_eq!(selected.reason, SelectionReason::Fixed);
    assert_eq!(
        fixture.pool.select(&context).unwrap().account_id(),
        replacement.id
    );
    context.cancellation.cancel();
    assert_eq!(
        fixture.pool.select(&context).unwrap_err(),
        AccountError::Cancelled
    );
    assert_eq!(
        fixture.pool.get(&first.id).unwrap().status,
        AccountStatus::Revoked
    );
    assert_eq!(fixture.budgets(), before);
}

#[tokio::test]
async fn preparing_authorization_does_not_claim_or_change_the_existing_budget() {
    for kind in [
        AccountKind::ApiKey,
        AccountKind::OAuth,
        AccountKind::NativeManaged,
    ] {
        let fixture = Fixture::new();
        let (account, reference) = match kind {
            AccountKind::ApiKey => (fixture.api("prepare-api"), None),
            AccountKind::OAuth => (fixture.oauth("prepare-oauth", false), None),
            AccountKind::NativeManaged => {
                let (account, reference) = fixture.native(AccountClient::CodexDesktop);
                (account, Some(reference))
            }
        };
        let context = fixture.admit(None);
        let selection = fixture.pool.select(&context).unwrap();
        assert_eq!(selection.account_id(), account.id);
        let before = fixture.budgets();
        let reads = fixture.store.reads.load(Ordering::SeqCst);
        let prepared = fixture
            .pool
            .prepare_authorization(&selection, &context)
            .unwrap();
        let repeated = fixture
            .pool
            .prepare_authorization(&selection, &context)
            .unwrap();
        assert!(prepared.matches(&repeated));
        match &prepared {
            AccountAuthorization::ApiKey(value) => {
                assert!(value.expose().starts_with("synthetic-key-"))
            }
            AccountAuthorization::OAuth(value) => {
                assert!(value.expose() == "synthetic-access-initial")
            }
            AccountAuthorization::NativeManaged {
                client,
                reference_id,
            } => {
                assert_eq!(*client, AccountClient::CodexDesktop);
                assert_eq!(Some(reference_id), reference.as_ref());
                assert_eq!(fixture.store.reads.load(Ordering::SeqCst), reads);
            }
        }
        assert!(!fixture.dispatched(&context));
        fixture
            .pool
            .dispatch(&selection, &context, |authorization| {
                assert!(prepared.matches(&authorization));
                async {}
            })
            .await
            .unwrap();
        assert!(fixture.dispatched(&context));
        assert_eq!(fixture.budgets(), before);
    }
}

#[test]
fn preparing_authorization_revalidates_before_reading_any_credential() {
    for mutation in [
        "cancelled",
        "database-cancelled",
        "replaced",
        "revoked",
        "disabled",
        "401",
        "429",
        "quota",
        "missing-destination",
        "wrong-destination",
    ] {
        let fixture = Fixture::new();
        let account = fixture.oauth("prepare-revalidation", false);
        let mut context = fixture.admit(None);
        let selection = fixture.pool.select(&context).unwrap();
        let expected = match mutation {
            "cancelled" => {
                context.cancellation.cancel();
                AccountError::Cancelled
            }
            "database-cancelled" => {
                fixture
                    .db
                    .conn()
                    .lock()
                    .unwrap()
                    .execute(
                        "UPDATE proxy_tasks SET cancelled=1 WHERE id=?1",
                        [&context.task_id],
                    )
                    .unwrap();
                AccountError::Cancelled
            }
            "replaced" => {
                fixture.replacement(&account, "synthetic-new-access", "synthetic-new-refresh");
                AccountError::StaleSelection
            }
            "revoked" => {
                fixture.pool.revoke(&account.id, true).unwrap();
                AccountError::Revoked
            }
            "disabled" => {
                fixture.pool.update(updated(&account, false)).unwrap();
                AccountError::NoEligibleAccount
            }
            "401" | "429" => {
                let parallel = fixture.pool.select(&fixture.admit(None)).unwrap();
                fixture
                    .pool
                    .record_outcome(
                        &parallel,
                        AccountOutcome::from_http(if mutation == "401" { 401 } else { 429 }, None),
                    )
                    .unwrap();
                AccountError::NoEligibleAccount
            }
            "quota" => {
                fixture
                    .pool
                    .update_quota(&account.id, account.credential_generation, quota(0.0))
                    .unwrap();
                AccountError::NoEligibleAccount
            }
            "missing-destination" => {
                context.destination = None;
                AccountError::DestinationApprovalRequired
            }
            "wrong-destination" => {
                context.destination = Some("https://unapproved.example.test/v1".into());
                AccountError::DestinationApprovalRequired
            }
            _ => unreachable!(),
        };
        let before = fixture.budgets();
        let reads = fixture.store.reads.load(Ordering::SeqCst);
        assert_eq!(
            fixture
                .pool
                .prepare_authorization(&selection, &context)
                .unwrap_err(),
            expected
        );
        assert_eq!(fixture.store.reads.load(Ordering::SeqCst), reads);
        assert!(!fixture.dispatched(&context));
        assert_eq!(fixture.budgets(), before);
    }
}

#[tokio::test]
async fn changed_destinations_require_reconsent_and_cannot_authorize_old_request_snapshots() {
    let fixture = Fixture::new();
    let account = fixture.oauth("destination-consent", false);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &account.id);
    let old_context = fixture.admit(None);
    let old_selection = fixture.pool.select(&old_context).unwrap();
    const CHANGED_DESTINATION: &str = "https://changed.example.test/v1";
    fixture.destination(CHANGED_DESTINATION);
    let blocked = fixture.pool.get(&account.id).unwrap();
    assert_eq!(blocked.status, AccountStatus::DestinationApprovalRequired);
    assert!(!blocked.destination_approved);
    assert!(!blocked.can_refresh);
    assert!(
        fixture
            .pool
            .refresh_candidates(&old_context)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture
            .pool
            .prepare_authorization(&old_selection, &old_context)
            .unwrap_err(),
        AccountError::DestinationApprovalRequired
    );
    assert_eq!(
        fixture
            .pool
            .refresh(&account.id, &AccountCancellation::default())
            .await
            .unwrap_err(),
        AccountError::DestinationApprovalRequired
    );
    assert_eq!(callback.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture
            .pool
            .reconsent_destination(&account.id, account.revision, CHANGED_DESTINATION, false,)
            .unwrap_err(),
        AccountError::AuthorizationRequired
    );
    assert_eq!(
        fixture
            .pool
            .reconsent_destination(&account.id, account.revision, TEST_DESTINATION, true,)
            .unwrap_err(),
        AccountError::DestinationApprovalRequired
    );
    let approved = fixture
        .pool
        .reconsent_destination(&account.id, account.revision, CHANGED_DESTINATION, true)
        .unwrap();
    assert_eq!(
        approved.credential_generation,
        account.credential_generation + 1
    );
    assert_eq!(
        approved.authorization_status,
        AuthorizationStatus::Unverified
    );
    assert!(approved.destination_approved);
    assert!(!approved.can_refresh);
    assert!(
        !fixture
            .pool
            .runtime
            .refreshers
            .lock()
            .unwrap()
            .contains_key(&account.id)
    );
    assert_eq!(
        fixture
            .pool
            .prepare_authorization(&old_selection, &old_context)
            .unwrap_err(),
        AccountError::StaleSelection
    );
    let mut current_context = fixture.admit(None);
    current_context.destination = Some(CHANGED_DESTINATION.into());
    current_context.http_only = true;
    let current_selection = fixture.pool.select(&current_context).unwrap();
    let mut wrong_snapshot = current_context.clone();
    wrong_snapshot.destination = Some(TEST_DESTINATION.into());
    let reads = fixture.store.reads.load(Ordering::SeqCst);
    assert_eq!(
        fixture
            .pool
            .prepare_authorization(&current_selection, &wrong_snapshot)
            .unwrap_err(),
        AccountError::DestinationApprovalRequired
    );
    let transports = AtomicUsize::new(0);
    let before = fixture.budgets();
    assert_eq!(
        fixture
            .pool
            .dispatch(&current_selection, &wrong_snapshot, |_| {
                transports.fetch_add(1, Ordering::SeqCst);
                async {}
            })
            .await
            .unwrap_err(),
        AccountError::DestinationApprovalRequired
    );
    assert_eq!(transports.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.store.reads.load(Ordering::SeqCst), reads);
    assert!(!fixture.dispatched(&current_context));
    assert_eq!(fixture.budgets(), before);
    assert!(matches!(
        fixture
            .pool
            .prepare_authorization(&current_selection, &current_context)
            .unwrap(),
        AccountAuthorization::OAuth(_)
    ));
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let record = load_record(&connection, &account.id).unwrap();
    assert_eq!(
        record.approved_destination_fingerprint,
        Some(destination_fingerprint(&account.provider_id, CHANGED_DESTINATION).unwrap())
    );
    let returned = serde_json::to_string(&approved).unwrap();
    assert!(!returned.contains(CHANGED_DESTINATION));
    assert!(!returned.contains("synthetic-access-initial"));
}

#[tokio::test]
async fn cancellation_before_first_poll_and_database_cancel_do_not_send() {
    let fixture = Fixture::new();
    fixture.api("cancel");
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let sent = AtomicUsize::new(0);
    let pending = fixture.pool.dispatch(&selection, &context, |_| async {
        sent.fetch_add(1, Ordering::SeqCst);
    });
    context.cancellation.cancel();
    assert_eq!(pending.await.unwrap_err(), AccountError::Cancelled);
    assert_eq!(sent.load(Ordering::SeqCst), 0);
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let connection = fixture.db.conn();
    connection
        .lock()
        .unwrap()
        .execute(
            "UPDATE proxy_tasks SET cancelled=1 WHERE id=?1",
            [&context.task_id],
        )
        .unwrap();
    assert_eq!(
        fixture
            .pool
            .dispatch(&selection, &context, |_| async {
                sent.fetch_add(1, Ordering::SeqCst);
            })
            .await
            .unwrap_err(),
        AccountError::Cancelled
    );
    assert_eq!(sent.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn cancelling_an_in_flight_send_drops_transport_without_new_attempts() {
    let fixture = Fixture::new();
    fixture.api("transport");
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let dropped = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(Notify::new());
    let before = fixture.budgets();
    let pool = fixture.pool.clone();
    let send_context = context.clone();
    let send_selection = selection.clone();
    let count = dropped.clone();
    let notify = started.clone();
    let task = tokio::spawn(async move {
        pool.dispatch(&send_selection, &send_context, |_| async move {
            let _drop = DropCounter(count);
            notify.notify_one();
            std::future::pending::<()>().await;
        })
        .await
    });
    started.notified().await;
    context.cancellation.cancel();
    assert_eq!(task.await.unwrap().unwrap_err(), AccountError::Cancelled);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    fixture
        .pool
        .record_outcome(&selection, outcome(OutcomeKind::Cancelled))
        .unwrap();
    assert_eq!(fixture.budgets(), before);
}

#[tokio::test]
async fn a_shared_weak_cancellation_blocks_transport_creation_before_first_poll() {
    let fixture = Fixture::new();
    fixture.api("shared-cancel");
    let mut context = fixture.admit(None);
    let signal = crate::cancellation::Cancellation::default();
    let weak = signal.downgrade();
    context.cancellation = signal.clone();
    let selection = fixture.pool.select(&context).unwrap();
    let before = fixture.budgets();
    let from_runtime = crate::cancellation::Cancellation(weak.upgrade().unwrap());
    assert!(Arc::ptr_eq(&from_runtime.0, &context.cancellation.0));
    let created = AtomicUsize::new(0);
    let polled = AtomicUsize::new(0);
    let dispatch = fixture.pool.dispatch(&selection, &context, |_| {
        created.fetch_add(1, Ordering::SeqCst);
        async {
            polled.fetch_add(1, Ordering::SeqCst);
        }
    });
    from_runtime.cancel();
    tokio::time::timeout(Duration::from_secs(1), signal.cancelled())
        .await
        .unwrap();
    assert_eq!(dispatch.await.unwrap_err(), AccountError::Cancelled);
    assert_eq!(
        fixture.pool.select(&context).unwrap_err(),
        AccountError::Cancelled
    );
    assert_eq!(created.load(Ordering::SeqCst), 0);
    assert_eq!(polled.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.budgets(), before);
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let dispatched: bool = connection
        .query_row(
            "SELECT dispatched FROM account_pool_attempts WHERE attempt_id=?1",
            [&context.attempt_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!dispatched);
    drop(connection);
    assert!(format!("{signal:?}").contains("cancelled: true"));
    drop(from_runtime);
    drop(context);
    drop(signal);
    assert!(weak.upgrade().is_none());
}

#[tokio::test]
async fn dispatch_creation_claim_and_first_poll_share_the_cancellation_gate() {
    let fixture = Fixture::new();
    fixture.api("atomic-start");
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let before = fixture.budgets();
    let created = Arc::new(AtomicUsize::new(0));
    let polled = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let factory_count = created.clone();
    let poll_count = polled.clone();
    let drop_count = dropped.clone();
    let signal = context.cancellation.clone();
    let runtime = fixture.pool.runtime.clone();
    let database = fixture.db.conn();
    let mut dispatch = Box::pin(fixture.pool.dispatch(&selection, &context, move |_| {
        factory_count.fetch_add(1, Ordering::SeqCst);
        assert!(signal.0.gate.try_lock().is_err());
        assert!(runtime.gate.try_lock().is_err());
        assert!(database.try_lock().is_err());
        let guard = DropCounter(drop_count);
        std::future::poll_fn(move |_task| {
            let _keep_alive = &guard;
            if poll_count.fetch_add(1, Ordering::SeqCst) == 0 {
                assert!(signal.0.gate.try_lock().is_err());
                assert!(runtime.gate.try_lock().is_err());
                assert!(database.try_lock().is_err());
            }
            Poll::<()>::Pending
        })
    }));
    std::future::poll_fn(|task| {
        assert!(dispatch.as_mut().poll(task).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(created.load(Ordering::SeqCst), 1);
    assert!(polled.load(Ordering::SeqCst) >= 1);
    assert!(context.cancellation.0.gate.try_lock().is_ok());
    assert!(fixture.pool.runtime.gate.try_lock().is_ok());
    let connection = fixture.db.conn();
    let dispatched: bool = connection
        .lock()
        .unwrap()
        .query_row(
            "SELECT dispatched FROM account_pool_attempts WHERE attempt_id=?1",
            [&context.attempt_id],
            |row| row.get(0),
        )
        .unwrap();
    assert!(dispatched);
    let previous_polls = polled.load(Ordering::SeqCst);
    context.cancellation.cancel();
    assert_eq!(dispatch.await.unwrap_err(), AccountError::Cancelled);
    assert_eq!(polled.load(Ordering::SeqCst), previous_polls);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    fixture
        .pool
        .record_outcome(&selection, outcome(OutcomeKind::Cancelled))
        .unwrap();
    assert_eq!(fixture.budgets(), before);
}

#[tokio::test]
async fn task_or_connection_cancellation_prevents_dispatch_claim_and_transport_creation() {
    for cancel_connection in [true, false] {
        let fixture = Fixture::new();
        fixture.api("connection-cancel");
        let context = fixture.admit(None);
        let selection = fixture.pool.select(&context).unwrap();
        let connection_cancellation = AccountCancellation::default();
        assert!(!Arc::ptr_eq(
            &connection_cancellation.0,
            &context.cancellation.0
        ));
        let before = fixture.budgets();
        let created = AtomicUsize::new(0);
        let polled = AtomicUsize::new(0);
        let dispatch = fixture.pool.dispatch_with_connection_on(
            &selection,
            &context,
            &connection_cancellation,
            |_connection, _authorization| {
                created.fetch_add(1, Ordering::SeqCst);
                async {
                    polled.fetch_add(1, Ordering::SeqCst);
                }
            },
        );
        if cancel_connection {
            connection_cancellation.cancel();
        } else {
            context.cancellation.cancel();
        }
        assert_eq!(dispatch.await.unwrap_err(), AccountError::Cancelled);
        assert_eq!(created.load(Ordering::SeqCst), 0);
        assert_eq!(polled.load(Ordering::SeqCst), 0);
        assert_eq!(fixture.budgets(), before);
        assert_eq!(context.cancellation.is_cancelled(), !cancel_connection);
        assert_eq!(connection_cancellation.is_cancelled(), cancel_connection);
        let connection = fixture.db.conn();
        let connection = connection.lock().unwrap();
        let dispatched: bool = connection
            .query_row(
                "SELECT dispatched FROM account_pool_attempts WHERE attempt_id=?1",
                [&context.attempt_id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!dispatched);
    }
}

#[tokio::test]
async fn connection_dispatch_first_poll_holds_both_gates_and_disconnect_drops_transport() {
    let fixture = Fixture::new();
    let account = fixture.api("connection-start");
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let prepared = fixture
        .pool
        .prepare_authorization(&selection, &context)
        .unwrap();
    assert!(!fixture.dispatched(&context));
    let connection_cancellation = AccountCancellation::default();
    let before = fixture.budgets();
    let created = Arc::new(AtomicUsize::new(0));
    let polled = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let factory_count = created.clone();
    let poll_count = polled.clone();
    let drop_count = dropped.clone();
    let task_signal = context.cancellation.clone();
    let connection_signal = connection_cancellation.clone();
    let runtime = fixture.pool.runtime.clone();
    let database = fixture.db.conn();
    let attempt_id = context.attempt_id.clone();
    let mut dispatch = Box::pin(fixture.pool.dispatch_with_connection_on(
        &selection,
        &context,
        &connection_cancellation,
        move |connection, authorization| {
            factory_count.fetch_add(1, Ordering::SeqCst);
            assert!(prepared.matches(&authorization));
            assert!(selection_on(connection, &attempt_id).unwrap().unwrap().1);
            let status: String = connection
                .query_row(
                    "SELECT status FROM proxy_attempts WHERE id=?1",
                    [&attempt_id],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(status, "sending");
            assert!(connection_signal.0.gate.try_lock().is_err());
            assert!(task_signal.0.gate.try_lock().is_err());
            assert!(runtime.gate.try_lock().is_err());
            assert!(database.try_lock().is_err());
            let guard = DropCounter(drop_count);
            std::future::poll_fn(move |_task| {
                let _keep_alive = &guard;
                if poll_count.fetch_add(1, Ordering::SeqCst) == 0 {
                    assert!(connection_signal.0.gate.try_lock().is_err());
                    assert!(task_signal.0.gate.try_lock().is_err());
                    assert!(runtime.gate.try_lock().is_err());
                    assert!(database.try_lock().is_err());
                }
                Poll::<()>::Pending
            })
        },
    ));
    std::future::poll_fn(|task| {
        assert!(dispatch.as_mut().poll(task).is_pending());
        Poll::Ready(())
    })
    .await;
    assert_eq!(created.load(Ordering::SeqCst), 1);
    assert!(polled.load(Ordering::SeqCst) >= 1);
    assert!(context.cancellation.0.gate.try_lock().is_ok());
    assert!(connection_cancellation.0.gate.try_lock().is_ok());
    assert!(fixture.pool.runtime.gate.try_lock().is_ok());
    let previous_polls = polled.load(Ordering::SeqCst);
    connection_cancellation.cancel();
    assert_eq!(dispatch.await.unwrap_err(), AccountError::Cancelled);
    assert_eq!(polled.load(Ordering::SeqCst), previous_polls);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(!context.cancellation.is_cancelled());
    let next_connection = AccountCancellation::default();
    assert_eq!(
        fixture
            .pool
            .dispatch_with_connection(&selection, &context, &next_connection, |_| async {})
            .await
            .unwrap_err(),
        AccountError::AlreadyDispatched
    );
    let after = fixture
        .pool
        .record_outcome(&selection, outcome(OutcomeKind::Cancelled))
        .unwrap();
    assert_eq!(after.authorization_status, account.authorization_status);
    assert_eq!(after.credential_generation, account.credential_generation);
    assert_eq!(fixture.budgets(), before);
}

#[tokio::test]
async fn synchronous_connection_validation_can_reject_before_transport_creation() {
    let fixture = Fixture::new();
    fixture.api("prepared-validation");
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let prepared = fixture
        .pool
        .prepare_authorization(&selection, &context)
        .unwrap();
    let connection_cancellation = AccountCancellation::default();
    let before = fixture.budgets();
    let validated = AtomicUsize::new(0);
    let transports = AtomicUsize::new(0);
    let result = fixture
        .pool
        .dispatch_with_connection_on(
            &selection,
            &context,
            &connection_cancellation,
            |connection, authorization| {
                validated.fetch_add(1, Ordering::SeqCst);
                assert!(prepared.matches(&authorization));
                assert!(context.cancellation.0.gate.try_lock().is_err());
                assert!(connection_cancellation.0.gate.try_lock().is_err());
                let reserved: i64 = connection.query_row(
                "SELECT reserved_tokens FROM proxy_attempts WHERE id=?1 AND status='sending'",
                [&context.attempt_id],
                |row| row.get(0),
            ).unwrap();
                let validation = if 33 > reserved {
                    Err("prepared_payload_exceeds_reservation")
                } else {
                    transports.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                };
                std::future::ready(validation)
            },
        )
        .await
        .unwrap();
    assert_eq!(result, Err("prepared_payload_exceeds_reservation"));
    assert_eq!(validated.load(Ordering::SeqCst), 1);
    assert_eq!(transports.load(Ordering::SeqCst), 0);
    assert!(fixture.dispatched(&context));
    assert_eq!(fixture.budgets(), before);
    assert_eq!(
        fixture
            .pool
            .dispatch(&selection, &context, |_| async {})
            .await
            .unwrap_err(),
        AccountError::AlreadyDispatched
    );
}

#[tokio::test]
async fn connection_dispatch_accepts_an_aliased_task_signal_without_locking_it_twice() {
    let fixture = Fixture::new();
    fixture.api("shared-connection");
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let connection_cancellation = context.cancellation.clone();
    assert!(Arc::ptr_eq(
        &connection_cancellation.0,
        &context.cancellation.0
    ));
    let before = fixture.budgets();
    let started = AtomicUsize::new(0);
    assert_eq!(
        fixture
            .pool
            .dispatch_with_connection(
                &selection,
                &context,
                &connection_cancellation,
                |authorization| {
                    assert!(matches!(authorization, AccountAuthorization::ApiKey(_)));
                    assert!(connection_cancellation.0.gate.try_lock().is_err());
                    async {
                        assert!(context.cancellation.0.gate.try_lock().is_err());
                        started.fetch_add(1, Ordering::SeqCst);
                        17
                    }
                },
            )
            .await
            .unwrap(),
        17
    );
    assert_eq!(started.load(Ordering::SeqCst), 1);
    assert!(context.cancellation.0.gate.try_lock().is_ok());
    assert!(connection_cancellation.0.gate.try_lock().is_ok());
    assert_eq!(fixture.budgets(), before);
}

#[test]
fn affinity_rotation_and_fixed_accounts_obey_failures_and_quota() {
    let fixture = Fixture::new();
    fixture.api("alpha");
    fixture.api("beta");
    let context = fixture.admit(Some("synthetic-session@example.test"));
    let first = fixture.pool.select(&context).unwrap();
    fixture
        .pool
        .record_outcome(&first, outcome(OutcomeKind::Success))
        .unwrap();
    let again = fixture
        .pool
        .select(&fixture.admit(Some("synthetic-session@example.test")))
        .unwrap();
    assert_eq!(again.account_id(), first.account_id());
    assert_eq!(again.reason, SelectionReason::Affinity);
    let limited = fixture
        .pool
        .record_outcome(&again, AccountOutcome::from_http(429, Some(3600)))
        .unwrap();
    assert_eq!(limited.status, AccountStatus::CoolingDown);
    let rotated = fixture
        .pool
        .select(&fixture.admit(Some("synthetic-session@example.test")))
        .unwrap();
    assert_ne!(rotated.account_id(), first.account_id());
    let exhausted = fixture
        .pool
        .record_outcome(
            &rotated,
            AccountOutcome {
                kind: OutcomeKind::QuotaExhausted,
                quota: Some(quota(0.0)),
                retry_after_seconds: None,
            },
        )
        .unwrap();
    assert_eq!(exhausted.status, AccountStatus::QuotaLimited);
    assert_eq!(
        fixture.pool.select(&fixture.admit(None)).unwrap_err(),
        AccountError::NoEligibleAccount
    );
    fixture
        .pool
        .save_policy(
            "accounts-provider",
            AccountPolicy {
                mode: SelectionMode::Fixed,
                fixed_account_id: Some(first.account_id().into()),
                ..Default::default()
            },
            true,
        )
        .unwrap();
    assert_eq!(
        fixture.pool.select(&fixture.admit(None)).unwrap_err(),
        AccountError::NoEligibleAccount
    );
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let mut query = connection
        .prepare("SELECT affinity_hash FROM account_pool_affinity")
        .unwrap();
    for hash in query.query_map([], |row| row.get::<_, String>(0)).unwrap() {
        assert!(!hash.unwrap().contains("synthetic-session"));
    }
}

#[test]
fn old_parallel_success_cannot_clear_401_or_429_for_the_same_generation() {
    let fixture = Fixture::new();
    let account = fixture.api("health");
    let first = fixture.pool.select(&fixture.admit(None)).unwrap();
    let second = fixture.pool.select(&fixture.admit(None)).unwrap();
    assert_eq!(
        fixture
            .pool
            .record_outcome(&first, AccountOutcome::from_http(401, None))
            .unwrap()
            .status,
        AccountStatus::LoginRequired
    );
    assert_eq!(
        fixture
            .pool
            .record_outcome(&second, outcome(OutcomeKind::Success))
            .unwrap()
            .status,
        AccountStatus::LoginRequired
    );
    let current = fixture.pool.get(&account.id).unwrap();
    fixture
        .pool
        .replace_credentials(ReplaceCredentialRequest {
            id: account.id.clone(),
            expected_revision: current.revision,
            approved_destination: Some(TEST_DESTINATION.into()),
            credentials: AccountCredentialInput::ApiKey {
                api_key: secret("synthetic-reset-key"),
            },
            allow_refresh: false,
            confirmed: true,
        })
        .unwrap();
    let first = fixture.pool.select(&fixture.admit(None)).unwrap();
    let second = fixture.pool.select(&fixture.admit(None)).unwrap();
    fixture
        .pool
        .record_outcome(&first, AccountOutcome::from_http(429, Some(120)))
        .unwrap();
    assert_eq!(
        fixture
            .pool
            .record_outcome(&second, outcome(OutcomeKind::Success))
            .unwrap()
            .status,
        AccountStatus::CoolingDown
    );
}

#[test]
fn stale_outcomes_and_provider_rebinding_cannot_invalidate_new_credentials() {
    let fixture = Fixture::new();
    let account = fixture.oauth("outcomes", false);
    let selection = fixture.pool.select(&fixture.admit(None)).unwrap();
    let replacement = fixture.replacement(
        &account,
        "synthetic-replaced-access",
        "synthetic-replaced-refresh",
    );
    let result = fixture
        .pool
        .record_outcome(&selection, AccountOutcome::from_http(401, None))
        .unwrap();
    assert_eq!(
        result.credential_generation,
        replacement.credential_generation
    );
    assert_eq!(result.authorization_status, AuthorizationStatus::Unverified);
    let account = fixture.api("binding");
    let mut request = updated(&account, true);
    request.provider_id = "other-provider".into();
    request.approved_destination = Some(TEST_DESTINATION.into());
    let moved = fixture.pool.update(request).unwrap();
    let mut request = updated(&moved, true);
    request.provider_id = "accounts-provider".into();
    request.approved_destination = Some(TEST_DESTINATION.into());
    let returned = fixture.pool.update(request).unwrap();
    assert_eq!(
        returned.credential_generation,
        account.credential_generation + 2
    );
}

#[test]
fn shorter_parallel_failures_cannot_shorten_an_existing_cooldown() {
    let fixture = Fixture::new();
    fixture.api("long-cooldown");
    let selections: Vec<_> = (0..3)
        .map(|_| fixture.pool.select(&fixture.admit(None)).unwrap())
        .collect();
    let limited = fixture
        .pool
        .record_outcome(&selections[0], AccountOutcome::from_http(429, Some(600)))
        .unwrap();
    let shorter_limit = fixture
        .pool
        .record_outcome(&selections[1], AccountOutcome::from_http(429, Some(10)))
        .unwrap();
    let shorter_failure = fixture
        .pool
        .record_outcome(&selections[2], AccountOutcome::from_http(503, Some(1)))
        .unwrap();
    assert_eq!(shorter_limit.cooldown_until, limited.cooldown_until);
    assert_eq!(shorter_failure.cooldown_until, limited.cooldown_until);
    assert_eq!(shorter_failure.status, AccountStatus::CoolingDown);
    assert_eq!(
        fixture.pool.select(&fixture.admit(None)).unwrap_err(),
        AccountError::NoEligibleAccount
    );
}

#[tokio::test]
async fn refresh_completion_preserves_a_concurrent_request_rate_limit() {
    for refresh_result in [
        None,
        Some(RefreshError::RateLimited {
            retry_after_seconds: 1,
        }),
        Some(RefreshError::Unavailable),
    ] {
        let fixture = Fixture::new();
        let account = fixture.oauth("refresh-cooldown", false);
        let selection = fixture.pool.select(&fixture.admit(None)).unwrap();
        let before = fixture.budgets();
        let callback = ControlledRefresher::new();
        callback.register(&fixture.pool, &account.id);
        if let Some(error) = refresh_result {
            callback.results.lock().unwrap().push_back(Err(error));
        }
        let task = start_refresh(&fixture.pool, &account.id, AccountCancellation::default());
        tokio::time::timeout(Duration::from_secs(2), callback.started.notified())
            .await
            .unwrap();
        let limited = fixture
            .pool
            .record_outcome(&selection, AccountOutcome::from_http(429, Some(600)))
            .unwrap();
        callback.permits.add_permits(1);
        match refresh_result {
            None => {
                assert_eq!(
                    task.await.unwrap().unwrap().credential_generation,
                    account.credential_generation + 1
                );
            }
            Some(RefreshError::RateLimited { .. }) => assert_eq!(
                task.await.unwrap().unwrap_err(),
                AccountError::RefreshRateLimited
            ),
            _ => assert_eq!(
                task.await.unwrap().unwrap_err(),
                AccountError::RefreshFailed
            ),
        }
        let current = fixture.pool.get(&account.id).unwrap();
        assert_eq!(current.cooldown_until, limited.cooldown_until);
        assert_eq!(current.status, AccountStatus::CoolingDown);
        assert_eq!(fixture.budgets(), before);
    }
}

#[tokio::test]
async fn rotated_secrets_cannot_survive_in_retained_quota_metadata() {
    for access_collision in [true, false] {
        let fixture = Fixture::new();
        let account = fixture.oauth("quota-collision", false);
        let collision = "synthetic-new-quota-secret";
        let mut evidence = quota(10.0);
        evidence.windows[0].id = collision.into();
        fixture
            .pool
            .update_quota(&account.id, account.credential_generation, evidence)
            .unwrap();
        let callback = ControlledRefresher::new();
        callback.register(&fixture.pool, &account.id);
        if access_collision {
            callback.result(collision, Some("synthetic-safe-refresh"));
        } else {
            callback.result("synthetic-safe-access", Some(collision));
        }
        callback.permits.add_permits(1);
        let refreshed = fixture
            .pool
            .refresh(&account.id, &AccountCancellation::default())
            .await
            .unwrap();
        assert!(refreshed.quota.is_none());
        assert!(
            !serde_json::to_string(&refreshed)
                .unwrap()
                .contains(collision)
        );
        assert!(!format!("{refreshed:?}").contains(collision));
        let connection = fixture.db.conn();
        let connection = connection.lock().unwrap();
        let metadata: String = connection
            .query_row(
                "SELECT metadata FROM account_pool_accounts WHERE id=?1",
                [&account.id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!metadata.contains(collision));
    }
}

#[test]
fn only_current_trusted_quota_evidence_is_displayed_and_secrets_cannot_be_labels() {
    let fixture = Fixture::new();
    let account = fixture.api("quota");
    assert!(fixture.pool.get(&account.id).unwrap().quota.is_none());
    let mut unknown = quota(0.0);
    unknown.source = QuotaSource::Unknown;
    assert_eq!(
        fixture
            .pool
            .update_quota(&account.id, account.credential_generation, unknown)
            .unwrap_err(),
        AccountError::InvalidQuotaEvidence
    );
    let mut invalid = quota(10.0);
    invalid.windows[0].id = "synthetic-key-quota".into();
    assert_eq!(
        fixture
            .pool
            .update_quota(&account.id, account.credential_generation, invalid)
            .unwrap_err(),
        AccountError::InvalidQuotaEvidence
    );
    fixture
        .pool
        .update_quota(&account.id, account.credential_generation, quota(0.0))
        .unwrap();
    let connection = fixture.db.conn();
    let connection = connection.lock().unwrap();
    let mut record = load_record(&connection, &account.id).unwrap();
    record.quota.as_mut().unwrap().valid_until = now() - 1;
    save_record(&connection, &record).unwrap();
    drop(connection);
    let current = fixture.pool.get(&account.id).unwrap();
    assert!(current.quota.is_none());
    assert_eq!(current.status, AccountStatus::Available);
    assert!(
        !fixture
            .pool
            .eligible(&fixture.admit(None))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn native_accounts_require_a_real_opaque_client_reference_and_never_read_tokens() {
    let fixture = Fixture::new();
    let account = fixture
        .pool
        .add(AddAccountRequest {
            alias: "native".into(),
            provider_id: "accounts-provider".into(),
            approved_destination: Some(TEST_DESTINATION.into()),
            credentials: AccountCredentialInput::NativeManaged {
                client: AccountClient::CodexDesktop,
            },
            available_models: vec![],
            allow_refresh: false,
            confirmed: true,
        })
        .unwrap();
    assert_eq!(account.status, AccountStatus::LoginRequired);
    assert!(fixture.store.values.lock().unwrap().is_empty());
    let reference = Uuid::new_v4().to_string();
    let authorized = fixture
        .pool
        .update_native_state(
            &account.id,
            account.credential_generation,
            NativeAccountState {
                client: AccountClient::CodexDesktop,
                reference_id: reference.clone(),
                authorized: true,
                expires_at: None,
                available_models: vec!["synthetic-model".into()],
            },
        )
        .unwrap();
    let mut native_quota = quota(0.0);
    native_quota.source = QuotaSource::NativeClient;
    fixture
        .pool
        .update_quota(&account.id, authorized.credential_generation, native_quota)
        .unwrap();
    let again = fixture
        .pool
        .update_native_state(
            &account.id,
            authorized.credential_generation,
            NativeAccountState {
                client: AccountClient::CodexDesktop,
                reference_id: reference,
                authorized: true,
                expires_at: None,
                available_models: vec!["synthetic-model".into()],
            },
        )
        .unwrap();
    assert_eq!(again.status, AccountStatus::QuotaLimited);
    assert!(fixture.store.values.lock().unwrap().is_empty());
    fixture.pool.revoke(&account.id, true).unwrap();
    assert_eq!(
        fixture
            .pool
            .update_native_state(
                &account.id,
                again.credential_generation,
                NativeAccountState {
                    client: AccountClient::CodexDesktop,
                    reference_id: Uuid::new_v4().to_string(),
                    authorized: true,
                    expires_at: None,
                    available_models: vec![],
                }
            )
            .unwrap_err(),
        AccountError::Revoked
    );
}

#[test]
fn synthetic_test_pools_share_runtime_and_credentials_only_with_the_same_database() {
    let fixture = Fixture::new();
    let first = AccountPool::test_pool(fixture.db.clone()).unwrap();
    let second = AccountPool::test_pool(fixture.db.clone()).unwrap();
    assert!(Arc::ptr_eq(&first.runtime, &second.runtime));
    assert!(Arc::ptr_eq(&first.credentials, &second.credentials));
    let account = first
        .add(AddAccountRequest {
            alias: "isolated-test-pool".into(),
            provider_id: "accounts-provider".into(),
            approved_destination: Some(TEST_DESTINATION.into()),
            credentials: AccountCredentialInput::ApiKey {
                api_key: secret("synthetic-test-pool-key"),
            },
            available_models: vec![],
            allow_refresh: false,
            confirmed: true,
        })
        .unwrap();
    let context = fixture.admit(None);
    let selection = first.select(&context).unwrap();
    assert_eq!(second.select(&context).unwrap().account_id(), account.id);
    assert!(
        matches!(second.prepare_authorization(&selection, &context).unwrap(),
        AccountAuthorization::ApiKey(value) if value.expose() == "synthetic-test-pool-key")
    );
    drop(first);
    drop(second);
    let recreated = AccountPool::test_pool(fixture.db.clone()).unwrap();
    assert!(
        matches!(recreated.prepare_authorization(&selection, &context).unwrap(),
        AccountAuthorization::ApiKey(value) if value.expose() == "synthetic-test-pool-key")
    );
    let isolated = Fixture::new();
    let foreign = AccountPool::test_pool(isolated.db.clone()).unwrap();
    assert!(!Arc::ptr_eq(&recreated.runtime, &foreign.runtime));
    assert!(!Arc::ptr_eq(&recreated.credentials, &foreign.credentials));
    assert!(foreign.list().unwrap().is_empty());
    assert_eq!(
        foreign.get(&account.id).unwrap_err(),
        AccountError::NotFound
    );
    assert!(
        foreign
            .credentials
            .get(&secret_id(
                &account.id,
                account.credential_generation,
                "access"
            ))
            .unwrap()
            .is_none()
    );
    recreated.revoke(&account.id, true).unwrap();
    let peer = AccountPool::test_pool(fixture.db.clone()).unwrap();
    assert_eq!(
        peer.get(&account.id).unwrap().status,
        AccountStatus::Revoked
    );
    assert!(
        peer.credentials
            .get(&secret_id(
                &account.id,
                account.credential_generation,
                "access"
            ))
            .unwrap()
            .is_none()
    );
    assert!(!fixture.dispatched(&context));
}

#[tokio::test]
async fn http_selection_filters_native_references_and_keeps_explicit_fixed_bindings() {
    let fixture = Fixture::new();
    let (native, reference) = fixture.native(AccountClient::CodexDesktop);
    let api = fixture.api("http-api");
    let oauth = fixture.oauth("http-oauth", false);
    let expired = fixture.oauth("http-expired-oauth", true);
    let callback = ControlledRefresher::new();
    callback.register(&fixture.pool, &expired.id);
    let mut http_context = fixture.admit(None);
    http_context.http_only = true;
    let eligible = fixture.pool.eligible(&http_context).unwrap();
    assert_eq!(eligible.len(), 2);
    assert!(eligible.iter().any(|account| account.id == api.id));
    assert!(eligible.iter().any(|account| account.id == oauth.id));
    assert!(
        eligible
            .iter()
            .all(|account| account.kind != AccountKind::NativeManaged)
    );
    let refresh = fixture.pool.refresh_candidates(&http_context).unwrap();
    assert_eq!(refresh.len(), 1);
    assert_eq!(refresh[0].id, expired.id);
    let selection = fixture.pool.select(&http_context).unwrap();
    assert_ne!(selection.account_id(), native.id);
    assert!(matches!(
        fixture
            .pool
            .prepare_authorization(&selection, &http_context)
            .unwrap(),
        AccountAuthorization::ApiKey(_) | AccountAuthorization::OAuth(_)
    ));
    fixture
        .pool
        .save_policy(
            "accounts-provider",
            AccountPolicy {
                mode: SelectionMode::Fixed,
                fixed_account_id: Some(native.id.clone()),
                ..AccountPolicy::default()
            },
            true,
        )
        .unwrap();
    let mut fixed_http_context = fixture.admit(None);
    fixed_http_context.http_only = true;
    assert!(
        fixture
            .pool
            .eligible(&fixed_http_context)
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        fixture.pool.select(&fixed_http_context).unwrap_err(),
        AccountError::NoEligibleAccount
    );
    let mut native_context = fixture.admit(None);
    native_context.destination = None;
    let native_selection = fixture.pool.select(&native_context).unwrap();
    let reads = fixture.store.reads.load(Ordering::SeqCst);
    assert!(
        matches!(fixture.pool.prepare_authorization(&native_selection, &native_context).unwrap(),
        AccountAuthorization::NativeManaged { client, reference_id }
            if client == AccountClient::CodexDesktop && reference_id == reference)
    );
    let mut wrong_transport = native_context.clone();
    wrong_transport.http_only = true;
    wrong_transport.destination = Some(TEST_DESTINATION.into());
    assert_eq!(
        fixture
            .pool
            .prepare_authorization(&native_selection, &wrong_transport)
            .unwrap_err(),
        AccountError::NoEligibleAccount
    );
    let transports = AtomicUsize::new(0);
    assert_eq!(
        fixture
            .pool
            .dispatch(&native_selection, &wrong_transport, |_| {
                transports.fetch_add(1, Ordering::SeqCst);
                async {}
            })
            .await
            .unwrap_err(),
        AccountError::NoEligibleAccount
    );
    assert_eq!(transports.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.store.reads.load(Ordering::SeqCst), reads);
    assert!(!fixture.dispatched(&native_context));
}

#[test]
fn configured_pools_stay_configured_after_disabling_or_revoking_the_last_account() {
    let fixture = Fixture::new();
    assert!(!fixture.pool.is_configured("accounts-provider").unwrap());
    assert!(!fixture.pool.is_configured("other-provider").unwrap());
    fixture
        .pool
        .save_policy("other-provider", AccountPolicy::default(), true)
        .unwrap();
    assert!(fixture.pool.is_configured("other-provider").unwrap());
    let account = fixture.api("configured");
    assert!(fixture.pool.is_configured("accounts-provider").unwrap());
    fixture.pool.update(updated(&account, false)).unwrap();
    assert!(fixture.pool.is_configured("accounts-provider").unwrap());
    assert_eq!(
        fixture.pool.select(&fixture.admit(None)).unwrap_err(),
        AccountError::NoEligibleAccount
    );
    fixture.pool.revoke(&account.id, true).unwrap();
    let restarted = AccountPool::with_store(fixture.db.clone(), fixture.store.clone()).unwrap();
    assert!(restarted.is_configured("accounts-provider").unwrap());
    assert_eq!(
        restarted.select(&fixture.admit(None)).unwrap_err(),
        AccountError::NoEligibleAccount
    );
    assert_eq!(
        restarted.is_configured("missing-provider").unwrap_err(),
        AccountError::ProviderUnavailable
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_status_polling_preserves_parallel_outcomes_quota_and_cooldown() {
    let fixture = Fixture::new();
    let (account, reference) = fixture.native(AccountClient::CodexDesktop);
    let selections: Vec<_> = (0..2)
        .map(|_| fixture.pool.select(&fixture.admit(None)).unwrap())
        .collect();
    let before = fixture.budgets();
    let limited = fixture
        .pool
        .record_outcome(&selections[0], AccountOutcome::from_http(429, Some(120)))
        .unwrap();
    let mut evidence = quota(5.0);
    evidence.source = QuotaSource::NativeClient;
    fixture
        .pool
        .update_quota(&account.id, account.credential_generation, evidence.clone())
        .unwrap();
    let barrier = Arc::new(Barrier::new(9));
    let mut polls = Vec::new();
    for _ in 0..8 {
        let pool = fixture.pool.clone();
        let account = account.clone();
        let reference = reference.clone();
        let barrier = barrier.clone();
        polls.push(tokio::spawn(async move {
            barrier.wait().await;
            pool.update_native_state(
                &account.id,
                account.credential_generation,
                native_state(AccountClient::CodexDesktop, &reference, true),
            )
        }));
    }
    barrier.wait().await;
    for poll in polls {
        let current = poll.await.unwrap().unwrap();
        assert_eq!(current.credential_generation, account.credential_generation);
        assert_eq!(current.cooldown_until, limited.cooldown_until);
        assert_eq!(current.status, AccountStatus::CoolingDown);
        assert_eq!(current.quota, Some(evidence.clone()));
    }
    let old_success = fixture
        .pool
        .record_outcome(&selections[1], outcome(OutcomeKind::Success))
        .unwrap();
    assert_eq!(old_success.cooldown_until, limited.cooldown_until);
    assert_eq!(
        old_success.credential_generation,
        account.credential_generation
    );
    assert_eq!(fixture.store.reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.budgets(), before);
}

#[test]
fn native_login_polling_cannot_clear_inference_401_without_a_new_authorization_reference() {
    let fixture = Fixture::new();
    let (account, reference) = fixture.native(AccountClient::CodexCli);
    let selection = fixture.pool.select(&fixture.admit(None)).unwrap();
    fixture
        .pool
        .record_outcome(&selection, AccountOutcome::from_http(401, None))
        .unwrap();
    let polled = fixture
        .pool
        .update_native_state(
            &account.id,
            account.credential_generation,
            native_state(AccountClient::CodexCli, &reference, true),
        )
        .unwrap();
    assert_eq!(polled.status, AccountStatus::LoginRequired);
    assert_eq!(
        polled.authorization_status,
        AuthorizationStatus::LoginRequired
    );
    assert_eq!(polled.last_validated_at, None);
    assert_eq!(polled.credential_generation, account.credential_generation);
    assert_eq!(
        fixture.pool.select(&fixture.admit(None)).unwrap_err(),
        AccountError::NoEligibleAccount
    );
    let replacement = Uuid::new_v4().to_string();
    let authorized = fixture
        .pool
        .update_native_state(
            &account.id,
            polled.credential_generation,
            native_state(AccountClient::CodexCli, &replacement, true),
        )
        .unwrap();
    assert_eq!(authorized.status, AccountStatus::Available);
    assert_eq!(
        authorized.credential_generation,
        account.credential_generation + 1
    );
    let signed_out = fixture
        .pool
        .update_native_state(
            &account.id,
            authorized.credential_generation,
            native_state(AccountClient::CodexCli, &replacement, false),
        )
        .unwrap();
    assert_eq!(signed_out.status, AccountStatus::LoginRequired);
    assert_eq!(
        signed_out.credential_generation,
        authorized.credential_generation + 1
    );
    assert_eq!(
        fixture
            .pool
            .update_native_state(
                &account.id,
                authorized.credential_generation,
                native_state(AccountClient::CodexCli, &replacement, true),
            )
            .unwrap_err(),
        AccountError::StaleSelection
    );
    assert_eq!(fixture.store.reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn native_reference_changes_reject_stale_state_dispatch_feedback_and_revocation_resurrection()
{
    let fixture = Fixture::new();
    let (account, reference) = fixture.native(AccountClient::CodexDesktop);
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let feedback = fixture.pool.select(&fixture.admit(None)).unwrap();
    let replacement = Uuid::new_v4().to_string();
    let authorized = fixture
        .pool
        .update_native_state(
            &account.id,
            account.credential_generation,
            native_state(AccountClient::CodexDesktop, &replacement, true),
        )
        .unwrap();
    let before = fixture.budgets();
    assert_eq!(
        fixture
            .pool
            .update_native_state(
                &account.id,
                account.credential_generation,
                native_state(AccountClient::CodexDesktop, &reference, false),
            )
            .unwrap_err(),
        AccountError::StaleSelection
    );
    let old_feedback = fixture
        .pool
        .record_outcome(&feedback, AccountOutcome::from_http(401, None))
        .unwrap();
    assert_eq!(
        old_feedback.authorization_status,
        AuthorizationStatus::Authorized
    );
    assert_eq!(
        old_feedback.credential_generation,
        authorized.credential_generation
    );
    let created = AtomicUsize::new(0);
    assert_eq!(
        fixture
            .pool
            .dispatch(&selection, &context, |_| {
                created.fetch_add(1, Ordering::SeqCst);
                std::future::ready(())
            })
            .await
            .unwrap_err(),
        AccountError::StaleSelection
    );
    let current = fixture.pool.select(&context).unwrap();
    assert_eq!(
        current.credential_generation(),
        authorized.credential_generation
    );
    let pending = fixture.pool.dispatch(&current, &context, |_| {
        created.fetch_add(1, Ordering::SeqCst);
        std::future::ready(())
    });
    fixture.pool.revoke(&account.id, true).unwrap();
    assert_eq!(pending.await.unwrap_err(), AccountError::Revoked);
    assert_eq!(created.load(Ordering::SeqCst), 0);
    assert_eq!(
        fixture
            .pool
            .update_native_state(
                &account.id,
                authorized.credential_generation,
                native_state(AccountClient::CodexDesktop, &replacement, true),
            )
            .unwrap_err(),
        AccountError::Revoked
    );
    assert_eq!(fixture.store.reads.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.budgets(), before);
}

#[tokio::test]
async fn native_runtime_checks_expire_or_invalidate_before_prepared_dispatch() {
    for invalidation in ["expired", "generation", "explicit"] {
        let fixture = Fixture::new();
        let (account, reference) = fixture.native(AccountClient::CodexCli);
        let context = fixture.admit(None);
        let selection = fixture.pool.select(&context).unwrap();
        assert!(matches!(fixture.pool.prepare_authorization(&selection, &context).unwrap(),
            AccountAuthorization::NativeManaged { .. }));
        let budgets = fixture.budgets();
        match invalidation {
            "explicit" => fixture.pool.invalidate_native_checks(&[account.id.clone()]).unwrap(),
            "expired" => {
                fixture.pool.runtime.native_checks.lock().unwrap().insert(account.id.clone(),
                    (account.credential_generation, std::time::Instant::now() - Duration::from_secs(301)));
            }
            _ => {
                fixture.pool.runtime.native_checks.lock().unwrap().insert(account.id.clone(),
                    (account.credential_generation - 1, std::time::Instant::now()));
            }
        }
        let current = fixture.pool.get(&account.id).unwrap();
        assert_eq!(current.status, AccountStatus::NativeCheckRequired);
        assert_eq!(current.authorization_status, AuthorizationStatus::Authorized);
        assert_eq!(current.expires_at, None);
        assert!(fixture.pool.eligible(&context).unwrap().is_empty());
        assert_eq!(fixture.pool.prepare_authorization(&selection, &context).unwrap_err(), AccountError::NoEligibleAccount);
        let dispatched = AtomicUsize::new(0);
        assert_eq!(fixture.pool.dispatch(&selection, &context, |_| {
            dispatched.fetch_add(1, Ordering::SeqCst);
            std::future::ready(())
        }).await.unwrap_err(), AccountError::NoEligibleAccount);
        assert_eq!(dispatched.load(Ordering::SeqCst), 0);
        assert!(!fixture.dispatched(&context));
        assert_eq!(fixture.budgets(), budgets);
        let checked = fixture.pool.update_native_state(&account.id, account.credential_generation,
            native_state(AccountClient::CodexCli, &reference, true)).unwrap();
        assert_eq!(checked.status, AccountStatus::Available);
        assert_eq!(checked.credential_generation, account.credential_generation);
        assert!(fixture.pool.prepare_authorization(&selection, &context).is_ok());
        assert_eq!(fixture.store.reads.load(Ordering::SeqCst), 0);
    }
}

#[test]
fn native_runtime_checks_are_not_restored_from_persisted_authorization() {
    let fixture = Fixture::new();
    let (account, reference) = fixture.native(AccountClient::CodexDesktop);
    let context = fixture.admit(None);
    let selection = fixture.pool.select(&context).unwrap();
    let Fixture { db, pool, store } = fixture;
    let previous_runtime = Arc::downgrade(&pool.runtime);
    drop(pool);
    assert!(previous_runtime.upgrade().is_none());
    let reopened = AccountPool::with_store(db, store.clone()).unwrap();
    let restored = reopened.get(&account.id).unwrap();
    assert_eq!(restored.status, AccountStatus::NativeCheckRequired);
    assert_eq!(restored.authorization_status, AuthorizationStatus::Authorized);
    assert_eq!(restored.expires_at, None);
    assert_eq!(reopened.prepare_authorization(&selection, &context).unwrap_err(), AccountError::NoEligibleAccount);
    let validated = reopened.update_native_state(&account.id, account.credential_generation,
        native_state(AccountClient::CodexDesktop, &reference, true)).unwrap();
    assert_eq!(validated.status, AccountStatus::Available);
    assert!(reopened.prepare_authorization(&selection, &context).is_ok());
    assert_eq!(store.reads.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn native_dispatch_only_delivers_the_exact_client_reference_and_drops_on_disconnect() {
    for client in [
        AccountClient::CodexDesktop,
        AccountClient::CodexCli,
        AccountClient::ClaudeDesktop,
        AccountClient::ClaudeCode,
        AccountClient::OpenCodeDesktop,
        AccountClient::OpenCodeCli,
        AccountClient::GeminiCli,
    ] {
        let fixture = Fixture::new();
        let (account, reference) = fixture.native(client);
        assert!(
            !serde_json::to_string(&account)
                .unwrap()
                .contains(&reference)
        );
        assert!(!format!("{:?}", native_state(client, &reference, true)).contains(&reference));
        let context = fixture.admit(None);
        let selection = fixture.pool.select(&context).unwrap();
        let connection = AccountCancellation::default();
        let before = fixture.budgets();
        let created = Arc::new(AtomicUsize::new(0));
        let polled = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let factory_count = created.clone();
        let poll_count = polled.clone();
        let drop_count = dropped.clone();
        let task_signal = context.cancellation.clone();
        let connection_signal = connection.clone();
        let mut dispatch = Box::pin(fixture.pool.dispatch_with_connection(
            &selection,
            &context,
            &connection,
            move |authorization| {
                factory_count.fetch_add(1, Ordering::SeqCst);
                assert!(task_signal.0.gate.try_lock().is_err());
                assert!(connection_signal.0.gate.try_lock().is_err());
                assert!(!format!("{authorization:?}").contains(&reference));
                match authorization {
                    AccountAuthorization::NativeManaged {
                        client: selected,
                        reference_id,
                    } => {
                        assert_eq!(selected, client);
                        assert_eq!(reference_id, reference);
                    }
                    _ => panic!("native dispatch must deliver a client reference"),
                }
                let guard = DropCounter(drop_count);
                std::future::poll_fn(move |_task| {
                    let _keep_alive = &guard;
                    if poll_count.fetch_add(1, Ordering::SeqCst) == 0 {
                        assert!(task_signal.0.gate.try_lock().is_err());
                        assert!(connection_signal.0.gate.try_lock().is_err());
                    }
                    Poll::<()>::Pending
                })
            },
        ));
        std::future::poll_fn(|task| {
            assert!(dispatch.as_mut().poll(task).is_pending());
            Poll::Ready(())
        })
        .await;
        connection.cancel();
        assert_eq!(dispatch.await.unwrap_err(), AccountError::Cancelled);
        assert_eq!(created.load(Ordering::SeqCst), 1);
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
        assert!(!context.cancellation.is_cancelled());
        fixture
            .pool
            .record_outcome(&selection, outcome(OutcomeKind::Cancelled))
            .unwrap();
        assert_eq!(fixture.budgets(), before);
        let next = fixture.admit(None);
        let next_selection = fixture.pool.select(&next).unwrap();
        let next_before = fixture.budgets();
        assert_eq!(
            fixture
                .pool
                .dispatch_with_connection(&next_selection, &next, &connection, |_| {
                    created.fetch_add(1, Ordering::SeqCst);
                    std::future::ready(())
                })
                .await
                .unwrap_err(),
            AccountError::Cancelled
        );
        assert_eq!(created.load(Ordering::SeqCst), 1);
        assert_eq!(fixture.store.reads.load(Ordering::SeqCst), 0);
        assert!(fixture.store.values.lock().unwrap().is_empty());
        assert_eq!(fixture.budgets(), next_before);
    }
}

struct SyntheticFiles(Vec<std::path::PathBuf>);
impl SyntheticFiles {
    fn new() -> Self {
        Self(vec![])
    }
    fn file(&mut self, content: &[u8]) -> std::path::PathBuf {
        let path =
            std::env::temp_dir().join(format!("xy-accounts-synthetic-{}.json", Uuid::new_v4()));
        std::fs::write(&path, content).unwrap();
        self.0.push(path.clone());
        path
    }
}
impl Drop for SyntheticFiles {
    fn drop(&mut self) {
        for path in &self.0 {
            let _ = std::fs::remove_file(path);
        }
    }
}

#[test]
fn selected_import_is_single_use_authorized_and_bound_to_the_open_file() {
    let fixture = Fixture::new();
    let mut files = SyntheticFiles::new();
    let path = files.file(br#"{"schemaVersion":1,"credentials":{"kind":"apiKey","apiKey":"synthetic-selected-original"}}"#);
    let chosen = fixture.pool.select_import_source(&path).unwrap();
    let mut request = ImportAccountRequest {
        source_id: chosen.source_id.clone(),
        alias: "selected".into(),
        provider_id: "accounts-provider".into(),
        approved_destination: Some(TEST_DESTINATION.into()),
        allow_refresh: false,
        confirmed: false,
    };
    assert_eq!(
        fixture.pool.import(request.clone()).unwrap_err(),
        AccountError::AuthorizationRequired
    );
    assert!(fixture.pool.list().unwrap().is_empty());
    let backup = files.file(b"");
    std::fs::remove_file(&backup).unwrap();
    std::fs::rename(&path, &backup).unwrap();
    std::fs::write(&path, br#"{"schemaVersion":1,"credentials":{"kind":"apiKey","apiKey":"synthetic-replacement-file"}}"#).unwrap();
    request.confirmed = true;
    let account = fixture.pool.import(request.clone()).unwrap();
    assert_eq!(account.source, AccountSource::SelectedFile);
    assert_eq!(
        fixture
            .store
            .get(&secret_id(&account.id, 1, "access"))
            .unwrap()
            .unwrap()
            .expose(),
        "synthetic-selected-original"
    );
    assert_eq!(
        fixture.pool.import(request).unwrap_err(),
        AccountError::ImportSelectionExpired
    );
    let invalid = files.file(
        br#"{"schemaVersion":1,"credentials":{"kind":"nativeManaged","client":"codexDesktop"}}"#,
    );
    let selected = fixture.pool.select_import_source(&invalid).unwrap();
    assert_eq!(
        fixture
            .pool
            .import(ImportAccountRequest {
                source_id: selected.source_id,
                alias: "bad".into(),
                provider_id: "accounts-provider".into(),
                approved_destination: Some(TEST_DESTINATION.into()),
                allow_refresh: false,
                confirmed: true
            })
            .unwrap_err(),
        AccountError::InvalidImportFile
    );
    let oversized = files.file(&vec![0; MAX_IMPORT_BYTES as usize + 1]);
    assert_eq!(
        fixture.pool.select_import_source(&oversized).unwrap_err(),
        AccountError::InvalidImportFile
    );
}
