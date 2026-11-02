use super::*;
use crate::governance::tasks;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn wait_for(mut condition: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(3), async {
        while !condition() {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
}

fn only_task(fixture: &Fixture) -> tasks::TaskDetail {
    let tasks = tasks::list(&fixture.db).unwrap();
    assert_eq!(tasks.len(), 1);
    tasks::detail(&fixture.db, &tasks[0].id).unwrap()
}

fn health(fixture: &Fixture) -> Value {
    serde_json::to_value(fixture.db.provider_health("gov-test").unwrap()).unwrap()
}

async fn assert_rejected(fixture: &Fixture, task: &str, status: u16, reason: &str) {
    let calls = fixture.calls();
    let ledger = budget::status(&fixture.db).unwrap().attempts;
    let previous_health = health(fixture);
    let response = fixture.post_task(task).send().await.unwrap();
    assert_eq!(response.status(), status);
    assert!(response.text().await.unwrap().contains(reason));
    assert_eq!(
        (
            fixture.calls(),
            budget::status(&fixture.db).unwrap().attempts
        ),
        (calls, ledger)
    );
    assert_eq!(health(fixture), previous_health);
}

#[tokio::test]
async fn retries_failover_and_client_retry_share_one_persistent_task_and_send_ledger() {
    let fixture = Fixture::new(
        vec![
            Reply::Error(503),
            Reply::Error(503),
            Reply::Error(503),
            Reply::Good,
            Reply::Good,
        ],
        strict(),
    )
    .await;
    let mut policy = fixture::budget(&fixture.db);
    policy.task_max_attempts = 5;
    governance::save(&fixture.db, budget::KEY, &policy).unwrap();
    let mut fallback = fixture::provider(&fixture.upstream_base);
    fallback.id = "fallback".into();
    fallback.sort_order = 2;
    fixture.db.save_provider(&fallback).unwrap();
    let first = fixture
        .post_task("combined-retry-task")
        .send()
        .await
        .unwrap();
    assert_eq!(first.status(), 200);
    first.bytes().await.unwrap();
    let second = fixture
        .post_task("combined-retry-task")
        .send()
        .await
        .unwrap();
    assert_eq!(second.status(), 200);
    second.bytes().await.unwrap();
    let task = only_task(&fixture);
    assert_eq!(
        (
            fixture.calls(),
            task.task.attempts,
            task.task.reserved_microusd
        ),
        (5, 5, 5280)
    );
    assert_eq!(
        task.attempts
            .iter()
            .map(|attempt| attempt.ordinal)
            .collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    assert!(
        task.attempts[..3]
            .iter()
            .all(|attempt| attempt.provider_id == "gov-test" && attempt.status == "http_error")
    );
    assert_eq!(task.attempts[3].provider_id, "fallback");
    assert!(
        task.attempts[..4]
            .iter()
            .all(|attempt| attempt.request_id == task.attempts[0].request_id)
    );
    assert_ne!(task.attempts[4].request_id, task.attempts[0].request_id);
    assert!(
        task.attempts
            .iter()
            .all(|attempt| attempt.event_id.is_some())
    );
    assert_rejected(
        &fixture,
        "combined-retry-task",
        402,
        "task_attempt_budget_exhausted",
    )
    .await;
    assert_eq!(only_task(&fixture).task.attempts, 5);
}

#[tokio::test]
async fn concurrent_socket_requests_obey_exact_attempt_and_microusd_boundaries() {
    for cost_limit in [false, true] {
        let fixture = Fixture::new(
            vec![],
            access::AccessPolicy {
                max_in_flight: 64,
                ..strict()
            },
        )
        .await;
        let mut policy = fixture::budget(&fixture.db);
        if cost_limit {
            policy.task_max_microusd = 3168;
        } else {
            policy.task_max_attempts = 3;
        }
        governance::save(&fixture.db, budget::KEY, &policy).unwrap();
        let replies = futures_util::future::join_all((0..24).map(|_| async {
            let response = fixture
                .post_task("concurrent-socket-task")
                .send()
                .await
                .unwrap();
            let status = response.status().as_u16();
            response.bytes().await.unwrap();
            status
        }))
        .await;
        assert_eq!(replies.iter().filter(|&&status| status == 200).count(), 3);
        assert_eq!(replies.iter().filter(|&&status| status == 402).count(), 21);
        let task = only_task(&fixture);
        assert_eq!(
            (
                fixture.calls(),
                task.task.attempts,
                task.task.reserved_microusd
            ),
            (3, 3, 3168)
        );
        assert_eq!(budget::status(&fixture.db).unwrap().attempts, 3);
        assert_eq!(task.task.active_attempts, 0);
    }
}

#[tokio::test]
async fn cancellation_stops_all_pending_requests_and_future_client_retries() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let fixture = Fixture::new(
        vec![Reply::Wait(gate.clone()), Reply::Wait(gate.clone())],
        strict(),
    )
    .await;
    let first = tokio::spawn(fixture.post_task("pending-cancel-task").send());
    let second = tokio::spawn(fixture.post_task("pending-cancel-task").send());
    wait_for(|| fixture.calls() == 2).await;
    let id = only_task(&fixture).task.id;
    tasks::cancel(&fixture.db, &id).unwrap();
    for pending in [first, second] {
        let response = tokio::time::timeout(Duration::from_secs(2), pending)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), 408);
        response.bytes().await.unwrap();
    }
    gate.add_permits(2);
    let task = only_task(&fixture);
    assert!(task.task.cancelled);
    assert_eq!(
        (
            task.task.attempts,
            task.task.active_attempts,
            task.task.unknown_cost_attempts
        ),
        (2, 0, 2)
    );
    assert!(
        task.attempts
            .iter()
            .all(|attempt| attempt.status == "cancelled")
    );
    assert_rejected(&fixture, "pending-cancel-task", 408, "request_cancelled").await;
}

#[tokio::test]
async fn cancellation_during_backoff_prevents_retry_dispatch() {
    let fixture = Fixture::new(vec![Reply::Error(503), Reply::Good], strict()).await;
    let pause = Arc::new(tasks::RetryPause::default());
    *fixture.db.tasks.retry_pause.lock().unwrap() = Some(pause.clone());
    let pending = tokio::spawn(fixture.post_task("backoff-cancel-task").send());
    tokio::time::timeout(Duration::from_secs(3), pause.entered.notified())
        .await
        .unwrap();
    let before = only_task(&fixture);
    assert_eq!(before.task.attempts, 1);
    assert_eq!(before.attempts[0].status, "http_error");
    assert_eq!(fixture.calls(), 1);
    tasks::cancel(&fixture.db, &before.task.id).unwrap();
    let response = tokio::time::timeout(Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), 408);
    response.bytes().await.unwrap();
    assert_eq!(fixture.calls(), 1);
    assert_eq!(budget::status(&fixture.db).unwrap().attempts, 1);
    assert_rejected(&fixture, "backoff-cancel-task", 408, "request_cancelled").await;
}

#[tokio::test]
async fn cancellation_during_stream_keeps_unknown_reservation_and_blocks_resend() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let fixture = Fixture::new(vec![Reply::Hold(gate.clone())], strict()).await;
    let mut payload = body();
    payload["stream"] = json!(true);
    let mut response = fixture
        .post_task("stream-cancel-task")
        .json(&payload)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    assert!(response.chunk().await.unwrap().is_some());
    let id = only_task(&fixture).task.id;
    tasks::cancel(&fixture.db, &id).unwrap();
    let _ = tokio::time::timeout(Duration::from_secs(2), response.bytes())
        .await
        .unwrap();
    gate.add_permits(1);
    let task = only_task(&fixture);
    assert!(task.task.cancelled);
    assert_eq!(
        (
            task.task.attempts,
            task.task.active_attempts,
            task.task.reserved_microusd
        ),
        (1, 0, 1056)
    );
    assert_eq!(task.attempts[0].cost_state, "unknown");
    assert_rejected(&fixture, "stream-cancel-task", 408, "request_cancelled").await;
}

async fn raw_request(fixture: &Fixture, task: &str) -> tokio::net::TcpStream {
    let mut stream = tokio::net::TcpStream::connect(fixture.base.trim_start_matches("http://"))
        .await
        .unwrap();
    let body = body().to_string();
    let request = format!(
        "POST /v1/chat/completions HTTP/1.1\r\nHost: {}\r\nAuthorization: Bearer {}\r\nX-XY-Task-ID: {task}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        fixture.base.trim_start_matches("http://"),
        fixture.token,
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    stream
}

#[tokio::test]
async fn tcp_disconnect_while_waiting_cancels_task_before_any_retry() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let fixture = Fixture::new(vec![Reply::Wait(gate.clone())], strict()).await;
    let stream = raw_request(&fixture, "disconnect-task").await;
    wait_for(|| fixture.calls() == 1).await;
    drop(stream);
    wait_for(|| only_task(&fixture).task.cancelled).await;
    gate.add_permits(1);
    assert_eq!(only_task(&fixture).task.active_attempts, 0);
    assert_rejected(&fixture, "disconnect-task", 408, "request_cancelled").await;
}

#[tokio::test]
async fn http1_write_eof_cancels_pending_task_consistently_with_connection_close() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let fixture = Fixture::new(vec![Reply::Wait(gate.clone())], strict()).await;
    let mut stream = raw_request(&fixture, "write-eof-task").await;
    wait_for(|| fixture.calls() == 1).await;
    stream.shutdown().await.unwrap();
    wait_for(|| only_task(&fixture).task.cancelled).await;
    let mut response = Vec::new();
    let _ = tokio::time::timeout(Duration::from_secs(2), stream.read_to_end(&mut response))
        .await
        .unwrap();
    gate.add_permits(1);
    assert_rejected(&fixture, "write-eof-task", 408, "request_cancelled").await;
}

#[tokio::test]
async fn timeout_and_missing_usage_keep_reserved_cost_and_stop_at_task_limit() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let fixture = Fixture::with_options(
        vec![Reply::Wait(gate)],
        strict(),
        Duration::from_millis(80),
        "unit-upstream-key",
    )
    .await;
    let mut policy = fixture::budget(&fixture.db);
    policy.task_max_attempts = 1;
    governance::save(&fixture.db, budget::KEY, &policy).unwrap();
    let response = fixture.post_task("timeout-task").send().await.unwrap();
    assert_eq!(response.status(), 402);
    response.bytes().await.unwrap();
    let task = only_task(&fixture);
    assert_eq!(
        (
            fixture.calls(),
            task.task.attempts,
            task.task.reserved_microusd
        ),
        (1, 1, 1056)
    );
    assert_eq!(
        (
            task.attempts[0].status.as_str(),
            task.attempts[0].cost_state.as_str()
        ),
        ("timeout", "unknown")
    );
    assert_rejected(
        &fixture,
        "timeout-task",
        402,
        "task_attempt_budget_exhausted",
    )
    .await;

    let fixture = Fixture::new(vec![Reply::MissingUsage, Reply::PartialUsage], strict()).await;
    let mut policy = fixture::budget(&fixture.db);
    policy.task_max_microusd = 2112;
    governance::save(&fixture.db, budget::KEY, &policy).unwrap();
    for _ in 0..2 {
        let response = fixture
            .post_task("incomplete-usage-task")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        response.bytes().await.unwrap();
    }
    let task = only_task(&fixture);
    assert_eq!(
        (task.task.reserved_microusd, task.task.unknown_cost_attempts),
        (2112, 2)
    );
    assert_eq!(task.attempts[0].usage_state, "missing");
    assert_eq!(task.attempts[1].usage_state, "partial");
    assert_rejected(
        &fixture,
        "incomplete-usage-task",
        402,
        "task_cost_budget_exhausted",
    )
    .await;
}

#[tokio::test]
async fn initial_quota_and_request_build_rejections_add_no_send_reservation_or_health_failure() {
    let fixture = Fixture::new(vec![], strict()).await;
    let mut policy = fixture::budget(&fixture.db);
    policy.task_max_attempts = 0;
    governance::save(&fixture.db, budget::KEY, &policy).unwrap();
    assert_rejected(
        &fixture,
        "zero-attempt-task",
        402,
        "task_attempt_budget_exhausted",
    )
    .await;
    assert_eq!((fixture.calls(), only_task(&fixture).task.attempts), (0, 0));
    let fixture =
        Fixture::with_options(vec![], strict(), Duration::from_secs(5), "bad\ncredential").await;
    assert_rejected(&fixture, "build-error-task", 400, "request_build").await;
    assert_eq!((fixture.calls(), only_task(&fixture).task.attempts), (0, 0));
    assert_eq!(budget::status(&fixture.db).unwrap().attempts, 0);
}
