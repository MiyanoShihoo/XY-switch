use super::*;
use crate::model_audit::Observation;

fn capture() -> Capture {
    Capture {
        provider_id: "decision-test".into(),
        requested_model: "expected".into(),
        protocol: "chat".into(),
        credential_fingerprint: "test-only".into(),
        context_key: "current-context".into(),
        generation: 1,
        store_diagnostic_ids: false,
    }
}
fn observation(model: &str) -> Observation {
    Observation {
        requested_model: Some("expected".into()),
        forwarded_model: Some("expected".into()),
        upstream_model: Some(model.into()),
        input_tokens: Some(11),
        output_tokens: Some(7),
        total_tokens: Some(18),
        reasoning_tokens: Some(0),
        max_tokens: Some(64),
        visible_output_bytes: Some(32),
        tool_payload_bytes: Some(0),
        complete: true,
        text_only: true,
        usage_final: true,
        protocol: Some("chat".into()),
        protocol_shape: Some("chat".into()),
        ..Default::default()
    }
}
fn evaluate(
    o: Observation,
    status: Option<u16>,
    history: &[VerificationEvidence],
) -> VerificationEvidence {
    engine::evaluate(
        o,
        &capture(),
        42,
        1000,
        status,
        "passive",
        &VerificationSettings::default(),
        history,
    )
    .verification
    .unwrap()
}
fn has(e: &VerificationEvidence, code: &str) -> bool {
    e.evidence_codes.iter().any(|c| c == code)
}

#[test]
fn http_error_model_is_not_a_successful_claim_or_an_eligible_identity_probe() {
    for status in [400, 401, 403, 429, 500, 503] {
        let e = evaluate(observation("expected"), Some(status), &[]);
        assert_eq!(e.claim_consistency, "UNKNOWN");
        assert_eq!(e.identity_assurance, "INCONCLUSIVE");
        assert!(has(&e, "HTTP_REQUEST_FAILED"));
        assert!(!has(&e, "CLAIM_MATCH"));
        assert!(!e.eligible_anomaly());
    }
}
#[test]
fn timeout_and_unknown_http_result_remain_unknown_not_fake_or_zero_usage() {
    let mut o = observation("expected");
    o.complete = false;
    o.input_tokens = None;
    o.output_tokens = None;
    o.total_tokens = None;
    let e = evaluate(o, None, &[]);
    assert_eq!(e.claim_consistency, "UNKNOWN");
    assert_eq!(e.total_tokens, None);
    assert!(has(&e, "HTTP_STATUS_UNKNOWN"));
    assert!(!e.eligible_anomaly());
}
#[test]
fn legacy_http_error_history_cannot_create_a_model_or_protocol_change() {
    let mut legacy = evaluate(observation("other"), Some(200), &[]);
    legacy.http_status = Some(503);
    legacy.protocol_shape = Some("responses".into());
    let e = evaluate(observation("expected"), Some(200), &[legacy]);
    assert_eq!(e.claim_consistency, "MATCH");
    assert!(!has(&e, "MODEL_ROUTE_CHANGED"));
    assert!(!has(&e, "PROTOCOL_SHAPE_CHANGED"));
}
#[test]
fn incomplete_parse_invalid_and_conflicting_history_are_not_route_baselines() {
    for kind in ["partial", "invalid", "conflict"] {
        let mut h = evaluate(observation("other"), Some(200), &[]);
        match kind {
            "partial" => h.stream_completed = false,
            "invalid" => h.evidence_codes.push("RESPONSE_INVALID".into()),
            _ => {
                h.reported_models.push("third".into());
                h.evidence_codes.push("MODEL_DECLARATIONS_CONFLICT".into());
            }
        }
        let e = evaluate(observation("expected"), Some(200), &[h]);
        assert!(!has(&e, "MODEL_ROUTE_CHANGED"), "{kind}");
    }
}
#[test]
fn failed_current_response_does_not_report_a_route_or_protocol_change() {
    let h = evaluate(observation("expected"), Some(200), &[]);
    let mut o = observation("other");
    o.protocol_shape = Some("responses".into());
    let e = evaluate(o, Some(503), &[h]);
    assert!(!has(&e, "MODEL_ROUTE_CHANGED"));
    assert!(!has(&e, "PROTOCOL_SHAPE_CHANGED"));
}
#[test]
fn another_context_provider_model_or_sampling_source_does_not_leak_into_history() {
    for field in ["context", "provider", "model", "source"] {
        let mut h = evaluate(observation("other"), Some(200), &[]);
        match field {
            "context" => h.context_key = "other-context".into(),
            "provider" => h.upstream_id = "other-provider".into(),
            "model" => h.requested_model = "other-model".into(),
            _ => h.source = "active".into(),
        }
        let e = evaluate(observation("expected"), Some(200), &[h]);
        assert_eq!(e.rolling_observed_count, 1, "{field}");
        assert!(!has(&e, "MODEL_ROUTE_CHANGED"), "{field}");
    }
}
#[test]
fn scalar_model_and_list_model_are_the_same_evidence_without_repeated_new_flags() {
    let h = evaluate(observation("expected"), Some(200), &[]);
    assert!(h.new_evidence);
    let e = evaluate(observation("expected"), Some(200), &[h.clone()]);
    assert!(!e.new_evidence);
    let mut o = observation("expected");
    o.upstream_models = vec!["expected".into(), "expected".into()];
    let mut reordered = h;
    reordered.evidence_codes.reverse();
    reordered.reported_models.push("expected".into());
    let e = evaluate(o, Some(200), &[reordered]);
    assert!(!e.new_evidence);
    assert!(!has(&e, "MODEL_ROUTE_CHANGED"));
}
#[test]
fn valid_successful_different_claims_still_report_an_actionable_route_change() {
    let h = evaluate(observation("other"), Some(200), &[]);
    let e = evaluate(observation("expected"), Some(200), &[h]);
    assert!(has(&e, "MODEL_ROUTE_CHANGED"));
    assert!(e.eligible_anomaly());
    assert_eq!(e.identity_assurance, "UNVERIFIED");
}
#[test]
fn legacy_error_warning_is_not_a_repeated_model_anomaly() {
    let mut h = evaluate(observation("other"), Some(200), &[]);
    h.http_status = Some(500);
    let e = evaluate(observation("wrong"), Some(200), &[h]);
    assert!(!e.repeated_anomaly);
}
#[test]
fn active_error_is_not_saved_as_completed_verification() {
    let e = engine::evaluate(
        observation("expected"),
        &capture(),
        43,
        1001,
        Some(500),
        "active",
        &VerificationSettings::default(),
        &[],
    )
    .verification
    .unwrap();
    assert_eq!(e.active_probe_state, "failed_or_unknown");
}
#[test]
fn reasoning_and_output_limit_difference_is_a_usage_warning_not_fake_model_identity() {
    // Counts observed in the redacted Grok Chat tool-call regression fixture.
    let mut o = observation("expected");
    o.input_tokens = Some(376);
    o.output_tokens = Some(405);
    o.total_tokens = Some(781);
    o.reasoning_tokens = Some(380);
    o.max_tokens = Some(256);
    o.tool_payload_bytes = Some(256);
    let e = evaluate(o, Some(200), &[]);
    assert!(has(&e, "OUTPUT_LIMIT_EXCEEDED"));
    assert_eq!(e.total_tokens, Some(781));
    assert_eq!(e.claim_consistency, "MATCH");
    assert_eq!(e.identity_assurance, "UNVERIFIED");
    assert_eq!(e.risk_level, "warning");
    assert!(!e.eligible_anomaly());
}
#[test]
fn contradictory_usage_is_actionable_but_does_not_trigger_identity_probing() {
    let mut o = observation("expected");
    o.total_tokens = Some(999);
    let e = evaluate(o, Some(200), &[]);
    assert!(has(&e, "USAGE_CONTRADICTORY"));
    assert_eq!(e.total_tokens, None);
    assert_eq!(e.risk_level, "warning");
    assert!(!e.eligible_anomaly());
}
#[test]
fn reported_model_match_never_authenticates_weights_even_after_many_samples() {
    let mut history = vec![];
    for _ in 0..30 {
        let e = evaluate(observation("expected"), Some(200), &history);
        assert_eq!(e.identity_assurance, "UNVERIFIED");
        assert!(e.rolling_observed_count <= VerificationSettings::default().rolling_window_size);
        history.insert(0, e);
    }
}

#[test]
fn partial_or_invalid_claims_cannot_trigger_a_paid_identity_probe() {
    for kind in ["invalid", "truncated", "partial"] {
        let mut o = observation("other");
        match kind {
            "invalid" => o.parse_invalid = true,
            "truncated" => o.model_evidence_truncated = true,
            _ => o.complete = false,
        }
        let e = evaluate(o, Some(200), &[]);
        assert_eq!(e.claim_consistency, "UNKNOWN", "{kind}");
        assert!(!e.eligible_anomaly(), "{kind}");
    }
    for code in [
        "RESPONSE_INVALID",
        "RESPONSE_INCOMPLETE",
        "STREAM_INCOMPLETE",
        "MODEL_EVIDENCE_TRUNCATED",
        "HTTP_STATUS_UNKNOWN",
    ] {
        let mut legacy = evaluate(observation("other"), Some(200), &[]);
        legacy.evidence_codes.push(code.into());
        assert!(!legacy.eligible_anomaly(), "{code}");
        let e = evaluate(observation("expected"), Some(200), &[legacy]);
        assert!(!has(&e, "MODEL_ROUTE_CHANGED"), "{code}");
    }
}
#[test]
fn usage_warning_is_not_a_repeated_identity_anomaly() {
    let h = evaluate(observation("other"), Some(200), &[]);
    let mut o = observation("other");
    o.forwarded_model = Some("other".into());
    o.max_tokens = Some(1);
    let e = evaluate(o, Some(200), &[h]);
    assert!(has(&e, "OUTPUT_LIMIT_EXCEEDED"));
    assert!(!e.eligible_anomaly());
    assert!(!e.repeated_anomaly);
}
