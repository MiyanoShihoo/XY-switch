use rusqlite::{Connection, Row, params, types::Type};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use toml_edit::DocumentMut;

const MIGRATION: &str = include_str!("../migrations/001_initial.sql");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", try_from = "ProviderInput")]
pub struct Provider {
    pub id: String,
    pub name: String,
    #[serde(default = "default_provider_type", alias = "type")]
    pub provider_type: String,
    pub endpoint: String,
    pub model: String,
    #[serde(alias = "group")]
    pub app_group: String,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub sort_order: i64,
    #[serde(default)]
    pub advanced: ProviderAdvancedConfig,
}

// The legacy group is accepted on import but never determines routing.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProviderInput {
    id: String,
    name: String,
    #[serde(default = "default_provider_type", alias = "type")]
    provider_type: String,
    endpoint: String,
    model: String,
    #[serde(default = "legacy_group", alias = "group")]
    app_group: String,
    #[serde(default)]
    enabled: bool,
    #[serde(default)]
    sort_order: i64,
    #[serde(default)]
    advanced: Value,
}

fn legacy_group() -> String {
    "Codex".into()
}

fn decode_advanced(
    provider_type: &str,
    mut value: Value,
) -> Result<ProviderAdvancedConfig, serde_json::Error> {
    if value.is_null() {
        value = Value::Object(Map::new());
    }
    if let Some(object) = value.as_object_mut() {
        object.entry("upstreamFormat").or_insert_with(|| {
            Value::String(
                if provider_type.eq_ignore_ascii_case("anthropic") {
                    "anthropic"
                } else {
                    "openai_responses"
                }
                .into(),
            )
        });
    }
    serde_json::from_value(value)
}

impl TryFrom<ProviderInput> for Provider {
    type Error = serde_json::Error;
    fn try_from(input: ProviderInput) -> Result<Self, Self::Error> {
        let advanced = decode_advanced(&input.provider_type, input.advanced)?;
        Ok(Self {
            id: input.id,
            name: input.name,
            provider_type: input.provider_type,
            endpoint: input.endpoint,
            model: input.model,
            app_group: input.app_group,
            enabled: input.enabled,
            sort_order: input.sort_order,
            advanced,
        })
    }
}

fn default_provider_type() -> String {
    "custom".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderModelMapping {
    pub model: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub context_window: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CodexProviderConfig {
    #[serde(default)]
    pub remote_compression: bool,
    #[serde(default)]
    pub apply_common_config: bool,
    #[serde(default)]
    pub context_1m: bool,
    #[serde(default = "default_compaction_threshold")]
    pub compaction_threshold: u64,
    #[serde(default)]
    pub config_toml: String,
}

fn default_compaction_threshold() -> u64 {
    900_000
}

/// Claude 供应商专属的快捷开关。
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeProviderConfig {
    /// 开启后，转发前把 Claude Code 的 Artifact 工具整个从 tools 数组里摘掉。
    ///
    /// 部分第三方 Anthropic 兼容网关（DeepSeek 在列）用严格的正则校验工具 JSON Schema，
    /// 拒收 Artifact 工具里的 Unicode 属性转义（`\p{Cc}`、`\p{Cf}` 等），于是**每一个**
    /// 请求都固定返回 400 `Invalid schema for function 'Artifact'`，与所选模型无关。
    #[serde(default)]
    pub disable_artifact_tool: bool,
}

impl Default for CodexProviderConfig {
    fn default() -> Self {
        Self {
            remote_compression: false,
            apply_common_config: false,
            context_1m: false,
            compaction_threshold: default_compaction_threshold(),
            config_toml: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderAdvancedConfig {
    #[serde(default)]
    pub is_full_url: bool,
    #[serde(default = "default_upstream_format")]
    pub upstream_format: String,
    #[serde(default)]
    pub model_mappings: Vec<ProviderModelMapping>,
    #[serde(default)]
    pub custom_user_agent: String,
    #[serde(default)]
    pub header_overrides: BTreeMap<String, String>,
    #[serde(default)]
    pub body_overrides: Map<String, Value>,
    #[serde(default)]
    pub codex: CodexProviderConfig,
    #[serde(default)]
    pub claude: ClaudeProviderConfig,
}

fn default_upstream_format() -> String {
    "openai_responses".to_string()
}

impl Default for ProviderAdvancedConfig {
    fn default() -> Self {
        Self {
            is_full_url: false,
            upstream_format: default_upstream_format(),
            model_mappings: Vec::new(),
            custom_user_agent: String::new(),
            header_overrides: BTreeMap::new(),
            body_overrides: Map::new(),
            codex: CodexProviderConfig::default(),
            claude: ClaudeProviderConfig::default(),
        }
    }
}

fn is_sensitive_name(name: &str) -> bool {
    let normalized = name.to_ascii_lowercase().replace('-', "_");
    [
        "authorization",
        "api_key",
        "apikey",
        "access_token",
        "auth_token",
        "password",
        "secret",
        "cookie",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn object_contains_sensitive_key(value: &Value) -> bool {
    match value {
        Value::Object(object) => object
            .iter()
            .any(|(key, value)| is_sensitive_name(key) || object_contains_sensitive_key(value)),
        Value::Array(values) => values.iter().any(object_contains_sensitive_key),
        _ => false,
    }
}

impl ProviderAdvancedConfig {
    fn validate(&self) -> Result<(), String> {
        if !matches!(
            self.upstream_format.as_str(),
            "openai_responses" | "openai_chat" | "anthropic"
        ) {
            return Err("无效的上游格式".to_string());
        }
        if self.model_mappings.len() > 200 {
            return Err("模型映射数量超过限制 (最多 200 个)".to_string());
        }
        let mut models = std::collections::BTreeSet::new();
        for mapping in &self.model_mappings {
            let model = mapping.model.trim();
            if model.is_empty() || model.len() > 256 {
                return Err("模型 ID 过长或为空".to_string());
            }
            if !models.insert(model.to_ascii_lowercase()) {
                return Err(format!("duplicate model mapping: {model}"));
            }
        }
        if self.custom_user_agent.len() > 512
            || (!self.custom_user_agent.is_empty()
                && reqwest::header::HeaderValue::from_str(&self.custom_user_agent).is_err())
        {
            return Err("User-Agent 过长或格式无效".to_string());
        }
        const PROTECTED_HEADERS: [&str; 12] = [
            "host",
            "content-length",
            "transfer-encoding",
            "connection",
            "proxy-authorization",
            "content-type",
            "authorization",
            "x-api-key",
            "x-goog-api-key",
            "cookie",
            "set-cookie",
            "accept-encoding",
        ];
        if self.header_overrides.len() > 64 {
            return Err("Header 覆盖数量超过限制".to_string());
        }
        for (name, value) in &self.header_overrides {
            let normalized = name.trim().to_ascii_lowercase();
            if reqwest::header::HeaderName::from_bytes(normalized.as_bytes()).is_err()
                || reqwest::header::HeaderValue::from_str(value).is_err()
            {
                return Err(format!("invalid header override: {name}"));
            }
            if PROTECTED_HEADERS.contains(&normalized.as_str()) || is_sensitive_name(&normalized) {
                return Err(format!(
                    "protected or sensitive header cannot be overridden: {name}"
                ));
            }
        }
        if self.body_overrides.contains_key("stream") || self.body_overrides.contains_key("model") {
            return Err(
                "body overrides cannot replace protocol fields model or stream".to_string(),
            );
        }
        if object_contains_sensitive_key(&Value::Object(self.body_overrides.clone())) {
            return Err("prompt not found".to_string());
        }
        if self.codex.context_1m
            && !(10_000..=10_000_000).contains(&self.codex.compaction_threshold)
        {
            return Err(
                "Codex compaction threshold must be between 10000 and 10000000".to_string(),
            );
        }
        if self.codex.config_toml.len() > 128 * 1024 {
            return Err("prompt not found".to_string());
        }
        if !self.codex.config_toml.trim().is_empty() {
            self.codex
                .config_toml
                .parse::<DocumentMut>()
                .map_err(|error| format!("invalid Codex config.toml: {error}"))?;
            for line in self.codex.config_toml.lines() {
                let key = line.split('=').next().unwrap_or_default().trim();
                if !key.starts_with('#') && is_sensitive_name(key) {
                    return Err("prompt not found".to_string());
                }
            }
        }
        Ok(())
    }
}

fn provider_from_row(row: &Row<'_>) -> rusqlite::Result<Provider> {
    let advanced_json: String = row.get(8)?;
    let provider_type: String = row.get(2)?;
    let advanced = serde_json::from_str(&advanced_json)
        .and_then(|value| decode_advanced(&provider_type, value))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(8, Type::Text, Box::new(error))
        })?;
    Ok(Provider {
        id: row.get(0)?,
        name: row.get(1)?,
        provider_type,
        endpoint: row.get(3)?,
        model: row.get(4)?,
        app_group: row.get(5)?,
        enabled: row.get::<_, i64>(6)? != 0,
        sort_order: row.get(7)?,
        advanced,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestRecordResult {
    pub event_id: i64,
    pub consecutive_failures: i64,
}

#[derive(Debug, Clone, Default)]
pub struct RequestUsage {
    pub audit: Option<crate::model_audit::AuditRecord>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cost: Option<f64>,
    pub price_source: Option<String>,
    pub price_version: Option<String>,
}

#[derive(Debug, Clone)]
pub struct RequestEvent {
    pub provider_id: String,
    pub route: String,
    pub status_code: Option<u16>,
    pub success: bool,
    pub affects_health: bool,
    pub latency_ms: i64,
    pub retry_count: i64,
    pub error_kind: Option<String>,
    pub model: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cost: Option<f64>,
    pub price_source: Option<String>,
    pub price_version: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderHealth {
    pub provider_id: String,
    pub status: String,
    pub latency_ms: Option<i64>,
    pub success_rate: f64,
    pub consecutive_failures: i64,
    pub circuit_open_until: Option<i64>,
    pub last_error: Option<String>,
    pub last_checked_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestSummary {
    pub total_requests: i64,
    pub successful_requests: i64,
    pub average_latency_ms: f64,
}

#[derive(Debug, Clone)]
pub struct McpRecord {
    pub id: String,
    pub name: String,
    pub transport: String,
    pub config_json: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Prompt {
    pub id: String,
    pub name: String,
    pub content: String,
    pub description: Option<String>,
    pub enabled: bool,
}
#[derive(Clone)]
pub struct Database {
    connection: Arc<Mutex<Connection>>,
    pub(crate) shadow: Arc<crate::shadow_audit_runtime::ShadowAuditRuntime>,
    pub(crate) verification: Arc<crate::model_verification::VerificationRuntime>,
}

impl Database {
    pub fn conn(&self) -> Arc<Mutex<Connection>> {
        self.connection.clone()
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        let connection = Connection::open(path).map_err(|error| error.to_string())?;
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch("PRAGMA secure_delete = ON; PRAGMA auto_vacuum = INCREMENTAL;")
            .map_err(|error| error.to_string())?;
        connection
            .execute_batch(MIGRATION)
            .map_err(|error| error.to_string())?;
        let has_advanced = connection
            .prepare("PRAGMA table_info(providers)")
            .and_then(|mut statement| {
                let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
                columns.collect::<Result<Vec<_>, _>>()
            })
            .map_err(|error| error.to_string())?
            .iter()
            .any(|column| column == "advanced_json");
        if !has_advanced {
            connection
                .execute(
                    "ALTER TABLE providers ADD COLUMN advanced_json TEXT NOT NULL DEFAULT '{}'",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }

        let prompt_columns = connection
            .prepare("PRAGMA table_info(prompts)")
            .and_then(|mut statement| {
                let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
                columns.collect::<Result<Vec<_>, _>>()
            })
            .map_err(|error| error.to_string())?;
        if !prompt_columns.iter().any(|column| column == "description") {
            connection
                .execute("ALTER TABLE prompts ADD COLUMN description TEXT", [])
                .map_err(|error| error.to_string())?;
        }
        if !prompt_columns.iter().any(|column| column == "enabled") {
            connection
                .execute(
                    "ALTER TABLE prompts ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1",
                    [],
                )
                .map_err(|error| error.to_string())?;
        }

        let request_event_columns = connection
            .prepare("PRAGMA table_info(request_events)")
            .and_then(|mut statement| {
                let columns = statement.query_map([], |row| row.get::<_, String>(1))?;
                columns.collect::<Result<Vec<_>, _>>()
            })
            .map_err(|error| error.to_string())?;
        for (column, column_type) in [
            ("model", "TEXT"),
            ("input_tokens", "INTEGER"),
            ("output_tokens", "INTEGER"),
            ("cache_creation_tokens", "INTEGER"),
            ("cache_read_tokens", "INTEGER"),
            ("cost", "REAL"),
            ("price_source", "TEXT"),
            ("price_version", "TEXT"),
            ("requested_model", "TEXT"),
            ("upstream_model", "TEXT"),
            ("total_tokens", "INTEGER"),
            ("reasoning_tokens", "INTEGER"),
            ("visible_output_bytes", "INTEGER"),
            ("tool_payload_bytes", "INTEGER"),
            ("max_tokens", "INTEGER"),
            ("text_delta_count", "INTEGER"),
            ("audit_flags", "TEXT"),
            ("audit_json", "TEXT"),
        ] {
            if !request_event_columns
                .iter()
                .any(|existing| existing == column)
            {
                connection
                    .execute(
                        &format!("ALTER TABLE request_events ADD COLUMN {column} {column_type}"),
                        [],
                    )
                    .map_err(|error| error.to_string())?;
            }
        }

        connection.execute_batch(
            "CREATE TABLE IF NOT EXISTS shadow_audit_samples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_id INTEGER NOT NULL UNIQUE REFERENCES request_events(id) ON DELETE CASCADE,
                provider_id TEXT NOT NULL,
                model TEXT NOT NULL,
                tokenizer_sha256 TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                result_json TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_shadow_audit_created ON shadow_audit_samples(created_at,id);
            CREATE INDEX IF NOT EXISTS idx_shadow_audit_provider ON shadow_audit_samples(provider_id,model);"
        ).map_err(|error| error.to_string())?;
        let connection = Arc::new(Mutex::new(connection));
        let shadow = Arc::new(crate::shadow_audit_runtime::ShadowAuditRuntime::new(
            Arc::downgrade(&connection),
        ));
        let verification = crate::model_verification::VerificationRuntime::new(Arc::downgrade(&connection));
        Ok(Self { connection, shadow, verification })
    }

    pub fn list_prompts(&self) -> Result<Vec<Prompt>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare("SELECT id,name,content,description,enabled FROM prompts ORDER BY updated_at DESC,name")
            .map_err(|error| error.to_string())?;
        statement
            .query_map([], |row| {
                Ok(Prompt {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    content: row.get(2)?,
                    description: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                })
            })
            .map_err(|error| error.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn save_prompt(&self, prompt: &Prompt) -> Result<(), String> {
        let id = prompt.id.trim();
        let name = prompt.name.trim();
        let content = prompt.content.trim();
        let description = prompt
            .description
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if id.is_empty() {
            return Err("prompt not found".to_string());
        }
        if name.is_empty() || name.chars().count() > 100 {
            return Err("prompt not found".to_string());
        }
        if content.is_empty() || content.chars().count() > 200_000 {
            return Err("prompt not found".to_string());
        }
        if description.is_some_and(|value| value.chars().count() > 500) {
            return Err("prompt not found".to_string());
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection.execute(
            "INSERT INTO prompts(id,name,content,description,enabled) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,content=excluded.content,description=excluded.description,enabled=excluded.enabled,updated_at=CURRENT_TIMESTAMP",
            params![id, name, content, description, prompt.enabled as i64],
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn delete_prompt(&self, id: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let changed = connection
            .execute("DELETE FROM prompts WHERE id=?1", [id.trim()])
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("prompt not found".to_string());
        }
        Ok(())
    }

    pub fn set_prompt_enabled(&self, id: &str, enabled: bool) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let changed = connection
            .execute(
                "UPDATE prompts SET enabled=?2,updated_at=CURRENT_TIMESTAMP WHERE id=?1",
                params![id.trim(), enabled as i64],
            )
            .map_err(|error| error.to_string())?;
        if changed == 0 {
            return Err("prompt not found".to_string());
        }
        Ok(())
    }
    pub fn list_providers(&self) -> Result<Vec<Provider>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection.prepare(
            "SELECT id,name,provider_type,endpoint,model,app_group,enabled,sort_order,advanced_json FROM providers WHERE provider_type <> 'local_usage' ORDER BY sort_order,name",
        ).map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], provider_from_row)
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn save_provider(&self, provider: &Provider) -> Result<(), String> {
        if provider.id.trim().is_empty() || provider.name.trim().is_empty() {
            return Err("prompt not found".to_string());
        }
        provider.advanced.validate()?;
        let advanced_json =
            serde_json::to_string(&provider.advanced).map_err(|error| error.to_string())?;
        if advanced_json.len() > 256 * 1024 {
            return Err("prompt not found".to_string());
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection.execute(
            "INSERT INTO providers(id,name,provider_type,endpoint,model,app_group,enabled,sort_order,advanced_json) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9) ON CONFLICT(id) DO UPDATE SET name=excluded.name,provider_type=excluded.provider_type,endpoint=excluded.endpoint,model=excluded.model,app_group=excluded.app_group,enabled=excluded.enabled,sort_order=excluded.sort_order,advanced_json=excluded.advanced_json,updated_at=CURRENT_TIMESTAMP",
            params![provider.id,provider.name,provider.provider_type,provider.endpoint,provider.model,provider.app_group,provider.enabled as i64,provider.sort_order,advanced_json],
        ).map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn reorder_providers(&self, provider_ids: &[String]) -> Result<(), String> {
        if provider_ids.is_empty() || provider_ids.len() > 500 {
            return Err("prompt not found".to_string());
        }
        let unique = provider_ids
            .iter()
            .map(|id| id.trim())
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() != provider_ids.len() || unique.contains("") {
            return Err("prompt not found".to_string());
        }

        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let provider_count: usize = transaction
            .query_row(
                "SELECT COUNT(*) FROM providers WHERE provider_type <> 'local_usage'",
                [],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        if provider_count != provider_ids.len() {
            return Err("prompt not found".to_string());
        }
        for (index, id) in provider_ids.iter().enumerate() {
            let changed = transaction
                .execute(
                    "UPDATE providers SET sort_order=?1,updated_at=CURRENT_TIMESTAMP WHERE id=?2",
                    params![((index + 1) * 10) as i64, id],
                )
                .map_err(|error| error.to_string())?;
            if changed != 1 {
                return Err(format!("provider not found while reordering: {id}"));
            }
        }
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn get_provider(&self, id: &str) -> Result<Option<Provider>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare("SELECT id,name,provider_type,endpoint,model,app_group,enabled,sort_order,advanced_json FROM providers WHERE id=?1")
            .map_err(|error| error.to_string())?;
        let mut rows = statement.query([id]).map_err(|error| error.to_string())?;
        let Some(row) = rows.next().map_err(|error| error.to_string())? else {
            return Ok(None);
        };
        provider_from_row(row)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    pub fn current_provider(&self) -> Result<Option<Provider>, String> {
        let id = {
            let connection = self
                .connection
                .lock()
                .map_err(|_| "database lock poisoned".to_string())?;
            connection
                .query_row(
                    "SELECT value FROM app_settings WHERE key='current_provider'",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .map_err(|error| error.to_string())?
        };
        self.get_provider(&id)
    }

    pub fn set_current_provider(&self, id: &str) -> Result<(), String> {
        let Some(provider) = self.get_provider(id)? else {
            return Err("prompt not found".to_string());
        };
        if !provider.enabled {
            return Err("prompt not found".to_string());
        }
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO app_settings(key,value) VALUES('current_provider',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP",
                [id],
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let result = connection.query_row(
            "SELECT value FROM app_settings WHERE key=?1",
            [key],
            |row| row.get(0),
        );
        match result {
            Ok(value) => Ok(Some(value)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        if key.trim().is_empty() {
            return Err("prompt not found".to_string());
        }
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP",
                params![key, value],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn record_request(&self, event: &RequestEvent) -> Result<RequestRecordResult, String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "INSERT INTO request_events(provider_id,route,status_code,success,latency_ms,retry_count,error_kind,model,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,cost,price_source,price_version,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                params![event.provider_id,event.route,event.status_code,event.success as i64,event.latency_ms,event.retry_count,event.error_kind,event.model,event.input_tokens,event.output_tokens,event.cache_creation_tokens,event.cache_read_tokens,event.cost,event.price_source,event.price_version,event.created_at],
            )
            .map_err(|error| error.to_string())?;
        let event_id = transaction.last_insert_rowid();
        // Diagnostic probes are independent observations, not health checks.
        // In particular they must not change health timestamps, latency or circuits.
        if event.route.starts_with("model_probe/") {
            let failures = transaction
                .query_row(
                    "SELECT consecutive_failures FROM provider_health WHERE provider_id=?1",
                    [&event.provider_id],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap_or(0);
            transaction.commit().map_err(|error| error.to_string())?;
            return Ok(RequestRecordResult {
                event_id,
                consecutive_failures: failures,
            });
        }
        transaction
            .execute(
                "INSERT INTO provider_health(provider_id,status,latency_ms,success_rate,consecutive_failures,last_error,last_checked_at) VALUES(?1,?2,?3,0,?4,?5,?6) ON CONFLICT(provider_id) DO UPDATE SET status=CASE WHEN ?7=0 THEN provider_health.status ELSE excluded.status END,latency_ms=excluded.latency_ms,consecutive_failures=CASE WHEN ?7=0 THEN provider_health.consecutive_failures WHEN excluded.status='healthy' THEN 0 ELSE provider_health.consecutive_failures+1 END,last_error=CASE WHEN ?7=0 THEN provider_health.last_error ELSE excluded.last_error END,last_checked_at=excluded.last_checked_at",
                params![event.provider_id,if event.success { "healthy" } else if event.affects_health { "unhealthy" } else { "unknown" },event.latency_ms,if event.success || !event.affects_health { 0 } else { 1 },event.error_kind,event.created_at,event.affects_health as i64],
            )
            .map_err(|error| error.to_string())?;
        transaction
            .execute(
                "UPDATE provider_health SET success_rate=COALESCE((SELECT AVG(success)*100.0 FROM (SELECT success FROM request_events WHERE provider_id=?1 ORDER BY id DESC LIMIT 100)),0) WHERE provider_id=?1",
                [&event.provider_id],
            )
            .map_err(|error| error.to_string())?;
        let failures = transaction
            .query_row(
                "SELECT consecutive_failures FROM provider_health WHERE provider_id=?1",
                [&event.provider_id],
                |row| row.get(0),
            )
            .map_err(|error| error.to_string())?;
        transaction.commit().map_err(|error| error.to_string())?;
        Ok(RequestRecordResult {
            event_id,
            consecutive_failures: failures,
        })
    }

    pub fn update_request_usage(&self, event_id: i64, usage: &RequestUsage) -> Result<(), String> {
        // Ordinary billing remains available with verification disabled. Analysis is
        // queued only for captures made while the opt-in module was enabled.
        let connection = self.connection.lock().map_err(|_| "database lock poisoned".to_string())?;
        let updated = connection.execute(
            "UPDATE request_events SET model=?2,input_tokens=?3,output_tokens=?4,cache_creation_tokens=?5,cache_read_tokens=?6,cost=?7,price_source=?8,price_version=?9 WHERE id=?1",
            params![event_id,usage.model,usage.input_tokens,usage.output_tokens,usage.cache_creation_tokens,usage.cache_read_tokens,usage.cost,usage.price_source,usage.price_version]
        ).map_err(|error|error.to_string())?;
        drop(connection);
        if updated != 1 {return Err(format!("request event {event_id} not found"));}
        if let Some(audit) = &usage.audit {
            if !audit.observation.verification_active {
                self.verification.submit(self.clone(),event_id,audit.observation.clone());
            }
        }
        Ok(())
    }

    pub fn set_circuit_open_until(
        &self,
        provider_id: &str,
        until: Option<i64>,
    ) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "UPDATE provider_health SET circuit_open_until=?2,status=CASE WHEN ?2 IS NULL THEN status ELSE 'circuit_open' END WHERE provider_id=?1",
                params![provider_id,until],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn provider_health(&self, provider_id: &str) -> Result<Option<ProviderHealth>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let result = connection.query_row(
            "SELECT provider_id,status,latency_ms,success_rate,consecutive_failures,circuit_open_until,last_error,last_checked_at FROM provider_health WHERE provider_id=?1",
            [provider_id],
            |row| Ok(ProviderHealth { provider_id: row.get(0)?, status: row.get(1)?, latency_ms: row.get(2)?, success_rate: row.get(3)?, consecutive_failures: row.get(4)?, circuit_open_until: row.get(5)?, last_error: row.get(6)?, last_checked_at: row.get(7)? }),
        );
        match result {
            Ok(health) => Ok(Some(health)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn list_provider_health(&self) -> Result<Vec<ProviderHealth>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection.prepare(
            "SELECT provider_id,status,latency_ms,success_rate,consecutive_failures,circuit_open_until,last_error,last_checked_at FROM provider_health ORDER BY provider_id",
        ).map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(ProviderHealth {
                    provider_id: row.get(0)?,
                    status: row.get(1)?,
                    latency_ms: row.get(2)?,
                    success_rate: row.get(3)?,
                    consecutive_failures: row.get(4)?,
                    circuit_open_until: row.get(5)?,
                    last_error: row.get(6)?,
                    last_checked_at: row.get(7)?,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn request_summary(&self) -> Result<RequestSummary, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection.query_row(
            "SELECT COUNT(*),COALESCE(SUM(success),0),COALESCE(AVG(latency_ms),0) FROM request_events WHERE route <> 'health_check'",
            [],
            |row| Ok(RequestSummary { total_requests: row.get(0)?, successful_requests: row.get(1)?, average_latency_ms: row.get(2)? }),
        ).map_err(|error| error.to_string())
    }

    pub fn delete_provider(&self, id: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute("DELETE FROM providers WHERE id=?1", [id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn list_mcp_records(&self) -> Result<Vec<McpRecord>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare(
                "SELECT id,name,transport,config_json,enabled FROM mcp_servers ORDER BY name,id",
            )
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([], |row| {
                Ok(McpRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    transport: row.get(2)?,
                    config_json: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                })
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|error| error.to_string())
    }

    pub fn get_mcp_record(&self, id: &str) -> Result<Option<McpRecord>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let result = connection.query_row(
            "SELECT id,name,transport,config_json,enabled FROM mcp_servers WHERE id=?1",
            [id],
            |row| {
                Ok(McpRecord {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    transport: row.get(2)?,
                    config_json: row.get(3)?,
                    enabled: row.get::<_, i64>(4)? != 0,
                })
            },
        );
        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(error) => Err(error.to_string()),
        }
    }

    pub fn save_mcp_record(&self, record: &McpRecord) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO mcp_servers(id,name,transport,config_json,enabled) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET name=excluded.name,transport=excluded.transport,config_json=excluded.config_json,enabled=excluded.enabled,updated_at=CURRENT_TIMESTAMP",
                params![record.id, record.name, record.transport, record.config_json, record.enabled as i64],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn delete_mcp_record(&self, id: &str) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute("DELETE FROM mcp_servers WHERE id=?1", [id])
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    pub fn mcp_app_states(&self, server_id: &str) -> Result<BTreeMap<String, bool>, String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let mut statement = connection
            .prepare("SELECT app,enabled FROM mcp_app_states WHERE server_id=?1 ORDER BY app")
            .map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([server_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? != 0))
            })
            .map_err(|error| error.to_string())?;
        rows.collect::<Result<BTreeMap<_, _>, _>>()
            .map_err(|error| error.to_string())
    }

    /// Replace one server's selection atomically; failures leave its old states intact.
    pub fn replace_mcp_app_states(
        &self,
        server_id: &str,
        states: &BTreeMap<String, bool>,
    ) -> Result<(), String> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        transaction
            .execute("DELETE FROM mcp_app_states WHERE server_id=?1", [server_id])
            .map_err(|error| error.to_string())?;
        for (app, enabled) in states {
            transaction
                .execute(
                    "INSERT INTO mcp_app_states(server_id,app,enabled) VALUES(?1,?2,?3)",
                    params![server_id, app, *enabled as i64],
                )
                .map_err(|error| error.to_string())?;
        }
        transaction.commit().map_err(|error| error.to_string())
    }

    pub fn set_mcp_app_enabled(
        &self,
        server_id: &str,
        app: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let connection = self
            .connection
            .lock()
            .map_err(|_| "database lock poisoned".to_string())?;
        connection
            .execute(
                "INSERT INTO mcp_app_states(server_id,app,enabled) VALUES(?1,?2,?3) ON CONFLICT(server_id,app) DO UPDATE SET enabled=excluded.enabled,updated_at=CURRENT_TIMESTAMP",
                params![server_id, app, enabled as i64],
            )
            .map_err(|error| error.to_string())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Database, RequestEvent};

    #[test]
    fn seeds_and_switches_current_provider() {
        let path = std::env::temp_dir().join(format!("xy-switch-{}.db", uuid::Uuid::new_v4()));
        let database = Database::open(&path).unwrap();

        assert_eq!(database.list_providers().unwrap().len(), 4);
        let mut provider_ids = database
            .list_providers()
            .unwrap()
            .into_iter()
            .map(|provider| provider.id)
            .collect::<Vec<_>>();
        provider_ids.reverse();
        database.reorder_providers(&provider_ids).unwrap();
        assert_eq!(
            database
                .list_providers()
                .unwrap()
                .into_iter()
                .map(|provider| provider.id)
                .collect::<Vec<_>>(),
            provider_ids
        );
        assert!(database.reorder_providers(&["openai".to_string()]).is_err());
        assert_eq!(
            database.current_provider().unwrap().unwrap().id,
            "anthropic"
        );
        database.set_current_provider("openai").unwrap();
        assert_eq!(database.current_provider().unwrap().unwrap().id, "openai");
        assert_eq!(database.current_provider().unwrap().unwrap().id, "openai");
        assert_eq!(database.current_provider().unwrap().unwrap().id, "openai");
        assert!(database.set_current_provider("deepseek").is_err());
        assert_eq!(database.get_setting("missing").unwrap(), None);
        database.set_setting("test_setting", "value").unwrap();
        assert_eq!(
            database.get_setting("test_setting").unwrap().as_deref(),
            Some("value")
        );

        for success in [true, false] {
            database
                .record_request(&RequestEvent {
                    provider_id: "openai".to_string(),
                    route: "/v1/chat/completions".to_string(),
                    status_code: Some(if success { 200 } else { 500 }),
                    success,
                    affects_health: true,
                    latency_ms: 100,
                    retry_count: 0,
                    error_kind: (!success).then(|| "upstream_status".to_string()),
                    model: None,
                    input_tokens: None,
                    output_tokens: None,
                    cache_creation_tokens: None,
                    cache_read_tokens: None,
                    cost: None,
                    price_source: None,
                    price_version: None,
                    created_at: 1,
                })
                .unwrap();
        }
        let health = database.provider_health("openai").unwrap().unwrap();
        assert_eq!(health.status, "unhealthy");
        assert_eq!(health.consecutive_failures, 1);
        assert_eq!(health.success_rate, 50.0);
        let summary = database.request_summary().unwrap();
        assert_eq!(summary.total_requests, 2);
        assert_eq!(summary.successful_requests, 1);

        drop(database);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn persists_custom_provider_crud_across_reopen() {
        let path = std::env::temp_dir().join(format!("xy-switch-crud-{}.db", uuid::Uuid::new_v4()));
        let database = Database::open(&path).unwrap();

        let mut provider = database
            .list_providers()
            .unwrap()
            .into_iter()
            .find(|provider| provider.id == "openai")
            .unwrap();
        provider.id = "custom-crud".to_string();
        provider.name = "Custom CRUD".to_string();
        provider.provider_type = "custom".to_string();
        provider.endpoint = "https://example.test/v1".to_string();
        provider.model = "model-v1".to_string();
        provider.app_group = "Codex".to_string();
        provider.enabled = true;
        provider.sort_order = 5;
        database.save_provider(&provider).unwrap();

        let created = database
            .list_providers()
            .unwrap()
            .into_iter()
            .find(|item| item.id == provider.id)
            .unwrap();
        assert_eq!(created.name, "Custom CRUD");
        assert_eq!(created.endpoint, "https://example.test/v1");
        assert_eq!(created.model, "model-v1");

        provider.name = "Custom CRUD Updated".to_string();
        provider.endpoint = "https://updated.example.test/v1".to_string();
        provider.model = "model-v2".to_string();
        provider.enabled = false;
        provider.sort_order = 999;
        database.save_provider(&provider).unwrap();
        drop(database);

        let reopened = Database::open(&path).unwrap();
        let updated = reopened
            .list_providers()
            .unwrap()
            .into_iter()
            .find(|item| item.id == provider.id)
            .unwrap();
        assert_eq!(updated.name, "Custom CRUD Updated");
        assert_eq!(updated.endpoint, "https://updated.example.test/v1");
        assert_eq!(updated.model, "model-v2");
        assert!(!updated.enabled);
        assert_eq!(updated.sort_order, 999);

        reopened.delete_provider(&provider.id).unwrap();
        drop(reopened);

        let reopened = Database::open(&path).unwrap();
        assert!(
            reopened
                .list_providers()
                .unwrap()
                .into_iter()
                .all(|item| item.id != provider.id)
        );
        reopened.delete_provider(&provider.id).unwrap();
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn persists_provider_order_across_reopen() {
        let path =
            std::env::temp_dir().join(format!("xy-switch-order-{}.db", uuid::Uuid::new_v4()));
        let database = Database::open(&path).unwrap();
        let provider_ids = database
            .list_providers()
            .unwrap()
            .into_iter()
            .map(|provider| provider.id)
            .collect::<Vec<_>>();
        assert!(provider_ids.len() >= 3);

        let expected = provider_ids.into_iter().rev().collect::<Vec<_>>();
        database.reorder_providers(&expected).unwrap();
        drop(database);

        let reopened = Database::open(&path).unwrap();
        let actual = reopened
            .list_providers()
            .unwrap()
            .into_iter()
            .take(expected.len())
            .map(|provider| provider.id)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn migrates_legacy_request_events_without_losing_data_and_is_idempotent() {
        let path = std::env::temp_dir().join(format!(
            "xy-switch-request-events-legacy-{}.db",
            uuid::Uuid::new_v4()
        ));
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE providers (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    provider_type TEXT NOT NULL,
                    endpoint TEXT NOT NULL,
                    api_key TEXT NOT NULL DEFAULT '',
                    model TEXT NOT NULL,
                    app_group TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    sort_order INTEGER NOT NULL DEFAULT 0,
                    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
                 );
                 INSERT INTO providers(id,name,provider_type,endpoint,model,app_group)
                 VALUES('legacy-provider','Legacy Provider','openai','https://example.test','legacy-model','Codex');
                 CREATE TABLE request_events (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    provider_id TEXT NOT NULL,
                    route TEXT NOT NULL,
                    status_code INTEGER,
                    success INTEGER NOT NULL DEFAULT 0,
                    latency_ms INTEGER NOT NULL DEFAULT 0,
                    retry_count INTEGER NOT NULL DEFAULT 0,
                    error_kind TEXT,
                    created_at INTEGER NOT NULL
                 );
                 INSERT INTO request_events(provider_id,route,status_code,success,latency_ms,retry_count,error_kind,created_at)
                 VALUES('legacy-provider','/v1/messages',200,1,42,0,NULL,123);",
            )
            .unwrap();
        drop(connection);

        for _ in 0..2 {
            drop(Database::open(&path).unwrap());
        }

        let connection = rusqlite::Connection::open(&path).unwrap();
        let preserved: (String, String, Option<i64>, i64) = connection
            .query_row(
                "SELECT provider_id,route,status_code,created_at FROM request_events WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();
        assert_eq!(
            preserved,
            (
                "legacy-provider".to_string(),
                "/v1/messages".to_string(),
                Some(200),
                123
            )
        );
        let usage: (Option<String>, Option<i64>, Option<i64>, Option<i64>, Option<i64>, Option<f64>, Option<String>, Option<String>) = connection
            .query_row(
                "SELECT model,input_tokens,output_tokens,cache_creation_tokens,cache_read_tokens,cost,price_source,price_version FROM request_events WHERE id=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?)),
            )
            .unwrap();
        assert_eq!(usage, (None, None, None, None, None, None, None, None));
        drop(connection);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn migrates_legacy_prompts_without_losing_data_and_is_idempotent() {
        let path = std::env::temp_dir().join(format!(
            "xy-switch-prompts-legacy-{}.db",
            uuid::Uuid::new_v4()
        ));
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE prompts (
                id TEXT PRIMARY KEY NOT NULL,
                name TEXT NOT NULL,
                content TEXT NOT NULL,
                target_apps TEXT NOT NULL DEFAULT '[]',
                active INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            INSERT INTO prompts(id,name,content,target_apps,active)
            VALUES('legacy','Legacy prompt','Preserve me','[\"claude\"]',1);",
            )
            .unwrap();
        drop(connection);

        for _ in 0..2 {
            let database = Database::open(&path).unwrap();
            let prompts = database.list_prompts().unwrap();
            let prompt = prompts.iter().find(|item| item.id == "legacy").unwrap();
            assert_eq!(prompt.name, "Legacy prompt");
            assert_eq!(prompt.content, "Preserve me");
            assert_eq!(prompt.description, None);
            assert!(prompt.enabled);
            drop(database);
        }

        let connection = rusqlite::Connection::open(&path).unwrap();
        let preserved: (String, i64) = connection
            .query_row(
                "SELECT target_apps, active FROM prompts WHERE id='legacy'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(preserved, ("[\"claude\"]".to_string(), 1));
        drop(connection);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn prompt_crud_persists_across_reopen() {
        let path = std::env::temp_dir().join(format!(
            "xy-switch-prompts-crud-{}.db",
            uuid::Uuid::new_v4()
        ));
        let database = Database::open(&path).unwrap();
        let mut prompt = super::Prompt {
            id: "crud-prompt".to_string(),
            name: "Prompt".to_string(),
            content: "Content".to_string(),
            description: Some("Description".to_string()),
            enabled: true,
        };
        database.save_prompt(&prompt).unwrap();
        prompt.name = "Updated prompt".to_string();
        prompt.enabled = false;
        database.save_prompt(&prompt).unwrap();
        drop(database);

        let reopened = Database::open(&path).unwrap();
        let stored = reopened
            .list_prompts()
            .unwrap()
            .into_iter()
            .find(|item| item.id == prompt.id)
            .unwrap();
        assert_eq!(stored.name, "Updated prompt");
        assert!(!stored.enabled);
        reopened.set_prompt_enabled(&prompt.id, true).unwrap();
        assert!(
            reopened
                .list_prompts()
                .unwrap()
                .into_iter()
                .find(|item| item.id == prompt.id)
                .unwrap()
                .enabled
        );
        reopened.delete_prompt(&prompt.id).unwrap();
        assert!(
            reopened
                .list_prompts()
                .unwrap()
                .into_iter()
                .all(|item| item.id != prompt.id)
        );
        drop(reopened);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn bulk_mcp_selection_failure_rolls_back_deletes_and_prior_inserts() {
        let path =
            std::env::temp_dir().join(format!("xy-switch-mcp-state-{}.db", uuid::Uuid::new_v4()));
        let database = Database::open(&path).unwrap();
        database
            .save_mcp_record(&super::McpRecord {
                id: "state-atomic".to_string(),
                name: "Atomic".to_string(),
                transport: "stdio".to_string(),
                config_json: "{}".to_string(),
                enabled: true,
            })
            .unwrap();
        let before = std::collections::BTreeMap::from([("claude".to_string(), true)]);
        database
            .replace_mcp_app_states("state-atomic", &before)
            .unwrap();
        database
            .connection
            .lock()
            .unwrap()
            .execute_batch(
                "CREATE TEMP TRIGGER selection_insert_failure BEFORE INSERT ON mcp_app_states
             WHEN NEW.app='codex' BEGIN SELECT RAISE(ABORT, 'synthetic selection failure'); END;",
            )
            .unwrap();
        let requested = std::collections::BTreeMap::from([
            ("claude".to_string(), false),
            ("codex".to_string(), true),
        ]);
        let error = database
            .replace_mcp_app_states("state-atomic", &requested)
            .unwrap_err();
        assert!(error.contains("synthetic selection failure"));
        assert_eq!(database.mcp_app_states("state-atomic").unwrap(), before);
        drop(database);
        std::fs::remove_file(path).unwrap();
    }
}
