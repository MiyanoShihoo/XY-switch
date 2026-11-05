use crate::{plugins::ToolCallPreview, privacy::PrivacyReport};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const DEFAULT_PORT: u16 = 15_722;
pub const SUPPORTED_VERSIONS: &[&str] = &["2026-07-28", "2025-11-25", "2025-06-18", "2025-03-26"];
pub const MODERN_VERSION: &str = "2026-07-28";
pub const MAX_MESSAGE_BYTES: usize = 128 * 1024;
pub const MAX_REPLY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedMcpSettings {
    pub enabled: bool,
}

impl Default for SharedMcpSettings {
    fn default() -> Self { Self { enabled: false } }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SharedMcpConnection {
    pub id: String,
    pub owner: String,
    pub clients: Vec<String>,
    pub server_id: String,
    pub active: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedMcpStatus {
    pub enabled: bool,
    pub running: bool,
    pub address: String,
    pub versions: Vec<String>,
    pub connections: Vec<SharedMcpConnection>,
    pub pending_count: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedToolApproval {
    pub id: String,
    pub owner: String,
    pub clients: Vec<String>,
    pub preview: ToolCallPreview,
    pub arguments: Value,
    pub privacy: PrivacyReport,
}

pub fn clients_for_owner(owner: &str) -> Option<&'static [&'static str]> {
    match owner {
        "codex" => Some(&["codex-desktop", "codex-cli"]),
        "claude" => Some(&["claude-code"]),
        "claude-desktop" | "claude-desktop-3p" => Some(&["claude-desktop", "claude-desktop-3p"]),
        "opencode" => Some(&["opencode-desktop", "opencode-cli"]),
        "gemini" => Some(&["gemini-cli"]),
        _ => None,
    }
}

pub(crate) fn config_target_for_owner(owner: &str) -> &str {
    match owner {
        "claude-desktop-3p" => "claude-desktop",
        _ => owner,
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default().as_millis().min(i64::MAX as u128) as i64
}

pub(crate) fn valid_connection_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

pub(crate) fn secret_id(id: &str) -> String { format!("shared-{id}") }
