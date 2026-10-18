use super::*;
use crate::{
    database::{Database, Provider, RequestEvent, RequestUsage},
    model_audit::Observation,
    streaming::{UpstreamFormat, audit_capture::Collector},
};
use serde_json::json;
use std::{
    path::Path,
    sync::{Arc, Barrier},
};

fn setup(enabled: bool, active: bool) -> (Database, Provider) {
    let db = Database::open(Path::new(":memory:")).unwrap();
    let p:Provider=serde_json::from_value(json!({"id":"verify-test","name":"Test","endpoint":"http://127.0.0.1:1","model":"expected","enabled":true,"advanced":{"upstreamFormat":"openai_chat"}})).unwrap();
    db.save_provider(&p).unwrap();
    if enabled || active {
        db.verification
            .set_settings(VerificationSettings {
                enabled,
                active_probe_enabled: active,
                ..Default::default()
            })
            .unwrap();
    }
    (db, p)
}
fn event(db: &Database, p: &Provider, at: i64) -> i64 {
    db.record_request(&RequestEvent {
        provider_id: p.id.clone(),
        route: "chat".into(),
        status_code: Some(200),
        success: true,
        affects_health: false,
        latency_ms: 1,
        retry_count: 0,
        error_kind: None,
        model: Some("expected".into()),
        input_tokens: Some(4),
        output_tokens: Some(1),
        cache_creation_tokens: None,
        cache_read_tokens: None,
        cost: None,
        price_source: None,
        price_version: None,
        created_at: at,
    })
    .unwrap()
    .event_id
}
fn observed(capture: Option<Capture>, reported: &str) -> Observation {
    Collector::new(UpstreamFormat::OpenAi,Observation{requested_model:Some("expected".into()),forwarded_model:Some("expected".into()),verification_capture:capture,..Default::default()})
        .non_stream(&json!({"model":reported,"id":"resp-private-diagnostic","choices":[{"message":{"content":"PRIVATE_REPLY"},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}}))
}
fn sample(
    db: &Database,
    p: &Provider,
    key: &str,
    reported: &str,
    at: i64,
) -> (i64, Capture, VerificationEvidence) {
    let cap = db.verification.capture(p, key, "chat", "expected").unwrap();
    let id = event(db, p, at);
    let audit = db
        .verification
        .process(id, observed(Some(cap.clone()), reported), "passive")
        .unwrap()
        .unwrap();
    (id, cap, audit.verification.unwrap())
}
fn count(db: &Database, table: &str) -> i64 {
    db.conn()
        .lock()
        .unwrap()
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
        .unwrap()
}

#[test]
fn defaults_and_a_thousand_business_responses_add_no_verification_metadata_or_probes() {
    let (db, p) = setup(false, false);
    let settings = db.verification.settings().unwrap();
    assert!(!settings.enabled && !settings.active_probe_enabled);
    assert_eq!(settings.max_requests_per_day, 1);
    assert_eq!(settings.soft_token_budget_per_day, 100);
    assert_eq!(settings.max_output_tokens, 8);
    assert_eq!(settings.timeout_seconds, 15);
    assert_eq!(settings.cooldown_hours, 24);
    for _ in 0..1000 {
        let cap = db.verification.capture(&p, "SECRET", "chat", "expected");
        assert!(cap.is_none());
        let id = event(&db, &p, now_ms());
        db.update_request_usage(
            id,
            &RequestUsage {
                model: Some("expected".into()),
                audit: Some(crate::model_audit::audit(observed(cap, "other"))),
                ..Default::default()
            },
        )
        .unwrap();
    }
    assert_eq!(count(&db, "request_events"), 1000);
    assert_eq!(count(&db, "verification_samples"), 0);
    assert_eq!(count(&db, "verification_probes"), 0);
    let n:i64=db.conn().lock().unwrap().query_row("SELECT COUNT(*) FROM request_events WHERE audit_json IS NOT NULL OR requested_model IS NOT NULL OR upstream_model IS NOT NULL",[],|r|r.get(0)).unwrap();
    assert_eq!(n, 0);
}
#[test]
fn one_hundred_anomalies_cannot_spend_when_active_opt_in_is_off() {
    let (db, p) = setup(true, false);
    for _ in 0..100 {
        let (id, cap, e) = sample(&db, &p, "SECRET", "different", now_ms());
        assert!(e.eligible_anomaly());
        assert!(db.verification.reserve(cap, id, now_ms()).is_err());
    }
    assert_eq!(count(&db, "verification_samples"), 100);
    assert_eq!(count(&db, "verification_probes"), 0);
}
#[test]
fn fifty_concurrent_anomalies_share_one_atomic_reservation() {
    let (db, p) = setup(true, true);
    let (id, cap, _) = sample(&db, &p, "SECRET", "different", now_ms());
    db.verification.approve(&cap, true).unwrap();
    let barrier = Arc::new(Barrier::new(50));
    let threads: Vec<_> = (0..50)
        .map(|_| {
            let db = db.clone();
            let cap = cap.clone();
            let b = barrier.clone();
            std::thread::spawn(move || {
                b.wait();
                db.verification.reserve(cap, id, now_ms()).is_ok()
            })
        })
        .collect();
    assert_eq!(
        threads
            .into_iter()
            .map(|t| usize::from(t.join().unwrap()))
            .sum::<usize>(),
        1
    );
    assert_eq!(count(&db, "verification_probes"), 1);
}
#[test]
fn quotas_require_anomaly_cost_confirmation_and_never_probe_active_results() {
    let (db, p) = setup(true, true);
    let (id, cap, e) = sample(&db, &p, "SECRET", "expected", now_ms());
    assert!(!e.eligible_anomaly());
    db.verification.approve(&cap, true).unwrap();
    assert!(db.verification.reserve(cap, id, now_ms()).is_err());
    let (id, cap, _) = sample(&db, &p, "SECRET", "wrong", now_ms());
    db.verification.approve(&cap, false).unwrap();
    assert_eq!(
        db.verification
            .reserve(cap.clone(), id, now_ms())
            .err()
            .unwrap(),
        "cost_boundary_unconfirmed"
    );
    db.verification.approve(&cap, true).unwrap();
    let active_id = event(&db, &p, now_ms());
    let active = db
        .verification
        .process(active_id, observed(Some(cap.clone()), "wrong"), "active")
        .unwrap()
        .unwrap();
    assert!(!active.verification.unwrap().eligible_anomaly());
    assert!(db.verification.reserve(cap, active_id, now_ms()).is_err());
    assert_eq!(count(&db, "verification_probes"), 0);
}
#[test]
fn utc_reset_does_not_bypass_cooldown_and_unknown_usage_keeps_reservation() {
    let (db, p) = setup(true, true);
    let clock = std::time::Instant::now();
    let first = 20 * store::DAY_MS + store::DAY_MS - 1_000;
    let (id, cap, _) = sample(&db, &p, "SECRET", "wrong", first);
    db.verification.approve(&cap, true).unwrap();
    let reservation = db.verification.reserve_at(cap, id, first, clock).unwrap();
    assert_eq!(reservation.reserved_tokens, 40);
    db.verification
        .settle(&reservation, None, "failed_or_unknown")
        .unwrap();
    let next_day = first + 2_000;
    let (id, cap, _) = sample(&db, &p, "SECRET", "wrong", next_day);
    assert_eq!(
        db.verification
            .reserve_at(cap, id, next_day, clock + std::time::Duration::from_secs(2))
            .err()
            .unwrap(),
        "cooldown"
    );
    let later = first + store::DAY_MS;
    let (id, cap, _) = sample(&db, &p, "SECRET", "wrong", later);
    let second = db
        .verification
        .reserve_at(
            cap,
            id,
            later,
            clock + std::time::Duration::from_secs(86_400),
        )
        .unwrap();
    db.verification
        .settle(&second, Some(120), "completed")
        .unwrap();
    let conn = db.conn();
    let conn = conn.lock().unwrap();
    let values: Vec<(Option<u64>, u64)> = conn
        .prepare("SELECT billed_tokens,budget_tokens FROM verification_probes ORDER BY created_at")
        .unwrap()
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(values, vec![(None, 40), (Some(120), 120)]);
}
#[test]
fn soft_budget_is_reserved_before_send_and_cannot_be_reset_by_new_credential() {
    let (db, p) = setup(true, true);
    let mut settings = db.verification.settings().unwrap();
    settings.max_requests_per_day = 20;
    settings.cooldown_hours = 1;
    settings.soft_token_budget_per_day = 60;
    db.verification.set_settings(settings).unwrap();
    let clock = std::time::Instant::now();
    let base = store::DAY_MS * 20;
    let (id, cap, _) = sample(&db, &p, "first-secret", "wrong", base);
    db.verification.approve(&cap, true).unwrap();
    let r = db.verification.reserve_at(cap, id, base, clock).unwrap();
    db.verification
        .settle(&r, None, "failed_or_unknown")
        .unwrap();
    let (id, cap, _) = sample(&db, &p, "second-secret", "wrong", base + 3_600_000);
    db.verification.approve(&cap, true).unwrap();
    assert_eq!(
        db.verification
            .reserve_at(
                cap,
                id,
                base + 3_600_000,
                clock + std::time::Duration::from_secs(3600)
            )
            .err()
            .unwrap(),
        "daily_token_budget"
    );
    assert_eq!(count(&db, "verification_probes"), 1);
}
#[test]
fn cache_keys_isolate_credentials_protocol_model_and_route_revisions() {
    let (db, mut p) = setup(true, true);
    let a = db
        .verification
        .capture(&p, "secret-a", "chat", "expected")
        .unwrap();
    db.verification.approve(&a, true).unwrap();
    for b in [
        db.verification
            .capture(&p, "secret-b", "chat", "expected")
            .unwrap(),
        db.verification
            .capture(&p, "secret-a", "responses", "expected")
            .unwrap(),
        db.verification
            .capture(&p, "secret-a", "chat", "other")
            .unwrap(),
    ] {
        assert_ne!(a.context_key, b.context_key);
        assert!(
            !db.verification
                .status(&p.id, "expected", Some(&b))
                .unwrap()
                .capability_confirmed
        );
    }
    p.advanced
        .body_overrides
        .insert("model".into(), json!("new-route"));
    let b = db
        .verification
        .capture(&p, "secret-a", "chat", "expected")
        .unwrap();
    assert_ne!(a.context_key, b.context_key);
    assert_eq!(count(&db, "verification_probes"), 0);
    assert!(!a.credential_fingerprint.contains("secret-a"));
    assert_eq!(
        a.credential_fingerprint,
        db.verification
            .capture(&p, "secret-a", "chat", "expected")
            .unwrap()
            .credential_fingerprint
    );
}
#[tokio::test]
async fn disabling_invalidates_queued_observations_and_notifies_inflight_probe() {
    let (db, p) = setup(true, true);
    let (id, cap, _) = sample(&db, &p, "SECRET", "wrong", now_ms());
    db.verification.approve(&cap, true).unwrap();
    let mut r = db.verification.reserve(cap.clone(), id, now_ms()).unwrap();
    let pending_id = event(&db, &p, now_ms());
    let obs = observed(Some(cap), "late");
    let before = count(&db, "verification_samples");
    db.verification
        .set_settings(VerificationSettings::default())
        .unwrap();
    r.cancellation.changed().await.unwrap();
    assert!(
        db.verification
            .process(pending_id, obs, "passive")
            .unwrap()
            .is_none()
    );
    assert!(db.verification.mark_running(&r).is_err());
    db.verification.settle(&r, None, "cancelled").unwrap();
    assert_eq!(count(&db, "verification_samples"), before);
}
#[test]
fn exact_snapshots_explicit_aliases_and_each_response_keep_independent_identity() {
    let (db, p) = setup(true, false);
    let (_, _, match_e) = sample(&db, &p, "SECRET", "expected", now_ms());
    assert_eq!(match_e.claim_consistency, "MATCH");
    assert_eq!(match_e.identity_assurance, "UNVERIFIED");
    let (_, _, wrong) = sample(&db, &p, "SECRET", "expected-20261018", now_ms() + 1);
    assert_eq!(wrong.claim_consistency, "MISMATCH");
    assert!(wrong.evidence_codes.contains(&"MODEL_ROUTE_CHANGED".into()));
    let mut settings = db.verification.settings().unwrap();
    settings.aliases.push(AliasRule {
        provider_id: p.id.clone(),
        requested_model: "expected".into(),
        reported_model: "expected-20261018".into(),
        user_confirmed: true,
        allow_route_change: false,
    });
    db.verification.set_settings(settings).unwrap();
    let (_, _, alias) = sample(&db, &p, "SECRET", "expected-20261018", now_ms() + 2);
    assert_eq!(alias.claim_consistency, "MATCH");
    assert_eq!(alias.identity_assurance, "UNVERIFIED");
    assert_eq!(alias.rolling_observed_count, 1);
    let cap = db
        .verification
        .capture(&p, "SECRET", "chat", "expected")
        .unwrap();
    let id = event(&db, &p, now_ms());
    let mut o = observed(Some(cap), "expected");
    o.upstream_models.push("other".into());
    let e = db
        .verification
        .process(id, o, "passive")
        .unwrap()
        .unwrap()
        .verification
        .unwrap();
    assert_eq!(e.identity_assurance, "INCONCLUSIVE");
    assert!(
        e.evidence_codes
            .contains(&"MODEL_DECLARATIONS_CONFLICT".into())
    );
}
#[test]
fn missing_partial_contradictory_usage_is_never_fabricated_as_zero() {
    let mut o = observed(None, "expected");
    assert_eq!(usage_total(&o), Some(5));
    o.total_tokens = Some(1);
    assert_eq!(usage_total(&o), None);
    o.total_tokens = None;
    assert_eq!(usage_total(&o), Some(5));
    o.output_tokens = None;
    assert_eq!(usage_total(&o), None);
    o.output_tokens = Some(1);
    o.complete = false;
    assert_eq!(usage_total(&o), None);
    o.complete = true;
    o.usage_final = false;
    assert_eq!(usage_total(&o), None);
    o.usage_final = true;
    o.input_tokens = Some(u64::MAX);
    assert_eq!(usage_total(&o), None);
}
#[test]
fn claude_cumulative_usage_uses_latest_not_sum_and_counts_cache_separately() {
    let mut c = Collector::new(UpstreamFormat::Anthropic, Observation::default());
    let mut buffer = Vec::new();
    for data in [
        json!({"type":"message_start","message":{"model":"claude","content":[],"usage":{"input_tokens":5,"output_tokens":0,"cache_read_input_tokens":10,"cache_creation_input_tokens":2}}}),
        json!({"type":"content_block_delta","delta":{"type":"text_delta","text":"OK"}}),
        json!({"type":"message_delta","delta":{},"usage":{"output_tokens":1}}),
        json!({"type":"message_delta","delta":{"stop_reason":"end_turn"},"usage":{"output_tokens":2}}),
        json!({"type":"message_stop"}),
    ] {
        c.push_sse(&mut buffer, format!("data: {data}\n\n").as_bytes(), 1);
    }
    c.finish_stream(true);
    assert_eq!(c.observation.output_tokens, Some(2));
    assert_eq!(usage_total(&c.observation), Some(19));
    assert!(c.observation.usage_final);
}
#[test]
fn metadata_storage_never_contains_response_text_auth_or_default_diagnostic_ids() {
    let (db, p) = setup(true, false);
    let (_, _, e) = sample(&db, &p, "sk-PRIVATE_AUTH_123456", "expected", now_ms());
    let text = serde_json::to_string(&e).unwrap();
    for private in [
        "PRIVATE_AUTH",
        "PRIVATE_REPLY",
        "resp-private-diagnostic",
        "127.0.0.1",
    ] {
        assert!(!text.contains(private), "leaked {private}");
    }
    let serialized: String = db
        .conn()
        .lock()
        .unwrap()
        .query_row("SELECT audit_json FROM request_events LIMIT 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!serialized.contains("PRIVATE_REPLY"));
    assert!(!serialized.contains("verificationCapture"));
    for bad in [
        "sk-SECRET",
        "BearerSECRET",
        "https://endpoint/key",
        "a private prompt",
        "x\nkey",
    ] {
        assert!(safe_id(bad).is_none());
    }
}
#[test]
fn hmac_matches_rfc4231_test_vector() {
    assert_eq!(
        runtime::hmac_hex(&[0x0b; 20], b"Hi There"),
        "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
    );
}
#[test]
fn observation_storage_failure_is_fail_open_and_probe_storage_failure_fail_closed() {
    let (db, p) = setup(true, true);
    let (id, cap, _) = sample(&db, &p, "SECRET", "wrong", now_ms());
    db.verification.approve(&cap, true).unwrap();
    db.conn()
        .lock()
        .unwrap()
        .execute("DROP TABLE verification_samples", [])
        .unwrap();
    assert!(
        db.update_request_usage(
            id,
            &RequestUsage {
                input_tokens: Some(4),
                ..Default::default()
            }
        )
        .is_ok()
    );
    assert!(db.verification.reserve(cap, id, now_ms()).is_err());
    assert_eq!(count(&db, "verification_probes"), 0);
}

#[test]
fn duplicate_observation_and_repeated_manual_attempt_do_not_rewrite_paid_evidence() {
    let (db, p) = setup(true, true);
    let (id, cap, e) = sample(&db, &p, "SECRET", "wrong", now_ms());
    assert!(
        db.verification
            .process(id, observed(Some(cap.clone()), "other"), "passive")
            .unwrap()
            .is_none()
    );
    assert_eq!(
        db.verification
            .status(&p.id, "expected", Some(&cap))
            .unwrap()
            .recent[0]
            .reported_models,
        e.reported_models
    );
    db.verification.approve(&cap, true).unwrap();
    let r = db.verification.reserve(cap.clone(), id, now_ms()).unwrap();
    db.verification.mark_running(&r).unwrap();
    assert!(db.verification.mark_running(&r).is_err());
    db.verification.settle(&r, Some(5), "completed").unwrap();
    db.verification
        .note_probe(id, cap.generation, "completed", Some(5));
    db.verification
        .note_skipped_probe(id, cap.generation, "anomaly_already_checked");
    let e = &db
        .verification
        .status(&p.id, "expected", Some(&cap))
        .unwrap()
        .recent[0];
    assert_eq!(e.active_probe_state, "completed");
    assert_eq!(e.active_probe_tokens, Some(5));
    assert!(
        db.verification
            .settle(&r, None, "failed_or_unknown")
            .is_err()
    );
}
#[test]
fn forward_wall_clock_jump_cannot_bypass_monotonic_cooldown() {
    let (db, p) = setup(true, true);
    let at = now_ms();
    let (id, cap, _) = sample(&db, &p, "SECRET", "wrong", at);
    db.verification.approve(&cap, true).unwrap();
    let r = db.verification.reserve(cap, id, at).unwrap();
    db.verification.settle(&r, Some(5), "completed").unwrap();
    let jumped = at + 2 * store::DAY_MS;
    let (id, cap, _) = sample(&db, &p, "SECRET", "wrong", jumped);
    assert_eq!(
        db.verification.reserve(cap, id, jumped).err().unwrap(),
        "cooldown"
    );
}
#[test]
fn shutdown_is_immediate_even_when_persistence_fails() {
    let (db, p) = setup(true, true);
    let cap = db
        .verification
        .capture(&p, "SECRET", "chat", "expected")
        .unwrap();
    db.conn()
        .lock()
        .unwrap()
        .execute("DROP TABLE app_settings", [])
        .unwrap();
    assert!(
        db.verification
            .set_settings(VerificationSettings::default())
            .is_err()
    );
    assert!(!db.verification.settings().unwrap().enabled);
    assert!(!db.verification.valid(&cap));
}
#[test]
fn confirmed_fallback_keeps_claim_match_without_unexplained_route_warning() {
    let (db, p) = setup(true, false);
    let mut settings = db.verification.settings().unwrap();
    settings.aliases.push(AliasRule {
        provider_id: p.id.clone(),
        requested_model: "expected".into(),
        reported_model: "fallback".into(),
        user_confirmed: true,
        allow_route_change: true,
    });
    db.verification.set_settings(settings).unwrap();
    sample(&db, &p, "SECRET", "expected", now_ms());
    let (_, _, e) = sample(&db, &p, "SECRET", "fallback", now_ms() + 1);
    assert_eq!(e.claim_consistency, "MATCH");
    assert!(e.evidence_codes.contains(&"ALLOWED_ROUTE_CHANGE".into()));
    assert!(!e.evidence_codes.contains(&"MODEL_ROUTE_CHANGED".into()));
    assert!(!e.eligible_anomaly());
}

#[test]
fn redacted_json_and_csv_exports_exclude_credentials_routes_models_and_diagnostics() {
    use super::export::{
        CsvMetadataExporter, JsonMetadataExporter, MetadataExporter, RedactedMetadata,
    };
    let (db, p) = setup(true, false);
    let (_, _, mut e) = sample(&db, &p, "sk-SECRET_EXPORT", "private-model", now_ms());
    e.response_id = Some("private-response-id".into());
    e.provider_request_id = Some("private-request-id".into());
    let mut rows = vec![RedactedMetadata::from(&e)];
    for output in [
        JsonMetadataExporter.write_redacted(&rows).unwrap(),
        CsvMetadataExporter.write_redacted(&rows).unwrap(),
    ] {
        let text = String::from_utf8(output).unwrap();
        for secret in [
            "sk-SECRET_EXPORT",
            "private-model",
            "private-response-id",
            "private-request-id",
            "hmac-v1",
            &e.context_key,
            &p.id,
        ] {
            assert!(!text.contains(secret), "exported private metadata");
        }
    }
    rows[0].observation_id = "=1+1".into();
    assert!(
        String::from_utf8(CsvMetadataExporter.write_redacted(&rows).unwrap())
            .unwrap()
            .contains("'=1+1")
    );
}


#[test]
fn daily_token_budget_accepts_zero_to_ten_thousand_and_rejects_out_of_range() {
    let (db, _) = setup(true, true);
    for value in [0, 1, 39, 40, 100, 10_000] {
        let mut settings = db.verification.settings().unwrap();
        settings.soft_token_budget_per_day = value;
        db.verification.set_settings(settings).unwrap();
        assert_eq!(db.verification.settings().unwrap().soft_token_budget_per_day, value);
    }
    let mut invalid = db.verification.settings().unwrap();
    invalid.soft_token_budget_per_day = 10_001;
    assert!(db.verification.set_settings(invalid).is_err());
    assert_eq!(db.verification.settings().unwrap().soft_token_budget_per_day, 10_000);
    assert!(serde_json::from_value::<VerificationSettings>(json!({"softTokenBudgetPerDay":-1})).is_err());
    assert!(serde_json::from_value::<VerificationSettings>(json!({"softTokenBudgetPerDay":1.5})).is_err());
}

#[test]
fn zero_or_insufficient_budget_keeps_passive_observation_without_reserving_a_probe() {
    let (db, p) = setup(true, true);
    for value in [0, 1, 39] {
        let mut settings = db.verification.settings().unwrap();
        settings.soft_token_budget_per_day = value;
        db.verification.set_settings(settings).unwrap();
        let (id, cap, evidence) = sample(&db, &p, "SECRET", "different", now_ms());
        assert!(evidence.eligible_anomaly());
        db.verification.approve(&cap, true).unwrap();
        assert_eq!(db.verification.reserve(cap, id, now_ms()).err().unwrap(), "daily_token_budget");
        assert_eq!(count(&db, "verification_probes"), 0);
    }
    assert_eq!(count(&db, "verification_samples"), 3);
    let mut settings = db.verification.settings().unwrap();
    settings.soft_token_budget_per_day = 40;
    db.verification.set_settings(settings).unwrap();
    let (id, cap, _) = sample(&db, &p, "SECRET", "different", now_ms());
    db.verification.approve(&cap, true).unwrap();
    assert!(db.verification.reserve(cap, id, now_ms()).is_ok());
    assert_eq!(count(&db, "verification_probes"), 1);
}

#[test]
fn setting_budget_to_zero_cancels_existing_capture_without_turning_off_passive_mode() {
    let (db, p) = setup(true, true);
    let (id, cap, _) = sample(&db, &p, "SECRET", "different", now_ms());
    db.verification.approve(&cap, true).unwrap();
    let reservation = db.verification.reserve(cap, id, now_ms()).unwrap();
    let mut settings = db.verification.settings().unwrap();
    settings.soft_token_budget_per_day = 0;
    db.verification.set_settings(settings).unwrap();
    assert!(!db.verification.valid(&reservation.capture));
    assert!(db.verification.settings().unwrap().enabled);
    assert!(db.verification.capture(&p, "SECRET", "chat", "expected").is_some());
    assert_eq!(count(&db, "verification_probes"), 1);
}
