use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MODEL_PREFIX: &str = "xy-group/";
pub const DAY_MS: i64 = 86_400_000;

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Strategy {
    #[default]
    RoundRobin,
    WeightedRoundRobin,
    LeastLoad,
    LatencyFirst,
    QuotaAware,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Limits {
    pub rpm: Option<u64>,
    pub tpm: Option<u64>,
    pub concurrency: Option<u32>,
    pub daily_microusd: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CooldownPolicy {
    pub rate_limit_seconds: u64,
    pub failure_seconds: u64,
    pub max_backoff_seconds: u64,
}

impl Default for CooldownPolicy {
    fn default() -> Self {
        Self { rate_limit_seconds: 60, failure_seconds: 30, max_backoff_seconds: 3600 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    pub id: String,
    pub provider_id: String,
    pub account_id: Option<String>,
    pub upstream_model: String,
    pub enabled: bool,
    pub priority: u16,
    pub weight: u16,
    pub quota_pool_id: Option<String>,
    #[serde(default)]
    pub quota_independent: bool,
    #[serde(default)]
    pub limits: Limits,
    #[serde(default)]
    pub cooldown: CooldownPolicy,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelGroup {
    pub id: String,
    pub name: String,
    pub virtual_model: String,
    pub enabled: bool,
    pub strategy: Strategy,
    pub project_id: Option<String>,
    pub affinity_ttl_seconds: u64,
    pub channels: Vec<Channel>,
    #[serde(default)]
    pub revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectBudget {
    pub id: String,
    pub daily_microusd: Option<u64>,
    #[serde(default)]
    pub revision: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configuration {
    pub groups: Vec<ChannelGroup>,
    pub projects: Vec<ProjectBudget>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SaveGroup {
    pub group: ChannelGroup,
    pub project: Option<ProjectBudget>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTotals {
    pub requests: u64,
    pub known_tokens: u64,
    pub token_known_requests: u64,
    pub known_microusd: u64,
    pub cost_known_requests: u64,
}

#[derive(Clone, Debug, Default)]
pub struct LedgerBaseline {
    pub usage: UsageTotals,
    pub days: BTreeMap<i64, LedgerDay>,
}

#[derive(Clone, Debug, Default)]
pub struct LedgerDay {
    pub committed_microusd: u64,
    pub unpriced_requests: u64,
}

#[derive(Clone, Debug, Default)]
pub struct ProviderFacts {
    pub origin: String,
    pub enabled: bool,
    pub account_ids: Vec<String>,
    pub unavailable_accounts: Vec<String>,
    pub account_pool_configured: bool,
    pub account_quotas: BTreeMap<String, crate::accounts::QuotaSnapshot>,
    pub account_cooldowns: BTreeMap<String, i64>,
    pub account_models: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordedEvent {
    pub id: i64,
    pub provider_id: String,
    pub at_ms: i64,
    pub status: Option<u16>,
    pub latency_ms: Option<u64>,
    pub tokens: Option<u64>,
    pub cost_microusd: Option<u64>,
    pub error_kind: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RouteRequest {
    pub id: String,
    pub group_id: String,
    pub reserve_tokens: u64,
    pub reserve_microusd: Option<u64>,
    #[serde(default)]
    pub stateful: bool,
    pub session_key: Option<String>,
    pub previous_response_id: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Outcome {
    Http,
    Timeout,
    TransportUnknown,
    Cancelled,
    NotSent,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CompletionUsage {
    pub tokens: Option<u64>,
    pub microusd: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Completion {
    pub outcome: Outcome,
    pub status: Option<u16>,
    #[serde(default)]
    pub usage: CompletionUsage,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    pub response_id: Option<String>,
    pub conversation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase", deny_unknown_fields)]
pub enum TrialStep {
    Request { at_ms: i64, request: RouteRequest },
    StreamStarted { at_ms: i64, request_id: String },
    Complete { at_ms: i64, request_id: String, result: Completion },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrialInput {
    pub group_id: String,
    #[serde(default)]
    pub replay_event_ids: Vec<i64>,
    pub steps: Vec<TrialStep>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Exclusion {
    pub channel_id: String,
    pub reason: String,
    pub retry_at_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decision {
    pub request_id: String,
    pub channel_id: Option<String>,
    pub upstream_model: Option<String>,
    pub reason: String,
    pub excluded: Vec<Exclusion>,
    pub dispatch_allowed: bool,
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuotaView {
    pub remaining_requests: Option<u64>,
    pub remaining_tokens: Option<u64>,
    pub requests_reset_at_ms: Option<i64>,
    pub tokens_reset_at_ms: Option<i64>,
    pub source: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelView {
    pub group_id: String,
    pub channel_id: String,
    pub provider_id: String,
    pub pool_id: String,
    pub pool_declared: bool,
    pub health: String,
    pub cooldown_reason: Option<String>,
    pub recovery_at_ms: Option<i64>,
    pub rpm: u64,
    pub tpm_reserved: u64,
    pub tpm_unknown_requests: u64,
    pub concurrent: u32,
    pub error_rate: Option<f64>,
    pub latency_ms: Option<f64>,
    pub quota: QuotaView,
    pub recorded_usage: UsageTotals,
    pub simulated_usage: UsageTotals,
    pub unknown_results: u64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub phase: &'static str,
    pub live_enabled: bool,
    pub billing_hold: bool,
    pub configuration: Configuration,
    pub channels: Vec<ChannelView>,
    pub warnings: Vec<String>,
    pub generated_at_ms: i64,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrialResult {
    pub source: &'static str,
    pub live_dispatches: u64,
    pub ledger_writes: u64,
    pub decisions: Vec<Decision>,
    pub replayed_event_ids: Vec<i64>,
    pub channels: Vec<ChannelView>,
    pub warnings: Vec<String>,
}
