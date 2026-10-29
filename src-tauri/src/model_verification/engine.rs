use super::{Capture, VerificationEvidence, VerificationSettings, safe_id};
use crate::model_audit::{AuditRecord, Observation, audit_with_aliases};

pub fn usage_status(o: &Observation) -> &'static str {
    if o.complete && o.usage_final && o.input_tokens.is_some() && o.output_tokens.is_some() {
        "complete"
    } else if o.input_tokens.is_some() || o.output_tokens.is_some() || o.total_tokens.is_some() {
        "partial"
    } else if o.complete {
        "missing"
    } else {
        "unknown"
    }
}
pub fn usage_total(o: &Observation) -> Option<u64> {
    if usage_status(o) != "complete" {
        return None;
    }
    let mut total = o.input_tokens?.checked_add(o.output_tokens?)?;
    if o.protocol.as_deref() == Some("anthropic") {
        total = total
            .checked_add(o.cache_read_tokens.unwrap_or(0))?
            .checked_add(o.cache_creation_tokens.unwrap_or(0))?;
    }
    // Never release a reservation using contradictory or overflowed accounting.
    if o.total_tokens.is_some_and(|reported| reported != total)
        || o.reasoning_tokens
            .zip(o.output_tokens)
            .is_some_and(|(r, out)| r > out)
    {
        return None;
    }
    Some(total)
}

fn normalized_ids(values: &[String]) -> Vec<String> {
    let mut normalized: Vec<_> = values.iter().filter_map(|value| safe_id(value)).collect();
    normalized.sort();
    normalized.dedup();
    normalized
}

pub fn evaluate(
    mut o: Observation,
    capture: &Capture,
    event_id: i64,
    timestamp: i64,
    status: Option<u16>,
    source: &str,
    settings: &VerificationSettings,
    history: &[VerificationEvidence],
) -> AuditRecord {
    // Do not let another credential, route, source, or an out-of-window record
    // change the current response's decision, even for legacy/imported history.
    let history: Vec<_> = history
        .iter()
        .filter(|h| {
            h.context_key == capture.context_key
                && h.upstream_id == capture.provider_id
                && h.requested_model == capture.requested_model
                && h.source == source
        })
        .take(settings.rolling_window_size.saturating_sub(1))
        .collect();
    let http_success = status.is_some_and(|code| (200..300).contains(&code));
    o.requested_model = o.requested_model.as_deref().and_then(safe_id);
    o.forwarded_model = o.forwarded_model.as_deref().and_then(safe_id);
    o.upstream_model = o.upstream_model.as_deref().and_then(safe_id);
    o.upstream_models = o
        .upstream_models
        .iter()
        .filter_map(|m| safe_id(m))
        .take(16)
        .collect();
    let expected = o
        .forwarded_model
        .as_deref()
        .or(o.requested_model.as_deref())
        .unwrap_or("");
    let rules: Vec<_> = settings
        .aliases
        .iter()
        .filter(|a| {
            a.user_confirmed
                && a.provider_id == capture.provider_id
                && (a.requested_model == expected || a.requested_model == capture.requested_model)
        })
        .collect();
    let aliases: Vec<_> = rules.iter().map(|a| a.reported_model.clone()).collect();
    let mut audit = audit_with_aliases(o, &aliases);
    let o = &audit.observation;
    let mut declared = o.upstream_models.clone();
    if let Some(m) = &o.upstream_model {
        if !declared.contains(m) {
            declared.push(m.clone());
        }
    }
    let incomplete = !http_success || !o.complete || o.parse_invalid || o.model_evidence_truncated;
    let claim = if incomplete {
        "UNKNOWN"
    } else if declared.is_empty() {
        "MISSING"
    } else if audit.model_status == "matched" {
        "MATCH"
    } else if audit.model_status == "mismatched" || audit.model_status == "conflicting" {
        "MISMATCH"
    } else {
        "UNKNOWN"
    };
    declared.sort();
    declared.dedup();
    let mut codes: Vec<String> = vec![];
    if status.is_none() {
        codes.push("HTTP_STATUS_UNKNOWN".into());
    } else if !http_success {
        codes.push("HTTP_REQUEST_FAILED".into());
    }
    match claim {
        "MATCH" => codes.push("CLAIM_MATCH".into()),
        "MISMATCH" => codes.push("MODEL_CLAIM_MISMATCH".into()),
        "MISSING" => codes.push("MODEL_FIELD_MISSING".into()),
        _ => codes.push("RESPONSE_INCOMPLETE".into()),
    }
    // Preserve individual conflicts even when the transport is incomplete.
    if audit.flags.iter().any(|c| c == "inv.model_changed") {
        codes.push("MODEL_DECLARATIONS_CONFLICT".into());
    }
    if declared.is_empty() && claim != "MISSING" {
        codes.push("MODEL_FIELD_MISSING".into());
    }
    if audit.model_status == "mismatched" && claim != "MISMATCH" {
        codes.push("MODEL_CLAIM_MISMATCH".into());
    }
    if matches!(claim, "MISMATCH" | "MISSING")
        && o.forwarded_model == o.requested_model
        && rules.is_empty()
    {
        codes.push("ALIAS_UNRESOLVED".into());
    }
    if usage_status(o) != "complete" {
        codes.push("USAGE_MISSING".into());
    } else if usage_total(o).is_none() {
        codes.push("USAGE_CONTRADICTORY".into());
    }
    if audit
        .flags
        .iter()
        .any(|flag| flag == "inv.max_tokens_violated")
    {
        codes.push("OUTPUT_LIMIT_EXCEEDED".into());
    }
    if audit.flags.iter().any(|flag| flag == "inv.byte_bound") {
        codes.push("USAGE_BOUND_CONTRADICTION".into());
    }
    if o.streaming && !o.complete {
        codes.push("STREAM_INCOMPLETE".into());
    }
    if o.parse_invalid {
        codes.push("RESPONSE_INVALID".into());
    }
    if o.model_evidence_truncated {
        codes.push("MODEL_EVIDENCE_TRUNCATED".into());
    }
    if !incomplete
        && o.protocol_shape
            .as_deref()
            .is_some_and(|s| s != capture.protocol)
    {
        codes.push("PROTOCOL_SHAPE_CHANGED".into());
    }
    let valid: Vec<_> = history
        .iter()
        .filter(|h| {
            h.has_complete_response()
                && matches!(h.claim_consistency.as_str(), "MATCH" | "MISMATCH")
                && normalized_ids(&h.reported_models).len() == 1
                && !h.evidence_codes.iter().any(|code| {
                    matches!(
                        code.as_str(),
                        "RESPONSE_INVALID" | "RESPONSE_INCOMPLETE" | "MODEL_DECLARATIONS_CONFLICT"
                    )
                })
        })
        .collect();
    let previous = valid.first().copied();
    let changed = !incomplete
        && declared.len() == 1
        && previous.is_some_and(|p| normalized_ids(&p.reported_models) != declared);
    let expected = o
        .forwarded_model
        .as_deref()
        .or(o.requested_model.as_deref())
        .unwrap_or("");
    let allowed_model = |m: &String| {
        m == expected
            || rules
                .iter()
                .any(|r| r.reported_model == *m && r.allow_route_change)
    };
    let allowed = changed
        && declared.iter().all(allowed_model)
        && previous.is_some_and(|p| p.reported_models.iter().all(allowed_model));
    if changed {
        codes.push(
            if allowed {
                "ALLOWED_ROUTE_CHANGE"
            } else {
                "MODEL_ROUTE_CHANGED"
            }
            .into(),
        );
    }
    if !incomplete
        && previous.is_some_and(|p| {
            p.protocol_shape.is_some()
                && o.protocol_shape.is_some()
                && p.protocol_shape != o.protocol_shape
        })
        && !codes.iter().any(|c| c == "PROTOCOL_SHAPE_CHANGED")
    {
        codes.push("PROTOCOL_SHAPE_CHANGED".into());
    }
    let changes = usize::from(changed && !allowed)
        + history
            .iter()
            .take(settings.rolling_window_size.saturating_sub(1))
            .filter(|h| {
                h.eligible_anomaly() && h.evidence_codes.iter().any(|c| c == "MODEL_ROUTE_CHANGED")
            })
            .count();
    let warning = codes.iter().any(|c| {
        matches!(
            c.as_str(),
            "MODEL_CLAIM_MISMATCH"
                | "MODEL_ROUTE_CHANGED"
                | "PROTOCOL_SHAPE_CHANGED"
                | "MODEL_DECLARATIONS_CONFLICT"
                | "HTTP_REQUEST_FAILED"
                | "USAGE_CONTRADICTORY"
                | "USAGE_BOUND_CONTRADICTION"
                | "OUTPUT_LIMIT_EXCEEDED"
        )
    });
    let explanation = if status.is_none() {
        "未取得 HTTP 结果；保留已收集字段，不把超时或未知用量判成假模型。"
    } else if !http_success {
        "请求未成功；错误响应中的模型字段不能作为成功响应的型号证据，也不参与路由变化基线。"
    } else {
        match claim {
            "MATCH" => "模型声明一致（或属于已确认别名），普通响应字段不证明底层模型身份。",
            "MISMATCH" => {
                "本条响应的声明与实际转发模型不一致；可能是别名、动态路由或回退，不能据此断言假冒。"
            }
            "MISSING" => "本条响应未提供可用模型字段，无法形成身份判断。",
            _ => "本条响应不完整或证据冲突，只保留已观察事实。",
        }
    };
    let new_evidence = !history.iter().any(|h| {
        normalized_ids(&h.evidence_codes) == normalized_ids(&codes)
            && normalized_ids(&h.reported_models) == declared
            && h.http_status == status
            && h.stream_completed == o.complete
    });
    let mut evidence = VerificationEvidence {
        observation_id: event_id.to_string(),
        upstream_id: capture.provider_id.clone(),
        requested_model: capture.requested_model.clone(),
        forwarded_model: o.forwarded_model.clone(),
        reported_models: declared,
        credential_fingerprint: capture.credential_fingerprint.clone(),
        context_key: capture.context_key.clone(),
        protocol: capture.protocol.clone(),
        protocol_shape: o.protocol_shape.clone(),
        claim_consistency: claim.into(),
        identity_assurance: if incomplete
            || audit.model_status == "conflicting"
            || matches!(claim, "MISSING" | "UNKNOWN")
        {
            "INCONCLUSIVE"
        } else {
            "UNVERIFIED"
        }
        .into(),
        evidence_codes: codes.clone(),
        explanation: explanation.into(),
        risk_level: if warning {
            "warning"
        } else if claim == "MATCH" {
            "none"
        } else {
            "notice"
        }
        .into(),
        observed_at: timestamp,
        usage_status: if usage_status(o) == "complete" && usage_total(o).is_none() {
            "contradictory"
        } else {
            usage_status(o)
        }
        .into(),
        input_tokens: o.input_tokens,
        output_tokens: o.output_tokens,
        reasoning_tokens: o.reasoning_tokens,
        total_tokens: usage_total(o),
        cache_read_tokens: o.cache_read_tokens,
        cache_creation_tokens: o.cache_creation_tokens,
        is_streaming: o.streaming,
        stream_completed: o.complete,
        http_status: status,
        finish_reason: o.finish_reason.clone(),
        response_id: o.response_id.clone(),
        provider_request_id: o.provider_request_id.clone(),
        evidence_source: "untrusted_response_metadata".into(),
        source: source.into(),
        rolling_observed_count: (history.len() + 1).min(settings.rolling_window_size),
        route_change_count: changes,
        repeated_anomaly: false,
        new_evidence,
        active_probe_state: if source == "active" {
            if incomplete {
                "failed_or_unknown"
            } else {
                "completed"
            }
        } else if !settings.active_probe_enabled {
            "disabled"
        } else {
            "not_needed"
        }
        .into(),
        active_probe_tokens: if source == "active" {
            usage_total(o)
        } else {
            Some(0)
        },
    };
    evidence.repeated_anomaly = evidence.eligible_anomaly()
        && 1 + history.iter().filter(|h| h.eligible_anomaly()).count()
            >= settings.repeated_anomaly_threshold;
    audit.verification = Some(evidence);
    audit
}
