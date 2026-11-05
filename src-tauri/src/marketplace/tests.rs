use super::*;
use super::requests::{CatalogQuery, ListQuery, ReportRequest, ResolveReportRequest};
use crate::plugins::{PluginKind, PluginManifest, PluginPermissions};
use ring::{rand::SystemRandom, signature::{Ed25519KeyPair, KeyPair}};
use rusqlite::{Connection, params};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::{Arc, Barrier, OnceLock}};

const TEST_PASSWORD: &str = "synthetic market test phrase 2026";
const REVIEWER_USERNAME: &str = "independent_reviewer";
const REVIEWER_NAME: &str = "Independent Reviewer";

struct Fixture {
    market: Marketplace,
    directory: PathBuf,
    private_key: Vec<u8>,
    public_key: Vec<u8>,
    reviewers: Vec<ReviewerConfig>,
}

impl Fixture {
    fn new() -> Self {
        let directory = std::env::temp_dir().join(format!("xy-marketplace-test-{}",uuid::Uuid::new_v4()));
        let private_key = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap().as_ref().to_vec();
        let public_key = Ed25519KeyPair::from_pkcs8(&private_key).unwrap().public_key().as_ref().to_vec();
        static REVIEWER_HASH: OnceLock<PasswordHash> = OnceLock::new();
        let reviewers = vec![ReviewerConfig { username: REVIEWER_USERNAME.to_owned(),display_name: REVIEWER_NAME.to_owned(),
            password_hash: REVIEWER_HASH.get_or_init(|| hash_password_for_deployment(TEST_PASSWORD).unwrap()).clone() }];
        let config = fixture_config(&directory,&private_key,&reviewers);
        let market = Marketplace::open(config).unwrap();
        Self { market,directory,private_key,public_key,reviewers }
    }

    fn config(&self) -> MarketplaceConfig { fixture_config(&self.directory,&self.private_key,&self.reviewers) }
    fn database(&self) -> Connection { Connection::open(self.directory.join("market.sqlite3")).unwrap() }
    fn register(&self, username: &str, display_name: &str) -> AuthSession {
        self.market.inner.store.register(username,display_name,TEST_PASSWORD,86400).unwrap()
    }
    fn reviewer(&self) -> AuthSession { self.market.inner.store.login(REVIEWER_USERNAME,TEST_PASSWORD,86400).unwrap() }
    fn submit(&self, token: &str, package: PluginPackage) -> Submission {
        self.market.inner.store.submit(&self.market.inner,token,SubmitRequest { package,replaces_submission_id: None }).unwrap()
    }
    fn approve(&self, token: &str, submission: &Submission) {
        self.market.inner.store.decide(&self.market.inner,token,&submission.submission_id,&DecisionRequest {
            version: submission.package.manifest.version.clone(),digest: submission.digest.clone(),decision: ReviewDecision::Approve,note: String::new(),
        }).unwrap();
    }
    fn publish(&self, token: &str, submission: &Submission) -> PublishedPlugin {
        self.market.inner.store.publish(&self.market.inner,token,&submission.submission_id,&expected(submission)).unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        if self.directory.parent() == Some(std::env::temp_dir().as_path())
            && self.directory.file_name().and_then(|name| name.to_str()).is_some_and(|name| name.starts_with("xy-marketplace-test-")) {
            let _ = std::fs::remove_dir_all(&self.directory);
        }
    }
}

fn fixture_config(directory: &std::path::Path, key: &[u8], reviewers: &[ReviewerConfig]) -> MarketplaceConfig {
    MarketplaceConfig { database_path: directory.join("market.sqlite3"),listen: "127.0.0.1:0".parse().unwrap(),
        public_origin: "https://market.example.test".to_owned(),allowed_origins: vec!["https://market.example.test".to_owned(),"tauri://localhost".to_owned()],
        reviewers: reviewers.to_vec(),signing_key_id: "test-publisher".to_owned(),signing_private_key: key.to_vec(),
        verification_keys: BTreeMap::new(),session_ttl_seconds: 86400,allow_development_http: false,allow_non_loopback: false }
}

fn package(id: &str, version: &str, author: &str) -> PluginPackage {
    PluginPackage { manifest: PluginManifest {
        schema_version: 1,id: id.to_owned(),name: "Shared example skill".to_owned(),description: "A small reviewed text package".to_owned(),
        author: author.to_owned(),version: version.to_owned(),minimum_host_version: "1.0.0".to_owned(),kind: PluginKind::Skill,
        entry: "SKILL.md".to_owned(),clients: vec!["codex-desktop".to_owned(),"claude-desktop".to_owned(),"opencode-desktop".to_owned()],
        permissions: PluginPermissions::default(),dependencies: Vec::new(),
    },files: BTreeMap::from([("SKILL.md".to_owned(),"# Shared skill\nSummarize the text selected by the user.\n".to_owned())]) }
}

fn network_package(id: &str, version: &str, author: &str, expanded: bool) -> PluginPackage {
    let mut package = package(id,version,author);
    package.manifest.kind = PluginKind::Mcp;
    package.manifest.entry = "tools.json".to_owned();
    if expanded { package.manifest.permissions.network.push("https://example.com".to_owned()); }
    package.files = BTreeMap::from([("tools.json".to_owned(),json!({"schemaVersion":1,"tools":[{
        "name":"echo","description":"Return explicitly supplied text","inputSchema":{"type":"object","properties":{"message":{"type":"string","maxLength":1000}},"required":["message"],"additionalProperties":false},
        "operation":{"type":"json_passthrough"}
    }]}).to_string())]);
    package
}

fn expected(submission: &Submission) -> VersionRequest {
    VersionRequest { version: submission.package.manifest.version.clone(),digest: submission.digest.clone() }
}

fn verify_fixture_signature(fixture: &Fixture, published: &PublishedPlugin) {
    assert_eq!(published.digest,crate::plugins::package_digest(&published.package).unwrap());
    let canonical = crate::plugins::canonical_package_bytes(&published.package).unwrap();
    assert_eq!(published.digest,auth::encode_hex(&Sha256::digest(canonical)));
    let payload = crate::plugins::signing_payload(&published.package.manifest.id,&published.package.manifest.version,&published.digest).unwrap();
    assert_eq!(payload,signature_payload(&published.package.manifest.id,&published.package.manifest.version,&published.digest));
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519,&fixture.public_key)
        .verify(&payload,&auth::decode_hex(&published.signature,64).unwrap()).unwrap();
}

struct Api { url: String,client: reqwest::Client,server: tokio::task::JoinHandle<()> }

impl Api {
    async fn start(market: Marketplace) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}",listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener,router(market)).await.unwrap(); });
        Self { url,client: reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().unwrap(),server }
    }
    async fn request(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (u16,Value) {
        let mut request = self.client.request(method.parse::<reqwest::Method>().unwrap(),format!("{}{path}",self.url));
        if let Some(token) = token { request=request.bearer_auth(token); }
        if let Some(body) = body { request=request.json(&body); }
        let response = request.send().await.unwrap();
        let status = response.status().as_u16();
        assert_eq!(response.headers()["cache-control"],"no-store");
        assert_eq!(response.headers()["x-content-type-options"],"nosniff");
        let bytes = response.bytes().await.unwrap();
        let body = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
        (status,body)
    }
    async fn register(&self, username: &str, display: &str) -> String {
        let (status,session) = self.request("POST","/v1/auth/register",None,Some(json!({"username":username,"displayName":display,"password":TEST_PASSWORD}))).await;
        assert_eq!(status,201);
        assert_eq!(session["user"]["reviewer"],false);
        session["token"].as_str().unwrap().to_owned()
    }
}

impl Drop for Api { fn drop(&mut self) { self.server.abort(); } }

fn assert_error<T>(result: Result<T, MarketplaceError>, error: MarketplaceError) {
    assert_eq!(result.err(), Some(error));
}

fn decision(submission: &Submission, choice: ReviewDecision) -> DecisionRequest {
    DecisionRequest {
        version: submission.package.manifest.version.clone(), digest: submission.digest.clone(),
        decision: choice, note: "Review the requested capability carefully.".to_owned(),
    }
}

fn withdrawal_request(submission: &Submission) -> WithdrawRequest {
    WithdrawRequest {
        version: submission.package.manifest.version.clone(), digest: submission.digest.clone(),
        reason: WithdrawalReason::Privacy, note: "The author requested removal.".to_owned(),
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_two_users_and_independent_reviewer_complete_workflow() {
    let fixture = Fixture::new();
    let api = Api::start(fixture.market.clone()).await;
    let author = api.register("ordinary_author", "Example Author").await;
    let reader = api.register("ordinary_reader", "Example Reader").await;
    let (status, login) = api.request("POST", "/v1/auth/login", None,
        Some(json!({"username":REVIEWER_USERNAME,"password":TEST_PASSWORD}))).await;
    assert_eq!(status, 200);
    assert_eq!(login["user"]["reviewer"], true);
    let reviewer = login["token"].as_str().unwrap();
    let original = package("shared-example", "1.0.0", "Example Author");
    let (status, submitted) = api.request("POST", "/v1/submissions", Some(&author),
        Some(json!({"package":original}))).await;
    assert_eq!(status, 201);
    assert_eq!(submitted["status"], "submitted");
    let id = submitted["submissionId"].as_str().unwrap();
    let detail_path = format!("/v1/submissions/{id}");
    let release_path = "/v1/plugins/shared-example/versions/1.0.0";
    let expected_json = json!({"version":"1.0.0","digest":submitted["digest"]});
    for path in ["/v1/submissions", detail_path.as_str(), "/v1/review/submissions", "/v1/review/audit"] {
        assert_eq!(api.request("GET", path, None, None).await.0, 401);
    }
    assert_eq!(api.request("GET", &detail_path, Some(&reader), None).await.0, 404);
    assert_eq!(api.request("GET", "/v1/review/submissions", Some(&reader), None).await.0, 403);
    assert_eq!(api.request("GET", release_path, None, None).await.0, 404);
    assert!(api.request("GET", "/v1/plugins", None, None).await.1["items"].as_array().unwrap().is_empty());
    assert_eq!(api.request("POST", &format!("{detail_path}/publish"), Some(&author), Some(expected_json.clone())).await.0, 409);
    let (status, reader_submission) = api.request("POST", "/v1/submissions", Some(&reader),
        Some(json!({"package":package("reader-example","1.0.0","Example Reader")}))).await;
    assert_eq!(status, 201);
    let reader_items = api.request("GET", "/v1/submissions", Some(&reader), None).await.1;
    assert_eq!(reader_items["items"].as_array().unwrap().len(), 1);
    assert_eq!(reader_items["items"][0]["submissionId"], reader_submission["submissionId"]);
    assert!(reader_items["items"][0].get("package").is_none());
    let decision_path = format!("/v1/review/submissions/{id}/decision");
    let approval = json!({"version":"1.0.0","digest":submitted["digest"],"decision":"approve"});
    assert_eq!(api.request("POST", &decision_path, Some(&author), Some(approval.clone())).await.0, 403);
    assert_eq!(api.request("POST", &decision_path, Some(reviewer), Some(approval)).await.0, 200);
    assert_eq!(api.request("GET", release_path, None, None).await.0, 404);
    let (status, published_json) = api.request("POST", &format!("{detail_path}/publish"), Some(&author), Some(expected_json)).await;
    assert_eq!(status, 200);
    let published: PublishedPlugin = serde_json::from_value(published_json.clone()).unwrap();
    assert!(published.package == original);
    verify_fixture_signature(&fixture, &published);
    assert_eq!(api.request("GET", release_path, None, None).await.1, published_json);
    let (status, catalog) = api.request("GET", "/v1/plugins?client=codex-desktop", None, None).await;
    assert_eq!(status, 200);
    assert_eq!(catalog["items"].as_array().unwrap().len(), 1);
    assert_eq!(catalog["items"][0]["authorDisplayName"], "Example Author");
    let public_text = catalog.to_string();
    for private_field in ["ordinary_author", "ordinary_reader", "userId", "username", "reviewNote", "password", "token"] {
        assert!(!public_text.contains(private_field));
    }
    let audit = api.request("GET", &format!("{detail_path}/audit"), Some(&author), None).await.1;
    let actions: Vec<_> = audit["items"].as_array().unwrap().iter().map(|item| item["action"].as_str().unwrap()).collect();
    assert_eq!(actions, vec!["published", "approved", "submitted"]);
    assert_eq!(api.request("GET", &format!("{detail_path}/audit"), Some(&reader), None).await.0, 404);
    let stored = fixture.market.inner.store.submission(&fixture.market.inner, &author, id).unwrap();
    let remove = serde_json::to_value(json!({"version":"1.0.0","digest":stored.digest,"reason":"privacy"})).unwrap();
    assert_eq!(api.request("POST", &format!("{detail_path}/withdraw"), Some(&reader), Some(remove.clone())).await.0, 404);
    assert_eq!(api.request("POST", &format!("{detail_path}/withdraw"), Some(&author), Some(remove)).await.0, 200);
    assert_eq!(api.request("GET", release_path, None, None).await.0, 404);
    assert!(api.request("GET", "/v1/plugins", None, None).await.1["items"].as_array().unwrap().is_empty());
    let (status, notice) = api.request("GET", &format!("{release_path}/withdrawal"), None, None).await;
    assert_eq!(status, 200);
    let notice: PluginWithdrawal = serde_json::from_value(notice).unwrap();
    let payload = crate::plugins::withdrawal_payload(&notice).unwrap();
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &fixture.public_key)
        .verify(&payload, &auth::decode_hex(&notice.signature, 64).unwrap()).unwrap();
    assert_eq!(notice.digest, published.digest);
    assert_eq!(api.request("POST", "/v1/auth/logout", Some(&reader), None).await.0, 204);
    assert_eq!(api.request("GET", "/v1/auth/me", Some(&reader), None).await.0, 401);
}

#[test]
fn reviewer_cannot_approve_own_submission_and_versions_are_immutable() {
    let fixture = Fixture::new();
    let author = fixture.register("author_identity", "Author Identity");
    let reviewer = fixture.reviewer();
    let self_submission = fixture.submit(&reviewer.token, package("reviewer-own", "1.0.0", REVIEWER_NAME));
    assert_error(fixture.market.inner.store.decide(&fixture.market.inner, &reviewer.token,
        &self_submission.submission_id, &decision(&self_submission, ReviewDecision::Approve)), MarketplaceError::Forbidden);
    let submission = fixture.submit(&author.token, package("immutable-example", "1.0.0", "Author Identity"));
    let mut wrong = decision(&submission, ReviewDecision::Approve);
    wrong.digest = "00".repeat(32);
    assert_error(fixture.market.inner.store.decide(&fixture.market.inner, &reviewer.token, &submission.submission_id, &wrong), MarketplaceError::Conflict);
    wrong = decision(&submission, ReviewDecision::Approve);
    wrong.version = "1.0.1".to_owned();
    assert_error(fixture.market.inner.store.decide(&fixture.market.inner, &reviewer.token, &submission.submission_id, &wrong), MarketplaceError::Conflict);
    fixture.approve(&reviewer.token, &submission);
    let mut replacement = submission.package.clone();
    replacement.files.insert("SKILL.md".to_owned(), "Changed after approval".to_owned());
    assert_error(fixture.market.inner.store.submit(&fixture.market.inner, &author.token,
        SubmitRequest { package: replacement, replaces_submission_id: None }), MarketplaceError::Conflict);
    let connection = fixture.database();
    for query in ["UPDATE submissions SET package_json='{}' WHERE id=?1",
        "UPDATE submissions SET digest=replace(digest,'a','b') WHERE id=?1",
        "UPDATE submissions SET approved_version='9.0.0' WHERE id=?1",
        "UPDATE submissions SET status='submitted' WHERE id=?1"] {
        assert!(connection.execute(query, [&submission.submission_id]).is_err());
    }
    let published = fixture.publish(&author.token, &submission);
    verify_fixture_signature(&fixture, &published);
    assert!(connection.execute("UPDATE releases SET package_json='{}' WHERE submission_id=?1", [&submission.submission_id]).is_err());
    assert_error(fixture.market.inner.store.publish(&fixture.market.inner, &author.token, &submission.submission_id, &expected(&submission)), MarketplaceError::Conflict);
    assert_error(fixture.market.inner.store.decide(&fixture.market.inner, &reviewer.token, &submission.submission_id,
        &decision(&submission, ReviewDecision::Approve)), MarketplaceError::Conflict);
}

#[test]
fn updates_permission_expansions_and_requested_changes_require_fresh_review() {
    let fixture = Fixture::new();
    let author = fixture.register("update_author", "Update Author");
    let stranger = fixture.register("other_author", "Other Author");
    let reviewer = fixture.reviewer();
    let initial = fixture.submit(&author.token, network_package("updated-example", "1.0.0", "Update Author", false));
    fixture.approve(&reviewer.token, &initial);
    fixture.publish(&author.token, &initial);
    let update = fixture.market.inner.store.submit(&fixture.market.inner, &author.token, SubmitRequest {
        package: network_package("updated-example", "1.1.0", "Update Author", true),
        replaces_submission_id: Some(initial.submission_id.clone()),
    }).unwrap();
    assert_eq!(update.status, SubmissionStatus::Submitted);
    assert_eq!(update.previous_published_version.as_deref(), Some("1.0.0"));
    assert_eq!(update.permission_changes.network, vec!["https://example.com"]);
    assert_error(fixture.market.inner.store.publish(&fixture.market.inner, &author.token,
        &update.submission_id, &expected(&update)), MarketplaceError::Conflict);
    assert_error(fixture.market.inner.store.download(&fixture.market.inner, "updated-example", "1.1.0"), MarketplaceError::NotFound);
    assert_error(fixture.market.inner.store.submit(&fixture.market.inner, &stranger.token, SubmitRequest {
        package: network_package("updated-example", "2.0.0", "Other Author", false), replaces_submission_id: None,
    }), MarketplaceError::Forbidden);
    fixture.market.inner.store.decide(&fixture.market.inner, &reviewer.token, &update.submission_id,
        &decision(&update, ReviewDecision::RequestChanges)).unwrap();
    assert_error(fixture.market.inner.store.decide(&fixture.market.inner, &reviewer.token, &update.submission_id,
        &decision(&update, ReviewDecision::Approve)), MarketplaceError::Conflict);
    let correction = fixture.market.inner.store.submit(&fixture.market.inner, &author.token, SubmitRequest {
        package: network_package("updated-example", "1.1.1", "Update Author", true),
        replaces_submission_id: Some(update.submission_id.clone()),
    }).unwrap();
    assert_eq!(correction.status, SubmissionStatus::Submitted);
    fixture.approve(&reviewer.token, &correction);
    let published = fixture.publish(&author.token, &correction);
    verify_fixture_signature(&fixture, &published);
    let catalog = fixture.market.inner.store.catalog(&fixture.market.inner, &CatalogQuery::default(), None).unwrap();
    assert_eq!(catalog.items.len(), 1);
    assert_eq!(catalog.items[0].version, "1.1.1");
    let versions = fixture.market.inner.store.catalog(&fixture.market.inner, &CatalogQuery::default(), Some("updated-example")).unwrap();
    assert_eq!(versions.items.len(), 2);
    fixture.market.inner.store.withdraw(&fixture.market.inner, &reviewer.token, &correction.submission_id,
        &withdrawal_request(&correction)).unwrap();
    assert_error(fixture.market.inner.store.download(&fixture.market.inner, "updated-example", "1.1.1"), MarketplaceError::NotFound);
    let catalog = fixture.market.inner.store.catalog(&fixture.market.inner, &CatalogQuery::default(), None).unwrap();
    assert_eq!(catalog.items[0].version, "1.0.0");
    let rejected = fixture.submit(&author.token, package("rejected-example", "1.0.0", "Update Author"));
    fixture.market.inner.store.decide(&fixture.market.inner, &reviewer.token, &rejected.submission_id,
        &decision(&rejected, ReviewDecision::Reject)).unwrap();
    assert_error(fixture.market.inner.store.publish(&fixture.market.inner, &author.token,
        &rejected.submission_id, &expected(&rejected)), MarketplaceError::Conflict);
    fixture.market.inner.store.withdraw(&fixture.market.inner, &author.token, &rejected.submission_id,
        &withdrawal_request(&rejected)).unwrap();
    assert_error(fixture.market.inner.store.withdrawal(&fixture.market.inner, "rejected-example", "1.0.0"), MarketplaceError::NotFound);
}

#[test]
fn publication_and_withdrawal_rollback_when_audit_persistence_fails() {
    let fixture = Fixture::new();
    let author = fixture.register("atomic_author", "Atomic Author");
    let reviewer = fixture.reviewer();
    let submission = fixture.submit(&author.token, package("atomic-example", "1.0.0", "Atomic Author"));
    fixture.approve(&reviewer.token, &submission);
    let connection = fixture.database();
    connection.execute_batch("CREATE TRIGGER fail_test_audit BEFORE INSERT ON audit WHEN NEW.action='published' BEGIN SELECT RAISE(ABORT,'test fault'); END;").unwrap();
    assert_error(fixture.market.inner.store.publish(&fixture.market.inner, &author.token,
        &submission.submission_id, &expected(&submission)), MarketplaceError::Conflict);
    let release_count: i64 = connection.query_row("SELECT count(*) FROM releases", [], |row| row.get(0)).unwrap();
    assert_eq!(release_count, 0);
    assert_eq!(fixture.market.inner.store.submission(&fixture.market.inner, &author.token,
        &submission.submission_id).unwrap().status, SubmissionStatus::Approved);
    connection.execute_batch("DROP TRIGGER fail_test_audit;").unwrap();
    fixture.publish(&author.token, &submission);
    connection.execute_batch("CREATE TRIGGER fail_test_audit BEFORE INSERT ON audit WHEN NEW.action='withdrawn' BEGIN SELECT RAISE(ABORT,'test fault'); END;").unwrap();
    assert_error(fixture.market.inner.store.withdraw(&fixture.market.inner, &author.token,
        &submission.submission_id, &withdrawal_request(&submission)), MarketplaceError::Conflict);
    assert!(fixture.market.inner.store.download(&fixture.market.inner, "atomic-example", "1.0.0").is_ok());
    let notice_count: i64 = connection.query_row("SELECT count(*) FROM withdrawals", [], |row| row.get(0)).unwrap();
    assert_eq!(notice_count, 0);
    connection.execute_batch("DROP TRIGGER fail_test_audit;").unwrap();
    fixture.market.inner.store.withdraw(&fixture.market.inner, &author.token,
        &submission.submission_id, &withdrawal_request(&submission)).unwrap();
    assert!(connection.execute("UPDATE audit SET action='changed'", []).is_err());
    assert!(connection.execute("DELETE FROM audit", []).is_err());
}

#[test]
fn independent_connections_cannot_race_duplicate_submission_decisions_or_publish() {
    let fixture = Fixture::new();
    let author = fixture.register("race_author", "Race Author");
    let reviewer = fixture.reviewer();
    let competitors = [Marketplace::open(fixture.config()).unwrap(), Marketplace::open(fixture.config()).unwrap()];
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = competitors.iter().cloned().map(|market| {
        let barrier = barrier.clone();
        let token = author.token.clone();
        std::thread::spawn(move || {
            barrier.wait();
            market.inner.store.submit(&market.inner, &token, SubmitRequest {
                package: package("race-example", "1.0.0", "Race Author"), replaces_submission_id: None,
            })
        })
    }).collect();
    barrier.wait();
    let submissions: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    assert_eq!(submissions.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(submissions.iter().filter(|result| result.as_ref().err() == Some(&MarketplaceError::Conflict)).count(), 1);
    let submission = submissions.into_iter().find_map(Result::ok).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = competitors.iter().cloned().map(|market| {
        let barrier = barrier.clone();
        let token = reviewer.token.clone();
        let id = submission.submission_id.clone();
        let request = decision(&submission, ReviewDecision::Approve);
        std::thread::spawn(move || {
            barrier.wait();
            market.inner.store.decide(&market.inner, &token, &id, &request)
        })
    }).collect();
    barrier.wait();
    let outcomes: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(outcomes.iter().filter(|result| result.as_ref().err() == Some(&MarketplaceError::Conflict)).count(), 1);
    let barrier = Arc::new(Barrier::new(3));
    let threads: Vec<_> = competitors.iter().cloned().map(|market| {
        let barrier = barrier.clone();
        let token = author.token.clone();
        let id = submission.submission_id.clone();
        let request = expected(&submission);
        std::thread::spawn(move || {
            barrier.wait();
            market.inner.store.publish(&market.inner, &token, &id, &request)
        })
    }).collect();
    barrier.wait();
    let outcomes: Vec<_> = threads.into_iter().map(|thread| thread.join().unwrap()).collect();
    assert_eq!(outcomes.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(outcomes.iter().filter(|result| result.as_ref().err() == Some(&MarketplaceError::Conflict)).count(), 1);
    verify_fixture_signature(&fixture, outcomes.iter().find_map(|result| result.as_ref().ok()).unwrap());
    let count: i64 = fixture.database().query_row("SELECT count(*) FROM audit WHERE submission_id=?1", [&submission.submission_id], |row| row.get(0)).unwrap();
    assert_eq!(count, 3);
}

#[test]
fn concurrent_publish_and_withdraw_leave_no_downloadable_withdrawn_package() {
    let fixture = Fixture::new();
    let author = fixture.register("withdraw_author", "Withdraw Author");
    let reviewer = fixture.reviewer();
    let submission = fixture.submit(&author.token, package("withdraw-race", "1.0.0", "Withdraw Author"));
    fixture.approve(&reviewer.token, &submission);
    let publisher = Marketplace::open(fixture.config()).unwrap();
    let withdrawer = Marketplace::open(fixture.config()).unwrap();
    let barrier = Arc::new(Barrier::new(3));
    let publish_barrier = barrier.clone();
    let publish_id = submission.submission_id.clone();
    let publish_token = author.token.clone();
    let publish_expected = expected(&submission);
    let publish = std::thread::spawn(move || {
        publish_barrier.wait();
        publisher.inner.store.publish(&publisher.inner, &publish_token, &publish_id, &publish_expected)
    });
    let withdraw_barrier = barrier.clone();
    let withdraw_id = submission.submission_id.clone();
    let withdraw_token = reviewer.token.clone();
    let withdraw_request = withdrawal_request(&submission);
    let withdraw = std::thread::spawn(move || {
        withdraw_barrier.wait();
        withdrawer.inner.store.withdraw(&withdrawer.inner, &withdraw_token, &withdraw_id, &withdraw_request)
    });
    barrier.wait();
    let published = publish.join().unwrap();
    assert!(published.is_ok() || published.as_ref().err() == Some(&MarketplaceError::Conflict));
    assert_eq!(withdraw.join().unwrap().unwrap().status, SubmissionStatus::Withdrawn);
    assert_error(fixture.market.inner.store.download(&fixture.market.inner, "withdraw-race", "1.0.0"), MarketplaceError::NotFound);
    assert!(fixture.market.inner.store.catalog(&fixture.market.inner, &CatalogQuery::default(), None).unwrap().items.is_empty());
    let connection = fixture.database();
    let releases: i64 = connection.query_row("SELECT count(*) FROM releases WHERE status='withdrawn'", [], |row| row.get(0)).unwrap();
    let notices: i64 = connection.query_row("SELECT count(*) FROM withdrawals", [], |row| row.get(0)).unwrap();
    assert_eq!(releases, i64::from(published.is_ok()));
    assert_eq!(notices, releases);
}

#[test]
fn withdrawal_rejects_corrupt_release_without_state_audit_or_notice_side_effects() {
    let fixture = Fixture::new();
    let author = fixture.register("corruption_author", "Corruption Author");
    let reviewer = fixture.reviewer();
    let submission = fixture.submit(&author.token, package("corruption-example", "1.0.0", "Corruption Author"));
    fixture.approve(&reviewer.token, &submission);
    fixture.publish(&author.token, &submission);
    let connection = fixture.database();
    let audit_before: i64 = connection.query_row("SELECT count(*) FROM audit", [], |row| row.get(0)).unwrap();
    let state_before: (String, i64) = connection.query_row("SELECT status,updated_at FROM submissions WHERE id=?1",
        [&submission.submission_id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    connection.execute_batch("DROP TRIGGER releases_immutable;").unwrap();
    connection.execute("UPDATE releases SET digest=?1 WHERE submission_id=?2",
        params!["00".repeat(32), submission.submission_id]).unwrap();
    assert_error(fixture.market.inner.store.withdraw(&fixture.market.inner, &author.token,
        &submission.submission_id, &withdrawal_request(&submission)), MarketplaceError::Unavailable);
    let state_after: (String, i64) = connection.query_row("SELECT status,updated_at FROM submissions WHERE id=?1",
        [&submission.submission_id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
    assert_eq!(state_before, state_after);
    let release_state: String = connection.query_row("SELECT status FROM releases WHERE submission_id=?1",
        [&submission.submission_id], |row| row.get(0)).unwrap();
    assert_eq!(release_state, "published");
    assert_eq!(connection.query_row("SELECT count(*) FROM audit", [], |row| row.get::<_, i64>(0)).unwrap(), audit_before);
    assert_eq!(connection.query_row("SELECT count(*) FROM withdrawals", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_error(fixture.market.inner.store.download(&fixture.market.inner, "corruption-example", "1.0.0"), MarketplaceError::Unavailable);
}

#[test]
fn withdrawal_reads_verify_signed_digest_approval_version_and_submission_link() {
    let fixture = Fixture::new();
    let author = fixture.register("notice_author", "Notice Author");
    let reviewer = fixture.reviewer();
    let submission = fixture.submit(&author.token, package("notice-example", "1.0.0", "Notice Author"));
    fixture.approve(&reviewer.token, &submission);
    fixture.publish(&author.token, &submission);
    fixture.market.inner.store.withdraw(&fixture.market.inner, &author.token,
        &submission.submission_id, &withdrawal_request(&submission)).unwrap();
    let notice = fixture.market.inner.store.withdrawal(&fixture.market.inner, "notice-example", "1.0.0").unwrap();
    let connection = fixture.database();
    connection.execute_batch("DROP TRIGGER withdrawals_no_update;").unwrap();
    let mut mismatched = notice.clone();
    mismatched.digest = "00".repeat(32);
    let signer = Ed25519KeyPair::from_pkcs8(&fixture.private_key).unwrap();
    let payload = crate::plugins::withdrawal_payload(&mismatched).unwrap();
    mismatched.signature = auth::encode_hex(signer.sign(&payload).as_ref());
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &fixture.public_key)
        .verify(&payload, &auth::decode_hex(&mismatched.signature, 64).unwrap()).unwrap();
    connection.execute("UPDATE withdrawals SET digest=?1,signature=?2 WHERE plugin_id=?3",
        params![mismatched.digest, mismatched.signature, notice.id]).unwrap();
    assert_error(fixture.market.inner.store.withdrawal(&fixture.market.inner, "notice-example", "1.0.0"), MarketplaceError::Unavailable);
    connection.execute("UPDATE withdrawals SET digest=?1,signature=?2 WHERE plugin_id=?3",
        params![notice.digest, notice.signature, notice.id]).unwrap();
    assert!(fixture.market.inner.store.withdrawal(&fixture.market.inner, "notice-example", "1.0.0").is_ok());
    connection.execute_batch("DROP TRIGGER submissions_immutable; DROP TRIGGER submissions_transitions;").unwrap();
    connection.execute("UPDATE submissions SET approved_version='9.0.0' WHERE id=?1", [&submission.submission_id]).unwrap();
    assert_error(fixture.market.inner.store.withdrawal(&fixture.market.inner, "notice-example", "1.0.0"), MarketplaceError::Unavailable);
    connection.execute("UPDATE submissions SET approved_version='1.0.0' WHERE id=?1", [&submission.submission_id]).unwrap();
    let unrelated = fixture.submit(&author.token, package("unrelated-notice", "1.0.0", "Notice Author"));
    fixture.market.inner.store.withdraw(&fixture.market.inner, &author.token,
        &unrelated.submission_id, &withdrawal_request(&unrelated)).unwrap();
    connection.execute_batch("DROP TRIGGER releases_immutable;").unwrap();
    connection.execute("UPDATE releases SET submission_id=?1 WHERE plugin_id='notice-example'", [&unrelated.submission_id]).unwrap();
    assert_error(fixture.market.inner.store.withdrawal(&fixture.market.inner, "notice-example", "1.0.0"), MarketplaceError::Unavailable);
}

#[test]
fn every_permission_dimension_expansion_requires_new_independent_review() {
    let fixture = Fixture::new();
    let author = fixture.register("permission_author", "Permission Author");
    let reviewer = fixture.reviewer();
    let scopes = [
        PluginPermissions { network: vec!["https://example.com".to_owned()], ..Default::default() },
        PluginPermissions { read_paths: vec!["package:tools.json".to_owned()], ..Default::default() },
        PluginPermissions { write_paths: vec!["package:tools.json".to_owned()], ..Default::default() },
        PluginPermissions { process: true, ..Default::default() },
        PluginPermissions { credentials: vec!["market-test-credential".to_owned()], ..Default::default() },
    ];
    for (index, permissions) in scopes.into_iter().enumerate() {
        let plugin_id = format!("permission-example-{index}");
        let initial = fixture.submit(&author.token, network_package(&plugin_id, "1.0.0", "Permission Author", false));
        fixture.approve(&reviewer.token, &initial);
        fixture.publish(&author.token, &initial);
        let mut expanded = network_package(&plugin_id, "1.1.0", "Permission Author", false);
        expanded.manifest.permissions = permissions.clone();
        let update = fixture.market.inner.store.submit(&fixture.market.inner, &author.token, SubmitRequest {
            package: expanded, replaces_submission_id: Some(initial.submission_id.clone()),
        }).unwrap();
        assert_eq!(update.status, SubmissionStatus::Submitted);
        assert_eq!(update.permission_changes, permissions);
        assert_eq!(update.previous_published_version.as_deref(), Some("1.0.0"));
        assert_error(fixture.market.inner.store.publish(&fixture.market.inner, &author.token,
            &update.submission_id, &expected(&update)), MarketplaceError::Conflict);
        assert_error(fixture.market.inner.store.download(&fixture.market.inner, &plugin_id, "1.1.0"), MarketplaceError::NotFound);
        fixture.approve(&reviewer.token, &update);
        verify_fixture_signature(&fixture, &fixture.publish(&author.token, &update));
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_identity_and_all_upload_fields_block_secrets_without_echo_or_persistence() {
    let fixture = Fixture::new();
    let api = Api::start(fixture.market.clone()).await;
    let author = api.register("privacy_author", "Privacy Author").await;
    let secret = "sk-proj-syntheticprivatevalue12345";
    let identity = api.request("GET", "/v1/auth/me", Some(&author), None).await.1;
    let user_id = identity["userId"].as_str().unwrap();
    let original = serde_json::to_value(package("private-example", "1.0.0", "Privacy Author")).unwrap();
    let paths = [
        "/manifest/id", "/manifest/name", "/manifest/description", "/manifest/author", "/manifest/version",
        "/manifest/minimumHostVersion", "/manifest/kind", "/manifest/entry", "/manifest/clients/0",
        "/files/SKILL.md",
    ];
    let mut candidates = Vec::new();
    for path in paths {
        let mut candidate = original.clone();
        *candidate.pointer_mut(path).unwrap() = Value::String(secret.to_owned());
        candidates.push(candidate);
    }
    for key in ["network", "readPaths", "writePaths", "credentials"] {
        let mut candidate = original.clone();
        candidate["manifest"]["permissions"][key] = json!([secret]);
        candidates.push(candidate);
    }
    let mut dependency = original.clone();
    dependency["manifest"]["dependencies"] = json!([secret]);
    candidates.push(dependency);
    let mut filename = original.clone();
    filename["files"].as_object_mut().unwrap().insert(format!("{secret}.md"), Value::String("Public text".to_owned()));
    candidates.push(filename);
    for private in [author.as_str(), user_id] {
        let mut candidate = original.clone();
        candidate["manifest"]["description"] = Value::String(private.to_owned());
        candidates.push(candidate);
    }
    for encoded in [
        format!("{{\"accessToken\":\"{secret}\"}}"),
        secret.bytes().map(|byte| format!("%{byte:02x}")).collect::<String>(),
        secret.bytes().map(|byte| format!("\\u{byte:04x}")).collect::<String>(),
        auth::encode_hex(secret.as_bytes()),
        "c2stcHJvai1zeW50aGV0aWNwcml2YXRldmFsdWUxMjM0NQ==".to_owned(),
    ] {
        let mut candidate = original.clone();
        candidate["files"]["SKILL.md"] = Value::String(encoded);
        candidates.push(candidate);
    }
    for candidate in candidates {
        let (status, error) = api.request("POST", "/v1/submissions", Some(&author), Some(json!({"package":candidate}))).await;
        assert!(matches!(status, 400 | 422), "unexpected privacy status: {status}");
        let text = error.to_string();
        for private in [secret, author.as_str(), user_id] { assert!(!text.contains(private)); }
        assert!(matches!(error["error"]["code"].as_str(), Some("invalid_request" | "privacy_rejected")));
    }
    let (status, _) = api.request("POST", "/v1/auth/register", None,
        Some(json!({"username":"fake_reviewer","displayName":"Public Name","password":TEST_PASSWORD,"reviewer":true}))).await;
    assert_eq!(status, 400);
    let (status, _) = api.request("POST", "/v1/auth/register", None,
        Some(json!({"username":REVIEWER_USERNAME,"displayName":REVIEWER_NAME,"password":TEST_PASSWORD}))).await;
    assert_eq!(status, 409);
    let connection = fixture.database();
    assert_eq!(connection.query_row("SELECT count(*) FROM submissions", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(connection.query_row("SELECT count(*) FROM plugins", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    assert_eq!(connection.query_row("SELECT count(*) FROM audit WHERE action='submitted'", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    let stored_hash: Vec<u8> = connection.query_row("SELECT token_hash FROM sessions WHERE user_id=?1", [user_id], |row| row.get(0)).unwrap();
    assert_eq!(stored_hash, auth::token_hash(&author).unwrap());
    assert_ne!(stored_hash, author.as_bytes());
    let stored_password: String = connection.query_row("SELECT password_hash FROM users WHERE id=?1", [user_id], |row| row.get(0)).unwrap();
    assert!(!stored_password.contains(TEST_PASSWORD));
}

#[test]
fn public_multiclient_tool_metadata_survives_sorted_json_and_submission() {
    let fixture = Fixture::new();
    let author = fixture.register("metadata_author", "QA Plugin Author");
    let mut candidate = network_package("qa.reviewed-echo", "1.0.0", "QA Plugin Author", false);
    candidate.manifest.name = "QA reviewed echo".to_owned();
    candidate.manifest.description = "A reviewed local echo tool.".to_owned();
    candidate.manifest.clients = ["codex-desktop", "codex-cli", "claude-desktop", "claude-code",
        "opencode-desktop", "opencode-cli", "gemini-cli"].map(str::to_owned).to_vec();
    let value = serde_json::to_value(&candidate).unwrap();
    privacy::inspect_package(&value).unwrap();
    privacy::inspect_text(&value["manifest"].to_string()).unwrap();
    let submitted = fixture.submit(&author.token, candidate.clone());
    assert_eq!(submitted.status, SubmissionStatus::Submitted);
    assert_eq!(serde_json::to_value(&submitted.package).unwrap(), value);
}

#[test]
fn aggregate_scanning_keeps_opaque_and_fragmented_secrets_private() {
    let mut candidate = package("encoded-example", "1.0.0", "Example Author");
    candidate.files.insert("SKILL.md".to_owned(), "Q".repeat(128));
    assert_error(privacy::inspect_package(&serde_json::to_value(&candidate).unwrap()), MarketplaceError::PrivacyRejected);
    for fragments in [json!(["c2stcHJv", "ai1zeW50aGV0aWNwcml2YXRldmFsdWUxMjM0NQ=="]),
        json!(["qa-current-", "market-bearer"])] {
        candidate.files.insert("SKILL.md".to_owned(), fragments.to_string());
        assert_error(privacy::inspect_package_with_secrets(&serde_json::to_value(&candidate).unwrap(),
            &["qa-current-market-bearer"]), MarketplaceError::PrivacyRejected);
    }
}

#[test]
fn pinned_dependencies_remain_public_metadata_and_cannot_hide_known_secrets() {
    let fixture = Fixture::new();
    let author = fixture.register("dependency_author", "Dependency Author");
    let mut candidate = package("dependent-example", "1.0.0", "Dependency Author");
    candidate.manifest.dependencies = vec!["another-plugin@1.2.0".to_owned(), "prerelease-plugin@2.0.0-beta.1+build7".to_owned()];
    let value = serde_json::to_value(&candidate).unwrap();
    privacy::inspect_package(&value).unwrap();
    validate_package(&candidate).unwrap_or_else(|failure| {
        panic!("shared plugin validator rejected pinned dependency metadata: {}", failure.code)
    });
    let submitted = fixture.submit(&author.token, candidate.clone());
    assert_eq!(submitted.status, SubmissionStatus::Submitted);
    assert_eq!(submitted.package.manifest.dependencies, candidate.manifest.dependencies);
    assert_eq!(submitted.digest, package_digest(&candidate).unwrap());
    assert_error(privacy::inspect_package_with_secrets(&value, &["another-plugin@1.2.0"]), MarketplaceError::PrivacyRejected);
    candidate.manifest.dependencies[0] = format!("{}@1.2.0", author.token);
    assert_error(privacy::inspect_package_with_secrets(&serde_json::to_value(candidate).unwrap(), &[&author.token]), MarketplaceError::PrivacyRejected);
}

#[test]
fn configured_signing_key_ids_match_the_native_installation_contract() {
    let fixture = Fixture::new();
    for key_id in ["publisher", "publisher-2026", "publisher.release"] {
        let mut config = fixture.config();
        config.signing_key_id = key_id.to_owned();
        config.validate().unwrap();
        crate::plugins::validation::validate_id(key_id).unwrap();
    }
    for key_id in ["", "Publisher", "2026-publisher", "publisher_key", "publisher-", ".publisher", "publisher..release"] {
        let mut config = fixture.config();
        config.signing_key_id = key_id.to_owned();
        assert_error(config.validate(), MarketplaceError::InvalidConfiguration);
        let mut config = fixture.config();
        config.verification_keys.insert(key_id.to_owned(), fixture.public_key.clone());
        assert_error(config.validate(), MarketplaceError::InvalidConfiguration);
    }
}

#[test]
fn offline_state_verification_checks_real_release_and_withdrawal_signatures() {
    let fixture = Fixture::new();
    let author = fixture.register("offline_author", "Offline Author");
    let reviewer = fixture.reviewer();
    let submission = fixture.submit(&author.token, package("offline-example", "1.0.0", "Offline Author"));
    fixture.market.verify_state().unwrap();
    fixture.approve(&reviewer.token, &submission);
    let published = fixture.publish(&author.token, &submission);
    fixture.market.verify_state().unwrap();
    let connection = fixture.database();
    let restore_trigger = |name: &str| {
        let expected = Connection::open_in_memory().unwrap();
        expected.execute_batch(schema::SQL).unwrap();
        let sql: String = expected.query_row("SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1", [name], |row| row.get(0)).unwrap();
        connection.execute_batch(&sql).unwrap();
    };
    connection.execute_batch("DROP TRIGGER releases_immutable;").unwrap();
    assert_error(fixture.market.verify_state(), MarketplaceError::Unavailable);
    connection.execute("UPDATE releases SET signature=?1 WHERE submission_id=?2",
        params!["00".repeat(64), submission.submission_id]).unwrap();
    restore_trigger("releases_immutable");
    assert_error(fixture.market.verify_state(), MarketplaceError::Unavailable);
    connection.execute_batch("DROP TRIGGER releases_immutable;").unwrap();
    connection.execute("UPDATE releases SET signature=?1 WHERE submission_id=?2",
        params![published.signature, submission.submission_id]).unwrap();
    restore_trigger("releases_immutable");
    fixture.market.verify_state().unwrap();
    fixture.market.inner.store.withdraw(&fixture.market.inner, &author.token,
        &submission.submission_id, &withdrawal_request(&submission)).unwrap();
    fixture.market.verify_state().unwrap();
    let notice = fixture.market.inner.store.withdrawal(&fixture.market.inner, "offline-example", "1.0.0").unwrap();
    connection.execute_batch("DROP TRIGGER withdrawals_no_update;").unwrap();
    connection.execute("UPDATE withdrawals SET signature=?1 WHERE plugin_id=?2",
        params!["00".repeat(64), notice.id]).unwrap();
    restore_trigger("withdrawals_no_update");
    assert_error(fixture.market.verify_state(), MarketplaceError::Unavailable);
    connection.execute_batch("DROP TRIGGER withdrawals_no_update;").unwrap();
    connection.execute("UPDATE withdrawals SET signature=?1 WHERE plugin_id=?2",
        params![notice.signature, notice.id]).unwrap();
    restore_trigger("withdrawals_no_update");
    fixture.market.verify_state().unwrap();
}

#[test]
fn reports_require_independent_review_and_private_notes_are_not_persisted() {
    let fixture = Fixture::new();
    let author = fixture.register("report_author", "Report Author");
    let reporter = fixture.register("report_reader", "Report Reader");
    let reviewer = fixture.reviewer();
    let submission = fixture.submit(&author.token, package("report-example", "1.0.0", "Report Author"));
    fixture.approve(&reviewer.token, &submission);
    fixture.publish(&author.token, &submission);
    let request = ReportRequest { reason: "privacy".to_owned(), note: "Please review the published text.".to_owned() };
    let report = fixture.market.inner.store.report(&reporter.token, "report-example", "1.0.0", &request).unwrap();
    let query = ListQuery { limit: None, offset: None, status: None };
    assert_error(fixture.market.inner.store.reports(&author.token, &query), MarketplaceError::Forbidden);
    assert_error(fixture.market.inner.store.reports(&reporter.token, &query), MarketplaceError::Forbidden);
    let decision = ResolveReportRequest { resolution: "resolved".to_owned(), note: "Reviewed the report and requested withdrawal.".to_owned() };
    assert_error(fixture.market.inner.store.resolve_report(&author.token, &report.report_id, &decision), MarketplaceError::Forbidden);
    let resolved = fixture.market.inner.store.resolve_report(&reviewer.token, &report.report_id, &decision).unwrap();
    assert_eq!(resolved.status, "resolved");
    assert_error(fixture.market.inner.store.resolve_report(&reviewer.token, &report.report_id, &decision), MarketplaceError::Conflict);
    let own = fixture.market.inner.store.report(&reviewer.token, "report-example", "1.0.0", &request).unwrap();
    assert_error(fixture.market.inner.store.resolve_report(&reviewer.token, &own.report_id, &decision), MarketplaceError::Forbidden);
    let private = ReportRequest { reason: "privacy".to_owned(), note: reporter.token.clone() };
    assert_error(fixture.market.inner.store.report(&reporter.token, "report-example", "1.0.0", &private), MarketplaceError::PrivacyRejected);
    let reports = fixture.market.inner.store.reports(&reviewer.token, &query).unwrap();
    assert_eq!(reports.items.len(), 1);
    assert_eq!(reports.items[0].reporter_display_name, REVIEWER_NAME);
    let text = serde_json::to_string(&reports).unwrap();
    for private in ["report_author", "report_reader", "username", "userId", reporter.token.as_str()] {
        assert!(!text.contains(private));
    }
    assert_eq!(fixture.database().query_row("SELECT count(*) FROM reports", [], |row| row.get::<_, i64>(0)).unwrap(), 2);
    assert_eq!(fixture.market.inner.store.audit(&author.token, Some(&submission.submission_id), &query).unwrap()
        .items.iter().filter(|event| event.action == "report_resolved").count(), 1);
}

#[test]
fn published_packages_verify_with_independent_native_trust_and_reject_tampering() {
    let fixture = Fixture::new();
    let author = fixture.register("integrity_author", "Integrity Author");
    let reviewer = fixture.reviewer();
    let submission = fixture.submit(&author.token, package("integrity-example", "1.0.0", "Integrity Author"));
    fixture.approve(&reviewer.token, &submission);
    let published = fixture.publish(&author.token, &submission);
    let trusted = crate::plugins::TrustedSigningKey {
        id: published.signing_key_id.clone(), label: "Independent test trust".to_owned(),
        public_key: auth::encode_hex(&fixture.public_key),
        fingerprint: auth::encode_hex(&Sha256::digest(&fixture.public_key)), enabled: true,
    };
    let verified = crate::plugins::validation::verify_published(&published, &trusted).unwrap();
    assert_eq!(verified.digest, submission.digest);
    let mut disabled = trusted.clone();
    disabled.enabled = false;
    assert_eq!(crate::plugins::validation::verify_published(&published, &disabled).unwrap_err().code, "plugin_untrusted_key");
    let mut untrusted = trusted.clone();
    untrusted.id = "different-publisher".to_owned();
    assert_eq!(crate::plugins::validation::verify_published(&published, &untrusted).unwrap_err().code, "plugin_untrusted_key");
    let mut changed = published.clone();
    changed.package.files.get_mut("SKILL.md").unwrap().push_str("Changed public instructions.\n");
    assert_eq!(crate::plugins::validation::verify_published(&changed, &trusted).unwrap_err().code, "plugin_digest");
    changed.digest = package_digest(&changed.package).unwrap();
    assert_eq!(crate::plugins::validation::verify_published(&changed, &trusted).unwrap_err().code, "plugin_signature");
    let mut changed = published.clone();
    changed.package.manifest.version = "1.0.1".to_owned();
    changed.digest = package_digest(&changed.package).unwrap();
    assert_eq!(crate::plugins::validation::verify_published(&changed, &trusted).unwrap_err().code, "plugin_signature");
    let mut changed = published.clone();
    changed.signature = "00".repeat(64);
    assert_eq!(crate::plugins::validation::verify_published(&changed, &trusted).unwrap_err().code, "plugin_signature");
}
