mod diagnostics_http_regressions {
    use super::*;
    use std::collections::BTreeMap;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Mutex,
    };
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[derive(Default)]
    struct DiagnosticKeys {
        overrides: BTreeMap<String, Result<Option<String>, String>>,
        reads: Mutex<Vec<String>>,
    }

    impl ApiKeyProvider for DiagnosticKeys {
        fn get(&self, provider_id: &str) -> Result<Option<String>, String> {
            self.reads.lock().unwrap().push(provider_id.to_owned());
            self.overrides
                .get(provider_id)
                .cloned()
                .unwrap_or_else(|| Ok(Some("local-fixture-key".into())))
        }
    }

    struct LocalServer {
        address: std::net::SocketAddr,
        calls: Arc<AtomicUsize>,
        task: tokio::task::JoinHandle<()>,
    }

    impl Drop for LocalServer {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    #[derive(Clone, Copy)]
    enum UpstreamCase {
        RetryThenSuccess,
        Unavailable,
        Pending,
        InvalidJson,
        InvalidCustomArguments,
        IncompleteStream,
    }

    async fn local_upstream(case: UpstreamCase) -> LocalServer {
        async fn respond(
            State((case, calls)): State<(UpstreamCase, Arc<AtomicUsize>)>,
            Json(payload): Json<Value>,
        ) -> Response {
            let count = calls.fetch_add(1, Ordering::SeqCst);
            match case {
                UpstreamCase::Unavailable => {
                    return (StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":{"type":"fixture_upstream","message":"Local upstream unavailable"}})))
                        .into_response();
                }
                UpstreamCase::RetryThenSuccess if count == 0 => {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        Json(json!({"error":"local retry"})),
                    )
                        .into_response();
                }
                UpstreamCase::Pending => std::future::pending::<()>().await,
                UpstreamCase::InvalidJson => {
                    return (
                        [(header::CONTENT_TYPE, "application/json")],
                        "invalid local JSON\n",
                    )
                        .into_response();
                }
                UpstreamCase::IncompleteStream => {
                    return ([(header::CONTENT_TYPE, "text/event-stream")],
                        "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"partial\"},\"finish_reason\":null}]}\n\n")
                        .into_response();
                }
                _ => {}
            }
            let (message, finish) = if matches!(case, UpstreamCase::InvalidCustomArguments) {
                (
                    json!({"role":"assistant","content":null,"tool_calls":[{"id":"local-call","type":"function","function":{"name":"local_tool","arguments":"{\"input\":123}"}}]}),
                    "tool_calls",
                )
            } else {
                (
                    json!({"role":"assistant","content":"local success"}),
                    "stop",
                )
            };
            Json(json!({"id":"local-response","model":payload["model"],
                "choices":[{"index":0,"message":message,"finish_reason":finish}],
                "usage":{"prompt_tokens":4,"completion_tokens":2,"total_tokens":6}}))
            .into_response()
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let server_calls = calls.clone();
        let task = tokio::spawn(async move {
            axum::serve(
                listener,
                Router::new()
                    .route("/v1/chat/completions", post(respond))
                    .with_state((case, server_calls)),
            )
            .await
            .unwrap();
        });
        LocalServer {
            address,
            calls,
            task,
        }
    }

    async fn truncated_upstream() -> LocalServer {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let server_calls = calls.clone();
        let task = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            server_calls.fetch_add(1, Ordering::SeqCst);
            let mut request = Vec::new();
            let mut buffer = [0; 4096];
            loop {
                let count = socket.read(&mut buffer).await.unwrap();
                if count == 0 {
                    return;
                }
                request.extend_from_slice(&buffer[..count]);
                assert!(request.len() <= 65536);
                if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_ascii_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| line.strip_prefix("content-length:"))
                        .unwrap()
                        .trim()
                        .parse()
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 1000\r\nConnection: close\r\n\r\n{}").await.unwrap();
            socket.shutdown().await.unwrap();
        });
        LocalServer {
            address,
            calls,
            task,
        }
    }

    struct DiagnosticFixture {
        database: Database,
        keys: Arc<DiagnosticKeys>,
        proxy: LocalServer,
    }

    impl DiagnosticFixture {
        async fn new(
            overrides: BTreeMap<String, Result<Option<String>, String>>,
            timeout: Duration,
        ) -> Self {
            let database = Database::open(std::path::Path::new(":memory:")).unwrap();
            for provider in database.list_providers().unwrap() {
                database.delete_provider(&provider.id).unwrap();
            }
            assert!(database.list_providers().unwrap().is_empty());
            let keys = Arc::new(DiagnosticKeys {
                overrides,
                ..Default::default()
            });
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let state = ProxyState {
                database: database.clone(),
                client: crate::network::client_builder()
                    .timeout(timeout)
                    .redirect(reqwest::redirect::Policy::none())
                    .build()
                    .unwrap(),
                api_keys: keys.clone(),
                local_addr: Some(address),
            };
            let task = tokio::spawn(async move {
                axum::serve(listener, router_with_state(state))
                    .await
                    .unwrap();
            });
            Self {
                database,
                keys,
                proxy: LocalServer {
                    address,
                    calls: Arc::new(AtomicUsize::new(0)),
                    task,
                },
            }
        }

        fn provider(&self, id: &str, endpoint: String, sort_order: i64) -> Provider {
            let url = reqwest::Url::parse(&endpoint).unwrap();
            assert!(matches!(
                url.host_str(),
                Some("127.0.0.1" | "localhost" | "[::1]")
            ));
            let provider = Provider {
                id: id.into(),
                name: id.into(),
                provider_type: "openai_compatible".into(),
                endpoint,
                model: "local-diagnostics-model".into(),
                app_group: "Codex".into(),
                enabled: true,
                sort_order,
                advanced: crate::database::ProviderAdvancedConfig {
                    upstream_format: "openai_chat".into(),
                    ..Default::default()
                },
            };
            self.database.save_provider(&provider).unwrap();
            provider
        }

        fn open_circuit(&self, id: &str) {
            self.database.conn().lock().unwrap().execute(
                "INSERT INTO provider_health(provider_id,status,latency_ms,success_rate,consecutive_failures,last_error,last_checked_at,circuit_open_until)
                 VALUES(?1,'unhealthy',80,20,3,'upstream_status',123,?2)",
                rusqlite::params![id, now_millis() + 60000],
            ).unwrap();
        }

        fn health(&self, id: &str) -> Value {
            serde_json::to_value(self.database.provider_health(id).unwrap()).unwrap()
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

        fn request(&self, route: &str) -> reqwest::RequestBuilder {
            crate::network::test_client().post(format!("http://{}{route}", self.proxy.address))
        }
    }

    fn chat_payload(stream: bool) -> Value {
        json!({"model":"local-alias","messages":[{"role":"user","content":"local diagnostic"}],"stream":stream})
    }

    fn responses_payload(stream: bool) -> Value {
        json!({"model":"local-alias","input":"local diagnostic","stream":stream})
    }

    #[tokio::test]
    async fn local_rejections_have_zero_sends_zero_cost_and_unchanged_health() {
        let upstream = local_upstream(UpstreamCase::RetryThenSuccess).await;
        for (case, status, stage, code) in [
            (
                "no_provider",
                503,
                "provider_selection",
                "provider_not_selected",
            ),
            ("disabled", 503, "provider_selection", "provider_disabled"),
            ("missing_key", 401, "credentials", "api_key_missing"),
            ("credential_error", 500, "credentials", "credential_error"),
            ("invalid_json", 400, "request_conversion", "invalid_json"),
            (
                "invalid_headers",
                400,
                "provider_configuration",
                "request_build",
            ),
            ("budget", 402, "budget", "budget_rejected"),
            ("loop_ipv4", 508, "provider_configuration", "loop_detected"),
            ("loop_ipv6", 508, "provider_configuration", "loop_detected"),
            ("circuit", 503, "provider_selection", "circuit_open"),
        ] {
            let overrides = match case {
                "missing_key" => BTreeMap::from([("primary".into(), Ok(None))]),
                "credential_error" => BTreeMap::from([(
                    "primary".into(),
                    Err("Local credential fixture failed".into()),
                )]),
                _ => BTreeMap::new(),
            };
            let fixture = DiagnosticFixture::new(overrides, Duration::from_secs(5)).await;
            if case != "no_provider" {
                let endpoint = match case {
                    "loop_ipv4" => format!("http://{}/v1", fixture.proxy.address),
                    "loop_ipv6" => format!("http://[::1]:{}/v1", fixture.proxy.address.port()),
                    _ => format!("http://{}/v1", upstream.address),
                };
                let mut provider = fixture.provider("primary", endpoint, 1);
                fixture.database.set_current_provider("primary").unwrap();
                if case == "disabled" {
                    provider.enabled = false;
                }
                fixture.database.save_provider(&provider).unwrap();
                if case == "invalid_headers" {
                    provider
                        .advanced
                        .header_overrides
                        .insert("invalid\nheader".into(), "local".into());
                    fixture
                        .database
                        .conn()
                        .lock()
                        .unwrap()
                        .execute(
                            "UPDATE providers SET advanced_json=?1 WHERE id=?2",
                            rusqlite::params![
                                serde_json::to_string(&provider.advanced).unwrap(),
                                provider.id
                            ],
                        )
                        .unwrap();
                }
                if case == "circuit" {
                    fixture.open_circuit("primary");
                }
                if case == "budget" {
                    fixture
                        .database
                        .set_setting(
                            crate::governance::budget::KEY,
                            &serde_json::to_string(&crate::governance::budget::BudgetPolicy {
                                enabled: true,
                                ..Default::default()
                            })
                            .unwrap(),
                        )
                        .unwrap();
                }
            }
            let before = fixture.health("primary");
            let request = fixture.request("/v1/chat/completions");
            let response = if case == "invalid_json" {
                request.body("{local invalid JSON").send().await.unwrap()
            } else {
                request.json(&chat_payload(false)).send().await.unwrap()
            };
            assert_eq!(response.status().as_u16(), status, "{case}");
            assert_eq!(
                response.json::<Value>().await.unwrap()["error"]["type"],
                code,
                "{case}"
            );
            let logs = fixture.logs();
            assert_eq!(logs.len(), 1, "{case}");
            let log = &logs[0];
            let diagnostic = log.diagnostics.as_ref().unwrap();
            assert_eq!(diagnostic.failure_stage.as_deref(), Some(stage), "{case}");
            assert_eq!(diagnostic.send_attempts, 0, "{case}");
            assert!(!diagnostic.upstream_response_received, "{case}");
            assert!(diagnostic.attempts.is_empty(), "{case}");
            assert!(!log.success, "{case}");
            assert_eq!(log.total_cost_usd.parse::<f64>().unwrap(), 0.0, "{case}");
            assert_eq!(log.total_tokens, Some(0), "{case}");
            assert_eq!(log.source, "local_rejection", "{case}");
            assert_eq!(fixture.health("primary"), before, "{case}");
            if matches!(
                case,
                "no_provider" | "disabled" | "loop_ipv4" | "loop_ipv6" | "circuit"
            ) {
                assert!(fixture.keys.reads.lock().unwrap().is_empty(), "{case}");
            }
        }
        assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn retry_success_keeps_every_http_attempt_and_matches_actual_sends() {
        let upstream = local_upstream(UpstreamCase::RetryThenSuccess).await;
        let fixture = DiagnosticFixture::new(BTreeMap::new(), Duration::from_secs(5)).await;
        fixture.provider("primary", format!("http://{}/v1", upstream.address), 1);
        fixture.database.set_current_provider("primary").unwrap();
        let response = fixture
            .request("/v1/chat/completions")
            .json(&chat_payload(false))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"],
            "local success"
        );
        let logs = fixture.logs();
        assert_eq!(logs.len(), 1);
        assert!(logs[0].success);
        let diagnostic = logs[0].diagnostics.as_ref().unwrap();
        assert_eq!(
            diagnostic.send_attempts as usize,
            upstream.calls.load(Ordering::SeqCst)
        );
        assert_eq!(diagnostic.send_attempts, 2);
        assert!(diagnostic.upstream_response_received);
        assert_eq!(diagnostic.failure_stage, None);
        assert_eq!(
            diagnostic.attempts,
            vec![
                RequestAttempt {
                    status_code: Some(503),
                    error_kind: Some("upstream_status".into())
                },
                RequestAttempt {
                    status_code: Some(200),
                    error_kind: None
                },
            ]
        );
    }

    #[tokio::test]
    async fn failover_local_rejections_keep_provider_evidence_separate_and_preserve_http_failure() {
        for (case, code) in [
            ("missing_key", "api_key_missing"),
            ("loop", "loop_detected"),
            ("circuit", "circuit_open"),
        ] {
            let upstream = local_upstream(UpstreamCase::Unavailable).await;
            let overrides = if case == "missing_key" {
                BTreeMap::from([("backup".into(), Ok(None))])
            } else {
                BTreeMap::new()
            };
            let fixture = DiagnosticFixture::new(overrides, Duration::from_secs(5)).await;
            fixture.provider("primary", format!("http://{}/v1", upstream.address), 1);
            let backup_endpoint = if case == "loop" {
                fixture.proxy.address
            } else {
                upstream.address
            };
            fixture.provider("backup", format!("http://{backup_endpoint}/v1"), 2);
            fixture.database.set_current_provider("primary").unwrap();
            if case == "circuit" {
                fixture.open_circuit("backup");
            }
            let before = fixture.health("backup");
            let response = fixture
                .request("/v1/chat/completions")
                .json(&chat_payload(false))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{case}");
            assert_eq!(
                response.json::<Value>().await.unwrap()["error"]["type"],
                "fixture_upstream",
                "{case}"
            );
            let logs = fixture.logs();
            assert_eq!(logs.len(), 2, "{case}");
            let primary = logs
                .iter()
                .find(|log| log.provider_id == "primary")
                .unwrap();
            let primary_diagnostic = primary.diagnostics.as_ref().unwrap();
            assert_eq!(
                primary_diagnostic.send_attempts as usize,
                upstream.calls.load(Ordering::SeqCst),
                "{case}"
            );
            assert_eq!(primary_diagnostic.send_attempts, 3, "{case}");
            assert_eq!(primary_diagnostic.attempts.len(), 3, "{case}");
            assert!(
                primary_diagnostic
                    .attempts
                    .iter()
                    .all(|attempt| attempt.status_code == Some(503)),
                "{case}"
            );
            let backup = logs.iter().find(|log| log.provider_id == "backup").unwrap();
            let backup_diagnostic = backup.diagnostics.as_ref().unwrap();
            assert_eq!(backup.error_message.as_deref(), Some(code), "{case}");
            assert_eq!(backup_diagnostic.send_attempts, 0, "{case}");
            assert!(!backup_diagnostic.upstream_response_received, "{case}");
            assert!(backup_diagnostic.attempts.is_empty(), "{case}");
            assert_eq!(fixture.health("backup"), before, "{case}");
            if case != "missing_key" {
                assert!(
                    !fixture
                        .keys
                        .reads
                        .lock()
                        .unwrap()
                        .iter()
                        .any(|id| id == "backup"),
                    "{case}"
                );
            }
        }
    }

    #[tokio::test]
    async fn timeout_evidence_survives_a_later_provider_without_credentials() {
        let upstream = local_upstream(UpstreamCase::Pending).await;
        let fixture = DiagnosticFixture::new(
            BTreeMap::from([("backup".into(), Ok(None))]),
            Duration::from_millis(100),
        )
        .await;
        fixture.provider("primary", format!("http://{}/v1", upstream.address), 1);
        fixture.provider("backup", format!("http://{}/v1", upstream.address), 2);
        fixture.database.set_current_provider("primary").unwrap();
        let response = fixture
            .request("/v1/chat/completions")
            .json(&chat_payload(false))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["type"],
            "upstream_unavailable"
        );
        let logs = fixture.logs();
        assert_eq!(logs.len(), 2);
        let primary = logs
            .iter()
            .find(|log| log.provider_id == "primary")
            .unwrap();
        let diagnostic = primary.diagnostics.as_ref().unwrap();
        assert_eq!(
            diagnostic.send_attempts as usize,
            upstream.calls.load(Ordering::SeqCst)
        );
        assert_eq!(diagnostic.send_attempts, 3);
        assert_eq!(diagnostic.failure_stage.as_deref(), Some("upstream_send"));
        assert!(!diagnostic.upstream_response_received);
        assert_eq!(
            diagnostic.attempts,
            vec![
                RequestAttempt {
                    status_code: None,
                    error_kind: Some("timeout".into())
                };
                3
            ]
        );
        assert_eq!(
            logs.iter()
                .find(|log| log.provider_id == "backup")
                .unwrap()
                .diagnostics
                .as_ref()
                .unwrap()
                .send_attempts,
            0
        );
    }

    #[tokio::test]
    async fn truncated_response_keeps_upstream_http_200_and_records_read_failure() {
        let upstream = truncated_upstream().await;
        let fixture = DiagnosticFixture::new(BTreeMap::new(), Duration::from_secs(5)).await;
        fixture.provider("primary", format!("http://{}/v1", upstream.address), 1);
        fixture.database.set_current_provider("primary").unwrap();
        let response = fixture
            .request("/v1/chat/completions")
            .json(&chat_payload(false))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["type"],
            "response_error"
        );
        let logs = fixture.logs();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].status_code, 200);
        assert!(!logs[0].success);
        let diagnostic = logs[0].diagnostics.as_ref().unwrap();
        assert_eq!(diagnostic.failure_stage.as_deref(), Some("response_read"));
        assert!(diagnostic.upstream_response_received);
        assert_eq!(
            diagnostic.send_attempts as usize,
            upstream.calls.load(Ordering::SeqCst)
        );
        assert_eq!(
            diagnostic.attempts,
            vec![RequestAttempt {
                status_code: Some(200),
                error_kind: None
            }]
        );
    }

    #[tokio::test]
    async fn invalid_json_records_failure_and_preserves_native_response_bytes() {
        for (route, stage) in [
            ("/v1/chat/completions", "response_read"),
            ("/v1/responses", "response_conversion"),
        ] {
            let upstream = local_upstream(UpstreamCase::InvalidJson).await;
            let fixture = DiagnosticFixture::new(BTreeMap::new(), Duration::from_secs(5)).await;
            fixture.provider("primary", format!("http://{}/v1", upstream.address), 1);
            fixture.database.set_current_provider("primary").unwrap();
            let payload = if route == "/v1/responses" {
                responses_payload(false)
            } else {
                chat_payload(false)
            };
            let response = fixture.request(route).json(&payload).send().await.unwrap();
            if route == "/v1/responses" {
                assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
                assert_eq!(
                    response.json::<Value>().await.unwrap()["error"]["type"],
                    "invalid_upstream_response"
                );
            } else {
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(
                    response.bytes().await.unwrap().as_ref(),
                    b"invalid local JSON\n"
                );
            }
            let logs = fixture.logs();
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].status_code, 200);
            assert!(!logs[0].success);
            let diagnostic = logs[0].diagnostics.as_ref().unwrap();
            assert_eq!(diagnostic.failure_stage.as_deref(), Some(stage));
            assert!(diagnostic.upstream_response_received);
            assert_eq!(
                diagnostic.send_attempts as usize,
                upstream.calls.load(Ordering::SeqCst)
            );
            assert_eq!(diagnostic.attempts[0].status_code, Some(200));
        }
    }

    #[tokio::test]
    async fn local_tool_conversion_failure_preserves_upstream_evidence_and_prior_health() {
        let upstream = local_upstream(UpstreamCase::InvalidCustomArguments).await;
        let fixture = DiagnosticFixture::new(BTreeMap::new(), Duration::from_secs(5)).await;
        fixture.provider("primary", format!("http://{}/v1", upstream.address), 1);
        fixture.database.set_current_provider("primary").unwrap();
        let baseline = fixture
            .request("/v1/chat/completions")
            .json(&chat_payload(false))
            .send()
            .await
            .unwrap();
        assert_eq!(baseline.status(), StatusCode::OK);
        baseline.bytes().await.unwrap();
        let before = fixture.health("primary");
        let mut payload = responses_payload(false);
        payload["tools"] = json!([{"type":"custom","name":"local_tool","format":{"type":"text"}}]);
        let response = fixture
            .request("/v1/responses")
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        assert_eq!(
            response.json::<Value>().await.unwrap()["error"]["type"],
            "response_conversion"
        );
        let logs = fixture.logs();
        assert_eq!(logs.len(), 2);
        let log = logs.iter().find(|log| !log.success).unwrap();
        assert_eq!(log.status_code, 200);
        let diagnostic = log.diagnostics.as_ref().unwrap();
        assert_eq!(
            diagnostic.failure_stage.as_deref(),
            Some("response_conversion")
        );
        assert_eq!(diagnostic.send_attempts, 1);
        assert!(diagnostic.upstream_response_received);
        assert_eq!(diagnostic.attempts[0].status_code, Some(200));
        assert_eq!(upstream.calls.load(Ordering::SeqCst), 2);
        assert_eq!(fixture.health("primary"), before);
    }

    #[tokio::test]
    async fn incomplete_sse_is_failed_after_consumption_without_losing_http_evidence() {
        for route in ["/v1/chat/completions", "/v1/responses"] {
            let upstream = local_upstream(UpstreamCase::IncompleteStream).await;
            let fixture = DiagnosticFixture::new(BTreeMap::new(), Duration::from_secs(5)).await;
            fixture.provider("primary", format!("http://{}/v1", upstream.address), 1);
            fixture.database.set_current_provider("primary").unwrap();
            let payload = if route == "/v1/responses" {
                responses_payload(true)
            } else {
                chat_payload(true)
            };
            let sent = fixture.request(route).json(&payload).send().await;
            match sent {
                Err(error) => assert!(error.is_request()),
                Ok(response) => {
                    assert_eq!(response.status(), StatusCode::OK);
                    let mut chunks = response.bytes_stream();
                    let mut delivered = Vec::new();
                    let mut failed = false;
                    while let Some(chunk) = chunks.next().await {
                        match chunk {
                            Ok(bytes) => delivered.extend_from_slice(&bytes),
                            Err(_) => { failed = true; break; }
                        }
                    }
                    assert!(failed, "a stream without a finish event must fail");
                    assert!(!String::from_utf8_lossy(&delivered).contains("partial"));
                }
            }
            let logs = fixture.logs();
            assert_eq!(logs.len(), 1);
            assert_eq!(logs[0].status_code, 200);
            assert!(!logs[0].success);
            let diagnostic = logs[0].diagnostics.as_ref().unwrap();
            let expected = if route == "/v1/responses" { ("response_stream", "stream_interrupted") }
                else { ("response_privacy", "privacy_response_scan_failed") };
            assert_eq!(diagnostic.failure_stage.as_deref(), Some(expected.0));
            assert_eq!(logs[0].error_message.as_deref(), Some(expected.1));
            assert_eq!(
                diagnostic.send_attempts as usize,
                upstream.calls.load(Ordering::SeqCst)
            );
            assert!(diagnostic.upstream_response_received);
            assert_eq!(diagnostic.attempts[0].status_code, Some(200));
        }
    }
}
