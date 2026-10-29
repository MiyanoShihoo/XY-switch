fn gated_anomaly(
    db: &Database,
    p: &Provider,
    active: bool,
    timeout: u64,
) -> (i64, crate::model_verification::Capture) {
    use crate::{model_audit::Observation, model_verification::VerificationSettings};
    db.verification
        .set_settings(VerificationSettings {
            enabled: true,
            active_probe_enabled: active,
            timeout_seconds: timeout,
            ..Default::default()
        })
        .unwrap();
    let protocol = proxy::response_model_protocol(p).unwrap();
    let capture = db
        .verification
        .capture(p, "probe-secret", protocol, &p.model)
        .unwrap();
    let now = crate::model_verification::now_ms();
    let id = db
        .record_request(&RequestEvent {
            provider_id: p.id.clone(),
            route: protocol.into(),
            status_code: Some(200),
            success: true,
            affects_health: false,
            latency_ms: 1,
            retry_count: 0,
            error_kind: None,
            model: Some(p.model.clone()),
            input_tokens: Some(4),
            output_tokens: Some(1),
            cache_read_tokens: None,
            cache_creation_tokens: None,
            cost: None,
            price_source: None,
            price_version: None,
            created_at: now,
        })
        .unwrap()
        .event_id;
    db.verification
        .process(
            id,
            Observation {
                requested_model: Some(p.model.clone()),
                forwarded_model: Some(p.model.clone()),
                upstream_model: Some("unexpected-route".into()),
                upstream_models: vec!["unexpected-route".into()],
                complete: true,
                usage_final: true,
                input_tokens: Some(4),
                output_tokens: Some(1),
                total_tokens: Some(5),
                protocol: Some(protocol.into()),
                protocol_shape: Some(protocol.into()),
                verification_capture: Some(capture.clone()),
                ..Default::default()
            },
            "passive",
        )
        .unwrap()
        .unwrap();
    db.verification.approve(&capture, true).unwrap();
    (id, capture)
}
fn probe_count(db: &Database) -> u64 {
    db.conn()
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM verification_probes", [], |r| r.get(0))
        .unwrap()
}
async fn slow_probe_server() -> (
    String,
    Arc<std::sync::atomic::AtomicUsize>,
    tokio::task::JoinHandle<()>,
) {
    let seen = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = seen.clone();
    let app = Router::new().fallback(move |Json(body): Json<Value>| {
        let counter = counter.clone();
        async move {
            assert_eq!(body["messages"][0]["content"], "Reply OK.");
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            tokio::time::sleep(Duration::from_secs(5)).await;
            Json(serde_json::from_str::<Value>(&chat(Some("expected"))).unwrap())
        }
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (url, seen, task)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn real_http_gates_budgets_errors_cancellation_and_supported_protocol_rechecks() {
    use std::sync::atomic::Ordering;
    // All operations below use the exact production guard, not the parser-only helper.
    let (url, seen, task) = server(vec![(200, "application/json", chat(Some("expected")))]).await;
    let p = provider(&url, "openai_chat");
    let db = database(&p);
    db.verification.set_settings(Default::default()).unwrap();
    assert_eq!(
        guarded_probe(&db, &p, "probe-secret", &p.model, 1)
            .await
            .err()
            .unwrap(),
        "active_probing_disabled"
    );
    let (id, _) = gated_anomaly(&db, &p, false, 15);
    for _ in 0..100 {
        assert!(
            guarded_probe(&db, &p, "probe-secret", &p.model, id)
                .await
                .is_err()
        );
    }
    assert_eq!(seen.lock().unwrap().len(), 0);
    assert_eq!(probe_count(&db), 0);
    let (id, cap) = gated_anomaly(&db, &p, true, 15);
    let barrier = Arc::new(tokio::sync::Barrier::new(50));
    let mut calls = Vec::new();
    for _ in 0..50 {
        let db = db.clone();
        let p = p.clone();
        let b = barrier.clone();
        calls.push(tokio::spawn(async move {
            b.wait().await;
            guarded_probe(&db, &p, "probe-secret", &p.model, id)
                .await
                .is_ok()
        }));
    }
    let mut successes = 0;
    for call in calls {
        successes += usize::from(call.await.unwrap());
    }
    assert_eq!(successes, 1);
    assert_eq!(seen.lock().unwrap().len(), 1);
    assert_eq!(probe_count(&db), 1);
    let status = db.verification.status(&p.id, &p.model, Some(&cap)).unwrap();
    assert_eq!(status.budget_used_today, 5);
    assert_eq!(status.probe_tokens_today, Some(5));
    let evidence = status
        .recent
        .iter()
        .find(|e| e.observation_id == id.to_string())
        .unwrap();
    assert_eq!(evidence.active_probe_state, "completed");
    assert_eq!(evidence.active_probe_tokens, Some(5));
    assert!(
        guarded_probe(&db, &p, "probe-secret", &p.model, id)
            .await
            .is_err()
    );
    assert_eq!(seen.lock().unwrap().len(), 1);
    task.abort();

    for status_code in [429, 500] {
        let (url, seen, task) = server(vec![(
            status_code,
            "application/json",
            r#"{"error":{"message":"PRIVATE_UPSTREAM_TEXT"}}"#.into(),
        )])
        .await;
        let p = provider(&url, "openai_chat");
        let db = database(&p);
        let (id, cap) = gated_anomaly(&db, &p, true, 15);
        let result = guarded_probe(&db, &p, "probe-secret", &p.model, id)
            .await
            .unwrap();
        assert!(!result.success);
        assert_eq!(result.http_status, Some(status_code));
        assert!(
            !serde_json::to_string(&result)
                .unwrap()
                .contains("PRIVATE_UPSTREAM_TEXT")
        );
        assert!(
            guarded_probe(&db, &p, "probe-secret", &p.model, id)
                .await
                .is_err()
        );
        assert_eq!(seen.lock().unwrap().len(), 1);
        let status = db.verification.status(&p.id, &p.model, Some(&cap)).unwrap();
        assert_eq!(status.probe_requests_today, 1);
        assert_eq!(status.budget_used_today, 40);
        assert_eq!(status.probe_tokens_today, None);
        task.abort();
    }
    // Actual timeout occupies the quota; it is not treated as a free failed request.
    let (url, seen, task) = slow_probe_server().await;
    let p = provider(&url, "openai_chat");
    let db = database(&p);
    let (id, cap) = gated_anomaly(&db, &p, true, 1);
    let result = guarded_probe(&db, &p, "probe-secret", &p.model, id)
        .await
        .unwrap();
    assert!(!result.success);
    assert!(result.error_message.unwrap().contains("超时"));
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    let status = db.verification.status(&p.id, &p.model, Some(&cap)).unwrap();
    assert_eq!(status.budget_used_today, 40);
    assert_eq!(status.probe_tokens_today, None);
    task.abort();

    let (url, seen, task) = slow_probe_server().await;
    let p = provider(&url, "openai_chat");
    let db = database(&p);
    let (id, _) = gated_anomaly(&db, &p, true, 15);
    let task_db = db.clone();
    let task_provider = p.clone();
    let probe = tokio::spawn(async move {
        guarded_probe(
            &task_db,
            &task_provider,
            "probe-secret",
            &task_provider.model,
            id,
        )
        .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        while seen.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    db.verification.set_settings(Default::default()).unwrap();
    let result = tokio::time::timeout(Duration::from_secs(2), probe)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(!result.success);
    assert_eq!(seen.load(Ordering::SeqCst), 1);
    let samples: u64 = db
        .conn()
        .lock()
        .unwrap()
        .query_row("SELECT COUNT(*) FROM verification_samples", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(samples, 1);
    assert_eq!(probe_count(&db), 1);
    task.abort();

    for (format, model, reply, output_field) in [
        (
            "openai_responses",
            "expected",
            json!({"model":"expected","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}],"usage":{"input_tokens":4,"output_tokens":1,"total_tokens":5}}),
            "max_output_tokens",
        ),
        (
            "anthropic",
            "expected",
            json!({"model":"expected","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":4,"output_tokens":1}}),
            "max_tokens",
        ),
    ] {
        let (url, seen, task) = server(vec![(200, "application/json", reply.to_string())]).await;
        let mut p = provider(&url, format);
        p.model = model.into();
        let db = database(&p);
        let (id, cap) = gated_anomaly(&db, &p, true, 15);
        let result = guarded_probe(&db, &p, "probe-secret", &p.model, id)
            .await
            .unwrap();
        assert!(result.success, "{format}: {result:?}");
        assert_eq!(
            result
                .audit
                .verification
                .as_ref()
                .unwrap()
                .identity_assurance,
            "UNVERIFIED"
        );
        let records = seen.lock().unwrap();
        assert_eq!(records.len(), 1);
        let sent = &records[0]["body"];
        assert_eq!(sent[output_field], 8);
        for field in [
            "tools",
            "system",
            "systemInstruction",
            "instructions",
            "cachedContent",
        ] {
            assert!(sent.get(field).is_none());
        }
        assert_eq!(
            db.verification
                .status(&p.id, model, Some(&cap))
                .unwrap()
                .probe_tokens_today,
            Some(5)
        );
        task.abort();
    }
    // Gemini is a supported client/metadata format, not a selectable native upstream.
    // Keep that existing business boundary: unsupported routes fail before reservation.
    let p = provider("http://127.0.0.1:1", "gemini");
    let db = Database::open(Path::new(":memory:")).unwrap();
    db.verification
        .set_settings(crate::model_verification::VerificationSettings {
            enabled: true,
            active_probe_enabled: true,
            ..Default::default()
        })
        .unwrap();
    assert!(db.save_provider(&p).is_err());
    assert!(
        guarded_probe(&db, &p, "probe-secret", &p.model, 1)
            .await
            .is_err()
    );
    assert_eq!(probe_count(&db), 0);
}
