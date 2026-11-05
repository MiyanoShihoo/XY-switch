use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

pub const PACKAGE_SCHEMA_VERSION: u32 = 1;
pub const MAX_PACKAGE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PACKAGE_FILES: usize = 128;
pub const MAX_FILE_BYTES: usize = 256 * 1024;
pub const MAX_TOOL_BYTES: usize = 64 * 1024;
pub const HOST_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const SUPPORTED_CLIENTS: &[&str] = &[
    "codex-desktop",
    "codex-cli",
    "claude-desktop",
    "claude-desktop-3p",
    "claude-code",
    "opencode-desktop",
    "opencode-cli",
    "gemini-cli",
];

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginPackage {
    pub manifest: PluginManifest,
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub description: String,
    pub author: String,
    pub version: String,
    pub minimum_host_version: String,
    pub kind: PluginKind,
    pub entry: String,
    pub clients: Vec<String>,
    pub permissions: PluginPermissions,
    pub dependencies: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PluginKind {
    Mcp,
    Skill,
    Provider,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginPermissions {
    pub network: Vec<String>,
    pub read_paths: Vec<String>,
    pub write_paths: Vec<String>,
    pub process: bool,
    pub credentials: Vec<String>,
}

impl PluginPermissions {
    pub fn is_empty(&self) -> bool {
        self.network.is_empty()
            && self.read_paths.is_empty()
            && self.write_paths.is_empty()
            && !self.process
            && self.credentials.is_empty()
    }

    pub fn added_to(&self, previous: &Self) -> Self {
        let added = |current: &[String], old: &[String]| {
            current
                .iter()
                .filter(|scope| !old.contains(scope))
                .cloned()
                .collect()
        };
        Self {
            network: added(&self.network, &previous.network),
            read_paths: added(&self.read_paths, &previous.read_paths),
            write_paths: added(&self.write_paths, &previous.write_paths),
            process: self.process && !previous.process,
            credentials: added(&self.credentials, &previous.credentials),
        }
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PublishedPlugin {
    pub package: PluginPackage,
    pub digest: String,
    pub signature: String,
    pub signing_key_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolBundle {
    pub schema_version: u32,
    pub tools: Vec<DeclarativeTool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclarativeTool {
    pub name: String,
    pub description: String,
    pub input_schema: ToolInputSchema,
    pub operation: ToolOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolInputSchema {
    #[serde(rename = "type")]
    pub schema_type: String,
    pub properties: BTreeMap<String, ToolInputProperty>,
    pub required: Vec<String>,
    pub additional_properties: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolInputProperty {
    #[serde(rename = "type")]
    pub value_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_length: Option<u32>,
    #[serde(rename = "enum", default, skip_serializing_if = "Option::is_none")]
    pub enum_values: Option<Vec<Value>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
pub enum ToolOperation {
    #[serde(rename = "json_passthrough")]
    JsonPassthrough,
    #[serde(rename = "read_text")]
    ReadText {
        #[serde(rename = "pathArgument")]
        path_argument: String,
    },
    #[serde(rename = "http_get")]
    HttpGet {
        #[serde(rename = "urlArgument")]
        url_argument: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderMapping {
    pub schema_version: u32,
    pub upstream_format: ProviderFormat,
    pub models: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderFormat {
    OpenaiChat,
    OpenaiResponses,
    Anthropic,
    Gemini,
}

#[derive(Debug, Clone)]
pub enum PluginEntry {
    Mcp(ToolBundle),
    Skill(String),
    Provider(ProviderMapping),
}

#[derive(Debug, Clone)]
pub struct ValidatedPackage {
    pub digest: String,
    pub canonical_size: usize,
    pub file_count: usize,
    pub entry: PluginEntry,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageValidation {
    pub digest: String,
    pub canonical_size: usize,
    pub file_count: usize,
    pub manifest: PluginManifest,
    pub runtime_supported: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TrustedKeyInput {
    pub id: String,
    pub label: String,
    pub public_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedSigningKey {
    pub id: String,
    pub label: String,
    pub public_key: String,
    pub fingerprint: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientPermissionChange {
    pub client: String,
    pub added: PluginPermissions,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPreview {
    pub preview_id: String,
    pub expires_at: i64,
    pub validation: PackageValidation,
    pub signing_key_id: String,
    pub signing_key_fingerprint: String,
    pub previous_version: Option<String>,
    pub permission_changes: Vec<ClientPermissionChange>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InstallConfirmation {
    pub preview_id: String,
    pub digest: String,
    pub accepted_permissions: PluginPermissions,
    pub clients: Vec<String>,
    pub confirmed: bool,
    pub enable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginVersionInfo {
    pub version: String,
    pub digest: String,
    pub installed_at: i64,
    pub signing_key_id: String,
    pub verification: String,
    pub withdrawn: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledPlugin {
    pub id: String,
    pub manifest: Option<PluginManifest>,
    pub active_version: Option<String>,
    pub digest: Option<String>,
    pub installed: bool,
    pub enabled_clients: Vec<String>,
    pub versions: Vec<PluginVersionInfo>,
    pub verification: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedTool {
    pub plugin_id: String,
    pub version: String,
    pub digest: String,
    pub client: String,
    pub tool: DeclarativeTool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedSkill {
    pub plugin_id: String,
    pub version: String,
    pub digest: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedProviderMapping {
    pub plugin_id: String,
    pub version: String,
    pub digest: String,
    pub mapping: ProviderMapping,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallPreview {
    pub preview_id: String,
    pub expires_at: i64,
    pub plugin_id: String,
    pub version: String,
    pub plugin_digest: String,
    pub client: String,
    pub tool: String,
    pub arguments_digest: String,
    pub required_permissions: PluginPermissions,
    pub effect: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ToolCallConfirmation {
    pub preview_id: String,
    pub plugin_digest: String,
    pub arguments_digest: String,
    pub accepted_permissions: PluginPermissions,
    pub confirmed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolExecutionResult {
    pub plugin_id: String,
    pub version: String,
    pub digest: String,
    pub client: String,
    pub tool: String,
    pub value: Value,
    pub redacted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WithdrawalReason {
    Security,
    Privacy,
    Withdrawn,
}

impl WithdrawalReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Security => "security",
            Self::Privacy => "privacy",
            Self::Withdrawn => "withdrawn",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginWithdrawal {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub reason: WithdrawalReason,
    pub signing_key_id: String,
    pub signature: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginError {
    pub code: &'static str,
    pub message: &'static str,
}

pub type PluginResult<T> = Result<T, PluginError>;

impl PluginError {
    pub fn new(code: &'static str) -> Self {
        let message = match code {
            "plugin_package_size" => "Plugin package exceeds the bounded text package limit.",
            "plugin_schema" => {
                "Plugin JSON or schema is invalid; unknown and duplicate fields are rejected."
            }
            "plugin_path" => {
                "Plugin paths must be portable relative text-file paths without links or aliases."
            }
            "plugin_privacy" => {
                "Plugin content contains private information or cannot be safely inspected."
            }
            "plugin_semver" => "Plugin versions must be valid, bounded semantic versions.",
            "plugin_host_version" => "This plugin requires a newer host version.",
            "plugin_permissions" => {
                "Plugin permission declarations are invalid or exceed host limits."
            }
            "plugin_client" => "Select explicitly supported plugin clients.",
            "plugin_dependency" => {
                "Pinned plugin dependencies are missing, disabled, invalid or cyclic."
            }
            "plugin_entry" => {
                "Plugin entry is not a supported declarative tool, Markdown skill or provider mapping."
            }
            "plugin_digest" => "Plugin digest does not match the canonical typed package.",
            "plugin_signature" => "Plugin signature is invalid for its configured trust key.",
            "plugin_untrusted_key" => {
                "Configure the publisher key from a separately trusted source first."
            }
            "plugin_trust_conflict" => "A trust key identifier cannot be silently replaced.",
            "plugin_confirmation" => {
                "Explicit approval must match the exact preview, digest, clients and permissions."
            }
            "plugin_preview_expired" => {
                "Preview expired or local plugin state changed; create a new preview."
            }
            "plugin_immutable_version" => {
                "A published plugin version cannot be overwritten or changed."
            }
            "plugin_process_unsupported" => {
                "Process execution requires OS isolation and is not supported by this host."
            }
            "plugin_credentials_unsupported" => {
                "Credential access requires OS isolation and is not supported by this host."
            }
            "plugin_write_unsupported" => {
                "Writing files is not supported by this immutable plugin host."
            }
            "plugin_external_read_unsupported" => {
                "This host can read only explicitly granted text files from the verified package."
            }
            "plugin_disabled" => "The plugin is not enabled and approved for this client.",
            "plugin_withdrawn" => {
                "The publisher withdrew this package; execution and rollback are blocked."
            }
            "plugin_storage" => {
                "Plugin storage could not be safely accessed; no content was returned."
            }
            "plugin_storage_boundary" => {
                "Plugin storage contains a link, alias or changed directory boundary."
            }
            "plugin_database" => "Plugin metadata could not be read or saved safely.",
            "plugin_not_found" => "The requested installed plugin version does not exist.",
            "plugin_limit" => "The local plugin, preview or execution limit was reached.",
            "plugin_tool_arguments" => "Tool arguments do not match the bounded declared schema.",
            "plugin_tool_not_found" => "The verified plugin does not declare this tool.",
            "plugin_network" => {
                "The request must use an approved HTTPS origin and public resolved addresses."
            }
            "plugin_http" => {
                "The bounded text request failed; redirects and proxy credentials are not allowed."
            }
            _ => "The plugin operation was safely rejected.",
        };
        Self { code, message }
    }
}

impl std::fmt::Display for PluginError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for PluginError {}
