use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AliasRule {
    pub provider_id: String,
    pub requested_model: String,
    pub reported_model: String,
    pub user_confirmed: bool,
    #[serde(default)]
    pub allow_route_change: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VerificationSettings {
    pub enabled: bool,
    pub active_probe_enabled: bool,
    pub max_requests_per_day: u32,
    pub soft_token_budget_per_day: u64,
    pub max_output_tokens: u32,
    pub timeout_seconds: u64,
    pub cooldown_hours: u64,
    pub rolling_window_size: usize,
    pub repeated_anomaly_threshold: usize,
    pub store_diagnostic_ids: bool,
    pub aliases: Vec<AliasRule>,
}
impl Default for VerificationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            active_probe_enabled: false,
            max_requests_per_day: 1,
            soft_token_budget_per_day: 100,
            max_output_tokens: 8,
            timeout_seconds: 15,
            cooldown_hours: 24,
            rolling_window_size: 10,
            repeated_anomaly_threshold: 2,
            store_diagnostic_ids: false,
            aliases: vec![],
        }
    }
}
impl VerificationSettings {
    pub fn validate(&self) -> Result<(), String> {
        if self.soft_token_budget_per_day > 10_000 {
            return Err("每日软 Token 预算必须为 0–10000 的整数；0 表示不允许收费复核。".into());
        }
        if !(1..=20).contains(&self.max_requests_per_day)
            || !(1..=64).contains(&self.max_output_tokens)
            || !(1..=30).contains(&self.timeout_seconds)
            || !(1..=168).contains(&self.cooldown_hours)
            || !(2..=100).contains(&self.rolling_window_size)
            || !(1..=self.rolling_window_size).contains(&self.repeated_anomaly_threshold)
        {
            return Err("检测预算或窗口参数超出允许范围".into());
        }
        if self.aliases.len() > 128
            || self.aliases.iter().any(|a| {
                !a.user_confirmed
                    || safe_id(&a.provider_id).is_none()
                    || safe_id(&a.requested_model).is_none()
                    || safe_id(&a.reported_model).is_none()
            })
        {
            return Err("只接受明确确认、有效且不超过 128 条的模型别名".into());
        }
        Ok(())
    }
}

/// Captured at dispatch; never serialize authentication or request bodies.
#[derive(Debug, Clone)]
pub struct Capture {
    pub provider_id: String,
    pub requested_model: String,
    pub protocol: String,
    pub credential_fingerprint: String,
    pub context_key: String,
    pub generation: u64,
    pub store_diagnostic_ids: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationEvidence {
    pub observation_id: String,
    pub upstream_id: String,
    pub requested_model: String,
    pub forwarded_model: Option<String>,
    pub reported_models: Vec<String>,
    pub credential_fingerprint: String,
    pub context_key: String,
    pub protocol: String,
    pub protocol_shape: Option<String>,
    pub claim_consistency: String,
    pub identity_assurance: String,
    pub evidence_codes: Vec<String>,
    pub explanation: String,
    pub risk_level: String,
    pub observed_at: i64,
    pub usage_status: String,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_creation_tokens: Option<u64>,
    pub is_streaming: bool,
    pub stream_completed: bool,
    pub http_status: Option<u16>,
    pub finish_reason: Option<String>,
    pub response_id: Option<String>,
    pub provider_request_id: Option<String>,
    pub evidence_source: String,
    pub source: String,
    pub rolling_observed_count: usize,
    pub route_change_count: usize,
    pub repeated_anomaly: bool,
    pub new_evidence: bool,
    pub active_probe_state: String,
    pub active_probe_tokens: Option<u64>,
}
impl VerificationEvidence {
    pub fn has_complete_response(&self) -> bool {
        self.stream_completed
            && self.http_status.is_some_and(|s| (200..300).contains(&s))
            && !self.evidence_codes.iter().any(|code| {
                matches!(
                    code.as_str(),
                    "HTTP_REQUEST_FAILED"
                        | "HTTP_STATUS_UNKNOWN"
                        | "RESPONSE_INVALID"
                        | "RESPONSE_INCOMPLETE"
                        | "STREAM_INCOMPLETE"
                        | "MODEL_EVIDENCE_TRUNCATED"
                )
            })
    }
    pub fn eligible_anomaly(&self) -> bool {
        self.source == "passive"
            && self.has_complete_response()
            && self.evidence_codes.iter().any(|c| {
                matches!(
                    c.as_str(),
                    "MODEL_CLAIM_MISMATCH" | "MODEL_ROUTE_CHANGED" | "PROTOCOL_SHAPE_CHANGED"
                )
            })
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerificationStatus {
    pub settings: VerificationSettings,
    pub context_error: Option<String>,
    pub recent: Vec<VerificationEvidence>,
    pub observed_responses: u64,
    pub probe_requests_today: u64,
    pub probe_tokens_today: Option<u64>,
    pub budget_used_today: u64,
    pub capability_confirmed: bool,
    pub dropped_observations: u64,
    pub storage_error: bool,
}

pub fn safe_id(value: &str) -> Option<String> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    (!value.is_empty()
        && value.len() <= 256
        && !lower.starts_with("sk-")
        && !lower.starts_with("bearer")
        && !value.contains("://")
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-/+:".contains(&b)))
    .then(|| value.to_owned())
}

/// Extension boundary only: no implementation currently produces trusted identity.
#[allow(dead_code)]
pub trait TrustedEvidenceVerifier: Send + Sync {
    fn verify(&self, proof: &[u8], expected_route: &str) -> Result<TrustedIdentity, String>;
}
#[derive(Debug)]
#[allow(dead_code)]
pub struct TrustedIdentity {
    pub model: String,
    pub issuer: String,
    pub proof_version: String,
}
#[allow(dead_code)]
pub trait DeepVerificationPlugin: Send + Sync {
    fn reference_version(&self) -> &str;
    fn compare_public_sample(&self, sample: &[u8]) -> Result<DeepComparison, String>;
}
#[derive(Debug)]
#[allow(dead_code)]
pub struct DeepComparison {
    pub reference_version: String,
    pub sample_count: usize,
    pub conclusion: String,
}
