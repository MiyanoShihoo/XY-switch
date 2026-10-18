use super::{AxisResult, AxisResults, StreamBilling, StreamSummary};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Observation {
    pub requested_model: Option<String>,
    pub forwarded_model: Option<String>,
    pub upstream_model: Option<String>,
    /// Distinct declarations from this response only; bounded by the collector.
    pub upstream_models: Vec<String>,
    pub model_evidence_truncated: bool,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub max_tokens: Option<u64>,
    pub visible_output_bytes: Option<u64>,
    pub tool_payload_bytes: Option<u64>,
    pub complete: bool,
    pub text_only: bool,
    pub stream: Option<StreamSummary>,
    pub protocol: Option<String>,
    pub protocol_shape: Option<String>,
    pub streaming: bool,
    pub parse_invalid: bool,
    pub usage_final: bool,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub finish_reason: Option<String>,
    pub response_id: Option<String>,
    pub provider_request_id: Option<String>,
    #[serde(skip)]
    pub verification_capture: Option<crate::model_verification::Capture>,
    #[serde(skip)]
    pub verification_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditRecord {
    pub observation: Observation,
    pub flags: Vec<String>,
    pub gaps: Vec<String>,
    pub model_status: String,
    pub axes: AxisResults,
    #[serde(default)]
    pub stream_billing: Option<StreamBilling>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<crate::model_verification::VerificationEvidence>,
}

fn whole_stream_billing(observation: &Observation) -> Option<StreamBilling> {
    let stream = observation.stream.as_ref()?;
    let visible_tokens = observation
        .output_tokens
        .zip(observation.reasoning_tokens)
        .and_then(|(output, reasoning)| output.checked_sub(reasoning));
    let visible_bytes = observation.visible_output_bytes;
    let mut result = StreamBilling {
        status: AxisResult::NotComparable,
        text_frames: stream.text_delta_count,
        visible_tokens,
        visible_bytes,
        tokens_per_byte: visible_tokens
            .zip(visible_bytes)
            .filter(|(_, bytes)| *bytes > 0)
            .map(|(tokens, bytes)| tokens as f64 / bytes as f64),
        bytes_per_frame: visible_bytes
            .filter(|_| stream.text_delta_count > 0)
            .map(|bytes| bytes as f64 / stream.text_delta_count as f64),
        frames_per_token: visible_tokens
            .filter(|tokens| *tokens > 0)
            .map(|tokens| stream.text_delta_count as f64 / tokens as f64),
        gaps: vec!["scv.token_boundaries_unverified".into()],
        limitations: vec![
            "整段文本长度使用采集的可见输出 UTF-8 字节数；多字节字符不按字符个数代替，字节数未知时不生成字节比率。".into(),
            "非空 SSE 文本帧可能只携带一个 token 的部分文本；帧数大于 token 数仍可能是诚实转发，绝非硬证据。".into(),
            "整段比率只有在传输保留生成 token 边界的假设下可比；尚未排除重切分、缓冲和包装行为。".into(),
            "visibleTokens 已扣除已知推理 token，仍可能包含特殊 token；这些统计不证明模型身份或虚报用量。".into(),
        ],
    };
    if !observation.complete {
        result.gaps.push("scv.incomplete".into());
    }
    if !observation.text_only {
        result.gaps.push("scv.non_text".into());
    }
    if observation.reasoning_tokens.is_none() {
        result.gaps.push("scv.reasoning_unknown".into());
    }
    if observation.output_tokens.is_none() {
        result.gaps.push("scv.output_missing".into());
    }
    if observation
        .output_tokens
        .zip(observation.reasoning_tokens)
        .is_some_and(|(output, reasoning)| reasoning > output)
    {
        result.gaps.push("scv.invalid_reasoning".into());
    }
    if observation.tool_payload_bytes != Some(0) {
        result.gaps.push("scv.tools_present_or_unknown".into());
    }
    if stream.batched {
        result.gaps.push("scv.batched".into());
    }
    if !stream.minimum_samples_met || stream.text_delta_count < 8 {
        result.gaps.push("scv.insufficient_samples".into());
    }
    match visible_bytes {
        None => result.gaps.push("scv.bytes_missing".into()),
        Some(0) => result.gaps.push("scv.no_text".into()),
        Some(_) => (),
    }
    if result.gaps.len() == 1 {
        if let Some(tokens) = visible_tokens {
            result.status = if stream.text_delta_count > tokens {
                AxisResult::Contradiction
            } else {
                AxisResult::Consistent
            };
        }
    }
    Some(result)
}

fn present(value: &Option<String>) -> Option<&str> {
    value.as_deref().filter(|value| !value.trim().is_empty())
}

pub fn audit(observation: Observation) -> AuditRecord {
    audit_with_aliases(observation, &[])
}

pub fn audit_with_aliases(observation: Observation, aliases: &[String]) -> AuditRecord {
    let mut flags = Vec::new();
    let mut gaps = Vec::new();
    // Compare against the exact model sent after mappings and overrides, never
    // against a provider-wide verdict or the billing-model fallback.
    let expected =
        present(&observation.forwarded_model).or_else(|| present(&observation.requested_model));
    let mut declared: Vec<&str> = observation
        .upstream_models
        .iter()
        .map(String::as_str)
        .filter(|s| !s.trim().is_empty())
        .collect();
    if let Some(model) = present(&observation.upstream_model) {
        declared.push(model);
    }
    declared.sort_unstable();
    declared.dedup();
    if expected.is_none() {
        gaps.push("model.requested_missing".into());
    }
    if declared.is_empty() {
        gaps.push("model.upstream_missing".into());
    }
    if observation.model_evidence_truncated {
        gaps.push("model.evidence_truncated".into());
    }
    let model_status = if declared.len() > 1 {
        flags.push("inv.model_changed".into());
        "conflicting"
    } else if observation.model_evidence_truncated {
        "unknown"
    } else {
        match (expected, declared.first()) {
            (Some(expected), Some(upstream)) if expected == *upstream || aliases.iter().any(|a| a == upstream) => "matched",
            (Some(_), Some(_)) => {
                flags.push("inv.model_mismatch".into());
                "mismatched"
            }
            _ => "unknown",
        }
    }
    .to_string();

    match (
        observation.input_tokens,
        observation.output_tokens,
        observation.total_tokens,
    ) {
        (Some(input), Some(output), Some(total)) => {
            // Widen instead of allowing hostile/overflowed usage to panic.
            if u128::from(input) + u128::from(output) != u128::from(total) {
                flags.push("inv.usage_selfcontradictory".into());
            }
        }
        _ => gaps.push("usage.fields_missing".into()),
    }
    match (observation.output_tokens, observation.max_tokens) {
        (Some(output), Some(limit)) if output > limit => {
            flags.push("inv.max_tokens_violated".into())
        }
        (Some(_), Some(_)) => (),
        _ => gaps.push("max_tokens.missing".into()),
    }

    let start_gaps = gaps.len();
    if !observation.complete {
        gaps.push("byte_bound.incomplete".into());
    }
    if !observation.text_only {
        gaps.push("byte_bound.non_text".into());
    }
    if observation.reasoning_tokens.is_none() {
        gaps.push("byte_bound.reasoning_unknown".into());
    }
    if observation.output_tokens.is_none() {
        gaps.push("byte_bound.output_missing".into());
    }
    if observation.visible_output_bytes.is_none() || observation.tool_payload_bytes.is_none() {
        gaps.push("byte_bound.payload_unmeasured".into());
    }
    if let (Some(reasoning), Some(output)) =
        (observation.reasoning_tokens, observation.output_tokens)
    {
        if reasoning > output {
            gaps.push("byte_bound.invalid_reasoning".into());
        }
    }
    if gaps.len() == start_gaps {
        let visible_tokens =
            observation.output_tokens.unwrap() - observation.reasoning_tokens.unwrap();
        let byte_budget = u128::from(observation.visible_output_bytes.unwrap())
            + u128::from(observation.tool_payload_bytes.unwrap())
            + 4;
        if u128::from(visible_tokens) > byte_budget {
            flags.push("inv.byte_bound".into());
        }
    }

    let mut axes = AxisResults::default();
    // Mixed output has no per-axis billing slope. A strictly homogeneous output
    // permits only this conditional, statistical comparison: IF the longest
    // emitted run is one generated token, the rest can use at most one token per
    // byte. SSE protocols do not guarantee that premise, so this never emits an
    // invariant/proof flag and always records the remaining confounder.
    match observation.stream.as_ref() {
        None => gaps.push("scv.no_stream".into()),
        Some(stream) if stream.batched => gaps.push("scv.batched".into()),
        Some(stream) if !stream.minimum_samples_met => gaps.push("scv.insufficient_samples".into()),
        Some(stream) => {
            let eligible = observation.complete
                && observation.text_only
                && observation.tool_payload_bytes == Some(0);
            if eligible {
                if let (Some(output), Some(reasoning), Some(bytes)) = (
                    observation.output_tokens,
                    observation.reasoning_tokens,
                    observation.visible_output_bytes,
                ) {
                    if output >= reasoning && stream.visible_output_chars >= 16 {
                        let lower_billed = output.saturating_sub(reasoning).saturating_sub(4);
                        let compare = |count: u64, run: u64, run_bytes: u64| -> AxisResult {
                            if count != stream.visible_output_chars || run < 2 || run_bytes == 0 {
                                return AxisResult::NotComparable;
                            }
                            let Some(remaining) = bytes.checked_sub(run_bytes) else {
                                return AxisResult::NotComparable;
                            };
                            if lower_billed > remaining.saturating_add(1) {
                                AxisResult::Contradiction
                            } else {
                                AxisResult::Consistent
                            }
                        };
                        axes.digit = compare(
                            stream.digit_chars,
                            stream.max_digit_run,
                            stream.max_digit_run,
                        );
                        axes.han = compare(
                            stream.han_chars,
                            stream.max_han_run,
                            stream.max_han_run_bytes,
                        );
                        axes.space = compare(
                            stream.space_chars,
                            stream.max_space_run,
                            stream.max_space_run_bytes,
                        );
                    }
                }
            }
            if [axes.digit, axes.han, axes.space]
                .iter()
                .all(|axis| *axis == AxisResult::NotComparable)
            {
                gaps.push("scv.billing_axis_uncalibrated".into());
            } else {
                gaps.push("scv.token_boundaries_unverified".into());
            }
        }
    }
    let stream_billing = whole_stream_billing(&observation);
    AuditRecord {
        verification: None,
        observation,
        flags,
        gaps,
        model_status,
        axes,
        stream_billing,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn honest() -> Observation {
        Observation {
            requested_model: Some("model-a".into()),
            upstream_model: Some("model-a".into()),
            input_tokens: Some(10),
            output_tokens: Some(8),
            total_tokens: Some(18),
            reasoning_tokens: Some(0),
            max_tokens: Some(32),
            visible_output_bytes: Some(24),
            tool_payload_bytes: Some(0),
            complete: true,
            text_only: true,
            ..Default::default()
        }
    }
    #[test]
    fn per_response_models_compare_final_forwarded_name_and_keep_conflicts() {
        let mut observation = honest();
        observation.forwarded_model = Some("model-b".into());
        // Returning the client alias is not a match after a different model was sent.
        assert_eq!(audit(observation.clone()).model_status, "mismatched");
        observation.upstream_model = Some("model-b".into());
        assert_eq!(audit(observation.clone()).model_status, "matched");
        observation.upstream_models = vec!["model-a".into(), "model-b".into()];
        let conflicting = audit(observation.clone());
        assert_eq!(conflicting.model_status, "conflicting");
        assert!(conflicting.flags.contains(&"inv.model_changed".into()));
        observation.upstream_models.clear();
        observation.model_evidence_truncated = true;
        assert_eq!(audit(observation).model_status, "unknown");
    }

    #[test]
    fn model_audit_honest_and_missing_are_distinct() {
        assert!(audit(honest()).flags.is_empty());
        let missing = audit(Observation::default());
        assert!(missing.flags.is_empty());
        assert_eq!(missing.model_status, "unknown");
        assert!(
            missing
                .gaps
                .contains(&"byte_bound.reasoning_unknown".into())
        );
    }
    #[test]
    fn model_audit_byte_bound_confounders_and_slack() {
        let mut observation = honest();
        observation.output_tokens = Some(29);
        observation.total_tokens = Some(39);
        assert!(
            audit(observation.clone())
                .flags
                .contains(&"inv.byte_bound".into())
        );
        observation.reasoning_tokens = Some(1);
        assert!(audit(observation.clone()).flags.is_empty());
        observation.reasoning_tokens = Some(0);
        observation.tool_payload_bytes = Some(1);
        assert!(audit(observation.clone()).flags.is_empty());
        observation.tool_payload_bytes = Some(0);
        observation.reasoning_tokens = None;
        assert!(
            !audit(observation.clone())
                .flags
                .contains(&"inv.byte_bound".into())
        );
        observation.reasoning_tokens = Some(0);
        observation.complete = false;
        assert!(
            !audit(observation.clone())
                .flags
                .contains(&"inv.byte_bound".into())
        );
        observation.complete = true;
        observation.text_only = false;
        assert!(!audit(observation).flags.contains(&"inv.byte_bound".into()));
    }
    #[test]
    fn model_audit_only_explicit_alias_matches() {
        let mut observation = honest();
        observation.upstream_model = Some("model-a-mini".into());
        assert_eq!(audit(observation.clone()).model_status, "mismatched");
        observation.forwarded_model = observation.upstream_model.clone();
        assert_eq!(audit(observation.clone()).model_status, "matched");
        observation.upstream_model = None;
        assert_eq!(audit(observation).model_status, "unknown");
    }
    #[test]
    fn model_audit_limits_overflow_and_invalid_reasoning() {
        let mut observation = honest();
        observation.output_tokens = Some(u64::MAX);
        observation.input_tokens = Some(u64::MAX);
        observation.total_tokens = Some(u64::MAX - 1);
        let record = audit(observation);
        assert!(record.flags.contains(&"inv.usage_selfcontradictory".into()));
        assert!(record.flags.contains(&"inv.max_tokens_violated".into()));
        let mut observation = honest();
        observation.reasoning_tokens = Some(9);
        let record = audit(observation);
        assert!(record.gaps.contains(&"byte_bound.invalid_reasoning".into()));
        assert!(!record.flags.contains(&"inv.byte_bound".into()));
    }
    #[test]
    fn model_audit_homogeneous_axes_are_conditional_not_proof() {
        use super::super::StreamAccumulator;
        let mut stream = StreamAccumulator::default();
        for index in 0..8 {
            stream.observe("123", 10 + index * 10);
        }
        let mut observation = honest();
        observation.stream = Some(stream.finish());
        let normal = audit(observation.clone());
        assert_eq!(normal.axes.digit, AxisResult::Consistent);
        assert_eq!(normal.axes.han, AxisResult::NotComparable);
        assert!(normal.flags.is_empty());
        assert!(
            normal
                .gaps
                .contains(&"scv.token_boundaries_unverified".into())
        );
        observation.output_tokens = Some(28);
        observation.total_tokens = Some(38);
        let inconsistent = audit(observation.clone());
        assert_eq!(inconsistent.axes.digit, AxisResult::Contradiction);
        assert!(inconsistent.flags.is_empty());
        observation.reasoning_tokens = None;
        assert_eq!(
            audit(observation.clone()).axes.digit,
            AxisResult::NotComparable
        );
        observation.reasoning_tokens = Some(0);
        observation.stream.as_mut().unwrap().batched = true;
        assert_eq!(audit(observation).axes.digit, AxisResult::NotComparable);
    }
    #[test]
    fn model_audit_mixed_output_never_infers_axis_billing() {
        use super::super::StreamAccumulator;
        let mut stream = StreamAccumulator::default();
        for index in 0..8 {
            stream.observe("123a", index * 10);
        }
        let mut observation = honest();
        observation.stream = Some(stream.finish());
        let record = audit(observation);
        assert_eq!(record.axes.digit, AxisResult::NotComparable);
        assert!(
            record
                .gaps
                .contains(&"scv.billing_axis_uncalibrated".into())
        );
    }
    #[test]
    fn model_audit_whole_stream_ratios_use_utf8_bytes() {
        use super::super::StreamAccumulator;
        let mut stream = StreamAccumulator::default();
        let text = "汉字";
        for index in 0..8 {
            stream.observe(text, 10 + index * 10);
        }
        let summary = stream.finish();
        assert_eq!(summary.visible_output_chars, 16);
        let mut observation = honest();
        observation.visible_output_bytes = Some((text.len() * 8) as u64);
        observation.stream = Some(summary);
        let record = audit(observation);
        assert!(record.flags.is_empty());
        let billing = record.stream_billing.unwrap();
        assert_eq!(billing.visible_bytes, Some(48));
        assert_eq!(billing.visible_tokens, Some(8));
        assert_eq!(billing.text_frames, 8);
        assert_eq!(billing.tokens_per_byte, Some(1.0 / 6.0));
        assert_eq!(billing.bytes_per_frame, Some(6.0));
        assert_eq!(billing.frames_per_token, Some(1.0));
        assert_eq!(billing.status, AxisResult::Consistent);
        assert!(
            billing
                .gaps
                .contains(&"scv.token_boundaries_unverified".into())
        );
        let json = serde_json::to_value(&billing).unwrap();
        assert_eq!(json["visibleBytes"], 48);
        assert!(json.get("tokensPerByte").is_some());
        assert!(json.get("bytesPerFrame").is_some());
        for obsolete in ["visibleChars", "tokensPerChar", "charsPerFrame"] {
            assert!(json.get(obsolete).is_none());
        }
    }
    #[test]
    fn model_audit_whole_stream_missing_bytes_do_not_fall_back_to_chars() {
        use super::super::StreamAccumulator;
        let mut stream = StreamAccumulator::default();
        for index in 0..8 {
            stream.observe("汉字", 10 + index * 10);
        }
        let mut observation = honest();
        observation.stream = Some(stream.finish());
        observation.visible_output_bytes = None;
        let billing = audit(observation.clone()).stream_billing.unwrap();
        assert_eq!(billing.status, AxisResult::NotComparable);
        assert_eq!(billing.visible_bytes, None);
        assert_eq!(billing.tokens_per_byte, None);
        assert_eq!(billing.bytes_per_frame, None);
        assert_eq!(billing.frames_per_token, Some(1.0));
        assert!(billing.gaps.contains(&"scv.bytes_missing".into()));
        observation.visible_output_bytes = Some(0);
        let empty = audit(observation).stream_billing.unwrap();
        assert_eq!(empty.status, AxisResult::NotComparable);
        assert_eq!(empty.tokens_per_byte, None);
        assert!(empty.gaps.contains(&"scv.no_text".into()));
    }
    #[test]
    fn model_audit_legacy_stream_character_fields_are_not_relabelled_as_bytes() {
        let restored: StreamBilling = serde_json::from_value(serde_json::json!({
            "status": "not_comparable", "textFrames": 8, "visibleTokens": 8,
            "visibleChars": 16, "tokensPerChar": 0.5, "charsPerFrame": 2.0,
            "framesPerToken": 1.0, "gaps": [], "limitations": []
        }))
        .unwrap();
        assert_eq!(restored.visible_bytes, None);
        assert_eq!(restored.tokens_per_byte, None);
        assert_eq!(restored.bytes_per_frame, None);
        assert_eq!(restored.frames_per_token, Some(1.0));
    }
    #[test]
    fn model_audit_old_record_deserializes_without_whole_stream_field() {
        let record = audit(honest());
        let mut json = serde_json::to_value(record).unwrap();
        json.as_object_mut().unwrap().remove("streamBilling");
        let restored: AuditRecord = serde_json::from_value(json).unwrap();
        assert!(restored.stream_billing.is_none());
    }
}
