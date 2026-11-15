use super::*;
use crate::database::{ProviderAdvancedConfig, ProviderModelMapping};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

const REAL_MODEL: &str = "local-upstream-model";
const VIRTUAL_MODEL: &str = "xy-group/offline-fixture";
const JSON_ROUTES: [&str; 6] = [
    "/v1/chat/completions",
    "/chat/completions",
    "/v1/responses",
    "/responses",
    "/v1/messages",
    "/messages",
];

struct GateKeys {
    forbid_access: AtomicBool,
    reads: AtomicUsize,
    pool_accesses: AtomicUsize,
    pool: Option<AccountPool>,
}

impl ApiKeyProvider for GateKeys {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        assert!(
            !self.forbid_access.load(Ordering::SeqCst),
            "reserved model reached provider credentials"
        );
        Ok(Some("local-fixture-key".into()))
    }

    fn account_pool(&self) -> Option<&AccountPool> {
        self.pool_accesses.fetch_add(1, Ordering::SeqCst);
        assert!(
            !self.forbid_access.load(Ordering::SeqCst),
            "reserved model reached account selection"
        );
        self.pool.as_ref()
    }
}

struct LocalServer {
    address: std::net::SocketAddr,
    task: tokio::task::JoinHandle<()>,
}

impl Drop for LocalServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Clone, Default)]
struct LocalUpstream {
    calls: Arc<AtomicUsize>,
    failures: Arc<AtomicUsize>,
    received: Arc<Mutex<Vec<(String, Value)>>>,
}

async fn local_upstream(
    State(state): State<LocalUpstream>,
    request: axum::extract::Request,
) -> Response {
    state.calls.fetch_add(1, Ordering::SeqCst);
    let path = request.uri().path().to_owned();
    let bytes = axum::body::to_bytes(request.into_body(), MAX_REQUEST_BYTES)
        .await
        .unwrap();
    let payload: Value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    state.received.lock().unwrap().push((path, payload.clone()));
    if state
        .failures
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |remaining| {
            remaining.checked_sub(1)
        })
        .is_ok()
    {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"local retry fixture"})),
        )
            .into_response();
    }
    Json(json!({
        "id":"local-reply", "object":"chat.completion", "model":payload["model"],
        "choices":[{"index":0,"message":{"role":"assistant","content":"local success"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}
    })).into_response()
}

fn local_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

#[derive(Debug, PartialEq, Eq)]
struct GateSnapshot {
    credential_reads: usize,
    account_pool_accesses: usize,
    upstream_calls: usize,
    database_changes: i64,
    rows: Vec<(&'static str, i64)>,
}

struct Fixture {
    database: Database,
    keys: Arc<GateKeys>,
    upstream: LocalUpstream,
    upstream_server: LocalServer,
    proxy: LocalServer,
    client: reqwest::Client,
}

impl Fixture {
    async fn new(pooled: bool) -> Self {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        for provider in database.list_providers().unwrap() {
            database.delete_provider(&provider.id).unwrap();
        }
        let upstream = LocalUpstream::default();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .fallback(local_upstream)
            .with_state(upstream.clone());
        let upstream_server = LocalServer {
            address,
            task: tokio::spawn(async move { axum::serve(listener, router).await.unwrap() }),
        };
        let keys = Arc::new(GateKeys {
            forbid_access: AtomicBool::new(true),
            reads: AtomicUsize::new(0),
            pool_accesses: AtomicUsize::new(0),
            pool: pooled.then(|| AccountPool::test_pool(database.clone()).unwrap()),
        });
        let client = local_client();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = ProxyState {
            database: database.clone(),
            client: client.clone(),
            api_keys: keys.clone(),
            local_addr: Some(address),
        };
        let proxy = LocalServer {
            address,
            task: tokio::spawn(async move {
                axum::serve(listener, router_with_state(state))
                    .await
                    .unwrap()
            }),
        };
        let fixture = Self {
            database,
            keys,
            upstream,
            upstream_server,
            proxy,
            client,
        };
        fixture.add_provider("primary", 0);
        fixture.add_provider("fallback", 1);
        fixture.database.set_current_provider("primary").unwrap();
        fixture
    }

    fn add_provider(&self, id: &str, sort_order: i64) -> Provider {
        let provider = Provider {
            id: id.into(),
            name: id.into(),
            provider_type: "openai_compatible".into(),
            endpoint: format!("http://{}/{id}", self.upstream_server.address),
            model: REAL_MODEL.into(),
            app_group: "Codex".into(),
            enabled: true,
            sort_order,
            advanced: ProviderAdvancedConfig {
                upstream_format: "openai_chat".into(),
                ..Default::default()
            },
        };
        self.database.save_provider(&provider).unwrap();
        provider
    }

    fn enable_budget(&self) {
        let policy = crate::governance::budget::BudgetPolicy {
            enabled: true,
            input_reservation: 1024,
            max_output_tokens: 16,
            ..Default::default()
        };
        crate::governance::save(&self.database, crate::governance::budget::KEY, &policy).unwrap();
        crate::model_pricing::save_on(
            &self.database.conn().lock().unwrap(),
            &crate::model_pricing::PricingInput {
                id: REAL_MODEL.into(),
                input_per_million: 1.0,
                output_per_million: 2.0,
                cache_read_per_million: None,
                cache_creation_per_million: None,
            },
        )
        .unwrap();
    }

    fn snapshot(&self) -> GateSnapshot {
        let connection = self.database.conn();
        let connection = connection.lock().unwrap();
        let rows = [
            "proxy_tasks",
            "proxy_attempts",
            "budget_ledger",
            "request_events",
            "governance_events",
            "synthetic_runs",
            "provider_health",
        ]
        .into_iter()
        .map(|table| {
            (
                table,
                connection
                    .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                        row.get(0)
                    })
                    .unwrap(),
            )
        })
        .collect();
        GateSnapshot {
            credential_reads: self.keys.reads.load(Ordering::SeqCst),
            account_pool_accesses: self.keys.pool_accesses.load(Ordering::SeqCst),
            upstream_calls: self.upstream.calls.load(Ordering::SeqCst),
            database_changes: connection
                .query_row("SELECT total_changes()", [], |row| row.get(0))
                .unwrap(),
            rows,
        }
    }

    fn request(&self, route: &str, payload: &Value) -> reqwest::RequestBuilder {
        self.client
            .post(format!("http://{}{route}", self.proxy.address))
            .json(payload)
    }

    async fn assert_blocked(&self, request: reqwest::RequestBuilder) {
        let before = self.snapshot();
        assert_eq!(
            (
                before.credential_reads,
                before.account_pool_accesses,
                before.upstream_calls
            ),
            (0, 0, 0)
        );
        for (table, rows) in &before.rows {
            if matches!(
                *table,
                "proxy_tasks" | "proxy_attempts" | "budget_ledger" | "request_events"
            ) {
                assert_eq!(*rows, 0, "fixture must begin with an empty {table}");
            }
        }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response.json::<Value>().await.unwrap(),
            json!({"error":{
                "type":"channel_group_pilot_only",
                "message":"Channel group virtual models are available for offline pilot simulation only."
            }})
        );
        assert_eq!(
            self.snapshot(),
            before,
            "offline gate must not access credentials, dispatch, or write to SQLite"
        );
    }

    async fn assert_success(&self, route: &str, model: Option<&str>, diagnostic: bool) {
        let before = self.upstream.calls.load(Ordering::SeqCst);
        let mut request = self
            .request(route, &client_payload(route, model, false))
            .header("x-xy-task-id", uuid::Uuid::new_v4().to_string());
        if diagnostic {
            request = request.header("x-xy-diagnostic-provider", "primary");
        }
        let response = request.send().await.unwrap();
        let status = response.status();
        let bytes = response.bytes().await.unwrap();
        assert_eq!(
            status,
            StatusCode::OK,
            "{}",
            String::from_utf8_lossy(&bytes)
        );
        assert_eq!(self.upstream.calls.load(Ordering::SeqCst), before + 1);
        assert_eq!(
            self.upstream.received.lock().unwrap().last().unwrap().1["model"],
            REAL_MODEL
        );
    }

    fn save_legacy_body_model(&self, id: &str, model: &str) {
        let mut provider = self.database.get_provider(id).unwrap().unwrap();
        provider
            .advanced
            .body_overrides
            .insert("model".into(), json!(model));
        self.database
            .conn()
            .lock()
            .unwrap()
            .execute(
                "UPDATE providers SET advanced_json=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&provider.advanced).unwrap(), id],
            )
            .unwrap();
    }
}

fn client_payload(route: &str, model: Option<&str>, stream: bool) -> Value {
    let mut payload = if route.contains("/responses") {
        json!({"input":"local fixture", "max_output_tokens":8})
    } else if route.contains("/models/") {
        json!({"contents":[{"role":"user","parts":[{"text":"local fixture"}]}], "generationConfig":{"maxOutputTokens":8}})
    } else {
        json!({"messages":[{"role":"user","content":"local fixture"}], "max_tokens":8})
    };
    if let Some(model) = model {
        payload["model"] = json!(model);
    }
    payload["stream"] = json!(stream);
    payload
}

fn mapping(model: &str, display_name: &str) -> ProviderModelMapping {
    ProviderModelMapping {
        model: model.into(),
        display_name: display_name.into(),
        context_window: None,
    }
}

#[tokio::test]
async fn channel_group_json_models_reject_before_credentials_tasks_and_dispatch() {
    let fixture = Fixture::new(false).await;
    fixture.enable_budget();
    for route in JSON_ROUTES {
        for model in [VIRTUAL_MODEL, " \txy-group/unknown \n", "xy-group/"] {
            for stream in [false, true] {
                fixture
                    .assert_blocked(
                        fixture.request(route, &client_payload(route, Some(model), stream)),
                    )
                    .await;
            }
        }
    }
}

#[tokio::test]
async fn channel_group_gemini_url_and_body_reject_before_operation_validation() {
    let fixture = Fixture::new(false).await;
    fixture.enable_budget();
    for operation in [
        "xy-group/unknown:generateContent",
        "xy-group/unknown:streamGenerateContent?alt=sse",
        "xy-group%2Funknown:generateContent",
        "%78%79-group%2funknown:streamGenerateContent",
        "%20xy-group%2Funknown%20:generateContent",
        "xy-group/:generateContent",
        "xy-group/unknown:countTokens",
        "xy-group/unknown",
    ] {
        let route = format!("/v1beta/models/{operation}");
        fixture
            .assert_blocked(
                fixture.request(&route, &client_payload(&route, Some(REAL_MODEL), false)),
            )
            .await;
    }
    let route = "/v1beta/models/ordinary:generateContent";
    fixture
        .assert_blocked(fixture.request(route, &client_payload(route, Some(VIRTUAL_MODEL), false)))
        .await;
    fixture
        .assert_blocked(
            fixture
                .client
                .post(format!(
                    "http://{}/v1beta/models/xy-group/unknown:generateContent",
                    fixture.proxy.address
                ))
                .header(header::CONTENT_TYPE, "application/json")
                .body("{"),
        )
        .await;
}

#[tokio::test]
async fn channel_group_diagnostic_header_cannot_bypass_reserved_namespace() {
    let fixture = Fixture::new(false).await;
    fixture.enable_budget();
    let mut primary = fixture.database.get_provider("primary").unwrap().unwrap();
    primary
        .advanced
        .model_mappings
        .push(mapping(REAL_MODEL, VIRTUAL_MODEL));
    fixture.database.save_provider(&primary).unwrap();
    for target in ["primary", "fallback", "missing-provider"] {
        for route in JSON_ROUTES {
            fixture
                .assert_blocked(
                    fixture
                        .request(route, &client_payload(route, Some(VIRTUAL_MODEL), false))
                        .header("x-xy-diagnostic-provider", target),
                )
                .await;
        }
        let route = "/v1beta/models/xy-group/offline-fixture:streamGenerateContent";
        fixture
            .assert_blocked(
                fixture
                    .request(route, &client_payload(route, Some(REAL_MODEL), true))
                    .header("x-xy-diagnostic-provider", target),
            )
            .await;
    }
}

#[tokio::test]
async fn channel_group_effective_models_reject_before_primary_or_fallback_credentials() {
    for candidate in ["primary", "fallback"] {
        for injection in [
            "default",
            "default-empty",
            "default-unmapped",
            "alias",
            "mapping-id",
            "body-override",
        ] {
            let fixture = Fixture::new(false).await;
            fixture.enable_budget();
            fixture.upstream.failures.store(3, Ordering::SeqCst);
            let mut provider = fixture.database.get_provider(candidate).unwrap().unwrap();
            let requested = match injection {
                "default" => {
                    provider.model = VIRTUAL_MODEL.into();
                    None
                }
                "default-empty" => {
                    provider.model = VIRTUAL_MODEL.into();
                    Some("")
                }
                "default-unmapped" => {
                    provider.model = VIRTUAL_MODEL.into();
                    Some(REAL_MODEL)
                }
                "alias" => {
                    provider
                        .advanced
                        .model_mappings
                        .push(mapping(VIRTUAL_MODEL, "offline alias"));
                    Some("OFFLINE ALIAS")
                }
                "mapping-id" => {
                    provider
                        .advanced
                        .model_mappings
                        .push(mapping(VIRTUAL_MODEL, "offline alias"));
                    Some("XY-GROUP/OFFLINE-FIXTURE")
                }
                "body-override" => Some(REAL_MODEL),
                _ => unreachable!(),
            };
            fixture.database.save_provider(&provider).unwrap();
            if injection == "body-override" {
                fixture.save_legacy_body_model(candidate, " \txy-group/legacy-override ");
            }
            for route in JSON_ROUTES
                .into_iter()
                .chain(["/v1beta/models/ordinary:generateContent"])
            {
                fixture
                    .assert_blocked(
                        fixture
                            .request(route, &client_payload(route, requested, false))
                            .header("x-xy-task-id", "offline-candidate"),
                    )
                    .await;
            }
        }
    }
}

#[tokio::test]
async fn channel_group_gate_is_independent_of_group_existence_enabled_state_and_store() {
    use crate::channel_groups::types::{
        Channel, ChannelGroup, CooldownPolicy, Limits, SaveGroup, Strategy,
    };
    for enabled in [false, true] {
        let fixture = Fixture::new(false).await;
        let snapshot = crate::channel_groups::save(
            &fixture.database,
            &[],
            SaveGroup {
                group: ChannelGroup {
                    id: "offline-fixture".into(),
                    name: "Offline fixture".into(),
                    virtual_model: VIRTUAL_MODEL.into(),
                    enabled,
                    strategy: Strategy::RoundRobin,
                    project_id: None,
                    affinity_ttl_seconds: 300,
                    channels: vec![Channel {
                        id: "offline-channel".into(),
                        provider_id: "primary".into(),
                        account_id: None,
                        upstream_model: REAL_MODEL.into(),
                        enabled: true,
                        priority: 0,
                        weight: 1,
                        quota_pool_id: None,
                        quota_independent: false,
                        limits: Limits::default(),
                        cooldown: CooldownPolicy::default(),
                    }],
                    revision: 0,
                },
                project: None,
            },
        )
        .unwrap();
        assert!(!snapshot.live_enabled);
        assert!(snapshot.billing_hold);
        for model in [VIRTUAL_MODEL, "xy-group/not-configured"] {
            let route = "/v1/chat/completions";
            fixture
                .assert_blocked(fixture.request(route, &client_payload(route, Some(model), false)))
                .await;
        }
    }
    let fixture = Fixture::new(false).await;
    fixture
        .database
        .set_setting("channel_groups.pilot.v1", "not JSON")
        .unwrap();
    let route = "/v1/responses";
    fixture
        .assert_blocked(fixture.request(route, &client_payload(route, Some(VIRTUAL_MODEL), false)))
        .await;
}

#[tokio::test]
async fn channel_group_gate_precedes_missing_provider_disabled_provider_and_task_identity() {
    let fixture = Fixture::new(false).await;
    fixture.enable_budget();
    let route = "/v1/responses";
    fixture
        .assert_blocked(
            fixture
                .request(route, &client_payload(route, Some(VIRTUAL_MODEL), false))
                .header("x-xy-switch-loopback", "1"),
        )
        .await;
    fixture
        .assert_blocked(
            fixture
                .request(route, &client_payload(route, Some(VIRTUAL_MODEL), false))
                .header("x-xy-task-id", "one-task")
                .header("idempotency-key", "another-task"),
        )
        .await;
    let mut primary = fixture.database.get_provider("primary").unwrap().unwrap();
    primary.enabled = false;
    fixture.database.save_provider(&primary).unwrap();
    fixture
        .assert_blocked(fixture.request(route, &client_payload(route, Some(VIRTUAL_MODEL), false)))
        .await;
    for provider in fixture.database.list_providers().unwrap() {
        fixture.database.delete_provider(&provider.id).unwrap();
    }
    fixture
        .assert_blocked(fixture.request(route, &client_payload(route, Some(VIRTUAL_MODEL), false)))
        .await;
}

#[tokio::test]
async fn channel_group_gate_never_enters_configured_account_pool_or_refresh() {
    use crate::accounts::{AccountCredentialInput, AccountSecret, AddAccountRequest};
    let fixture = Fixture::new(true).await;
    fixture.enable_budget();
    let pool = fixture.keys.pool.as_ref().unwrap();
    pool.add(AddAccountRequest {
        alias: "expired local fixture".into(),
        provider_id: "primary".into(),
        approved_destination: Some(
            fixture
                .database
                .get_provider("primary")
                .unwrap()
                .unwrap()
                .endpoint,
        ),
        credentials: AccountCredentialInput::OAuth {
            access_token: AccountSecret::new("local-expired-access".into()).unwrap(),
            refresh_token: Some(AccountSecret::new("local-refresh-token".into()).unwrap()),
            expires_at: Some(1),
        },
        available_models: vec![],
        allow_refresh: true,
        confirmed: true,
    })
    .unwrap();
    assert!(pool.is_configured("primary").unwrap());
    let route = "/v1/chat/completions";
    fixture
        .assert_blocked(fixture.request(route, &client_payload(route, Some(VIRTUAL_MODEL), false)))
        .await;
    let mut provider = fixture.database.get_provider("primary").unwrap().unwrap();
    provider
        .advanced
        .model_mappings
        .push(mapping(VIRTUAL_MODEL, "pooled alias"));
    fixture.database.save_provider(&provider).unwrap();
    fixture
        .assert_blocked(
            fixture
                .request(route, &client_payload(route, Some("pooled alias"), false))
                .header("x-xy-task-id", "pooled-offline"),
        )
        .await;
}

#[tokio::test]
async fn channel_group_gate_preserves_ordinary_protocols_and_unused_virtual_configuration() {
    let fixture = Fixture::new(false).await;
    fixture.enable_budget();
    fixture.keys.forbid_access.store(false, Ordering::SeqCst);
    let mut primary = fixture.database.get_provider("primary").unwrap().unwrap();
    primary
        .advanced
        .model_mappings
        .push(mapping(REAL_MODEL, "ordinary alias"));
    primary
        .advanced
        .model_mappings
        .push(mapping(VIRTUAL_MODEL, "unused virtual alias"));
    fixture.database.save_provider(&primary).unwrap();
    let mut disabled = fixture.database.get_provider("fallback").unwrap().unwrap();
    disabled.model = VIRTUAL_MODEL.into();
    disabled.enabled = false;
    fixture.database.save_provider(&disabled).unwrap();
    for route in JSON_ROUTES
        .into_iter()
        .chain(["/v1beta/models/ordinary:generateContent"])
    {
        fixture
            .assert_success(route, Some("ORDINARY ALIAS"), false)
            .await;
    }
    for model in [
        None,
        Some("xy-groupish/model"),
        Some("prefix/xy-group/model"),
        Some("XY-GROUP/not-a-mapping"),
    ] {
        fixture
            .assert_success("/v1/chat/completions", model, false)
            .await;
    }
    primary.model = VIRTUAL_MODEL.into();
    fixture.database.save_provider(&primary).unwrap();
    disabled.enabled = true;
    disabled
        .advanced
        .model_mappings
        .push(mapping(REAL_MODEL, "ordinary alias"));
    fixture.database.save_provider(&disabled).unwrap();
    fixture
        .assert_success("/v1/responses", Some("ordinary alias"), false)
        .await;
}

#[tokio::test]
async fn channel_group_gate_preserves_explicit_ordinary_diagnostic_models() {
    let fixture = Fixture::new(false).await;
    fixture.enable_budget();
    fixture.keys.forbid_access.store(false, Ordering::SeqCst);
    for id in ["primary", "fallback"] {
        let mut provider = fixture.database.get_provider(id).unwrap().unwrap();
        provider.model = VIRTUAL_MODEL.into();
        provider
            .advanced
            .model_mappings
            .push(mapping(VIRTUAL_MODEL, REAL_MODEL));
        fixture.database.save_provider(&provider).unwrap();
        fixture.save_legacy_body_model(id, VIRTUAL_MODEL);
    }
    for route in JSON_ROUTES
        .into_iter()
        .chain(["/v1beta/models/local-upstream-model:generateContent"])
    {
        fixture.assert_success(route, Some(REAL_MODEL), true).await;
    }
    assert!(
        fixture
            .upstream
            .received
            .lock()
            .unwrap()
            .iter()
            .all(|(path, _)| path.starts_with("/primary/"))
    );
}

#[tokio::test]
async fn channel_group_gate_preserves_ordinary_retry_failover_and_budget_ledger() {
    let fixture = Fixture::new(false).await;
    fixture.enable_budget();
    fixture.keys.forbid_access.store(false, Ordering::SeqCst);
    fixture.upstream.failures.store(3, Ordering::SeqCst);
    let route = "/v1/chat/completions";
    let response = fixture
        .request(route, &client_payload(route, Some(REAL_MODEL), false))
        .header("x-xy-task-id", "ordinary-retry-failover")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    response.bytes().await.unwrap();
    let snapshot = fixture.snapshot();
    assert_eq!(
        (
            snapshot.upstream_calls,
            snapshot.credential_reads,
            snapshot.account_pool_accesses
        ),
        (4, 2, 2)
    );
    for (table, expected) in [
        ("proxy_tasks", 1),
        ("proxy_attempts", 4),
        ("budget_ledger", 4),
        ("request_events", 2),
    ] {
        assert_eq!(
            snapshot
                .rows
                .iter()
                .find(|(name, _)| *name == table)
                .unwrap()
                .1,
            expected,
            "{table}"
        );
    }
    let received = fixture.upstream.received.lock().unwrap();
    assert!(
        received[..3]
            .iter()
            .all(|(path, _)| path.starts_with("/primary/"))
    );
    assert!(received[3].0.starts_with("/fallback/"));
}
