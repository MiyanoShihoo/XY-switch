use super::*;
use crate::database::RequestEvent;
use std::path::Path;
use axum::body::Bytes;
use futures_util::{StreamExt, stream, stream::BoxStream};

fn database() -> Database {
    Database::open(Path::new(":memory:")).unwrap()
}

fn sent_diagnostics() -> RequestDiagnostics {
    RequestDiagnostics {
        send_attempts: 1,
        upstream_response_received: true,
        attempts: vec![RequestAttempt { status_code: Some(200), error_kind: None }],
        ..Default::default()
    }
}

fn event(success: bool, created_at: i64) -> RequestEvent {
    RequestEvent {
        provider_id: "openai".into(),
        route: "/v1/responses".into(),
        status_code: Some(if success { 200 } else { 503 }),
        success,
        affects_health: true,
        diagnostics: Some(sent_diagnostics()),
        latency_ms: 125,
        retry_count: 0,
        error_kind: (!success).then(|| "upstream_status".into()),
        model: None,
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        cost: None,
        price_source: None,
        price_version: None,
        created_at,
    }
}

fn health(database: &Database) -> serde_json::Value {
    serde_json::to_value(database.provider_health("openai").unwrap()).unwrap()
}

fn saved(database: &Database, event_id: i64) -> (bool, Option<String>, Option<RequestDiagnostics>, bool) {
    database.conn().lock().unwrap().query_row(
        "SELECT success,error_kind,diagnostics_json,affects_health FROM request_events WHERE id=?1",
        [event_id],
        |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, Option<String>>(2)?
            .map(|json| serde_json::from_str(&json).unwrap()), row.get(3)?)),
    ).unwrap()
}

#[test]
fn local_rejections_preserve_health_timestamps_latency_rate_and_circuit() {
    let database = database();
    for created_at in 1..=3 { database.record_request(&event(false, created_at)).unwrap(); }
    database.set_circuit_open_until("openai", Some(9_000_000)).unwrap();
    let before = health(&database);
    let mut rejection = event(false, 100);
    rejection.status_code = Some(402);
    rejection.error_kind = Some("budget_rejected".into());
    rejection.affects_health = false;
    rejection.diagnostics = Some(RequestDiagnostics::default());
    for _ in 0..4 { database.record_request(&rejection).unwrap(); }
    assert_eq!(health(&database), before);
    let total_tokens = database.conn().lock().unwrap().query_row(
        "SELECT total_tokens FROM request_events ORDER BY id DESC LIMIT 1", [],
        |row| row.get::<_, Option<i64>>(0),
    ).unwrap();
    assert_eq!(total_tokens, Some(0));
}

#[test]
fn unassigned_requests_preserve_foreign_keys_without_creating_a_provider() {
    let database = database();
    let before = database.list_providers().unwrap().len();
    let mut diagnostic = RequestDiagnostics::default();
    diagnostic.fail("provider_selection", "provider_not_selected");
    let result = database.record_unassigned_request("/v1/responses", 503, 2,
        "provider_not_selected", &diagnostic, 123).unwrap();
    assert!(result.event_id < 0);
    assert_eq!(database.list_providers().unwrap().len(), before);
    let connection = database.conn();
    let connection = connection.lock().unwrap();
    assert_eq!(connection.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |row| row.get::<_, i64>(0)).unwrap(), 0);
    let row = connection.query_row(
        "SELECT provider_id,input_tokens,output_tokens,total_tokens,cost,price_source,affects_health FROM usage_request_events WHERE id=?1",
        [result.event_id],
        |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?, row.get::<_, i64>(2)?,
            row.get::<_, i64>(3)?, row.get::<_, f64>(4)?, row.get::<_, String>(5)?, row.get::<_, bool>(6)?)),
    ).unwrap();
    assert_eq!(row, (String::new(), 0, 0, 0, 0.0, "local_rejection".into(), false));
    drop(connection);
    let mut invalid = event(true, 200);
    invalid.provider_id = "unassigned-provider-must-not-exist".into();
    assert!(database.record_request(&invalid).is_err());
}

#[test]
fn failure_after_http_200_keeps_dispatch_and_response_evidence() {
    let database = database();
    let result = database.record_request(&event(true, 100)).unwrap();
    database.mark_request_failed(result.event_id, "response_stream", "stream_interrupted", true).unwrap();
    let (success, code, diagnostic, affects_health) = saved(&database, result.event_id);
    assert!(!success);
    assert_eq!(code.as_deref(), Some("stream_interrupted"));
    let diagnostic = diagnostic.unwrap();
    assert_eq!(diagnostic.failure_stage.as_deref(), Some("response_stream"));
    assert_eq!(diagnostic.send_attempts, 1);
    assert!(diagnostic.upstream_response_received);
    assert_eq!(diagnostic.attempts[0].status_code, Some(200));
    assert!(affects_health);
    assert_eq!(health(&database)["status"], "unhealthy");
    assert_eq!(database.conn().lock().unwrap().query_row(
        "SELECT status_code FROM request_events WHERE id=?1", [result.event_id], |row| row.get::<_, i64>(0)).unwrap(), 200);
}

#[test]
fn an_older_stream_failure_does_not_replace_a_newer_success() {
    let database = database();
    let older = database.record_request(&event(true, 100)).unwrap();
    let mut newer = event(true, 200);
    newer.latency_ms = 42;
    database.record_request(&newer).unwrap();
    database.mark_request_failed(older.event_id, "response_stream", "stream_interrupted", true).unwrap();
    let health = health(&database);
    assert_eq!(health["status"], "healthy");
    assert_eq!(health["consecutiveFailures"], 0);
    assert_eq!(health["lastCheckedAt"], 200);
    assert_eq!(health["latencyMs"], 42);
    assert_eq!(health["successRate"], 50.0);
    assert!(health["circuitOpenUntil"].is_null());
}

#[test]
fn local_response_conversion_failure_does_not_penalize_provider() {
    let database = database();
    database.record_request(&event(true, 100)).unwrap();
    let before = health(&database);
    let result = database.record_request(&event(true, 200)).unwrap();
    database.mark_request_failed(result.event_id, "response_conversion", "response_conversion", false).unwrap();
    assert_eq!(health(&database), before);
    assert!(!saved(&database, result.event_id).3);
    assert!(saved(&database, result.event_id).2.unwrap().upstream_response_received);
}

#[test]
fn cancellation_does_not_count_as_an_upstream_failure() {
    let database = database();
    database.record_request(&event(true, 100)).unwrap();
    let before = health(&database);
    let result = database.record_request(&event(true, 200)).unwrap();
    database.mark_request_failed(result.event_id, "client_response", "stream_cancelled", false).unwrap();
    assert_eq!(health(&database), before);
    assert_eq!(saved(&database, result.event_id).1.as_deref(), Some("stream_cancelled"));
}

#[test]
fn failed_http_response_is_not_reclassified_when_its_stream_ends() {
    let database = database();
    let mut failed = event(false, 100);
    let diagnostic = failed.diagnostics.as_mut().unwrap();
    diagnostic.fail("upstream_http", "upstream_status");
    diagnostic.attempts[0].status_code = Some(503);
    let result = database.record_request(&failed).unwrap();
    let before = saved(&database, result.event_id);
    database.mark_request_failed(result.event_id, "response_stream", "stream_interrupted", true).unwrap();
    assert_eq!(saved(&database, result.event_id), before);
}

#[test]
fn legacy_request_diagnostics_stay_unknown_after_correction() {
    let database = database();
    let mut legacy = event(true, 100);
    legacy.diagnostics = None;
    let result = database.record_request(&legacy).unwrap();
    database.mark_request_failed(result.event_id, "response_read", "response_error", true).unwrap();
    assert!(saved(&database, result.event_id).2.is_none());
    let total_tokens = database.conn().lock().unwrap().query_row(
        "SELECT total_tokens FROM request_events WHERE id=?1", [result.event_id],
        |row| row.get::<_, Option<i64>>(0),
    ).unwrap();
    assert_eq!(total_tokens, None);
}

#[test]
fn diagnostic_migration_is_idempotent_and_keeps_legacy_records() {
    let database = database();
    let mut legacy = event(true, 100);
    legacy.diagnostics = None;
    let result = database.record_request(&legacy).unwrap();
    let connection = database.conn();
    let connection = connection.lock().unwrap();
    migrate(&connection).unwrap();
    migrate(&connection).unwrap();
    let row = connection.query_row("SELECT COUNT(*),diagnostics_json FROM usage_request_events WHERE id=?1",
        [result.event_id], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, Option<String>>(1)?))).unwrap();
    assert_eq!(row, (1, None));
}

fn observed(upstream: impl futures_util::Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static)
    -> (Database, i64, BoxStream<'static, Result<Bytes, reqwest::Error>>) {
    let database = database();
    let event_id = database.record_request(&event(true, 100)).unwrap().event_id;
    let observed = crate::streaming::audit_capture::observe_stream(
        upstream, crate::streaming::UpstreamFormat::Responses,
        crate::model_audit::Observation::default(), "fixture-model".into(), database.clone(),
        Some(event_id), std::time::Instant::now(),
    ).boxed();
    (database, event_id, observed)
}

fn partial_frame() -> Bytes {
    Bytes::from_static(b"event: response.output_text.delta\ndata: {\"type\":\"response.output_text.delta\",\"delta\":\"partial\"}\n\n")
}

fn final_frame() -> Bytes {
    Bytes::from_static(b"event: response.completed\ndata: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":2,\"output_tokens\":1}}}\n\n")
}

#[tokio::test]
async fn stream_eof_without_terminal_frame_is_recorded_as_interrupted() {
    let (database, event_id, mut observed) = observed(stream::iter(vec![Ok(partial_frame())]));
    assert_eq!(observed.next().await.unwrap().unwrap(), partial_frame());
    assert!(observed.next().await.is_none());
    assert_eq!(saved(&database, event_id).1.as_deref(), Some("stream_interrupted"));
    assert_eq!(saved(&database, event_id).2.unwrap().failure_stage.as_deref(), Some("response_stream"));
}

#[tokio::test]
async fn stream_transport_error_is_not_recorded_as_client_cancellation() {
    let transport_error = reqwest::Client::new().get("http://[invalid").build().unwrap_err();
    let (database, event_id, mut observed) = observed(stream::iter(vec![Ok(partial_frame()), Err(transport_error)]));
    assert_eq!(observed.next().await.unwrap().unwrap(), partial_frame());
    assert!(observed.next().await.unwrap().is_err());
    drop(observed);
    assert_eq!(saved(&database, event_id).1.as_deref(), Some("stream_interrupted"));
    assert_eq!(health(&database)["consecutiveFailures"], 1);
}

#[tokio::test]
async fn closing_a_partial_stream_records_cancellation_without_health_penalty() {
    let transport = stream::iter(vec![Ok(partial_frame())]).chain(stream::pending());
    let (database, event_id, mut observed) = observed(transport);
    assert_eq!(observed.next().await.unwrap().unwrap(), partial_frame());
    drop(observed);
    assert_eq!(saved(&database, event_id).1.as_deref(), Some("stream_cancelled"));
    assert!(!saved(&database, event_id).3);
    assert_eq!(health(&database)["consecutiveFailures"], 0);
}

#[tokio::test]
async fn closing_after_a_complete_terminal_frame_keeps_request_successful() {
    let transport = stream::iter(vec![Ok(final_frame())]).chain(stream::pending());
    let (database, event_id, mut observed) = observed(transport);
    assert_eq!(observed.next().await.unwrap().unwrap(), final_frame());
    drop(observed);
    let (success, code, diagnostic, _) = saved(&database, event_id);
    assert!(success);
    assert!(code.is_none());
    assert!(diagnostic.unwrap().failure_stage.is_none());
}

#[tokio::test]
async fn transport_error_after_protocol_completion_keeps_request_successful() {
    let transport_error = reqwest::Client::new().get("http://[invalid").build().unwrap_err();
    let (database, event_id, mut observed) = observed(stream::iter(vec![Ok(final_frame()), Err(transport_error)]));
    assert!(observed.next().await.unwrap().is_ok());
    assert!(observed.next().await.unwrap().is_err());
    drop(observed);
    assert!(saved(&database, event_id).0);
    assert!(saved(&database, event_id).1.is_none());
}
