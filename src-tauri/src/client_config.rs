use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{collections::BTreeSet, ffi::OsString, fs, path::{Component, Path, PathBuf}, sync::{Arc, Mutex}};

#[derive(Clone, Default)]
pub struct ClientConfigLocks(Arc<Mutex<ClientConfigState>>);

#[derive(Default)]
struct ClientConfigState {
    owners: BTreeSet<String>,
    restart_pending: bool,
}

pub(crate) struct ClientConfigLease {
    locks: ClientConfigLocks,
    owner: String,
}

impl ClientConfigLocks {
    pub(crate) fn acquire(&self, owner: &str) -> Result<ClientConfigLease, String> {
        let mut state = self.0.lock().map_err(|_| "客户端配置锁不可用")?;
        if state.restart_pending { return Err("客户端路径已修改，请重启 XY Switch 后继续操作".into()); }
        if state.owners.contains("*") || state.owners.contains(owner) || (owner == "*" && !state.owners.is_empty()) {
            return Err("客户端登录或配置操作正在进行，请等待完成或取消后再修改配置".into());
        }
        state.owners.insert(owner.into());
        Ok(ClientConfigLease { locks: self.clone(), owner: owner.into() })
    }

    pub(crate) fn require_restart(&self) -> Result<(), String> {
        self.0.lock().map_err(|_| "客户端配置锁不可用")?.restart_pending = true;
        Ok(())
    }

    pub(crate) fn restart_pending(&self) -> bool {
        self.0.lock().map(|state| state.restart_pending).unwrap_or(true)
    }
}

impl Drop for ClientConfigLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.locks.0.lock() { state.owners.remove(&self.owner); }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OpenCodeSchema { V1, V2 }

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClientSettings {
    pub opencode_config: Option<PathBuf>,
    pub opencode_schema: Option<OpenCodeSchema>,
    pub claude_desktop_profile: Option<PathBuf>,
    pub codex_binary: Option<PathBuf>,
    pub opencode_binary: Option<PathBuf>,
}

#[derive(Clone)]
pub struct OpenCodeTarget {
    pub path: PathBuf,
    pub schema: Option<OpenCodeSchema>,
}

#[derive(Clone)]
pub struct ClientTargets {
    pub opencode: Result<OpenCodeTarget, String>,
    pub claude_desktop_mcp: PathBuf,
    pub claude_desktop_library: PathBuf,
    pub claude_desktop_profile: Option<PathBuf>,
}

pub(crate) fn config_directory(home: &Path, app: &str, local: bool) -> PathBuf {
    if cfg!(target_os = "windows") {
        home.join("AppData").join(if local { "Local" } else { "Roaming" }).join(app)
    } else if cfg!(target_os = "macos") {
        home.join("Library/Application Support").join(app)
    } else { home.join(".config").join(app) }
}

pub(crate) fn validate_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || path.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir)) {
        return Err("客户端配置路径必须是没有相对跳转的绝对路径".into());
    }
    let extension = path.extension().and_then(|value| value.to_str());
    if !matches!(extension, Some("json" | "jsonc")) { return Err("客户端配置必须使用 JSON 或 JSONC 文件".into()); }
    regular_file(path)?;
    Ok(())
}

pub(crate) fn regular_file(path: &Path) -> Result<(), String> {
    regular_file_with_limit(path, 4 * 1024 * 1024)
}

fn regular_file_with_limit(path: &Path, maximum: u64) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            #[cfg(windows)]
            let redirected = {
                use std::os::windows::fs::MetadataExt;
                metadata.file_attributes() & 0x400 != 0
            };
            #[cfg(not(windows))]
            let redirected = false;
            if !metadata.is_file() || metadata.file_type().is_symlink() || redirected {
                return Err("客户端配置不能是目录、链接或重定向文件".into());
            }
            if metadata.len() > maximum { return Err("客户端文件超过大小限制".into()); }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err("无法检查客户端配置文件".into()),
    }
}

pub(crate) fn validate_codex_binary(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || !path.is_file()
        || path.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || !matches!(path.file_name().and_then(|value| value.to_str()), Some("codex" | "codex.exe"))
    {
        return Err("请选择已安装的 Codex 原生可执行文件".into());
    }
    regular_file_with_limit(path, 512 * 1024 * 1024)
}

pub(crate) fn validate_opencode_binary(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || !path.is_file()
        || path.components().any(|part| matches!(part, Component::ParentDir | Component::CurDir))
        || !matches!(path.file_name().and_then(|value| value.to_str()), Some("opencode" | "opencode.exe"))
    {
        return Err("请选择已安装的 OpenCode 原生可执行文件".into());
    }
    regular_file_with_limit(path, 512 * 1024 * 1024)
}

impl ClientTargets {
    pub fn for_home(home: &Path) -> Self { Self::resolve(home, &ClientSettings::default(), None, None) }

    pub fn resolve(home: &Path, settings: &ClientSettings, opencode_override: Option<OsString>, xdg: Option<OsString>) -> Self {
        let opencode = (|| {
            let explicit = settings.opencode_config.clone().or_else(|| opencode_override.filter(|value| !value.is_empty()).map(PathBuf::from));
            let path = if let Some(path) = explicit { path } else {
                let directory = xdg.filter(|value| !value.is_empty()).map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
                if !directory.is_absolute() { return Err("XDG_CONFIG_HOME 必须是绝对路径".into()); }
                let directory = directory.join("opencode");
                let json = directory.join("opencode.json");
                let jsonc = directory.join("opencode.jsonc");
                if json.exists() && jsonc.exists() { return Err("发现两份 OpenCode 全局配置，请明确选择要管理的文件".into()); }
                if jsonc.exists() { jsonc } else { json }
            };
            validate_path(&path)?;
            Ok(OpenCodeTarget { path, schema: settings.opencode_schema })
        })();
        let library = config_directory(home, "Claude-3p", true).join("configLibrary");
        Self {
            opencode,
            claude_desktop_mcp: config_directory(home, "Claude", false).join("claude_desktop_config.json"),
            claude_desktop_library: library,
            claude_desktop_profile: settings.claude_desktop_profile.clone(),
        }
    }

    pub fn validate_settings(home: &Path, settings: &ClientSettings) -> Result<(), String> {
        if let Some(path) = &settings.opencode_config { validate_path(path)?; }
        if let Some(path) = &settings.claude_desktop_profile {
            validate_path(path)?;
            let library = config_directory(home, "Claude-3p", true).join("configLibrary");
            if path.file_name().and_then(|value| value.to_str()) == Some("_meta.json") || path.extension().and_then(|value| value.to_str()) != Some("json") {
                return Err("请选择 Claude Desktop 保存的具体配置，不可修改 _meta.json".into());
            }
            let parent = path.parent().ok_or("Claude Desktop 配置路径无效")?;
            if !path.is_file() || fs::canonicalize(parent).map_err(|_| "无法检查配置目录")? != fs::canonicalize(&library).map_err(|_| "未找到 Claude Desktop 配置库")? {
                return Err("请选择 Claude Desktop 配置库内已保存的配置文件".into());
            }
        }
        if let Some(path) = &settings.codex_binary {
            validate_codex_binary(path)?;
        }
        if let Some(path) = &settings.opencode_binary {
            validate_opencode_binary(path)?;
        }
        Ok(())
    }
}

pub fn detect_opencode_schema(root: &Value, selected: Option<OpenCodeSchema>) -> Result<OpenCodeSchema, String> {
    let object = root.as_object().ok_or("OpenCode 配置根节点必须是对象")?;
    let legacy = object.contains_key("provider");
    let current = object.contains_key("providers") || root.get("mcp").and_then(Value::as_object).is_some_and(|mcp| mcp.contains_key("servers"));
    if legacy && current { return Err("OpenCode 配置混用了 V1 与 V2 格式".into()); }
    let detected = if current { Some(OpenCodeSchema::V2) } else if legacy { Some(OpenCodeSchema::V1) } else { None };
    if selected.is_some() && detected.is_some() && selected != detected { return Err("OpenCode 配置内容与所选版本不一致".into()); }
    selected.or(detected).ok_or_else(|| "请先明确选择 OpenCode 配置版本 V1 或 V2".into())
}

pub(crate) fn managed_opencode(text: &str, selected: Option<OpenCodeSchema>, address: &str, model: &str, token: &str) -> Result<Vec<u8>, String> {
    let mut root = if text.trim().is_empty() { Value::Object(Map::new()) } else { crate::jsonc::parse(text)? };
    let schema = detect_opencode_schema(&root, selected)?;
    let provider_key = if schema == OpenCodeSchema::V2 { "providers" } else { "provider" };
    let providers = root.as_object_mut().unwrap().entry(provider_key).or_insert_with(|| json!({})).as_object_mut().ok_or("OpenCode provider 配置必须是对象")?;
    let base_url = format!("{}/v1", address.trim_end_matches('/').trim_end_matches("/v1"));
    let provider = if schema == OpenCodeSchema::V2 {
        json!({"name":"XY Switch","package":"@opencode/ai/providers/openai-compatible","settings":{"baseURL":base_url,"apiKey":token},"models":{model:{"name":model}}})
    } else {
        json!({"name":"XY Switch","npm":"@ai-sdk/openai-compatible","options":{"baseURL":base_url,"apiKey":token},"models":{model:{"name":model}}})
    };
    providers.insert("xy-switch".into(), provider);
    root.as_object_mut().unwrap().insert("model".into(), Value::String(format!("xy-switch/{model}")));
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| "无法保存 OpenCode 配置")?;
    bytes.push(b'\n');
    Ok(bytes)
}

pub(crate) fn managed_claude_desktop(text: &str, address: &str, token: &str) -> Result<Vec<u8>, String> {
    let mut root = crate::jsonc::parse(text)?;
    let object = root.as_object_mut().ok_or("Claude Desktop 配置根节点必须是对象")?;
    if object.contains_key("bootstrapUrl") || object.contains_key("bootstrapOidc") { return Err("此 Claude Desktop 配置由组织下发，请在原生配置界面确认管理来源".into()); }
    object.insert("inferenceProvider".into(), json!("gateway"));
    object.insert("inferenceGatewayBaseUrl".into(), json!(format!("{}/v1", address.trim_end_matches('/').trim_end_matches("/v1"))));
    object.insert("inferenceGatewayApiKey".into(), json!(token));
    let mut bytes = serde_json::to_vec_pretty(&root).map_err(|_| "无法保存 Claude Desktop 配置")?;
    bytes.push(b'\n');
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persistent_client_operation_excludes_same_owner_and_global_updates() {
        let locks = ClientConfigLocks::default();
        let codex = locks.acquire("codex").unwrap();
        assert!(locks.acquire("codex").is_err());
        assert!(locks.acquire("*").is_err());
        assert!(locks.acquire("opencode").is_ok());
        drop(codex);
        let global = locks.acquire("*").unwrap();
        assert!(locks.acquire("claude-desktop").is_err());
        drop(global);
        assert!(locks.acquire("codex").is_ok());
    }
    #[test]
    fn saved_path_changes_block_all_stale_config_managers_until_restart() {
        let locks = ClientConfigLocks::default();
        let lease = locks.acquire("*").unwrap();
        locks.require_restart().unwrap();
        drop(lease);
        assert!(locks.restart_pending());
        for owner in ["*", "codex", "opencode", "claude-desktop"] { assert!(locks.acquire(owner).is_err()); }
        assert!(ClientConfigLocks::default().acquire("codex").is_ok());
    }
    #[test]
    fn writes_both_opencode_versions_without_erasing_other_providers() {
        for (text, schema, key, options) in [(r#"{"provider":{"original":{}},"theme":"dark"}"#, OpenCodeSchema::V1,"provider","options"), (r#"{"providers":{"original":{}},"mcp":{"servers":{}}}"#,OpenCodeSchema::V2,"providers","settings")] {
            let bytes = managed_opencode(text, Some(schema),"http://127.0.0.1:8877", "safe-model", "synthetic-token").unwrap();
            let root: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(root[key]["xy-switch"][options]["baseURL"], "http://127.0.0.1:8877/v1");
            assert!(root[key].get("original").is_some());
            assert_eq!(root["model"], "xy-switch/safe-model");
        }
        assert!(managed_opencode("{}",None,"http://localhost","model","token").is_err());
        assert!(detect_opencode_schema(&json!({"provider":{},"providers":{}}),None).is_err());
    }
    #[test]
    fn desktop_gateway_preserves_auth_preferences_and_uses_official_field() {
        let bytes = managed_claude_desktop(r#"{"displayName":"Work","inferenceCustomHeaders":{"X-Tenant":"tenant"}}"#, "http://127.0.0.1:8877", "local-only").unwrap();
        let root: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(root["inferenceGatewayBaseUrl"], "http://127.0.0.1:8877/v1");
        assert_eq!(root["inferenceCustomHeaders"]["X-Tenant"], "tenant");
        assert!(root.get("inferenceGatewayUrl").is_none());
        assert!(managed_claude_desktop(r#"{"bootstrapUrl":"https://managed.example"}"#, "http://localhost", "token").is_err());
    }
    #[test]
    fn refuses_relative_environment_config_overrides() {
        let home = std::env::temp_dir().join("xy-client-config-fixture");
        assert!(ClientTargets::resolve(&home,&ClientSettings::default(),Some("relative.json".into()),None).opencode.is_err());
    }
}
