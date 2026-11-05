use super::*;
use crate::accounts::{AccountClient, AccountCredentialInput, AccountInfo, AccountPolicy, AccountRefresher,
    AccountSecret, AddAccountRequest, NativeAccountState, RefreshError, RefreshRequest, RefreshedCredential,
    UpdateAccountRequest};
use crate::governance::tasks;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::{Notify, Semaphore};

struct PooledKeys { pool: AccountPool, legacy_reads: Arc<AtomicUsize> }

impl ApiKeyProvider for PooledKeys {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        self.legacy_reads.fetch_add(1, Ordering::SeqCst);
        Ok(Some("unapproved-legacy-key".into()))
    }
    fn account_pool(&self) -> Option<&AccountPool> { Some(&self.pool) }
}

struct PoolFixture { proxy: Fixture, pool: AccountPool, legacy_reads: Arc<AtomicUsize> }

impl PoolFixture {
    async fn new(replies: Vec<Reply>) -> Self {
        let legacy_reads = Arc::new(AtomicUsize::new(0));
        let reads = legacy_reads.clone();
        let proxy = Fixture::with_keys(replies, strict(), Duration::from_secs(5), move |db| {
            Arc::new(PooledKeys { pool: AccountPool::test_pool(db.clone()).unwrap(), legacy_reads: reads })
        }).await;
        let pool = AccountPool::test_pool(proxy.db.clone()).unwrap();
        Self { proxy, pool, legacy_reads }
    }
    fn add(&self, alias: &str, credentials: AccountCredentialInput, allow_refresh: bool) -> AccountInfo {
        self.pool.add(AddAccountRequest {
            alias: alias.into(), provider_id: "gov-test".into(),
            approved_destination: Some(self.proxy.upstream_base.clone()), credentials,
            available_models: vec![], allow_refresh, confirmed: true,
        }).unwrap()
    }
    fn api(&self, alias: &str, value: &str) -> AccountInfo {
        self.add(alias, AccountCredentialInput::ApiKey { api_key: secret(value) }, false)
    }
    fn oauth(&self) -> AccountInfo {
        self.add("OAuth test", AccountCredentialInput::OAuth {
            access_token: secret("expired-account-token"), refresh_token: Some(secret("synthetic-refresh-token")),
            expires_at: Some(timestamp() - 60),
        }, true)
    }
    fn task(&self) -> tasks::TaskDetail {
        let list = tasks::list(&self.proxy.db).unwrap();
        assert_eq!(list.len(), 1);
        tasks::detail(&self.proxy.db, &list[0].id).unwrap()
    }
    fn assert_no_legacy_key(&self) { assert_eq!(self.legacy_reads.load(Ordering::SeqCst), 0); }
}

fn secret(value: &str) -> AccountSecret { AccountSecret::new(value.into()).unwrap() }
fn timestamp() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs() as i64 }

struct HeldRefresh { entered: Arc<Notify>, release: Arc<Semaphore>, calls: AtomicUsize }

impl HeldRefresh {
    fn new() -> Arc<Self> { Arc::new(Self { entered: Arc::new(Notify::new()), release: Arc::new(Semaphore::new(0)), calls: AtomicUsize::new(0) }) }
}

impl AccountRefresher for HeldRefresh {
    fn refresh(&self, request: RefreshRequest) -> Pin<Box<dyn Future<Output = Result<RefreshedCredential, RefreshError>> + Send>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let entered = self.entered.clone();
        let release = self.release.clone();
        Box::pin(async move {
            entered.notify_one();
            tokio::select! {
                biased;
                _ = request.cancellation.cancelled() => Err(RefreshError::Cancelled),
                permit = release.acquire() => {
                    let _permit = permit.map_err(|_| RefreshError::Unavailable)?;
                    Ok(RefreshedCredential { access_token: secret("fresh-account-token"), refresh_token: None, expires_at: Some(timestamp() + 3600) })
                }
            }
        })
    }
}

fn install_refresh(fixture: &PoolFixture) -> Arc<HeldRefresh> {
    let account = fixture.oauth();
    let refresher = HeldRefresh::new();
    fixture.pool.register_refresher(&account.id, refresher.clone(), Duration::from_secs(5)).unwrap();
    refresher
}

#[tokio::test]
async fn pooled_http_dispatch_excludes_native_managed_accounts_and_fixed_native_policies() {
    for mode in ["native-only", "mixed", "fixed-native"] {
        let fixture = PoolFixture::new(vec![Reply::Good]).await;
        let native = fixture.add("Native account", AccountCredentialInput::NativeManaged {
            client: AccountClient::ClaudeDesktop,
        }, false);
        let reference = uuid::Uuid::new_v4().to_string();
        fixture.pool.update_native_state(&native.id, native.credential_generation, NativeAccountState {
            client: AccountClient::ClaudeDesktop, reference_id: reference.clone(),
            authorized: true, expires_at: None, available_models: vec![],
        }).unwrap();
        if mode != "native-only" { fixture.api("HTTP account", "http-account-token"); }
        if mode == "fixed-native" {
            fixture.pool.save_policy("gov-test", AccountPolicy {
                mode: SelectionMode::Fixed, fixed_account_id: Some(native.id), ..Default::default()
            }, true).unwrap();
        }
        let response = fixture.proxy.post().send().await.unwrap();
        if mode == "mixed" {
            assert_eq!(response.status(), 200);
            response.bytes().await.unwrap();
            assert_eq!(fixture.proxy.calls(), 1);
            let requests = fixture.proxy.mock.captured.lock().unwrap();
            assert_eq!(requests[0].authorization.as_deref(), Some("Bearer http-account-token"));
        } else {
            assert!(!response.status().is_success());
            let receipt = response.text().await.unwrap();
            assert!(!receipt.contains(&reference));
            assert_eq!(fixture.proxy.calls(), 0);
        }
        fixture.assert_no_legacy_key();
    }
}

async fn entered(refresh: &HeldRefresh) {
    tokio::time::timeout(Duration::from_secs(3), refresh.entered.notified()).await.unwrap();
}

#[tokio::test]
async fn pooled_unauthorized_rotates_accounts_and_reserves_the_same_task() {
    let fixture = PoolFixture::new(vec![Reply::Error(401), Reply::Good]).await;
    fixture.api("First", "account-key-first");
    fixture.api("Second", "account-key-second");
    let response = fixture.proxy.post_task("pooled-rotation").send().await.unwrap();
    assert_eq!(response.status(), 200);
    response.bytes().await.unwrap();
    let mut authorization: Vec<_> = fixture.proxy.mock.captured.lock().unwrap().iter().map(|request| request.authorization.clone().unwrap()).collect();
    authorization.sort();
    assert_eq!(authorization, vec!["Bearer account-key-first", "Bearer account-key-second"]);
    let task = fixture.task();
    assert_eq!(task.task.attempts, 2);
    assert_eq!(task.attempts.iter().map(|attempt| attempt.ordinal).collect::<Vec<_>>(), vec![1, 2]);
    assert_eq!(task.attempts[0].status_code, Some(401));
    assert_eq!(task.attempts[1].status_code, Some(200));
    assert_eq!(budget::status(&fixture.proxy.db).unwrap().attempts, 2);
    fixture.assert_no_legacy_key();
}

#[tokio::test]
async fn pooled_fixed_account_failure_does_not_bypass_policy() {
    let fixture = PoolFixture::new(vec![Reply::Error(503), Reply::Good]).await;
    let fixed = fixture.api("Fixed", "fixed-account-token");
    fixture.api("Other", "unused-account-token");
    fixture.pool.save_policy("gov-test", AccountPolicy { mode: SelectionMode::Fixed, fixed_account_id: Some(fixed.id), ..Default::default() }, true).unwrap();
    let response = fixture.proxy.post().send().await.unwrap();
    assert_eq!(response.status(), 503);
    response.bytes().await.unwrap();
    assert_eq!(fixture.proxy.calls(), 1);
    assert_eq!(fixture.task().task.attempts, 1);
    fixture.assert_no_legacy_key();
}

#[tokio::test]
async fn pooled_disabled_revoked_and_empty_policies_never_read_legacy_credentials() {
    for mode in ["disabled", "revoked", "empty"] {
        let fixture = PoolFixture::new(vec![]).await;
        if mode == "empty" {
            fixture.pool.save_policy("gov-test", AccountPolicy::default(), true).unwrap();
        } else {
            let account = fixture.api("Unavailable", "unavailable-account-token");
            if mode == "revoked" { fixture.pool.revoke(&account.id, true).unwrap(); }
            else {
                fixture.pool.update(UpdateAccountRequest {
                    id: account.id, expected_revision: account.revision, alias: account.alias,
                    provider_id: "gov-test".into(), approved_destination: Some(fixture.proxy.upstream_base.clone()),
                    enabled: false, available_models: vec![], confirmed: true,
                }).unwrap();
            }
        }
        let response = fixture.proxy.post().send().await.unwrap();
        assert_eq!(response.status(), 503, "{mode}");
        assert!(response.text().await.unwrap().contains("account_no_eligible_account"));
        assert_eq!(fixture.proxy.calls(), 0);
        fixture.assert_no_legacy_key();
    }
}

#[tokio::test]
async fn pooled_rotation_cannot_exceed_the_task_attempt_cap() {
    let fixture = PoolFixture::new(vec![Reply::Error(401), Reply::Good]).await;
    fixture.api("First", "account-key-first");
    fixture.api("Second", "account-key-second");
    let mut policy = fixture::budget(&fixture.proxy.db);
    policy.task_max_attempts = 1;
    governance::save(&fixture.proxy.db, budget::KEY, &policy).unwrap();
    let response = fixture.proxy.post().send().await.unwrap();
    assert_eq!(response.status(), 402);
    assert!(response.text().await.unwrap().contains("task_attempt_budget_exhausted"));
    assert_eq!(fixture.proxy.calls(), 1);
    assert_eq!(fixture.task().task.attempts, 1);
    assert_eq!(budget::status(&fixture.proxy.db).unwrap().attempts, 1);
    fixture.assert_no_legacy_key();
}

#[tokio::test]
async fn pooled_credentials_are_redacted_from_requests_and_responses() {
    let token = "ordinary-account-token";
    let reply = json!({"id":"echo","object":"chat.completion","model":"gov-model",
        "choices":[{"index":0,"message":{"role":"assistant","content":token},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}});
    let fixture = PoolFixture::new(vec![Reply::Raw { status: 200, content_type: "application/json",
        chunks: vec![serde_json::to_vec(&reply).unwrap()], request_id: Some("ordinary-account-token") }]).await;
    fixture.api("Private", token);
    let mut request = body();
    request["messages"][0]["content"] = json!(format!("{token} user@example.test"));
    let response = fixture.proxy.post().json(&request).send().await.unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.headers().get("x-request-id").is_none());
    let text = response.text().await.unwrap();
    assert!(!text.contains(token));
    assert!(text.contains("REDACTED"));
    let captured = fixture.proxy.mock.captured.lock().unwrap();
    assert_eq!(captured[0].authorization.as_deref(), Some("Bearer ordinary-account-token"));
    assert!(!captured[0].body.to_string().contains(token));
    assert!(!captured[0].body.to_string().contains("user@example.test"));
    fixture.assert_no_legacy_key();
}

#[tokio::test]
async fn pooled_redaction_expansion_is_rechecked_before_any_upstream_send() {
    let fixture = PoolFixture::new(vec![]).await;
    fixture.api("Short", "abc");
    let mut request = body();
    request["messages"][0]["content"] = json!("abc ".repeat(100));
    let response = fixture.proxy.post().json(&request).send().await.unwrap();
    assert_eq!(response.status(), 402);
    assert!(response.text().await.unwrap().contains("budget_input_reservation_exceeded"));
    assert_eq!(fixture.proxy.calls(), 0);
    assert_eq!(fixture.task().task.attempts, 1);
    assert_eq!(budget::status(&fixture.proxy.db).unwrap().attempts, 1);
    fixture.assert_no_legacy_key();
}

#[tokio::test]
async fn pooled_refresh_uses_fresh_bearer_and_current_privacy_settings() {
    let fixture = PoolFixture::new(vec![]).await;
    crate::privacy::save(&fixture.proxy.db, &crate::privacy::PrivacySettings { redact_personal_information: false, block_uninspected_attachments: false }).unwrap();
    let refresh = install_refresh(&fixture);
    let mut request = body();
    request["messages"][0]["content"] = json!("user@example.test fresh-account-token");
    let pending = tokio::spawn(fixture.proxy.post().json(&request).send());
    entered(&refresh).await;
    crate::privacy::save(&fixture.proxy.db, &crate::privacy::PrivacySettings::default()).unwrap();
    refresh.release.add_permits(1);
    let response = pending.await.unwrap().unwrap();
    assert_eq!(response.status(), 200);
    response.bytes().await.unwrap();
    let captured = fixture.proxy.mock.captured.lock().unwrap();
    assert_eq!(captured[0].authorization.as_deref(), Some("Bearer fresh-account-token"));
    assert!(!captured[0].body.to_string().contains("user@example.test"));
    assert!(!captured[0].body.to_string().contains("fresh-account-token"));
    assert_eq!(refresh.calls.load(Ordering::SeqCst), 1);
    fixture.assert_no_legacy_key();
}

#[tokio::test]
async fn pooled_refresh_cannot_send_after_cancel_or_destination_change() {
    for cancel in [true, false] {
        let fixture = PoolFixture::new(vec![]).await;
        let refresh = install_refresh(&fixture);
        let pending = tokio::spawn(fixture.proxy.post().send());
        entered(&refresh).await;
        if cancel { tasks::cancel(&fixture.proxy.db, &fixture.task().task.id).unwrap(); }
        else {
            let mut provider = fixture.proxy.db.get_provider("gov-test").unwrap().unwrap();
            provider.endpoint = format!("{}/unapproved", fixture.proxy.upstream_base);
            fixture.proxy.db.save_provider(&provider).unwrap();
        }
        refresh.release.add_permits(1);
        let response = pending.await.unwrap().unwrap();
        assert_eq!(response.status().as_u16(), if cancel { 408 } else { 503 });
        response.bytes().await.unwrap();
        assert_eq!(fixture.proxy.calls(), 0);
        fixture.assert_no_legacy_key();
    }
}

#[tokio::test]
async fn pooled_final_price_and_task_limits_are_rechecked_after_refresh() {
    for price_change in [true, false] {
        let fixture = PoolFixture::new(vec![]).await;
        let refresh = install_refresh(&fixture);
        let pending = tokio::spawn(fixture.proxy.post().send());
        entered(&refresh).await;
        if price_change {
            crate::model_pricing::save_on(&fixture.proxy.db.conn().lock().unwrap(), &crate::model_pricing::PricingInput {
                id: "gov-model".into(), input_per_million: 10.0, output_per_million: 20.0,
                cache_read_per_million: None, cache_creation_per_million: None,
            }).unwrap();
        } else {
            let mut policy = fixture::budget(&fixture.proxy.db);
            policy.task_max_attempts = 0;
            governance::save(&fixture.proxy.db, budget::KEY, &policy).unwrap();
        }
        refresh.release.add_permits(1);
        let response = pending.await.unwrap().unwrap();
        assert_eq!(response.status(), 402);
        let receipt = response.text().await.unwrap();
        assert!(receipt.contains(if price_change { "budget_prepared_reservation_exceeded" } else { "task_attempt_budget_exhausted" }));
        assert_eq!(fixture.proxy.calls(), 0);
        assert_eq!(fixture.task().task.attempts, 1);
        assert_eq!(budget::status(&fixture.proxy.db).unwrap().attempts, 1);
        fixture.assert_no_legacy_key();
    }
}
