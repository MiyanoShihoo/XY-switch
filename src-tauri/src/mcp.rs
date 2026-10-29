use crate::{
    credential_store,
    database::{Database, McpRecord},
    takeover::atomic_write,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use toml_edit::{Array, DocumentMut, Item, Table, Value as TomlValue, value};

const MCP_APPS: [McpApp; 4] = [
    McpApp::Claude,
    McpApp::Codex,
    McpApp::Gemini,
    McpApp::OpenCode,
];
type FileSnapshot = (PathBuf, Option<Vec<u8>>);

trait McpCredentialStore: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, String>;
    fn set(&self, id: &str, value: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

struct WindowsMcpCredentialStore;

impl McpCredentialStore for WindowsMcpCredentialStore {
    fn get(&self, id: &str) -> Result<Option<String>, String> {
        credential_store::get_secret("mcp", id)
    }

    fn set(&self, id: &str, value: &str) -> Result<(), String> {
        credential_store::set_secret("mcp", id, value)
    }

    fn delete(&self, id: &str) -> Result<(), String> {
        credential_store::delete_secret("mcp", id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum McpApp {
    Claude,
    Codex,
    Gemini,
    OpenCode,
}

impl McpApp {
    fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            "opencode" => Ok(Self::OpenCode),
            _ => Err(format!("不支持的 MCP 应用：{value}")),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
            Self::OpenCode => "OpenCode",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub transport: String,
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    pub url: Option<String>,
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub apps: BTreeMap<String, bool>,
    #[serde(default)]
    pub built_in: bool,
    #[serde(default)]
    pub secrets_configured: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpAppStatus {
    pub app: String,
    pub label: String,
    pub installed: bool,
    pub config_path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpImportResult {
    pub imported: usize,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct PersistedMcpConfig {
    #[serde(default)]
    description: String,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    url: Option<String>,
    #[serde(default)]
    env_keys: Vec<String>,
    #[serde(default)]
    header_keys: Vec<String>,
    #[serde(default)]
    built_in: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct McpSecrets {
    #[serde(default)]
    env: BTreeMap<String, String>,
    #[serde(default)]
    headers: BTreeMap<String, String>,
}

#[derive(Clone)]
pub struct McpManager {
    database: Database,
    home_dir: PathBuf,
    operation_lock: Arc<Mutex<()>>,
    credentials: Arc<dyn McpCredentialStore>,
}

impl McpManager {
    pub fn new(database: Database, home_dir: PathBuf) -> Self {
        Self {
            database,
            home_dir,
            operation_lock: Arc::new(Mutex::new(())),
            credentials: Arc::new(WindowsMcpCredentialStore),
        }
    }

    #[cfg(test)]
    fn with_credentials(
        database: Database,
        home_dir: PathBuf,
        credentials: Arc<dyn McpCredentialStore>,
    ) -> Self {
        Self {
            database,
            home_dir,
            operation_lock: Arc::new(Mutex::new(())),
            credentials,
        }
    }

    pub fn list_servers(&self) -> Result<Vec<McpServer>, String> {
        self.database
            .list_mcp_records()?
            .into_iter()
            .map(|record| self.server_from_record(record, false))
            .collect()
    }

    pub fn app_statuses(&self) -> Vec<McpAppStatus> {
        MCP_APPS
            .into_iter()
            .map(|app| {
                let path = self.config_path(app);
                McpAppStatus {
                    app: app.id().to_string(),
                    label: app.label().to_string(),
                    installed: self.is_installed(app),
                    config_path: path.display().to_string(),
                }
            })
            .collect()
    }

    pub fn save_server(&self, server: McpServer) -> Result<McpServer, String> {
        let _guard = self.operation_lock.lock().map_err(|_| "MCP 操作锁损坏".to_string())?;
        self.save_server_inner(server)
    }

    /// Backups restore configuration, never credentials. Omitted or redacted
    /// secret fields must not erase credentials already saved on this machine.
    pub(crate) fn import_server(&self, mut server: McpServer) -> Result<McpServer, String> {
        let _guard = self.operation_lock.lock().map_err(|_| "MCP 操作锁损坏".to_string())?;
        validate_server(&server)?;
        server.env.clear();
        server.headers.clear();
        server.secrets_configured = false;
        server.built_in = false;
        if let Some(record) = self.database.get_mcp_record(server.id.trim())? {
            let config: PersistedMcpConfig = serde_json::from_str(&record.config_json)
                .map_err(|error| format!("MCP 配置损坏：{error}"))?;
            server.env = config.env_keys.into_iter().map(|key| (key, String::new())).collect();
            server.headers = config.header_keys.into_iter().map(|key| (key, String::new())).collect();
        }
        self.save_server_inner(server)
    }

    fn save_server_inner(&self, mut server: McpServer) -> Result<McpServer, String> {
        validate_server(&server)?;
        server.id = server.id.trim().to_string();
        let existing_record = self.database.get_mcp_record(&server.id)?;
        let previous_app_states = self.database.mcp_app_states(&server.id)?;
        let existing_config = existing_record
            .as_ref()
            .map(|existing| {
                serde_json::from_str::<PersistedMcpConfig>(&existing.config_json)
                    .map_err(|error| format!("MCP 配置损坏：{error}"))
            })
            .transpose()?;
        if let Some(existing_config) = existing_config.as_ref() {
            server.built_in = existing_config.built_in;
        }
        let needs_existing_secrets = existing_config
            .as_ref()
            .is_some_and(|config| !config.env_keys.is_empty() || !config.header_keys.is_empty());
        let existing_secrets = if needs_existing_secrets {
            self.read_secrets(&server.id)?.unwrap_or_default()
        } else {
            McpSecrets::default()
        };
        let secrets = merge_secrets(&server, &existing_secrets);
        let persisted = PersistedMcpConfig {
            description: server.description.trim().to_string(),
            command: server.command.as_deref().map(str::trim).map(str::to_string),
            args: server.args.clone(),
            url: server.url.as_deref().map(str::trim).map(str::to_string),
            env_keys: secrets.env.keys().cloned().collect(),
            header_keys: secrets.headers.keys().cloned().collect(),
            built_in: server.built_in,
        };
        let record = McpRecord {
            id: server.id.trim().to_string(),
            name: server.name.trim().to_string(),
            transport: server.transport.to_ascii_lowercase(),
            config_json: serde_json::to_string(&persisted).map_err(|error| error.to_string())?,
            enabled: true,
        };
        self.database.save_mcp_record(&record)?;
        let secret_result = if secrets.env.is_empty() && secrets.headers.is_empty() {
            if needs_existing_secrets {
                self.credentials.delete(&record.id)
            } else {
                Ok(())
            }
        } else {
            self.write_secrets(&record.id, &secrets)
        };
        if let Err(error) = secret_result {
            if let Some(existing) = existing_record {
                let _ = self.database.save_mcp_record(&existing);
            } else {
                let _ = self.database.delete_mcp_record(&record.id);
            }
            return Err(error);
        }

        let mut hydrated = self.server_from_record(record.clone(), true)?;
        // Omitted keys retain existing selection; explicit form choices override it.
        let mut desired_app_states = previous_app_states.clone();
        desired_app_states.extend(server.apps.clone());
        hydrated.apps.extend(desired_app_states.clone());
        if let Err(error) = self.sync_selected_apps(
            &hydrated, &previous_app_states, &desired_app_states,
        ) {
            if let Some(existing) = existing_record {
                let _ = self.database.save_mcp_record(&existing);
                if !existing_secrets.env.is_empty() || !existing_secrets.headers.is_empty() {
                    let _ = self.write_secrets(&server.id, &existing_secrets);
                } else if !secrets.env.is_empty() || !secrets.headers.is_empty() {
                    let _ = self.credentials.delete(&server.id);
                }
            } else {
                let _ = self.database.delete_mcp_record(&record.id);
                if !secrets.env.is_empty() || !secrets.headers.is_empty() {
                    let _ = self.credentials.delete(&record.id);
                }
            }
            return Err(error);
        }
        self.server_from_record(record, false)
    }

    pub fn delete_server(&self, id: &str) -> Result<(), String> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| "MCP 操作锁损坏".to_string())?;
        let record = self
            .database
            .get_mcp_record(id)?
            .ok_or_else(|| "MCP 服务器不存在".to_string())?;
        let app_states = self.database.mcp_app_states(id)?;
        let server = self.server_from_record(record.clone(), true)?;
        if server.built_in {
            return Err("内置 MCP 模板不能删除".to_string());
        }
        let snapshots = self.update_enabled_app_files(&server, None)?;
        if let Err(error) = self.database.delete_mcp_record(id) {
            return Err(with_file_rollback(error, &snapshots));
        }
        if server.secrets_configured
            && let Err(error) = self.credentials.delete(id)
        {
            let mut rollback_errors = Vec::new();
            if let Err(rollback_error) = self.database.save_mcp_record(&record) {
                rollback_errors.push(format!("恢复数据库记录失败：{rollback_error}"));
            } else {
                for (app, enabled) in app_states {
                    if let Err(rollback_error) =
                        self.database.set_mcp_app_enabled(id, &app, enabled)
                    {
                        rollback_errors.push(format!("恢复 {app} 状态失败：{rollback_error}"));
                    }
                }
            }
            rollback_errors.extend(restore_snapshots(&snapshots));
            let secrets = McpSecrets {
                env: server.env,
                headers: server.headers,
            };
            if (!secrets.env.is_empty() || !secrets.headers.is_empty())
                && let Err(rollback_error) = self.write_secrets(id, &secrets)
            {
                rollback_errors.push(format!("恢复安全凭据失败：{rollback_error}"));
            }
            return Err(with_rollback_errors(error, rollback_errors));
        }
        Ok(())
    }

    pub fn set_app_enabled(
        &self,
        server_id: &str,
        app_name: &str,
        enabled: bool,
    ) -> Result<McpServer, String> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| "MCP 操作锁损坏".to_string())?;
        let app = McpApp::parse(app_name)?;
        if enabled && !self.is_installed(app) {
            return Err(format!("未检测到 {}，不会创建应用配置目录", app.label()));
        }
        let record = self
            .database
            .get_mcp_record(server_id)?
            .ok_or_else(|| "MCP 服务器不存在".to_string())?;
        let server = self.server_from_record(record.clone(), true)?;
        let path = self.config_path(app);
        let original = read_optional(&path)?;
        self.write_app_entry(app, server_id, enabled.then_some(&server))?;
        if let Err(error) = self
            .database
            .set_mcp_app_enabled(server_id, app.id(), enabled)
        {
            let _ = restore_optional(&path, original.as_deref());
            return Err(error);
        }
        self.server_from_record(record, false)
    }

    pub fn import_from_apps(&self) -> Result<McpImportResult, String> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| "MCP 操作锁损坏".to_string())?;
        let mut imported = 0;
        let mut errors = Vec::new();
        for app in MCP_APPS {
            if !self.is_installed(app) {
                continue;
            }
            match self.read_app_entries(app) {
                Ok(entries) => {
                    for (id, mut server) in entries {
                        match self.database.get_mcp_record(&id) {
                            Ok(Some(_)) => match self.database.mcp_app_states(&id) {
                                Ok(states) if states.get(app.id()).copied().unwrap_or(false) => {}
                                Ok(_) => {
                                    if let Err(error) =
                                        self.database.set_mcp_app_enabled(&id, app.id(), true)
                                    {
                                        errors.push(format!("{} / {id}：{error}", app.label()));
                                    } else {
                                        imported += 1;
                                    }
                                }
                                Err(error) => {
                                    errors.push(format!("{} / {id}：{error}", app.label()))
                                }
                            },
                            Ok(None) => {
                                server.apps.insert(app.id().to_string(), true);
                                match self.save_imported_server(&server, app) {
                                    Ok(()) => imported += 1,
                                    Err(error) => {
                                        errors.push(format!("{} / {id}：{error}", app.label()))
                                    }
                                }
                            }
                            Err(error) => errors.push(format!("{} / {id}：{error}", app.label())),
                        }
                    }
                }
                Err(error) => errors.push(format!("{}：{error}", app.label())),
            }
        }
        Ok(McpImportResult { imported, errors })
    }

    fn save_imported_server(&self, server: &McpServer, app: McpApp) -> Result<(), String> {
        validate_server(server)?;
        let secrets = McpSecrets {
            env: server.env.clone(),
            headers: server.headers.clone(),
        };
        let persisted = PersistedMcpConfig {
            description: format!("从 {} 导入", app.label()),
            command: server.command.clone(),
            args: server.args.clone(),
            url: server.url.clone(),
            env_keys: secrets.env.keys().cloned().collect(),
            header_keys: secrets.headers.keys().cloned().collect(),
            built_in: false,
        };
        self.write_secrets(&server.id, &secrets)?;
        let record = McpRecord {
            id: server.id.clone(),
            name: server.name.clone(),
            transport: server.transport.clone(),
            config_json: serde_json::to_string(&persisted).map_err(|error| error.to_string())?,
            enabled: true,
        };
        if let Err(error) = self.database.save_mcp_record(&record) {
            if !secrets.env.is_empty() || !secrets.headers.is_empty() {
                let _ = self.credentials.delete(&server.id);
            }
            return Err(error);
        }
        if let Err(error) = self
            .database
            .set_mcp_app_enabled(&server.id, app.id(), true)
        {
            let _ = self.database.delete_mcp_record(&server.id);
            if !secrets.env.is_empty() || !secrets.headers.is_empty() {
                let _ = self.credentials.delete(&server.id);
            }
            return Err(error);
        }
        Ok(())
    }

    fn server_from_record(
        &self,
        record: McpRecord,
        include_secrets: bool,
    ) -> Result<McpServer, String> {
        let config: PersistedMcpConfig = serde_json::from_str(&record.config_json)
            .map_err(|error| format!("MCP {} 配置损坏：{error}", record.id))?;
        let apps = MCP_APPS
            .into_iter()
            .map(|app| (app.id().to_string(), false))
            .chain(self.database.mcp_app_states(&record.id)?)
            .collect();
        let has_secrets = !config.env_keys.is_empty() || !config.header_keys.is_empty();
        let secrets = if include_secrets && has_secrets {
            self.read_secrets(&record.id)?.unwrap_or_default()
        } else {
            McpSecrets {
                env: config
                    .env_keys
                    .iter()
                    .map(|key| (key.clone(), String::new()))
                    .collect(),
                headers: config
                    .header_keys
                    .iter()
                    .map(|key| (key.clone(), String::new()))
                    .collect(),
            }
        };
        Ok(McpServer {
            id: record.id,
            name: record.name,
            description: config.description,
            transport: record.transport,
            command: config.command,
            args: config.args,
            url: config.url,
            env: secrets.env,
            headers: secrets.headers,
            apps,
            built_in: config.built_in,
            secrets_configured: has_secrets,
        })
    }

    fn read_secrets(&self, id: &str) -> Result<Option<McpSecrets>, String> {
        self.credentials
            .get(id)?
            .map(|value| {
                serde_json::from_str(&value).map_err(|error| format!("MCP 安全配置损坏：{error}"))
            })
            .transpose()
    }

    fn write_secrets(&self, id: &str, secrets: &McpSecrets) -> Result<(), String> {
        if secrets.env.is_empty() && secrets.headers.is_empty() {
            return Ok(());
        }
        let value = serde_json::to_string(secrets).map_err(|error| error.to_string())?;
        self.credentials.set(id, &value)
    }

    fn sync_selected_apps(
        &self,
        server: &McpServer,
        previous_states: &BTreeMap<String, bool>,
        desired_states: &BTreeMap<String, bool>,
    ) -> Result<(), String> {
        let affected: Vec<McpApp> = MCP_APPS.into_iter().filter(|app| {
            previous_states.get(app.id()).copied().unwrap_or(false)
                || server.apps.get(app.id()).copied().unwrap_or(false)
        }).collect();
        // Preflight every target before the first write; never create an uninstalled client.
        let mut snapshots = Vec::new();
        for app in &affected {
            if server.apps.get(app.id()).copied().unwrap_or(false) && !self.is_installed(*app) {
                return Err(format!("未检测到 {}，不会创建应用配置目录", app.label()));
            }
            let path = self.config_path(*app);
            snapshots.push((path.clone(), read_optional(&path)?));
        }
        for (app, (_, original)) in affected.iter().zip(&snapshots) {
            let enabled = server.apps.get(app.id()).copied().unwrap_or(false);
            if !enabled && original.is_none() { continue; }
            if let Err(error) = self.write_app_entry(*app, &server.id, enabled.then_some(server)) {
                return Err(with_file_rollback(error, &snapshots));
            }
        }
        // Selection changes commit together only after all client files have succeeded.
        if let Err(error) = self.database.replace_mcp_app_states(&server.id, desired_states) {
            return Err(with_file_rollback(error, &snapshots));
        }
        Ok(())
    }

    fn update_enabled_app_files(
        &self,
        server: &McpServer,
        replacement: Option<&McpServer>,
    ) -> Result<Vec<FileSnapshot>, String> {
        let mut snapshots = Vec::new();
        for app in MCP_APPS {
            if !server.apps.get(app.id()).copied().unwrap_or(false) {
                continue;
            }
            let path = self.config_path(app);
            let original = read_optional(&path)?;
            if let Err(error) = self.write_app_entry(app, &server.id, replacement) {
                return Err(with_file_rollback(error, &snapshots));
            }
            snapshots.push((path, original));
        }
        Ok(snapshots)
    }

    fn config_path(&self, app: McpApp) -> PathBuf {
        match app {
            McpApp::Claude => self.home_dir.join(".claude.json"),
            McpApp::Codex => self.home_dir.join(".codex/config.toml"),
            McpApp::Gemini => self.home_dir.join(".gemini/settings.json"),
            McpApp::OpenCode => self.home_dir.join(".config/opencode/opencode.json"),
        }
    }

    fn is_installed(&self, app: McpApp) -> bool {
        let path = self.config_path(app);
        path.exists()
            || match app {
                McpApp::Claude => self.home_dir.join(".claude").exists(),
                _ => path.parent().is_some_and(Path::exists),
            }
    }

    fn write_app_entry(
        &self,
        app: McpApp,
        id: &str,
        server: Option<&McpServer>,
    ) -> Result<(), String> {
        let path = self.config_path(app);
        if server.is_some() && !self.is_installed(app) {
            return Err(format!("未检测到 {}", app.label()));
        }
        match app {
            McpApp::Codex => write_codex_entry(&path, id, server),
            McpApp::Claude | McpApp::Gemini | McpApp::OpenCode => {
                write_json_entry(&path, app, id, server)
            }
        }
    }

    fn read_app_entries(&self, app: McpApp) -> Result<BTreeMap<String, McpServer>, String> {
        let path = self.config_path(app);
        match app {
            McpApp::Codex => read_codex_entries(&path),
            McpApp::Claude | McpApp::Gemini | McpApp::OpenCode => read_json_entries(&path, app),
        }
    }
}

fn merge_secrets(server: &McpServer, existing: &McpSecrets) -> McpSecrets {
    let merge = |incoming: &BTreeMap<String, String>, old: &BTreeMap<String, String>| {
        incoming
            .iter()
            .map(|(key, value)| {
                let value = if value.is_empty() {
                    old.get(key).cloned().unwrap_or_default()
                } else {
                    value.clone()
                };
                (key.clone(), value)
            })
            .filter(|(_, value)| !value.is_empty())
            .collect()
    };
    McpSecrets {
        env: merge(&server.env, &existing.env),
        headers: merge(&server.headers, &existing.headers),
    }
}

fn with_file_rollback(error: String, snapshots: &[FileSnapshot]) -> String {
    with_rollback_errors(error, restore_snapshots(snapshots))
}

fn restore_snapshots(snapshots: &[FileSnapshot]) -> Vec<String> {
    snapshots
        .iter()
        .rev()
        .filter_map(|(path, original)| restore_optional(path, original.as_deref()).err())
        .collect()
}

fn with_rollback_errors(error: String, rollback_errors: Vec<String>) -> String {
    if rollback_errors.is_empty() {
        error
    } else {
        format!("{error}；文件回滚失败：{}", rollback_errors.join("；"))
    }
}

pub(crate) fn validate_server(server: &McpServer) -> Result<(), String> {
    let id = server.id.trim();
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err("MCP ID 只能包含字母、数字、点、下划线和连字符".to_string());
    }
    if server.name.trim().is_empty() {
        return Err("MCP 名称不能为空".to_string());
    }
    match server.transport.as_str() {
        "stdio"
            if server
                .command
                .as_deref()
                .is_some_and(|value| !value.trim().is_empty()) => {}
        "stdio" => return Err("stdio MCP 必须填写命令".to_string()),
        "http" => {
            let url = server.url.as_deref().unwrap_or_default();
            let parsed = reqwest::Url::parse(url).map_err(|_| "HTTP MCP URL 无效".to_string())?;
            if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
                return Err("HTTP MCP URL 必须使用 http 或 https".to_string());
            }
        }
        _ => return Err("MCP 类型必须是 stdio 或 http".to_string()),
    }
    for app in server.apps.keys() { McpApp::parse(app)?; }
    for key in server.env.keys().chain(server.headers.keys()) {
        if key.trim().is_empty() || key.contains(['\r', '\n']) {
            return Err("环境变量和请求头名称不能为空或包含换行".to_string());
        }
    }
    Ok(())
}

fn read_json_root(path: &Path) -> Result<Value, String> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value: Value = serde_json::from_str(&text)
                .map_err(|error| format!("解析 {} 失败：{error}", path.display()))?;
            if value.is_object() {
                Ok(value)
            } else {
                Err(format!("{} 根节点必须是 JSON 对象", path.display()))
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(error) => Err(format!("读取 {} 失败：{error}", path.display())),
    }
}

fn json_section(app: McpApp) -> &'static str {
    match app {
        McpApp::Claude | McpApp::Gemini => "mcpServers",
        McpApp::OpenCode => "mcp",
        McpApp::Codex => unreachable!(),
    }
}

fn write_json_entry(
    path: &Path,
    app: McpApp,
    id: &str,
    server: Option<&McpServer>,
) -> Result<(), String> {
    if server.is_none() && !path.exists() {
        return Ok(());
    }
    let mut root = read_json_root(path)?;
    let section = json_section(app);
    let object = root.as_object_mut().unwrap();
    if !object.get(section).is_some_and(Value::is_object) {
        if object.contains_key(section) {
            return Err(format!("{} 中的 {section} 必须是对象", path.display()));
        }
        object.insert(section.to_string(), Value::Object(Map::new()));
    }
    let entries = object[section].as_object_mut().unwrap();
    if let Some(server) = server {
        entries.insert(id.to_string(), server_to_json_spec(app, server));
    } else {
        entries.remove(id);
    }
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    atomic_write(path, &bytes)
}

fn server_to_json_spec(app: McpApp, server: &McpServer) -> Value {
    let (command, args) = normalized_command(server);
    if app == McpApp::OpenCode {
        if server.transport == "stdio" {
            let command = std::iter::once(command).chain(args).collect::<Vec<_>>();
            let mut spec = json!({ "type": "local", "command": command, "enabled": true });
            if !server.env.is_empty() {
                spec["environment"] = json!(server.env);
            }
            return spec;
        }
        let mut spec = json!({ "type": "remote", "url": server.url, "enabled": true });
        if !server.headers.is_empty() {
            spec["headers"] = json!(server.headers);
        }
        return spec;
    }
    if server.transport == "stdio" {
        let mut spec = json!({ "type": "stdio", "command": command, "args": args });
        if !server.env.is_empty() {
            spec["env"] = json!(server.env);
        }
        spec
    } else {
        let mut spec = json!({ "type": "http", "url": server.url });
        if !server.headers.is_empty() {
            spec["headers"] = json!(server.headers);
        }
        spec
    }
}

fn read_json_entries(path: &Path, app: McpApp) -> Result<BTreeMap<String, McpServer>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let root = read_json_root(path)?;
    let Some(entries) = root.get(json_section(app)).and_then(Value::as_object) else {
        return Ok(BTreeMap::new());
    };
    entries
        .iter()
        .map(|(id, value)| json_spec_to_server(app, id, value).map(|server| (id.clone(), server)))
        .collect()
}

fn json_spec_to_server(app: McpApp, id: &str, value: &Value) -> Result<McpServer, String> {
    let object = value
        .as_object()
        .ok_or_else(|| "MCP 配置必须是对象".to_string())?;
    let mut server = empty_server(id);
    if app == McpApp::OpenCode {
        match object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or("local")
        {
            "local" => {
                let command = object
                    .get("command")
                    .and_then(Value::as_array)
                    .ok_or_else(|| "OpenCode local MCP 缺少 command 数组".to_string())?;
                server.transport = "stdio".to_string();
                server.command = command.first().and_then(Value::as_str).map(str::to_string);
                server.args = command
                    .iter()
                    .skip(1)
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                server.env = string_map(object.get("environment"));
            }
            "remote" => {
                server.transport = "http".to_string();
                server.url = object
                    .get("url")
                    .and_then(Value::as_str)
                    .map(str::to_string);
                server.headers = string_map(object.get("headers"));
            }
            other => return Err(format!("不支持的 OpenCode MCP 类型：{other}")),
        }
    } else {
        let transport = object
            .get("type")
            .and_then(Value::as_str)
            .unwrap_or_else(|| {
                if object.contains_key("url") {
                    "http"
                } else {
                    "stdio"
                }
            });
        server.transport = if matches!(transport, "http" | "sse") {
            "http".to_string()
        } else {
            "stdio".to_string()
        };
        server.command = object
            .get("command")
            .and_then(Value::as_str)
            .map(str::to_string);
        server.args = object
            .get("args")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        server.url = object
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string);
        server.env = string_map(object.get("env"));
        server.headers = string_map(object.get("headers"));
    }
    validate_server(&server)?;
    Ok(server)
}

fn string_map(value: Option<&Value>) -> BTreeMap<String, String> {
    value
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
        .collect()
}

fn write_codex_entry(path: &Path, id: &str, server: Option<&McpServer>) -> Result<(), String> {
    if server.is_none() && !path.exists() {
        return Ok(());
    }
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(format!("读取 {} 失败：{error}", path.display())),
    };
    let mut document = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>()
            .map_err(|error| format!("解析 {} 失败：{error}", path.display()))?
    };
    if !document.as_table().contains_key("mcp_servers") {
        document["mcp_servers"] = Item::Table(Table::new());
    }
    let entries = document["mcp_servers"]
        .as_table_mut()
        .ok_or_else(|| "Codex mcp_servers 必须是表".to_string())?;
    if let Some(server) = server {
        let mut table = Table::new();
        if server.transport == "stdio" {
            let (command, normalized_args) = normalized_command(server);
            table["command"] = value(command);
            let mut args = Array::new();
            for arg in normalized_args {
                args.push(arg);
            }
            table["args"] = Item::Value(TomlValue::Array(args));
            if !server.env.is_empty() {
                let mut env = Table::new();
                for (key, value) in &server.env {
                    env[key] = toml_edit::value(value);
                }
                table["env"] = Item::Table(env);
            }
        } else {
            table["url"] = value(server.url.clone().unwrap_or_default());
            if !server.headers.is_empty() {
                let mut headers = Table::new();
                for (key, value) in &server.headers {
                    headers[key] = toml_edit::value(value);
                }
                table["http_headers"] = Item::Table(headers);
            }
        }
        entries[id] = Item::Table(table);
    } else {
        entries.remove(id);
    }
    atomic_write(path, document.to_string().as_bytes())
}

fn normalized_command(server: &McpServer) -> (String, Vec<String>) {
    let command = server.command.clone().unwrap_or_default();
    #[cfg(windows)]
    if matches!(
        command.to_ascii_lowercase().as_str(),
        "npx" | "npm" | "pnpm" | "yarn" | "bunx"
    ) {
        return (
            "cmd".to_string(),
            std::iter::once("/c".to_string())
                .chain(std::iter::once(command))
                .chain(server.args.iter().cloned())
                .collect(),
        );
    }
    (command, server.args.clone())
}

fn read_codex_entries(path: &Path) -> Result<BTreeMap<String, McpServer>, String> {
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = fs::read_to_string(path)
        .map_err(|error| format!("读取 {} 失败：{error}", path.display()))?;
    let document = text
        .parse::<DocumentMut>()
        .map_err(|error| format!("解析 {} 失败：{error}", path.display()))?;
    let Some(entries) = document.get("mcp_servers").and_then(Item::as_table) else {
        return Ok(BTreeMap::new());
    };
    entries
        .iter()
        .filter_map(|(id, item)| item.as_table().map(|table| (id, table)))
        .map(|(id, table)| {
            let mut server = empty_server(id);
            if let Some(command) = table.get("command").and_then(Item::as_str) {
                server.transport = "stdio".to_string();
                server.command = Some(command.to_string());
                server.args = table
                    .get("args")
                    .and_then(Item::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(TomlValue::as_str)
                    .map(str::to_string)
                    .collect();
                server.env = toml_string_table(table.get("env"));
            } else {
                server.transport = "http".to_string();
                server.url = table.get("url").and_then(Item::as_str).map(str::to_string);
                server.headers = toml_string_table(table.get("http_headers"));
            }
            validate_server(&server)?;
            Ok((id.to_string(), server))
        })
        .collect()
}

fn toml_string_table(item: Option<&Item>) -> BTreeMap<String, String> {
    item.and_then(Item::as_table)
        .into_iter()
        .flat_map(Table::iter)
        .filter_map(|(key, value)| {
            value
                .as_str()
                .map(|value| (key.to_string(), value.to_string()))
        })
        .collect()
}

fn empty_server(id: &str) -> McpServer {
    McpServer {
        id: id.to_string(),
        name: id.to_string(),
        description: String::new(),
        transport: String::new(),
        command: None,
        args: Vec::new(),
        url: None,
        env: BTreeMap::new(),
        headers: BTreeMap::new(),
        apps: BTreeMap::new(),
        built_in: false,
        secrets_configured: false,
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("读取 {} 失败：{error}", path.display())),
    }
}

fn restore_optional(path: &Path, bytes: Option<&[u8]>) -> Result<(), String> {
    match bytes {
        Some(bytes) => atomic_write(path, bytes),
        None if path.exists() => {
            fs::remove_file(path).map_err(|error| format!("删除 {} 失败：{error}", path.display()))
        }
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[derive(Default)]
    struct TestCredentialStore {
        values: Mutex<BTreeMap<String, String>>,
        fail_delete: AtomicBool,
    }

    impl TestCredentialStore {
        fn fail_deletes(&self, value: bool) {
            self.fail_delete.store(value, Ordering::SeqCst);
        }
    }

    impl McpCredentialStore for TestCredentialStore {
        fn get(&self, id: &str) -> Result<Option<String>, String> {
            Ok(self
                .values
                .lock()
                .map_err(|_| "test credential lock poisoned".to_string())?
                .get(id)
                .cloned())
        }

        fn set(&self, id: &str, value: &str) -> Result<(), String> {
            self.values
                .lock()
                .map_err(|_| "test credential lock poisoned".to_string())?
                .insert(id.to_string(), value.to_string());
            Ok(())
        }

        fn delete(&self, id: &str) -> Result<(), String> {
            if self.fail_delete.load(Ordering::SeqCst) {
                return Err("simulated credential delete failure".to_string());
            }
            self.values
                .lock()
                .map_err(|_| "test credential lock poisoned".to_string())?
                .remove(id);
            Ok(())
        }
    }

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("xy-switch-mcp-{}", uuid::Uuid::new_v4()))
    }

    fn test_server(id: &str) -> McpServer {
        McpServer {
            id: id.to_string(),
            name: "Test MCP".to_string(),
            description: "test".to_string(),
            transport: "stdio".to_string(),
            command: Some("cmd".to_string()),
            args: vec!["/c".to_string(), "echo".to_string()],
            url: None,
            env: BTreeMap::new(),
            headers: BTreeMap::new(),
            apps: BTreeMap::new(),
            built_in: false,
            secrets_configured: false,
        }
    }

    fn test_manager(root: &Path) -> (McpManager, Database, Arc<TestCredentialStore>) {
        let database = Database::open(&root.join("test.db")).unwrap();
        let credentials = Arc::new(TestCredentialStore::default());
        let manager =
            McpManager::with_credentials(database.clone(), root.to_path_buf(), credentials.clone());
        (manager, database, credentials)
    }

    #[test]
    fn validates_stdio_and_http_servers() {
        assert!(validate_server(&test_server("valid.id")).is_ok());
        let mut invalid = test_server("../bad");
        assert!(validate_server(&invalid).is_err());
        invalid.id = "remote".to_string();
        invalid.transport = "http".to_string();
        invalid.command = None;
        invalid.url = Some("https://example.com/mcp".to_string());
        assert!(validate_server(&invalid).is_ok());
    }

    #[test]
    fn sensitive_mcp_values_never_enter_sqlite_config_json() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let (manager, database, credentials) = test_manager(&root);
        let mut server = test_server("secret-storage-test");
        server
            .env
            .insert("API_TOKEN".to_string(), "env-super-secret".to_string());
        server
            .headers
            .insert("X-API-Key".to_string(), "header-super-secret".to_string());

        let saved = manager.save_server(server).unwrap();
        let record = database
            .get_mcp_record("secret-storage-test")
            .unwrap()
            .unwrap();

        assert!(!record.config_json.contains("env-super-secret"));
        assert!(!record.config_json.contains("header-super-secret"));
        assert!(record.config_json.contains("API_TOKEN"));
        assert!(record.config_json.contains("X-API-Key"));
        assert!(saved.secrets_configured);
        assert_eq!(saved.env.get("API_TOKEN").map(String::as_str), Some(""));
        assert_eq!(saved.headers.get("X-API-Key").map(String::as_str), Some(""));
        let secure_value = credentials.get("secret-storage-test").unwrap().unwrap();
        assert!(secure_value.contains("env-super-secret"));
        assert!(secure_value.contains("header-super-secret"));

        drop(manager);
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn syncs_each_app_without_overwriting_unrelated_config() {
        let root = temp_root();
        for directory in [".claude", ".codex", ".gemini", ".config/opencode"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        fs::write(root.join(".claude.json"), b"{\"theme\":\"dark\"}\n").unwrap();
        fs::write(
            root.join(".codex/config.toml"),
            b"approval_policy = \"on-request\"\n",
        )
        .unwrap();
        fs::write(
            root.join(".gemini/settings.json"),
            b"{\"theme\":\"light\"}\n",
        )
        .unwrap();
        fs::write(
            root.join(".config/opencode/opencode.json"),
            b"{\"model\":\"keep\"}\n",
        )
        .unwrap();
        let database = Database::open(&root.join("test.db")).unwrap();
        let manager = McpManager::new(database, root.clone());
        let server = test_server("test-mcp");
        manager.save_server(server).unwrap();

        for app in MCP_APPS {
            manager.set_app_enabled("test-mcp", app.id(), true).unwrap();
        }
        let claude: Value =
            serde_json::from_slice(&fs::read(root.join(".claude.json")).unwrap()).unwrap();
        assert_eq!(claude["theme"], "dark");
        assert_eq!(claude["mcpServers"]["test-mcp"]["command"], "cmd");
        let codex = fs::read_to_string(root.join(".codex/config.toml")).unwrap();
        assert!(codex.contains("approval_policy = \"on-request\""));
        assert!(codex.contains("[mcp_servers.test-mcp]"));
        let gemini: Value =
            serde_json::from_slice(&fs::read(root.join(".gemini/settings.json")).unwrap()).unwrap();
        assert_eq!(gemini["theme"], "light");
        assert_eq!(gemini["mcpServers"]["test-mcp"]["command"], "cmd");
        let opencode: Value =
            serde_json::from_slice(&fs::read(root.join(".config/opencode/opencode.json")).unwrap())
                .unwrap();
        assert_eq!(opencode["model"], "keep");
        assert_eq!(opencode["mcp"]["test-mcp"]["type"], "local");

        for app in MCP_APPS {
            manager
                .set_app_enabled("test-mcp", app.id(), false)
                .unwrap();
        }
        let saved = manager
            .list_servers()
            .unwrap()
            .into_iter()
            .find(|server| server.id == "test-mcp")
            .unwrap();
        assert!(!saved.apps.values().any(|enabled| *enabled));
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn imports_from_all_supported_apps() {
        let root = temp_root();
        fs::create_dir_all(root.join(".claude")).unwrap();
        fs::create_dir_all(root.join(".codex")).unwrap();
        fs::write(
            root.join(".claude.json"),
            b"{\"mcpServers\":{\"from-claude\":{\"command\":\"node\",\"args\":[\"server.js\"]}}}\n",
        )
        .unwrap();
        fs::write(
            root.join(".codex/config.toml"),
            b"[mcp_servers.from-codex]\nurl = \"https://example.com/mcp\"\n",
        )
        .unwrap();
        let database = Database::open(&root.join("test.db")).unwrap();
        let manager = McpManager::new(database, root.clone());

        let result = manager.import_from_apps().unwrap();
        assert_eq!(result.imported, 2);
        assert!(result.errors.is_empty());
        let repeated = manager.import_from_apps().unwrap();
        assert_eq!(repeated.imported, 0);
        assert!(repeated.errors.is_empty());
        let servers = manager.list_servers().unwrap();
        assert!(servers.iter().any(|server| server.id == "from-claude"));
        assert!(servers.iter().any(|server| server.id == "from-codex"));
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn active_server_edit_rolls_back_database_and_prior_app_files() {
        let root = temp_root();
        for directory in [".claude", ".codex"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        fs::write(root.join(".claude.json"), b"{\"keep\":true}\n").unwrap();
        fs::write(root.join(".codex/config.toml"), b"theme = \"dark\"\n").unwrap();
        let database = Database::open(&root.join("test.db")).unwrap();
        let manager = McpManager::new(database, root.clone());
        manager.save_server(test_server("rollback-test")).unwrap();
        manager
            .set_app_enabled("rollback-test", "claude", true)
            .unwrap();
        manager
            .set_app_enabled("rollback-test", "codex", true)
            .unwrap();
        let claude_before = fs::read(root.join(".claude.json")).unwrap();
        fs::write(root.join(".codex/config.toml"), b"[broken\n").unwrap();

        let mut edited = manager
            .list_servers()
            .unwrap()
            .into_iter()
            .find(|server| server.id == "rollback-test")
            .unwrap();
        edited.name = "Changed".to_string();
        edited.args.push("changed".to_string());
        assert!(manager.save_server(edited).is_err());

        assert_eq!(fs::read(root.join(".claude.json")).unwrap(), claude_before);
        let stored = manager
            .list_servers()
            .unwrap()
            .into_iter()
            .find(|server| server.id == "rollback-test")
            .unwrap();
        assert_eq!(stored.name, "Test MCP");
        assert!(!stored.args.contains(&"changed".to_string()));
        drop(manager);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn credential_delete_failure_restores_database_and_app_files() {
        let root = temp_root();
        fs::create_dir_all(root.join(".claude")).unwrap();
        fs::write(root.join(".claude.json"), b"{\"theme\":\"dark\"}\n").unwrap();
        let (manager, database, credentials) = test_manager(&root);
        let mut server = test_server("delete-rollback-test");
        server
            .env
            .insert("API_KEY".to_string(), "delete-secret".to_string());
        manager.save_server(server).unwrap();
        manager
            .set_app_enabled("delete-rollback-test", "claude", true)
            .unwrap();
        let file_before = fs::read(root.join(".claude.json")).unwrap();
        credentials.fail_deletes(true);

        let error = manager.delete_server("delete-rollback-test").unwrap_err();

        assert!(error.contains("simulated credential delete failure"));
        assert!(
            database
                .get_mcp_record("delete-rollback-test")
                .unwrap()
                .is_some()
        );
        assert_eq!(
            database
                .mcp_app_states("delete-rollback-test")
                .unwrap()
                .get("claude"),
            Some(&true)
        );
        assert_eq!(fs::read(root.join(".claude.json")).unwrap(), file_before);
        assert!(credentials.get("delete-rollback-test").unwrap().is_some());

        credentials.fail_deletes(false);
        manager.delete_server("delete-rollback-test").unwrap();
        drop(manager);
        drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn wraps_windows_script_commands_with_cmd() {
        let mut server = test_server("npx-test");
        server.command = Some("npx".to_string());
        server.args = vec!["-y".to_string(), "package".to_string()];
        assert_eq!(
            normalized_command(&server),
            (
                "cmd".to_string(),
                vec![
                    "/c".to_string(),
                    "npx".to_string(),
                    "-y".to_string(),
                    "package".to_string()
                ]
            )
        );
    }
    fn selection_fixture(root: &Path) {
        for directory in [".claude", ".codex", ".gemini", ".config/opencode"] {
            fs::create_dir_all(root.join(directory)).unwrap();
        }
        for path in [".claude.json", ".gemini/settings.json", ".config/opencode/opencode.json"] {
            fs::write(root.join(path), b"{\"acceptanceKeep\":\"fixture\"}\n").unwrap();
        }
        fs::write(root.join(".codex/config.toml"), b"acceptance_keep = \"fixture\"\n").unwrap();
    }

    #[test]
    fn saving_explicit_selection_syncs_all_four_clients_and_persists_it() {
        let root = temp_root();
        selection_fixture(&root);
        let (manager, database, credentials) = test_manager(&root);
        let mut server = test_server("selection-create");
        server.apps = MCP_APPS.into_iter().map(|app| (app.id().to_string(), true)).collect();
        let saved = manager.save_server(server).unwrap();
        assert_eq!(saved.apps.len(), 4);
        assert!(saved.apps.values().all(|enabled| *enabled));
        assert_eq!(database.mcp_app_states(&saved.id).unwrap(), saved.apps);
        for app in MCP_APPS {
            let text = fs::read_to_string(manager.config_path(app)).unwrap();
            assert!(text.contains(&saved.id));
            assert!(text.contains("fixture"));
        }
        assert!(credentials.values.lock().unwrap().is_empty());
        drop(manager); drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn editing_explicit_selection_removes_only_deselected_client_entries() {
        let root = temp_root();
        selection_fixture(&root);
        let (manager, database, _) = test_manager(&root);
        let mut server = test_server("selection-edit");
        server.apps = MCP_APPS.into_iter().map(|app| (app.id().to_string(), true)).collect();
        let mut saved = manager.save_server(server).unwrap();
        saved.apps.insert("gemini".to_string(), false);
        saved.apps.insert("opencode".to_string(), false);
        saved.name = "Edited selection".to_string();
        let edited = manager.save_server(saved).unwrap();
        assert_eq!(edited.name, "Edited selection");
        assert_eq!(database.mcp_app_states(&edited.id).unwrap(), edited.apps);
        for app in MCP_APPS {
            let text = fs::read_to_string(manager.config_path(app)).unwrap();
            assert_eq!(text.contains(&edited.id), edited.apps[app.id()]);
            assert!(text.contains("fixture"));
        }
        drop(manager); drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn omitted_selection_keys_preserve_existing_client_state_on_edit() {
        let root = temp_root();
        selection_fixture(&root);
        let (manager, database, _) = test_manager(&root);
        let mut server = test_server("selection-omitted");
        server.apps.insert("codex".to_string(), true);
        let mut saved = manager.save_server(server).unwrap();
        saved.apps.clear();
        saved.name = "Changed without selection".to_string();
        let edited = manager.save_server(saved).unwrap();
        assert!(edited.apps["codex"]);
        assert!(!edited.apps["claude"]);
        assert!(fs::read_to_string(manager.config_path(McpApp::Codex)).unwrap().contains(&edited.id));
        assert_eq!(database.mcp_app_states(&edited.id).unwrap().len(), 1);
        drop(manager); drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn new_selection_failure_restores_prior_files_and_leaves_no_record_or_states() {
        let root = temp_root();
        selection_fixture(&root);
        fs::write(root.join(".codex/config.toml"), b"[broken\n").unwrap();
        let (manager, database, credentials) = test_manager(&root);
        let before: Vec<_> = MCP_APPS.into_iter().map(|app| {
            let path = manager.config_path(app);
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        }).collect();
        let mut server = test_server("selection-rollback");
        server.apps = MCP_APPS.into_iter().map(|app| (app.id().to_string(), true)).collect();
        assert!(manager.save_server(server).is_err());
        for (path, bytes) in before { assert_eq!(fs::read(path).unwrap(), bytes); }
        assert!(database.get_mcp_record("selection-rollback").unwrap().is_none());
        assert!(database.mcp_app_states("selection-rollback").unwrap().is_empty());
        assert!(credentials.values.lock().unwrap().is_empty());
        drop(manager); drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn selected_uninstalled_or_unknown_clients_are_rejected_without_creating_config() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let (manager, database, credentials) = test_manager(&root);
        let mut server = test_server("selection-uninstalled");
        server.apps.insert("claude".to_string(), true);
        assert!(manager.save_server(server.clone()).unwrap_err().contains("未检测到"));
        assert!(!root.join(".claude").exists());
        assert!(!root.join(".claude.json").exists());
        assert!(database.get_mcp_record(&server.id).unwrap().is_none());
        server.apps.clear();
        server.apps.insert("unsupported-client".to_string(), true);
        assert!(manager.save_server(server).unwrap_err().contains("不支持的 MCP 应用"));
        assert!(credentials.values.lock().unwrap().is_empty());
        drop(manager); drop(database);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn backup_import_preserves_local_secrets_and_ignores_file_credentials() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let (manager, database, credentials) = test_manager(&root);
        let mut server = test_server("qa-preserved");
        server.env.insert("QA_TOKEN".into(), "existing-environment-secret".into());
        server.headers.insert("Authorization".into(), "Bearer existing-header-secret".into());
        manager.save_server(server).unwrap();
        let before = credentials.get("qa-preserved").unwrap().unwrap();
        let mut imported = test_server("qa-preserved");
        imported.name = "Imported name".into();
        imported.env.insert("QA_TOKEN".into(), "untrusted-file-secret".into());
        imported.headers.clear();
        let saved = manager.import_server(imported).unwrap();
        assert_eq!(saved.name, "Imported name");
        assert!(saved.secrets_configured);
        assert_eq!(credentials.get("qa-preserved").unwrap().unwrap(), before);
        assert!(saved.env.values().all(String::is_empty));
        assert!(saved.headers.values().all(String::is_empty));
        let mut new_server = test_server("qa-new-import");
        new_server.env.insert("QA_TOKEN".into(), "file-token-not-restored".into());
        new_server.headers.insert("Authorization".into(), "Bearer file-token".into());
        assert!(!manager.import_server(new_server).unwrap().secrets_configured);
        assert!(credentials.get("qa-new-import").unwrap().is_none());
        drop(manager); drop(database);
        let canonical = root.canonicalize().unwrap();
        assert_eq!(canonical.parent(), Some(std::env::temp_dir().canonicalize().unwrap().as_path()));
        assert!(canonical.file_name().unwrap().to_string_lossy().starts_with("xy-switch-mcp-"));
        fs::remove_dir_all(canonical).unwrap();
    }

}
