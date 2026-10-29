#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn verification_real_proxy_1000_default_requests_and_passive_sse_never_add_calls() {
    use crate::model_verification::VerificationSettings;
    use futures_util::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn business(streaming: bool) -> Value {
        json!({"model":"business-model","stream":streaming,"max_tokens":77,
            "temperature":0.42,"top_p":0.66,"seed":42,
            "messages":[{"role":"system","content":"PRIVATE_BUSINESS_SYSTEM"},
                {"role":"user","content":"PRIVATE_BUSINESS_PROMPT"}],
            "tools":[{"type":"function","function":{"name":"fixture_tool","parameters":{"type":"object","properties":{}}}}],
            "tool_choice":"none"})
    }
    let json_reply = json!({"id":"private-diagnostic-id","model":"different-model",
        "choices":[{"index":0,"message":{"role":"assistant","content":"PRIVATE_BUSINESS_REPLY"},"finish_reason":"stop"}],
        "usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}}).to_string();
    let mut sse = format!(
        ": heartbeat\r\n\r\nevent: vendor.future\ndata: {}\n\n",
        json!({"new_field":true})
    );
    for index in 0..128 {
        sse.push_str(&format!("data: {}\n\n", json!({"id":"private-diagnostic-id","model":"different-model",
            "choices":[{"index":0,"delta":{"content":format!("PRIVATE_BUSINESS_REPLY_你好_{index}")},"finish_reason":null}]})));
    }
    sse.push_str(&format!(
        "data: {}\n\n",
        json!({"choices":[{"index":0,"delta":{},"finish_reason":"stop"}]})
    ));
    sse.push_str(&format!(
        "data: {}\n\ndata: [DONE]\n\n",
        json!({"choices":[],"usage":{"prompt_tokens":7,"completion_tokens":2,"total_tokens":9}})
    ));
    let hits = Arc::new(AtomicUsize::new(0));
    let upstream_hits = hits.clone();
    let upstream_json = json_reply.clone();
    let upstream_sse = sse.clone();
    let upstream = Router::new().fallback(move |headers: HeaderMap, Json(body): Json<Value>| {
        let hits = upstream_hits.clone();
        let json_reply = upstream_json.clone();
        let sse = upstream_sse.clone();
        async move {
            hits.fetch_add(1, Ordering::SeqCst);
            assert_eq!(headers[header::AUTHORIZATION], "Bearer test-api-key");
            let streaming = body["stream"] == true;
            assert_eq!(
                body,
                business(streaming),
                "verification modified the business request"
            );
            if streaming {
                // Split JSON, SSE delimiters, and multi-byte UTF-8 over transport chunks.
                let chunks: Vec<_> = sse
                    .as_bytes()
                    .chunks(13)
                    .map(Bytes::copy_from_slice)
                    .collect();
                Response::builder()
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .header("x-request-id", "private-request-diagnostic-id")
                    .body(Body::from_stream(futures_util::stream::iter(
                        chunks.into_iter().map(Ok::<Bytes, io::Error>),
                    )))
                    .unwrap()
            } else {
                Response::builder()
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(json_reply))
                    .unwrap()
            }
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let upstream_addr = listener.local_addr().unwrap();
    let upstream_task = tokio::spawn(async move { axum::serve(listener, upstream).await.unwrap() });
    let db = Database::open(std::path::Path::new(":memory:")).unwrap();
    // This throughput fixture tests verification's zero-extra-call guarantee,
    // not the independent default 120/minute governance admission limit.
    crate::governance::save(&db, crate::governance::access::KEY,
        &crate::governance::access::AccessPolicy { requests_per_minute: 10000, max_in_flight: 32, ..Default::default() }).unwrap();
    assert!(!db.verification.settings().unwrap().enabled);
    assert!(!db.verification.settings().unwrap().active_probe_enabled);
    let provider: Provider =
        serde_json::from_value(json!({"id":"zero-extra-http","name":"Local fixture",
        "endpoint":format!("http://{upstream_addr}"),"model":"business-model","enabled":true,
        "advanced":{"upstreamFormat":"openai_chat"}}))
        .unwrap();
    db.save_provider(&provider).unwrap();
    db.set_current_provider(&provider.id).unwrap();
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    let state = ProxyState {
        database: db.clone(),
        client: client.clone(),
        api_keys: Arc::new(FakeApiKeyProvider),
        local_addr: None,
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!(
        "http://{}/v1/chat/completions",
        listener.local_addr().unwrap()
    );
    let proxy_task = tokio::spawn(async move {
        axum::serve(listener, router_with_state(state))
            .await
            .unwrap()
    });
    let count = |table: &str| -> u64 {
        db.conn()
            .lock()
            .unwrap()
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap()
    };

    futures_util::stream::iter(0..1000)
        .map(|_| {
            let client = client.clone();
            let url = proxy_url.clone();
            let expected = json_reply.clone();
            async move {
                let response = client
                    .post(url)
                    .json(&business(false))
                    .send()
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(response.text().await.unwrap(), expected);
            }
        })
        .buffer_unordered(20)
        .collect::<Vec<_>>()
        .await;
    assert_eq!(hits.load(Ordering::SeqCst), 1000);
    assert_eq!(count("request_events"), 1000);
    assert_eq!(count("verification_samples"), 0);
    assert_eq!(count("verification_probes"), 0);
    let audit_count: u64 = db
        .conn()
        .lock()
        .unwrap()
        .query_row(
            "SELECT COUNT(*) FROM request_events WHERE audit_json IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(audit_count, 0);

    db.verification
        .set_settings(VerificationSettings {
            enabled: true,
            ..Default::default()
        })
        .unwrap();
    for _ in 0..100 {
        let response = client
            .post(&proxy_url)
            .json(&business(false))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.text().await.unwrap(), json_reply);
    }
    let response = client
        .post(&proxy_url)
        .json(&business(true))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.bytes().await.unwrap().as_ref(), sse.as_bytes());
    tokio::time::timeout(Duration::from_secs(5), async {
        while count("verification_samples") < 101 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(hits.load(Ordering::SeqCst), 1101);
    assert_eq!(count("verification_probes"), 0);
    let serialized: Vec<String> = db
        .conn()
        .lock()
        .unwrap()
        .prepare("SELECT audit_json FROM request_events WHERE audit_json IS NOT NULL ORDER BY id")
        .unwrap()
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    for item in &serialized {
        for secret in [
            "test-api-key",
            "PRIVATE_BUSINESS_SYSTEM",
            "PRIVATE_BUSINESS_PROMPT",
            "PRIVATE_BUSINESS_REPLY",
            "private-diagnostic-id",
            "private-request-diagnostic-id",
        ] {
            assert!(
                !item.contains(secret),
                "private material leaked into verification metadata"
            );
        }
        let record: crate::model_audit::AuditRecord = serde_json::from_str(item).unwrap();
        let evidence = record.verification.unwrap();
        assert_eq!(evidence.claim_consistency, "MISMATCH");
        assert_eq!(evidence.identity_assurance, "UNVERIFIED");
        assert_eq!(evidence.active_probe_state, "disabled");
        assert_eq!(evidence.total_tokens, Some(9));
    }
    let last: crate::model_audit::AuditRecord =
        serde_json::from_str(serialized.last().unwrap()).unwrap();
    assert!(last.observation.streaming && last.observation.complete);
    assert!(last.observation.usage_final);

    db.verification
        .set_settings(VerificationSettings::default())
        .unwrap();
    let response = client
        .post(&proxy_url)
        .json(&business(false))
        .send()
        .await
        .unwrap();
    assert_eq!(response.text().await.unwrap(), json_reply);
    assert_eq!(hits.load(Ordering::SeqCst), 1102);
    assert_eq!(count("verification_samples"), 101);
    assert_eq!(count("verification_probes"), 0);
    proxy_task.abort();
    upstream_task.abort();
    let _ = proxy_task.await;
    let _ = upstream_task.await;
}
