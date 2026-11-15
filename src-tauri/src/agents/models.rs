use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub records: usize,
    pub actual_cost_usd: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub unknown_cost_records: usize,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageRecord {
    pub key: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub total_tokens: i64,
    pub actual_cost_usd: Option<f64>,
    pub estimated_cost_usd: Option<f64>,
    pub price_source: Option<String>,
    pub price_version: Option<String>,
    pub basis: String,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentContext {
    pub used_tokens: i64,
    pub window_tokens: Option<i64>,
    pub measured_at: i64,
    pub basis: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentEvent {
    pub id: String,
    pub source_id: String,
    pub source_path: String,
    pub source_line: u64,
    pub client_id: String,
    pub session_id: String,
    pub agent_id: String,
    pub task_id: Option<String>,
    pub timestamp: i64,
    pub lifecycle_at: Option<i64>,
    pub observed_at: i64,
    pub kind: String,
    pub title: String,
    pub summary: Option<String>,
    pub status: Option<String>,
    pub related_agent_id: Option<String>,
    pub parent_agent_id: Option<String>,
    pub correlation_id: Option<String>,
    pub name: Option<String>,
    pub role: Option<String>,
    pub model: Option<String>,
    pub workspace: Option<String>,
    pub usage: Option<AgentUsageRecord>,
    pub context: Option<AgentContext>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapability {
    pub feature: String,
    pub mode: String,
    pub reason: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSource {
    pub id: String,
    pub client_id: String,
    pub name: String,
    pub connection: String,
    pub mode: String,
    pub last_checked_at: Option<i64>,
    pub last_event_at: Option<i64>,
    pub file_count: usize,
    pub detail: String,
    pub capabilities: Vec<AgentCapability>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentNode {
    pub id: String,
    pub session_id: String,
    pub client_id: String,
    pub source_id: String,
    pub name: String,
    pub role: Option<String>,
    pub model: Option<String>,
    pub workspace: Option<String>,
    pub parent_id: Option<String>,
    pub team_id: String,
    pub status: String,
    pub connection: String,
    pub created_at: i64,
    pub last_event_at: i64,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub current_task_id: Option<String>,
    pub usage: AgentUsage,
    pub context: Option<AgentContext>,
    pub evidence_event_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTask {
    pub id: String,
    pub agent_id: String,
    pub requested_by_agent_id: Option<String>,
    pub parent_task_id: Option<String>,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub started_at: Option<i64>,
    pub ended_at: Option<i64>,
    pub last_event_at: i64,
    pub summary: Option<String>,
    pub error: Option<String>,
    pub usage: AgentUsage,
    pub evidence_event_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEdge {
    pub id: String,
    pub source_agent_id: String,
    pub target_agent_id: String,
    pub kind: String,
    pub task_id: Option<String>,
    pub timestamp: i64,
    pub evidence_event_id: String,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentTeam {
    pub id: String,
    pub name: String,
    pub client_id: String,
    pub agent_count: usize,
    pub running_count: usize,
    pub last_event_at: i64,
    pub usage: AgentUsage,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsageGroup {
    pub dimension: String,
    pub id: String,
    pub label: String,
    pub usage: AgentUsage,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentHistory {
    pub oldest_at: Option<i64>,
    pub latest_at: Option<i64>,
    pub event_count: usize,
    pub retention_days: u32,
    pub truncated: bool,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentSnapshot {
    pub version: u32,
    pub revision: u64,
    pub generated_at: i64,
    pub replay_at: Option<i64>,
    pub sources: Vec<AgentSource>,
    pub teams: Vec<AgentTeam>,
    pub agents: Vec<AgentNode>,
    pub edges: Vec<AgentEdge>,
    pub tasks: Vec<AgentTask>,
    pub usage_groups: Vec<AgentUsageGroup>,
    pub totals: AgentUsage,
    pub history: AgentHistory,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentSnapshotQuery {
    pub team_id: Option<String>,
    pub at: Option<i64>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentEventQuery {
    pub team_id: Option<String>,
    pub agent_id: Option<String>,
    pub task_id: Option<String>,
    pub since: Option<i64>,
    pub until: Option<i64>,
    pub cursor: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentEventPage {
    pub events: Vec<AgentEvent>,
    pub next_cursor: Option<String>,
    pub total: usize,
}

pub struct LineContext<'a> {
    pub source_id: &'a str,
    pub client_id: &'a str,
    pub source_path: &'a str,
    pub line_number: u64,
    pub observed_at: i64,
}

pub type ParseRecord = fn(&serde_json::Value, &LineContext<'_>, &mut serde_json::Value) -> Vec<AgentEvent>;
