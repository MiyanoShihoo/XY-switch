use super::*;
use crate::governance::{self, budget, test_support as support};
use axum::http::HeaderMap;
use std::sync::Mutex;

struct BudgetKeys;
impl ApiKeyProvider for BudgetKeys {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Ok(Some("local-budget-fixture-key".into()))
    }
}

fn native_response(payload: &Value) -> Value {
    json!({"id":"resp_budget_native","status":"completed","model":payload["model"],
        "output":[
            {"type":"custom_tool_call","id":"custom_budget","status":"completed",
                "call_id":"call_budget","namespace":"local","name":"patch","input":"line\n中文"},
            {"type":"tool_search_call","id":"search_budget","status":"completed",
                "call_id":"call_search","execution":"client","arguments":{"query":"echo"}},
            {"type":"reasoning","id":"reasoning_budget","summary":[],"encrypted_content":"opaque-output"}
        ], "metadata":{"opaque":"保留"},
        "usage":{"input_tokens":20,"input_tokens_details":{"cached_tokens":3},"output_tokens":8,"total_tokens":28}})
}

fn native_frames(response: &Value) -> String {
    let mut frames = vec![json!({"type":"response.created","response":{
        "id":response["id"],"model":response["model"],"status":"in_progress","output":[]}})];
    for (index, item) in response["output"].as_array().unwrap().iter().enumerate() {
        frames.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
    }
    frames.push(json!({"type":"response.provider_extension","metadata":{"opaque":"保留"}}));
    frames.push(json!({"type":"response.completed","response":response}));
    let mut wire = ": native budget fixture\r\nretry: 1000\r\n\r\n".to_owned();
    for (sequence, mut event) in frames.into_iter().enumerate() {
        event["sequence_number"] = json!(sequence);
        wire.push_str(&format!(
            "id: budget-{sequence}\r\nevent: {}\r\ndata: {event}\r\n\r\n",
            event["type"].as_str().unwrap()
        ));
    }
    wire
}

fn dynamic_request() -> Value {
    let schema = json!({"type":"object","properties":{"file_id":{"type":"string"},"extra_body":{"type":"object"}},"additionalProperties":false});
    let namespace = json!({"type":"namespace","name":"local","description":"内联工具",
    "tools":[
        {"type":"function","name":"echo","parameters":schema,"strict":true,"defer_loading":true},
        {"type":"custom","name":"patch","format":{"type":"grammar","syntax":"lark","definition":"start: /.+/"}},
        {"type":"custom","name":"plain","format":{"type":"text"}},
        {"type":"custom","name":"regex","format":{"type":"grammar","syntax":"regex","definition":"[a-z]+"}}
    ]});
    json!({"model":"kimi-fixture","stream":false,"max_output_tokens":1000,
        "tools":[namespace,{"type":"tool_search","execution":"client","parameters":{"type":"object","properties":{"query":{"type":"string"}}}}],
        "input":[
            {"type":"message","role":"user","content":[{"type":"input_text","text":"工具发现与恢复"}]},
            {"type":"tool_search_call","execution":"client","call_id":"search","arguments":{"query":"echo","file_id":"inline"}},
            {"type":"tool_search_output","execution":"client","call_id":"search","tools":[namespace]},
            {"type":"additional_tools","role":"developer","tools":[namespace]},
            {"type":"function_call","call_id":"echo","namespace":"local","name":"echo","arguments":"{\"file_id\":\"inline\"}"},
            {"type":"function_call_output","call_id":"echo","output":"{\"encrypted_content\":\"inline text\"}"},
            {"type":"custom_tool_call","call_id":"patch","namespace":"local","name":"patch","input":"中文"},
            {"type":"custom_tool_call_output","call_id":"patch","output":[{"type":"input_text","text":"完整工具结果"}]},
            {"type":"reasoning","summary":[{"type":"summary_text","text":"可见推理"}],"content":[{"type":"reasoning_text","text":"内联历史"}],"encrypted_content":null},
            {"type":"message","role":"assistant","phase":"final_answer","content":[{"type":"output_text","text":"done","annotations":[]}]},
            {"type":"message","role":"user","content":"resume"}
        ],
        "tool_choice":{"type":"allowed_tools","mode":"auto","tools":[{"type":"function","namespace":"local","name":"echo"},{"type":"custom","namespace":"local","name":"patch"},{"type":"tool_search"}]},
        "text":{"format":{"type":"json_schema","name":"receipt","schema":schema,"strict":true}},
        "metadata":{"previous_response_id":"inline metadata"},"client_metadata":{"file_id":"inline metadata"}})
}

struct ResponsesFixture {
    database: Database,
    provider: Provider,
    endpoint: String,
    captured: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for ResponsesFixture {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
impl ResponsesFixture {
    async fn new() -> Self {
        async fn respond(
            State(captured): State<Arc<Mutex<Vec<(HeaderMap, Value)>>>>,
            headers: HeaderMap,
            Json(payload): Json<Value>,
        ) -> Response {
            captured.lock().unwrap().push((headers, payload.clone()));
            let response = native_response(&payload);
            if payload["stream"] == true {
                let wire = native_frames(&response);
                let chunks = wire
                    .as_bytes()
                    .chunks(7)
                    .map(|chunk| Ok::<_, std::io::Error>(Bytes::copy_from_slice(chunk)))
                    .collect::<Vec<_>>();
                Response::builder()
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .header("x-request-id", "req-budget-native")
                    .body(Body::from_stream(futures_util::stream::iter(chunks)))
                    .unwrap()
            } else {
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-request-id", "req-budget-native")
                    .body(Body::from(serde_json::to_vec(&response).unwrap()))
                    .unwrap()
            }
        }
        let captured = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream = listener.local_addr().unwrap();
        let upstream_captured = captured.clone();
        let upstream_task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/responses", post(respond))
                    .with_state(upstream_captured),
            )
            .await
            .unwrap();
        });
        let database = support::database();
        let mut provider = support::provider(&format!("http://{upstream}/v1"));
        provider.model = "kimi-fixture".into();
        provider.advanced.upstream_format = "openai_responses".into();
        database.save_provider(&provider).unwrap();
        database.set_current_provider(&provider.id).unwrap();
        support::native_budget(&database, &provider.model, 24_000, 64);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = ProxyState {
            database: database.clone(),
            client: crate::network::test_client(),
            api_keys: Arc::new(BudgetKeys),
            local_addr: Some(address),
        };
        let proxy_task = tokio::spawn(async move {
            axum::serve(listener, router_with_targets(state, &["codex".into()]))
                .await
                .unwrap();
        });
        Self {
            database,
            provider,
            endpoint: format!("http://{address}/v1/responses"),
            captured,
            tasks: vec![upstream_task, proxy_task],
        }
    }
    fn request(&self, payload: &Value) -> reqwest::RequestBuilder {
        crate::network::test_client()
            .post(&self.endpoint)
            .json(payload)
    }
    fn calls(&self) -> usize {
        self.captured.lock().unwrap().len()
    }
    fn ledger(&self) -> (i64, i64, i64) {
        let status = budget::status(&self.database).unwrap();
        (
            status.attempts,
            status.reserved_tokens,
            status.reserved_microusd,
        )
    }
    fn policy(&self) -> budget::BudgetPolicy {
        governance::load(&self.database, budget::KEY).unwrap()
    }
    fn logs(&self) -> Vec<crate::usage::UsageLogEntry> {
        crate::usage::query_usage_logs(
            &self.database.conn().lock().unwrap(),
            &crate::usage::UsageFilter::default(),
            100,
            0,
        )
        .unwrap()
    }
    async fn admit(&self, payload: &Value) {
        let response = self.request(payload).send().await.unwrap();
        let status = response.status();
        let body = response.text().await.unwrap();
        assert_eq!(status, StatusCode::OK, "{body}");
    }
    async fn reject(&self, payload: &Value, reason: &str) {
        let ledger = self.ledger();
        let calls = self.calls();
        let health =
            serde_json::to_value(self.database.provider_health(&self.provider.id).unwrap())
                .unwrap();
        let response = self.request(payload).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED, "{reason}");
        let error = response.json::<Value>().await.unwrap();
        assert_eq!(error["error"]["type"], "budget_rejected", "{error}");
        assert!(
            error["error"]["message"].as_str().unwrap().contains(reason),
            "{error}"
        );
        assert_eq!(self.calls(), calls, "{reason}");
        assert_eq!(self.ledger(), ledger, "{reason}");
        assert_eq!(
            serde_json::to_value(self.database.provider_health(&self.provider.id).unwrap())
                .unwrap(),
            health,
            "{reason}"
        );
        let logs = self.logs();
        let log = &logs[0];
        let diagnostic = log.diagnostics.as_ref().unwrap();
        assert_eq!(
            diagnostic.failure_stage.as_deref(),
            Some("budget"),
            "{reason}"
        );
        assert_eq!(diagnostic.send_attempts, 0, "{reason}");
        assert!(!diagnostic.upstream_response_received, "{reason}");
        assert!(diagnostic.attempts.is_empty(), "{reason}");
        assert!(!log.success, "{reason}");
        assert_eq!(log.status_code, 402, "{reason}");
        assert_eq!(
            (log.input_tokens, log.output_tokens, log.total_tokens),
            (0, 0, Some(0)),
            "{reason}"
        );
        assert_eq!(log.total_cost_usd.parse::<f64>().unwrap(), 0.0, "{reason}");
        assert_eq!(log.source, "local_rejection", "{reason}");
        let event: String = self.database.conn().lock().unwrap().query_row(
            "SELECT detail FROM governance_events WHERE kind='budget_rejected' ORDER BY id DESC LIMIT 1", [], |row| row.get(0)).unwrap();
        assert_eq!(event, reason);
    }
}

#[tokio::test]
async fn native_saved_requests_dynamic_tools_and_resumed_history_cross_budget_proxy_unchanged() {
    let fixture = ResponsesFixture::new().await;
    let mut requests = support::native_codex_requests();
    requests.push(dynamic_request());
    for original in requests {
        for stream in [false, true] {
            let mut payload = original.clone();
            payload["stream"] = json!(stream);
            let response = fixture.request(&payload).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["x-request-id"], "req-budget-native");
            let expected_response = native_response(&payload);
            if stream {
                assert_eq!(
                    response.text().await.unwrap(),
                    native_frames(&expected_response)
                );
            } else {
                assert_eq!(response.json::<Value>().await.unwrap(), expected_response);
            }
            payload["max_output_tokens"] = json!(64);
            let captured = fixture.captured.lock().unwrap();
            let (headers, forwarded) = captured.last().unwrap();
            assert_eq!(
                *forwarded, payload,
                "budget must preserve tools, metadata, results and complete history"
            );
            assert_eq!(
                headers[header::AUTHORIZATION],
                "Bearer local-budget-fixture-key"
            );
        }
    }
    assert_eq!(fixture.calls(), 10);
    assert_eq!(fixture.ledger(), (10, 240_640, 241_280));
    let logs = fixture.logs();
    assert_eq!(logs.len(), 10);
    for log in logs {
        assert!(log.success);
        assert_eq!(
            (log.input_tokens, log.output_tokens, log.cache_read_tokens),
            (20, 8, 3)
        );
        let diagnostic = log.diagnostics.unwrap();
        assert_eq!(diagnostic.send_attempts, 1);
        assert!(diagnostic.upstream_response_received);
        assert_eq!(diagnostic.failure_stage, None);
    }
}

#[tokio::test]
async fn native_utf8_input_boundary_blocks_before_send_without_changing_ledger() {
    let fixture = ResponsesFixture::new().await;
    let mut payload = dynamic_request();
    payload["input"][7]["output"] = json!("工具结果与历史".repeat(120));
    payload["max_output_tokens"] = json!(64);
    let bytes = serde_json::to_vec(&payload).unwrap().len() as i64 + 256;
    let mut policy = fixture.policy();
    policy.input_reservation = bytes;
    governance::save(&fixture.database, budget::KEY, &policy).unwrap();
    fixture.admit(&payload).await;
    assert_eq!(fixture.ledger(), (1, bytes + 64, bytes + 128));
    policy.input_reservation -= 1;
    governance::save(&fixture.database, budget::KEY, &policy).unwrap();
    fixture
        .reject(&payload, "budget_input_reservation_exceeded")
        .await;
    assert_eq!(fixture.calls(), 1);
}

#[tokio::test]
async fn native_unbounded_inputs_invalid_tools_and_daily_limits_record_zero_send_rejections() {
    let fixture = ResponsesFixture::new().await;
    let payload = dynamic_request();
    fixture.admit(&payload).await;
    let changes = [
        (
            json!({"previous_response_id":"remote-history"}),
            "budget_unbounded_input",
        ),
        (
            json!({"conversation":{"id":"remote-history"}}),
            "budget_unbounded_input",
        ),
        (
            json!({"prompt":{"id":"stored-prompt"}}),
            "budget_unbounded_input",
        ),
        (
            json!({"context_management":[{"type":"compaction"}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"tools":[{"type":"tool_search"}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"tools":[{"type":"tool_search","execution":"server"}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"tools":[{"type":"namespace","name":"remote","tools":[{"type":"web_search"}]}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"input":[{"type":"additional_tools","tools":[{"type":"file_search"}]}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"input":[{"type":"tool_search_call","execution":"server","arguments":{"query":"remote"}}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"input":[{"type":"tool_search_output","execution":"client","tools":[{"type":"web_search"}]}]}),
            "budget_server_tools_blocked",
        ),
        (
            json!({"input":[{"type":"reasoning","summary":[],"encrypted_content":"opaque-history"}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"input":[{"type":"message","role":"user","content":[{"type":"input_image","image_url":"https://example.invalid"}]}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"input":[{"type":"custom_tool_call_output","call_id":"patch","output":[{"type":"input_file","file_id":"remote"}]}]}),
            "budget_unbounded_input",
        ),
        (
            json!({"tools":[{"type":"function","name":"echo","parameters":"invalid-schema"}]}),
            "budget_invalid_tools",
        ),
        (
            json!({"input":[{"type":"custom_tool_call","call_id":"patch","name":"patch","input":{}}]}),
            "budget_invalid_input",
        ),
        (
            json!({"unknown_billable_feature":true}),
            "budget_unbounded_parameter",
        ),
        (
            json!({"service_tier":"flex"}),
            "budget_unknown_service_tier_price",
        ),
    ];
    for (change, reason) in changes {
        let mut invalid = payload.clone();
        invalid
            .as_object_mut()
            .unwrap()
            .extend(change.as_object().unwrap().clone());
        fixture.reject(&invalid, reason).await;
    }
    let original_policy = fixture.policy();
    let mut policy = original_policy.clone();
    policy.daily_tokens = fixture.ledger().1;
    governance::save(&fixture.database, budget::KEY, &policy).unwrap();
    fixture
        .reject(&payload, "hard_token_budget_exhausted")
        .await;
    policy = original_policy;
    policy.daily_microusd = fixture.ledger().2;
    governance::save(&fixture.database, budget::KEY, &policy).unwrap();
    fixture.reject(&payload, "hard_cost_budget_exhausted").await;
    assert_eq!(fixture.calls(), 1);
}
