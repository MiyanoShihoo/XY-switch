use crate::{
    database::{Database, Provider},
    proxy_manager::ProxyManager,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use toml_edit::{DocumentMut, Item, Table, value};

const PROXY_TOKEN_PLACEHOLDER: &str = "XY_SWITCH_MANAGED";

pub(crate) fn resolve_codex_home(
    home: &Path,
    override_path: Option<std::ffi::OsString>,
) -> Result<PathBuf, String> {
    match override_path.filter(|path| !path.is_empty()) {
        Some(path) => {
            let path = PathBuf::from(path);
            if !path.is_absolute() {
                return Err("CODEX_HOME 必须是绝对路径，未导出到其他目录".into());
            }
            Ok(path)
        }
        None => Ok(home.join(".codex")),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ManagedApp {
    Claude,
    Codex,
    Gemini,
}

impl ManagedApp {
    const ALL: [Self; 3] = [Self::Claude, Self::Codex, Self::Gemini];

    fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            _ => Err(format!("不支持的应用：{value}")),
        }
    }

    fn id(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Claude => "Claude Code",
            Self::Codex => "Codex",
            Self::Gemini => "Gemini CLI",
        }
    }
}

#[derive(Debug, Clone)]
struct ConfigFile {
    id: &'static str,
    path: PathBuf,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct BackupMetadata {
    target_path: String,
    existed: bool,
    managed_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTakeoverStatus {
    pub app: String,
    pub label: String,
    pub config_paths: Vec<String>,
    pub installed: bool,
    pub active: bool,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct AppTakeoverManager {
    database: Database,
    proxy: ProxyManager,
    home_dir: PathBuf,
    codex_dir: PathBuf,
    backup_dir: PathBuf,
    operation_lock: Arc<Mutex<()>>,
    recovery_errors: Arc<Mutex<HashMap<String, String>>>,
}

impl AppTakeoverManager {
    pub fn new(
        database: Database,
        proxy: ProxyManager,
        home_dir: PathBuf,
        app_data_dir: PathBuf,
    ) -> Self {
        Self {
            database,
            proxy,
            codex_dir: home_dir.join(".codex"),
            home_dir,
            backup_dir: app_data_dir.join("takeover-backups"),
            operation_lock: Arc::new(Mutex::new(())),
            recovery_errors: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_codex_home(mut self, directory: PathBuf) -> Self {
        self.codex_dir = directory;
        self
    }

    fn files(&self, app: ManagedApp) -> Vec<ConfigFile> {
        match app {
            ManagedApp::Claude => {
                let directory = self.home_dir.join(".claude");
                let current = directory.join("settings.json");
                let legacy = directory.join("claude.json");
                vec![ConfigFile {
                    id: "claude-settings",
                    path: if current.exists() || !legacy.exists() {
                        current
                    } else {
                        legacy
                    },
                }]
            }
            ManagedApp::Codex => {
                let directory = &self.codex_dir;
                vec![
                    ConfigFile {
                        id: "codex-config",
                        path: directory.join("config.toml"),
                    },
                    ConfigFile {
                        id: "codex-auth",
                        path: directory.join("auth.json"),
                    },
                    ConfigFile {
                        id: "codex-model-catalog",
                        path: directory.join("xy-switch-model-catalog.json"),
                    },
                ]
            }
            ManagedApp::Gemini => vec![
                ConfigFile {
                    id: "gemini-env",
                    path: self.home_dir.join(".gemini").join(".env"),
                },
                ConfigFile {
                    id: "gemini-settings",
                    path: self.home_dir.join(".gemini").join("settings.json"),
                },
            ],
        }
    }

    fn metadata_path(&self, file: &ConfigFile) -> PathBuf {
        self.backup_dir.join(format!("{}.json", file.id))
    }

    fn content_backup_path(&self, file: &ConfigFile) -> PathBuf {
        self.backup_dir.join(format!("{}.bin", file.id))
    }

    fn is_active(&self, app: ManagedApp) -> bool {
        self.files(app)
            .iter()
            .any(|file| self.metadata_path(file).exists())
    }

    fn status_for(&self, app: ManagedApp) -> AppTakeoverStatus {
        let files = self.files(app);
        let installed = files
            .iter()
            .any(|file| file.path.exists() || file.path.parent().is_some_and(Path::exists));
        let last_error = self
            .recovery_errors
            .lock()
            .ok()
            .and_then(|errors| errors.get(app.id()).cloned());
        AppTakeoverStatus {
            app: app.id().to_string(),
            label: app.label().to_string(),
            config_paths: files
                .iter()
                .map(|file| file.path.display().to_string())
                .collect(),
            installed,
            active: files.iter().any(|file| self.metadata_path(file).exists()),
            last_error,
        }
    }

    pub fn statuses(&self) -> Vec<AppTakeoverStatus> {
        ManagedApp::ALL
            .into_iter()
            .map(|app| self.status_for(app))
            .collect()
    }

    pub fn has_active(&self) -> bool {
        ManagedApp::ALL.into_iter().any(|app| self.is_active(app))
    }

    pub fn recover_on_startup(&self) {
        for app in ManagedApp::ALL {
            if !self.is_active(app) {
                continue;
            }
            if let Err(error) = self.disable_locked(app) {
                self.set_error(app, Some(format!("启动恢复失败：{error}")));
            }
        }
    }

    #[cfg(test)]
    pub fn sync_codex_provider(
        &self,
        provider: &Provider,
        proxy_address: Option<&str>,
    ) -> Result<(), String> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| "应用接管操作锁损坏".to_string())?;

        let requires_conversion = crate::proxy::requires_protocol_conversion(provider)?;
        let use_proxy = proxy_address.is_some() || requires_conversion;

        let (target_url, bearer_token, auth_key) = if use_proxy {
            let addr = proxy_address
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "此供应商格式需要本地路由转换协议，请先开启本地路由".to_string())?;
            let base = addr.trim_end_matches('/');
            let target = if base.ends_with("/v1") {
                base.to_string()
            } else {
                format!("{base}/v1")
            };
            (
                target,
                PROXY_TOKEN_PLACEHOLDER.to_string(),
                PROXY_TOKEN_PLACEHOLDER.to_string(),
            )
        } else {
            let key = crate::credential_store::get_api_key(&provider.id)?
                .ok_or_else(|| "请先保存此供应商的 API Key，再启用 Codex 直连".to_string())?;
            (
                crate::proxy::codex_direct_base_url(provider)?,
                key.clone(),
                key,
            )
        };

        let codex_dir = &self.codex_dir;
        let files = self.files(ManagedApp::Codex);
        let originals = files
            .iter()
            .map(|file| read_optional(&file.path))
            .collect::<Result<Vec<_>, String>>()?;

        let config_index = files
            .iter()
            .position(|file| file.id == "codex-config")
            .ok_or_else(|| "缺少 Codex config.toml 接管文件".to_string())?;
        let auth_index = files
            .iter()
            .position(|file| file.id == "codex-auth")
            .ok_or_else(|| "缺少 Codex auth.json 接管文件".to_string())?;

        let current_text = originals[config_index]
            .as_deref()
            .map(|bytes| {
                std::str::from_utf8(bytes)
                    .map(str::to_owned)
                    .map_err(|e| format!("读取 Codex 配置失败：{e}"))
            })
            .transpose()?
            .unwrap_or_default();
        let updated_config = managed_codex_config_internal(
            &current_text,
            &target_url,
            &bearer_token,
            provider,
            codex_dir,
        )?;

        let current_auth = originals[auth_index]
            .as_deref()
            .map(|bytes| {
                std::str::from_utf8(bytes)
                    .map(str::to_owned)
                    .map_err(|e| format!("读取 Codex 凭据失败：{e}"))
            })
            .transpose()?
            .unwrap_or_default();
        let updated_auth = managed_codex_auth_internal(&current_auth, &auth_key)?;
        let updated_catalog = codex_model_catalog_bytes(provider)?;

        let managed = files
            .iter()
            .map(|file| match file.id {
                "codex-config" => Ok(updated_config.clone()),
                "codex-auth" => Ok(updated_auth.clone()),
                "codex-model-catalog" => Ok(updated_catalog.clone()),
                _ => Err("未知的 Codex 接管配置文件".to_string()),
            })
            .collect::<Result<Vec<_>, String>>()?;

        // Validate every currently managed file before overwriting any of them.
        for file in &files {
            let metadata_path = self.metadata_path(file);
            if metadata_path.exists() {
                let metadata: BackupMetadata =
                    serde_json::from_slice(&fs::read(&metadata_path).map_err(|e| e.to_string())?)
                        .map_err(|e| e.to_string())?;
                if let Some(expected) = metadata.managed_sha256 {
                    if read_optional(&file.path)?.as_deref().map(sha256).as_deref()
                        != Some(expected.as_str())
                    {
                        return Err("Codex 配置在接管期间被外部修改，未覆盖".to_string());
                    }
                }
            }
        }

        // First-time takeover must preserve the original bytes for all three files,
        // including the generated model catalog.
        let pending_backups = files
            .iter()
            .enumerate()
            .filter(|(_, file)| !self.metadata_path(file).exists())
            .collect::<Vec<_>>();
        if !pending_backups.is_empty() {
            fs::create_dir_all(&self.backup_dir)
                .map_err(|error| format!("创建接管备份目录失败：{error}"))?;
            for (index, file) in &pending_backups {
                let prepared = (|| {
                    if let Some(bytes) = originals[*index].as_ref() {
                        atomic_write(&self.content_backup_path(file), bytes)?;
                    }
                    self.write_metadata(
                        file,
                        &BackupMetadata {
                            target_path: file.path.display().to_string(),
                            existed: originals[*index].is_some(),
                            managed_sha256: None,
                        },
                    )
                })();
                if let Err(error) = prepared {
                    let rollback = self.restore_files(&files, false);
                    return Err(with_rollback_error(error, rollback));
                }
            }
        }

        for (file, bytes) in files.iter().zip(&managed) {
            if let Err(error) = atomic_write(&file.path, bytes) {
                let rollback = self.restore_files(&files, false);
                return Err(with_rollback_error(error, rollback));
            }
        }

        for (file, bytes) in files.iter().zip(&managed) {
            let metadata_path = self.metadata_path(file);
            let metadata_bytes = fs::read(&metadata_path)
                .map_err(|error| format!("读取 {} 备份信息失败：{error}", file.path.display()))?;
            let mut metadata: BackupMetadata = serde_json::from_slice(&metadata_bytes)
                .map_err(|error| format!("解析 {} 备份信息失败：{error}", file.path.display()))?;
            metadata.target_path = file.path.display().to_string();
            metadata.managed_sha256 = Some(sha256(bytes));
            if let Err(error) = self.write_metadata(file, &metadata) {
                let rollback = self.restore_files(&files, false);
                return Err(with_rollback_error(error, rollback));
            }
        }
        Ok(())
    }

    /// Selection never enables takeover or starts a route implicitly.
    pub async fn sync_active_provider(&self, provider: &Provider) -> Result<(), String> {
        if !self.has_active() {
            return Ok(());
        }
        let status = self.proxy.status().await?;
        if !status.running {
            return Err("应用接管仍在开启，但本地路由未运行；请先启动路由或关闭接管".into());
        }
        self.sync_active_with_provider(provider, &status.address)
    }

    pub fn sync_all_active_apps(&self, proxy_address: Option<&str>) -> Result<(), String> {
        if !self.has_active() {
            return Ok(());
        }
        let provider = self.database.current_provider()?.ok_or("尚未选择供应商")?;
        self.sync_active_with_provider(&provider, proxy_address.ok_or("本地路由未运行")?)
    }

    fn sync_active_with_provider(&self, provider: &Provider, address: &str) -> Result<(), String> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| "应用接管操作锁损坏")?;
        if !provider.enabled {
            return Err("当前供应商未启用".into());
        }
        // Prepare and validate all apps before writing anything. Keep the original takeover
        // backups intact; a failed update restores the immediately preceding managed state.
        let mut changes = Vec::new();
        for app in ManagedApp::ALL {
            if !self.is_active(app) {
                continue;
            }
            let files = self.files(app);
            let current = files
                .iter()
                .map(|f| read_optional(&f.path))
                .collect::<Result<Vec<_>, _>>()?;
            let managed = build_managed_files(app, &files, &current, address, provider)?;
            for ((file, before), bytes) in files.into_iter().zip(current).zip(managed) {
                let metadata_bytes = fs::read(self.metadata_path(&file))
                    .map_err(|e| format!("{} 接管备份不完整：{e}", app.label()))?;
                let mut metadata: BackupMetadata =
                    serde_json::from_slice(&metadata_bytes).map_err(|e| e.to_string())?;
                if metadata.target_path != file.path.display().to_string()
                    || metadata.managed_sha256.is_none()
                    || before.as_deref().map(sha256) != metadata.managed_sha256
                {
                    return Err(format!("{} 配置在接管期间被外部修改，未覆盖", app.label()));
                }
                metadata.managed_sha256 = Some(sha256(&bytes));
                let updated = serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?;
                changes.push((file, before, metadata_bytes, bytes, updated));
            }
        }
        for (file, _, _, bytes, metadata) in &changes {
            let result = atomic_write(&file.path, bytes)
                .and_then(|_| atomic_write(&self.metadata_path(file), metadata));
            if let Err(error) = result {
                let mut failures = Vec::new();
                for (file, before, old_metadata, _, _) in &changes {
                    let restored = match before {
                        Some(bytes) => atomic_write(&file.path, bytes),
                        None => fs::remove_file(&file.path)
                            .or_else(|e| {
                                if e.kind() == std::io::ErrorKind::NotFound {
                                    Ok(())
                                } else {
                                    Err(e)
                                }
                            })
                            .map_err(|e| e.to_string()),
                    }
                    .and_then(|_| atomic_write(&self.metadata_path(file), old_metadata));
                    if let Err(e) = restored {
                        failures.push(e);
                    }
                }
                return Err(if failures.is_empty() {
                    error
                } else {
                    format!("{error}；同步回滚失败：{}", failures.join("；"))
                });
            }
        }
        Ok(())
    }

    pub async fn set_enabled(
        &self,
        app_name: &str,
        enabled: bool,
    ) -> Result<AppTakeoverStatus, String> {
        let app = ManagedApp::parse(app_name)?;
        let proxy_status = self.proxy.status().await?;
        if enabled && !proxy_status.running {
            return Err("请先启动本地路由，再开启应用接管".to_string());
        }
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| "应用接管操作锁损坏".to_string())?;
        let result = if enabled {
            self.enable_locked(app, &proxy_status.address)
        } else {
            self.disable_locked(app)
        };
        match result {
            Ok(()) => {
                self.set_error(app, None);
                Ok(self.status_for(app))
            }
            Err(error) => {
                self.set_error(app, Some(error.clone()));
                Err(error)
            }
        }
    }

    pub fn disable_all(&self) -> Result<(), String> {
        let _guard = self
            .operation_lock
            .lock()
            .map_err(|_| "应用接管操作锁损坏".to_string())?;
        let mut errors = Vec::new();
        for app in ManagedApp::ALL {
            if self.is_active(app) {
                if let Err(error) = self.disable_locked(app) {
                    self.set_error(app, Some(error.clone()));
                    errors.push(format!("{}：{error}", app.label()));
                } else {
                    self.set_error(app, None);
                }
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "无法停止路由，部分应用配置恢复失败：{}",
                errors.join("；")
            ))
        }
    }

    fn set_error(&self, app: ManagedApp, error: Option<String>) {
        if let Ok(mut errors) = self.recovery_errors.lock() {
            if let Some(error) = error {
                errors.insert(app.id().to_string(), error);
            } else {
                errors.remove(app.id());
            }
        }
    }

    fn enable_locked(&self, app: ManagedApp, proxy_address: &str) -> Result<(), String> {
        if self.is_active(app) {
            if self.is_committed(app)? {
                return Ok(());
            }
            self.restore_files(&self.files(app), false)?;
        }
        let provider = self
            .database
            .current_provider()?
            .ok_or_else(|| format!("{} 没有已启用的供应商", app.label()))?;
        let files = self.files(app);
        let originals = files
            .iter()
            .map(|file| read_optional(&file.path))
            .collect::<Result<Vec<_>, _>>()?;
        let managed = build_managed_files(app, &files, &originals, proxy_address, &provider)?;

        fs::create_dir_all(&self.backup_dir)
            .map_err(|error| format!("创建接管备份目录失败：{error}"))?;
        for (file, original) in files.iter().zip(&originals) {
            let prepared = (|| {
                if let Some(bytes) = original {
                    atomic_write(&self.content_backup_path(file), bytes)?;
                }
                self.write_metadata(
                    file,
                    &BackupMetadata {
                        target_path: file.path.display().to_string(),
                        existed: original.is_some(),
                        managed_sha256: None,
                    },
                )
            })();
            if let Err(error) = prepared {
                let rollback = self.restore_files(&files, false);
                return Err(with_rollback_error(error, rollback));
            }
        }

        for (index, (file, bytes)) in files.iter().zip(&managed).enumerate() {
            if let Err(error) = atomic_write(&file.path, bytes) {
                let rollback = self.restore_files(&files, false);
                return Err(with_rollback_error(error, rollback));
            }
            if let Err(error) = self.write_metadata(
                file,
                &BackupMetadata {
                    target_path: file.path.display().to_string(),
                    existed: originals[index].is_some(),
                    managed_sha256: Some(sha256(bytes)),
                },
            ) {
                let rollback = self.restore_files(&files, false);
                return Err(with_rollback_error(error, rollback));
            }
        }
        Ok(())
    }

    fn is_committed(&self, app: ManagedApp) -> Result<bool, String> {
        let files = self.files(app);
        if files.is_empty() {
            return Ok(false);
        }
        for file in files {
            let path = self.metadata_path(&file);
            if !path.exists() {
                return Ok(false);
            }
            let metadata: BackupMetadata =
                serde_json::from_slice(&fs::read(&path).map_err(|error| {
                    format!("读取 {} 备份信息失败：{error}", file.path.display())
                })?)
                .map_err(|error| format!("解析 {} 备份信息失败：{error}", file.path.display()))?;
            if metadata.managed_sha256.is_none() {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn write_metadata(&self, file: &ConfigFile, metadata: &BackupMetadata) -> Result<(), String> {
        let mut bytes = serde_json::to_vec_pretty(metadata)
            .map_err(|error| format!("序列化接管备份信息失败：{error}"))?;
        bytes.push(b'\n');
        atomic_write(&self.metadata_path(file), &bytes)
    }

    fn disable_locked(&self, app: ManagedApp) -> Result<(), String> {
        self.restore_files(&self.files(app), true)
    }

    fn restore_files(&self, files: &[ConfigFile], verify_owner: bool) -> Result<(), String> {
        let mut errors = Vec::new();
        for file in files {
            if let Err(error) = self.restore_file(file, verify_owner) {
                errors.push(error);
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("；"))
        }
    }

    fn restore_file(&self, file: &ConfigFile, verify_owner: bool) -> Result<(), String> {
        let metadata_path = self.metadata_path(file);
        if !metadata_path.exists() {
            return Ok(());
        }
        let metadata: BackupMetadata = serde_json::from_slice(
            &fs::read(&metadata_path)
                .map_err(|error| format!("读取 {} 备份信息失败：{error}", file.path.display()))?,
        )
        .map_err(|error| format!("解析 {} 备份信息失败：{error}", file.path.display()))?;
        let target_path = PathBuf::from(&metadata.target_path);
        if verify_owner && let Some(expected) = metadata.managed_sha256.as_deref() {
            let current = read_optional(&target_path)?;
            if current.as_deref().map(sha256).as_deref() != Some(expected) {
                return Err(format!(
                    "{} 在接管期间被其他程序修改，已保留备份且未覆盖新内容",
                    target_path.display()
                ));
            }
        }
        let backup_path = self.content_backup_path(file);
        if metadata.existed {
            let original = fs::read(&backup_path)
                .map_err(|error| format!("读取 {} 原始备份失败：{error}", target_path.display()))?;
            atomic_write(&target_path, &original)?;
        } else if target_path.exists() {
            fs::remove_file(&target_path).map_err(|error| {
                format!("删除接管创建的 {} 失败：{error}", target_path.display())
            })?;
        }
        if backup_path.exists() {
            fs::remove_file(&backup_path)
                .map_err(|error| format!("清理接管内容备份失败：{error}"))?;
        }
        fs::remove_file(&metadata_path)
            .map_err(|error| format!("清理接管备份信息失败：{error}"))?;
        Ok(())
    }
}

fn with_rollback_error(error: String, rollback: Result<(), String>) -> String {
    match rollback {
        Ok(()) => error,
        Err(rollback_error) => format!("{error}；回滚失败：{rollback_error}"),
    }
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("读取 {} 失败：{error}", path.display())),
    }
}

fn build_managed_files(
    app: ManagedApp,
    files: &[ConfigFile],
    originals: &[Option<Vec<u8>>],
    proxy_address: &str,
    provider: &Provider,
) -> Result<Vec<Vec<u8>>, String> {
    files
        .iter()
        .zip(originals)
        .map(|(file, original)| {
            if app == ManagedApp::Codex && file.id == "codex-model-catalog" {
                return codex_model_catalog_bytes(provider);
            }

            let text = original
                .as_deref()
                .map(|bytes| {
                    std::str::from_utf8(bytes)
                        .map(str::to_owned)
                        .map_err(|_| format!("{} 不是有效的 UTF-8 文件", file.path.display()))
                })
                .transpose()?
                .unwrap_or_default();
            match (app, file.id) {
                (ManagedApp::Claude, _) => managed_claude_settings(&text, proxy_address),
                (ManagedApp::Codex, "codex-config") => managed_codex_config(
                    &text,
                    proxy_address,
                    provider,
                    file.path.parent().ok_or("Codex 配置缺少父目录")?,
                ),
                (ManagedApp::Codex, "codex-auth") => managed_codex_auth(&text, provider),
                (ManagedApp::Gemini, "gemini-env") => {
                    Ok(managed_gemini_env(&text, proxy_address).into_bytes())
                }
                (ManagedApp::Gemini, "gemini-settings") => managed_gemini_settings(&text),
                _ => Err("未知的应用接管配置文件".to_string()),
            }
        })
        .collect()
}

fn managed_claude_settings(text: &str, proxy_address: &str) -> Result<Vec<u8>, String> {
    let mut root = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(text).map_err(|error| format!("Claude 配置无效：{error}"))?
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| "Claude 配置根节点必须是对象".to_string())?;
    let env = object
        .entry("env")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| "Claude 配置中的 env 必须是对象".to_string())?;
    env.insert(
        "ANTHROPIC_BASE_URL".to_string(),
        Value::String(proxy_address.to_string()),
    );
    env.insert(
        "ANTHROPIC_AUTH_TOKEN".to_string(),
        Value::String(PROXY_TOKEN_PLACEHOLDER.to_string()),
    );
    env.insert(
        "ANTHROPIC_API_KEY".to_string(),
        Value::String(PROXY_TOKEN_PLACEHOLDER.to_string()),
    );
    json_bytes(&root, "Claude")
}

fn merge_toml_table(target: &mut Table, source: &Table) {
    for (key, source_item) in source {
        if let (Some(target_table), Some(source_table)) = (
            target.get_mut(key).and_then(Item::as_table_mut),
            source_item.as_table(),
        ) {
            merge_toml_table(target_table, source_table);
        } else {
            target.insert(key, source_item.clone());
        }
    }
}

fn managed_codex_config(
    text: &str,
    proxy_address: &str,
    provider: &Provider,
    codex_dir: &Path,
) -> Result<Vec<u8>, String> {
    let use_proxy = crate::proxy::requires_codex_proxy(provider)?;
    let (target, token) = if use_proxy {
        let base = proxy_address.trim_end_matches('/');
        let target = if base.ends_with("/v1") {
            base.to_string()
        } else {
            format!("{base}/v1")
        };
        (target, PROXY_TOKEN_PLACEHOLDER.to_string())
    } else {
        let key = crate::credential_store::get_api_key(&provider.id)?
            .ok_or_else(|| "请先保存此供应商的 API Key，再启用 Codex 直连".to_string())?;
        (crate::proxy::codex_direct_base_url(provider)?, key)
    };
    managed_codex_config_internal(text, &target, &token, provider, codex_dir)
}

pub(crate) fn managed_codex_config_internal(
    text: &str,
    target_base_url: &str,
    bearer_token: &str,
    provider: &Provider,
    codex_dir: &Path,
) -> Result<Vec<u8>, String> {
    let mut document = if text.trim().is_empty() {
        DocumentMut::new()
    } else {
        text.parse::<DocumentMut>()
            .map_err(|error| format!("Codex 配置无效：{error}"))?
    };
    let codex = &provider.advanced.codex;
    if !codex.config_toml.trim().is_empty() {
        let additions = codex
            .config_toml
            .parse::<DocumentMut>()
            .map_err(|error| format!("供应商 Codex 配置无效：{error}"))?;
        merge_toml_table(document.as_table_mut(), additions.as_table());
    }
    // Current Codex rejects the legacy root `profile` selector and falls back to
    // default config. Flatten its effective settings, preserving the old tables.
    if let Some(selector) = document.get("profile") {
        let name = selector.as_str().ok_or("Codex profile 必须是字符串")?;
        let settings = document
            .get("profiles")
            .and_then(Item::as_table)
            .and_then(|profiles| profiles.get(name))
            .and_then(Item::as_table)
            .cloned()
            .ok_or("Codex 当前 profile 不存在或不是表，未覆盖配置")?;
        merge_toml_table(document.as_table_mut(), &settings);
        document.remove("profile");
    }
    if codex.apply_common_config {
        document["model_reasoning_effort"] = value("high");
        document["disable_response_storage"] = value(true);
    }
    if codex.context_1m {
        document["model_context_window"] = value(1_000_000_i64);
        document["model_auto_compact_token_limit"] = value(codex.compaction_threshold as i64);
    }
    // Own one provider only: never repurpose another switcher's `custom` entry.
    let directory = if codex_dir.is_absolute() {
        codex_dir.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| "无法解析 Codex 配置目录")?
            .join(codex_dir)
    };
    let catalog_path = directory.join("xy-switch-model-catalog.json");
    let catalog_path = catalog_path
        .to_str()
        .ok_or("Codex 模型目录路径不是有效 UTF-8")?;
    document["model_provider"] = value("xy_switch");
    document["model"] = value(&provider.model);
    document["model_catalog_json"] = value(catalog_path);
    if !document.as_table().contains_key("model_providers") {
        document["model_providers"] = Item::Table(Table::new());
    }
    let providers = document["model_providers"]
        .as_table_mut()
        .ok_or_else(|| "Codex 配置中的 model_providers 必须是表".to_string())?;
    for key in ["xy_switch"] {
        if !providers.contains_key(key) {
            providers[key] = Item::Table(Table::new());
        }
        {
            let proxy = providers[key]
                .as_table_mut()
                .ok_or("Codex xy_switch 供应商配置必须是表，未覆盖配置")?;
            proxy["name"] = value(if codex.remote_compression {
                "OpenAI"
            } else {
                "XY Switch"
            });
            proxy["base_url"] = value(target_base_url);
            proxy["wire_api"] = value("responses");
            proxy.remove("env_key");
            proxy.remove("http_headers");
            proxy.remove("env_http_headers");
            proxy["requires_openai_auth"] = value(false);
            proxy["experimental_bearer_token"] = value(bearer_token);
        }
    }
    Ok(document.to_string().into_bytes())
}

pub fn codex_model_catalog_bytes(provider: &Provider) -> Result<Vec<u8>, String> {
    let mut model_specs: Vec<(String, String, u64)> = Vec::new();
    let default_cw = if provider.advanced.codex.context_1m {
        1_000_000
    } else {
        128_000
    };

    let main_model = provider.model.trim();
    if !main_model.is_empty() {
        let mapping = provider
            .advanced
            .model_mappings
            .iter()
            .find(|mapping| mapping.model.trim().eq_ignore_ascii_case(main_model));
        let display = mapping
            .map(|mapping| mapping.display_name.trim())
            .filter(|name| !name.is_empty())
            .unwrap_or(main_model);
        let context = mapping
            .and_then(|mapping| mapping.context_window)
            .unwrap_or(default_cw);
        model_specs.push((main_model.to_string(), display.to_string(), context));
    }

    for mapping in &provider.advanced.model_mappings {
        let m = mapping.model.trim();
        if !m.is_empty()
            && !model_specs
                .iter()
                .any(|(slug, _, _)| slug.eq_ignore_ascii_case(m))
        {
            let display = if mapping.display_name.trim().is_empty() {
                m.to_string()
            } else {
                mapping.display_name.trim().to_string()
            };
            let cw = mapping.context_window.unwrap_or(default_cw);
            model_specs.push((m.to_string(), display, cw));
        }
    }

    if model_specs.is_empty() {
        model_specs.push(("gpt-4o".to_string(), "gpt-4o".to_string(), default_cw));
    }

    let reasoning_levels = json!([
        { "effort": "none", "description": "Disable Thinking" },
        { "effort": "low", "description": "Fast responses with lighter reasoning" },
        { "effort": "medium", "description": "Balances speed and reasoning depth for everyday tasks" },
        { "effort": "high", "description": "Greater reasoning depth for complex problems" },
        { "effort": "xhigh", "description": "Extra high reasoning depth for complex problems" }
    ]);

    // 必须用 requires_protocol_conversion，不能用 requires_codex_proxy：后者只判断
    // upstream_format 能不能解析，对任何合法格式都返回 true，于是 chat_bridge 恒为
    // true，下面两个 `!chat_bridge` 就恒为 false——原生 Responses 上游（xAI Grok、
    // DeepSeek 官方 Codex）明明支持 detail:"original" 和 search 工具，却被一律声明
    // 成不支持。requires_protocol_conversion 才是「是否真的要做 Responses→Chat 转换」：
    // openai_responses 返回 false，其余返回 true。
    let chat_bridge = crate::proxy::requires_protocol_conversion(provider)?;
    let models: Vec<Value> = model_specs
        .into_iter()
        .enumerate()
        .map(|(i, (slug, display, cw))| {
            json!({
                "slug": slug,
                "display_name": display,
                "description": display,
                "base_instructions": "You are Codex, a coding agent You and the user share a workspace, and your job is to collaborate with them until their goal is genuinely handled.",
                "default_reasoning_level": "medium",
                "supported_reasoning_levels": reasoning_levels,
                "shell_type": "unified_exec",
                "apply_patch_tool_type": "freeform",
                "visibility": "list",
                "supported_in_api": true,
                "priority": 1000 + i,
                "context_window": cw,
                "max_context_window": cw,
                "effective_context_window_percent": 95,
                "supports_parallel_tool_calls": true,
                // detail:"original" 只有 Responses API 认。只要这张卡是经本地路由做
                // Responses→Chat 转换（chat_bridge=true），就不能声明支持，
                // 否则 Codex 会把工具截图标成 original，严格的 Chat 网关整条请求 400。
                "supports_image_detail_original": !chat_bridge,
                "supports_reasoning_summaries": true,
                "default_reasoning_summary": "none",
                "support_verbosity": true,
                "truncation_policy": {
                    "mode": "tokens",
                    "limit": 10000
                },
                "input_modalities": ["text", "image"],
                "supports_search_tool": !chat_bridge,
                "experimental_supported_tools": [],
                "additional_speed_tiers": [],
                "service_tiers": []
            })
        })
        .collect();

    let catalog = json!({
        "models": models
    });

    serde_json::to_vec_pretty(&catalog).map_err(|e| format!("序列化 Codex 模型目录失败：{e}"))
}

#[cfg(test)]
pub fn write_codex_model_catalog(codex_dir: &Path, provider: &Provider) -> Result<(), String> {
    if !codex_dir.exists() {
        fs::create_dir_all(codex_dir).map_err(|e| format!("创建 Codex 目录失败：{e}"))?;
    }
    let xy_path = codex_dir.join("xy-switch-model-catalog.json");
    let bytes = codex_model_catalog_bytes(provider)?;
    atomic_write(&xy_path, &bytes)
}

fn managed_codex_auth(text: &str, provider: &Provider) -> Result<Vec<u8>, String> {
    let use_proxy = crate::proxy::requires_codex_proxy(provider)?;
    let token = if use_proxy {
        PROXY_TOKEN_PLACEHOLDER.to_string()
    } else {
        crate::credential_store::get_api_key(&provider.id)?
            .ok_or_else(|| "请先保存此供应商的 API Key，再启用 Codex 直连".to_string())?
    };
    managed_codex_auth_internal(text, &token)
}

fn managed_codex_auth_internal(text: &str, token: &str) -> Result<Vec<u8>, String> {
    let mut root = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(text).map_err(|error| format!("Codex auth 无效：{error}"))?
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| "Codex auth 根节点必须是对象".to_string())?;
    object.insert(
        "OPENAI_API_KEY".to_string(),
        Value::String(token.to_string()),
    );
    json_bytes(&root, "Codex auth")
}

fn managed_gemini_env(text: &str, proxy_address: &str) -> String {
    let newline = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let trailing_newline = text.ends_with('\n');
    let replacements = [
        ("GOOGLE_GEMINI_BASE_URL", proxy_address),
        ("GEMINI_API_KEY", PROXY_TOKEN_PLACEHOLDER),
    ];
    let mut replaced = HashMap::new();
    let mut lines = Vec::new();
    for line in text.lines() {
        let key = line
            .trim()
            .split_once('=')
            .map(|(key, _)| key.trim())
            .filter(|key| replacements.iter().any(|(managed, _)| key == managed));
        if let Some(key) = key {
            if replaced.insert(key.to_string(), true).is_none() {
                let value = replacements
                    .iter()
                    .find_map(|(managed, value)| (*managed == key).then_some(*value))
                    .unwrap();
                lines.push(format!("{key}={value}"));
            }
        } else {
            lines.push(line.to_string());
        }
    }
    for (key, value) in replacements {
        if !replaced.contains_key(key) {
            lines.push(format!("{key}={value}"));
        }
    }
    let mut result = lines.join(newline);
    if trailing_newline || text.is_empty() {
        result.push_str(newline);
    }
    result
}

fn managed_gemini_settings(text: &str) -> Result<Vec<u8>, String> {
    let mut root = if text.trim().is_empty() {
        Value::Object(Map::new())
    } else {
        serde_json::from_str::<Value>(text).map_err(|error| format!("Gemini 配置无效：{error}"))?
    };
    let object = root
        .as_object_mut()
        .ok_or_else(|| "Gemini 配置根节点必须是对象".to_string())?;
    let security = object
        .entry("security")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "Gemini 配置中的 security 必须是对象".to_string())?;
    let auth = security
        .entry("auth")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or_else(|| "Gemini 配置中的 security.auth 必须是对象".to_string())?;
    auth.insert(
        "selectedType".to_string(),
        Value::String("gemini-api-key".to_string()),
    );
    json_bytes(&root, "Gemini")
}

fn json_bytes(value: &Value, label: &str) -> Result<Vec<u8>, String> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .map_err(|error| format!("序列化 {label} 配置失败：{error}"))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub(crate) fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or_else(|| format!("{} 没有父目录", path.display()))?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("创建 {} 失败：{error}", parent.display()))?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        uuid::Uuid::new_v4()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp)
            .map_err(|error| format!("创建临时文件 {} 失败：{error}", temp.display()))?;
        file.write_all(bytes)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("写入临时文件 {} 失败：{error}", temp.display()))?;
        replace_file(&temp, path)
    })();
    if result.is_err() && temp.exists() {
        let _ = fs::remove_file(&temp);
    }
    result
}

#[cfg(windows)]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
    };
    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let target = target
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            target.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(format!(
            "原子替换配置文件失败：{}",
            std::io::Error::last_os_error()
        ))
    } else {
        Ok(())
    }
}

#[cfg(not(windows))]
fn replace_file(source: &Path, target: &Path) -> Result<(), String> {
    fs::rename(source, target).map_err(|error| format!("原子替换配置文件失败：{error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("xy-switch-takeover-{}", uuid::Uuid::new_v4()))
    }

    fn catalog_provider(upstream_format: &str, endpoint: &str) -> Provider {
        let mut provider = Provider {
            id: "cat".to_string(),
            name: "Cat".to_string(),
            provider_type: "custom".to_string(),
            endpoint: endpoint.to_string(),
            model: "grok-4.5".to_string(),
            app_group: "Codex".to_string(),
            enabled: true,
            sort_order: 1,
            advanced: Default::default(),
        };
        provider.advanced.upstream_format = upstream_format.to_string();
        provider
    }

    /// 回归：这两个能力位以前是用 `requires_codex_proxy` 算的，而那个函数对任何
    /// 合法 upstream_format 都返回 true，于是 `!chat_bridge` 恒为 false——原生
    /// Responses 上游（xAI Grok、DeepSeek 官方 Codex）明明支持 detail:"original"
    /// 和 search 工具，却被一律写成不支持，Codex 侧直接把这两个能力关掉。
    /// 现在改用 `requires_protocol_conversion`，只有真的要做 Responses→Chat
    /// 转换时才声明不支持。
    #[test]
    fn model_catalog_capabilities_follow_real_protocol_conversion() {
        let root = temp_root();
        let native = root.join("native");
        write_codex_model_catalog(
            &native,
            &catalog_provider("openai_responses", "https://api.x.ai/v1"),
        )
        .unwrap();
        let catalog: Value =
            serde_json::from_slice(&fs::read(native.join("xy-switch-model-catalog.json")).unwrap())
                .unwrap();
        let entry = &catalog["models"][0];
        assert_eq!(entry["supports_image_detail_original"], json!(true));
        assert_eq!(entry["supports_search_tool"], json!(true));

        // Chat 上游确实要做 Responses→Chat 转换，这两个能力就必须声明为不支持。
        let bridged = root.join("bridged");
        write_codex_model_catalog(
            &bridged,
            &catalog_provider("openai_chat", "https://api.example.com/v1"),
        )
        .unwrap();
        let catalog: Value = serde_json::from_slice(
            &fs::read(bridged.join("xy-switch-model-catalog.json")).unwrap(),
        )
        .unwrap();
        let entry = &catalog["models"][0];
        assert_eq!(entry["supports_image_detail_original"], json!(false));
        assert_eq!(entry["supports_search_tool"], json!(false));

        // 目录里的推理档位不含 max：xAI 实测对 max 报 400，
        // 而 Codex 不会把目录外的档位夹回范围，所以目录必须是准的。
        let levels = entry["supported_reasoning_levels"].as_array().unwrap();
        assert!(
            !levels.iter().any(|level| level["effort"] == json!("max")),
            "模型目录不应声明 max 档"
        );
        assert!(levels.iter().any(|level| level["effort"] == json!("xhigh")));

        let _ = fs::remove_dir_all(&root);
    }

    async fn manager() -> (AppTakeoverManager, ProxyManager, PathBuf) {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(&root.join("test.db")).unwrap();
        let proxy = ProxyManager::new(database.clone());
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        proxy
            .apply(crate::proxy_manager::ProxySettings {
                enabled: true,
                port,
                ..Default::default()
            })
            .await
            .unwrap();
        (
            AppTakeoverManager::new(
                database,
                proxy.clone(),
                root.join("home"),
                root.join("data"),
            ),
            proxy,
            root,
        )
    }

    #[tokio::test]
    async fn claude_takeover_restores_exact_original() {
        let (manager, proxy, root) = manager().await;
        let path = root.join("home/.claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = br#"{"env":{"ANTHROPIC_API_KEY":"original-secret"},"theme":"dark"}"#;
        fs::write(&path, original).unwrap();

        let status = manager.set_enabled("claude", true).await.unwrap();
        assert!(status.active);
        let managed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(managed["theme"], "dark");
        assert_eq!(
            managed["env"]["ANTHROPIC_AUTH_TOKEN"],
            PROXY_TOKEN_PLACEHOLDER
        );
        assert!(
            managed["env"]["ANTHROPIC_BASE_URL"]
                .as_str()
                .unwrap()
                .starts_with("http://127.0.0.1:")
        );

        let status = manager.set_enabled("claude", false).await.unwrap();
        assert!(!status.active);
        assert_eq!(fs::read(&path).unwrap(), original);
        proxy.stop().await;
        drop(manager);
        drop(proxy);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn claude_legacy_path_is_restored_after_current_path_appears() {
        let (manager, proxy, root) = manager().await;
        let directory = root.join("home/.claude");
        let legacy_path = directory.join("claude.json");
        let current_path = directory.join("settings.json");
        fs::create_dir_all(&directory).unwrap();
        let legacy_original = b"{\"legacy\":true}\n";
        let current_external = b"{\"createdLater\":true}\n";
        fs::write(&legacy_path, legacy_original).unwrap();

        manager.set_enabled("claude", true).await.unwrap();
        assert_ne!(fs::read(&legacy_path).unwrap(), legacy_original);

        // Claude may migrate to settings.json while XY Switch still owns claude.json.
        fs::write(&current_path, current_external).unwrap();
        manager.set_enabled("claude", false).await.unwrap();

        assert_eq!(fs::read(&legacy_path).unwrap(), legacy_original);
        assert_eq!(fs::read(&current_path).unwrap(), current_external);
        assert!(!manager.status_for(ManagedApp::Claude).active);

        proxy.stop().await;
        drop(manager);
        drop(proxy);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn codex_and_gemini_transforms_preserve_unrelated_settings() {
        let mut provider = Provider {
            id: "openai".to_string(),
            name: "OpenAI".to_string(),
            provider_type: "openai".to_string(),
            endpoint: "https://api.openai.com/v1".to_string(),
            model: "configured-model".to_string(),
            app_group: "Codex".to_string(),
            enabled: true,
            sort_order: 1,
            advanced: Default::default(),
        };
        provider.advanced.codex.remote_compression = true;
        provider.advanced.codex.apply_common_config = true;
        provider.advanced.codex.context_1m = true;
        provider.advanced.codex.compaction_threshold = 850_000;
        provider.advanced.codex.config_toml = "sandbox_mode = \"workspace-write\"\n".to_string();
        let codex = String::from_utf8(
            managed_codex_config(
                "approval_policy = \"on-request\"\n",
                "http://127.0.0.1:15721",
                &provider,
                &temp_root(),
            )
            .unwrap(),
        )
        .unwrap();
        assert!(codex.contains("approval_policy = \"on-request\""));
        assert!(codex.contains("model_provider = \"xy_switch\""));
        assert!(codex.contains("model = \"configured-model\""));
        assert!(codex.contains("model_reasoning_effort = \"high\""));
        assert!(codex.contains("model_context_window = 1000000"));
        assert!(codex.contains("model_auto_compact_token_limit = 850000"));
        assert!(codex.contains("sandbox_mode = \"workspace-write\""));
        assert!(codex.contains("name = \"OpenAI\""));
        assert!(codex.contains("base_url = \"http://127.0.0.1:15721/v1\""));

        let auth: Value = serde_json::from_slice(
            &managed_codex_auth(r#"{"tokens":{"keep":true}}"#, &provider).unwrap(),
        )
        .unwrap();
        assert_eq!(auth["tokens"]["keep"], true);
        assert_eq!(auth["OPENAI_API_KEY"], PROXY_TOKEN_PLACEHOLDER);

        let env = managed_gemini_env(
            "# keep\r\nOTHER=value\r\nGEMINI_API_KEY=old\r\nGEMINI_API_KEY=duplicate\r\n",
            "http://127.0.0.1:15721",
        );
        assert!(env.contains("# keep\r\nOTHER=value"));
        assert_eq!(env.matches("GEMINI_API_KEY=").count(), 1);
        assert!(env.contains("GOOGLE_GEMINI_BASE_URL=http://127.0.0.1:15721"));

        let settings: Value = serde_json::from_slice(
            &managed_gemini_settings(r#"{"other":true,"security":{"keep":1}}"#).unwrap(),
        )
        .unwrap();
        assert_eq!(settings["other"], true);
        assert_eq!(settings["security"]["keep"], 1);
        assert_eq!(
            settings["security"]["auth"]["selectedType"],
            "gemini-api-key"
        );
    }

    #[tokio::test]
    async fn codex_takeover_restores_config_and_auth_exactly() {
        let (manager, proxy, root) = manager().await;
        let directory = root.join("home/.codex");
        let config_path = directory.join("config.toml");
        let auth_path = directory.join("auth.json");
        let catalog_path = directory.join("xy-switch-model-catalog.json");
        fs::create_dir_all(&directory).unwrap();
        let original_config = b"approval_policy = \"on-request\"\n";
        let original_auth = br#"{"tokens":{"access_token":"original"}}"#;
        fs::write(&config_path, original_config).unwrap();
        fs::write(&auth_path, original_auth).unwrap();

        manager.set_enabled("codex", true).await.unwrap();
        let managed_config = fs::read_to_string(&config_path).unwrap();
        let managed_auth: Value = serde_json::from_slice(&fs::read(&auth_path).unwrap()).unwrap();
        assert!(managed_config.contains("model_provider = \"xy_switch\""));
        assert_eq!(managed_auth["OPENAI_API_KEY"], PROXY_TOKEN_PLACEHOLDER);
        assert_eq!(managed_auth["tokens"]["access_token"], "original");
        assert!(catalog_path.is_file());

        manager.set_enabled("codex", false).await.unwrap();
        assert_eq!(fs::read(&config_path).unwrap(), original_config);
        assert_eq!(fs::read(&auth_path).unwrap(), original_auth);
        assert!(!catalog_path.exists());

        proxy.stop().await;
        drop(manager);
        drop(proxy);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn startup_recovery_restores_active_takeover() {
        let (manager, proxy, root) = manager().await;
        let path = root.join("home/.claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let original = b"{\"keep\":true}\n";
        fs::write(&path, original).unwrap();
        manager.set_enabled("claude", true).await.unwrap();

        let recovered = AppTakeoverManager::new(
            manager.database.clone(),
            proxy.clone(),
            root.join("home"),
            root.join("data"),
        );
        recovered.recover_on_startup();
        assert_eq!(fs::read(&path).unwrap(), original);
        assert!(!recovered.status_for(ManagedApp::Claude).active);
        proxy.stop().await;
        drop(manager);
        drop(recovered);
        drop(proxy);
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn restore_does_not_overwrite_external_change() {
        let (manager, proxy, root) = manager().await;
        let path = root.join("home/.claude/settings.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, b"{}\n").unwrap();
        manager.set_enabled("claude", true).await.unwrap();
        fs::write(&path, b"{\"changedByUser\":true}\n").unwrap();

        let error = manager.set_enabled("claude", false).await.unwrap_err();
        assert!(error.contains("被其他程序修改"));
        assert_eq!(fs::read(&path).unwrap(), b"{\"changedByUser\":true}\n");
        assert!(manager.status_for(ManagedApp::Claude).active);
        proxy.stop().await;
        drop(manager);
        drop(proxy);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn writes_codex_model_catalog_with_all_models() {
        let dir = temp_root();
        fs::create_dir_all(&dir).unwrap();
        let mut provider = Provider {
            id: "example-provider".to_string(),
            name: "Example Provider".to_string(),
            provider_type: "custom".to_string(),
            endpoint: "https://gateway.example.com/v1".to_string(),
            model: "gpt-5.6-sol".to_string(),
            app_group: "Codex".to_string(),
            enabled: true,
            sort_order: 1,
            advanced: Default::default(),
        };
        provider
            .advanced
            .model_mappings
            .push(crate::database::ProviderModelMapping {
                model: "deepseek-v3".to_string(),
                display_name: "DeepSeek V3".to_string(),
                context_window: Some(64_000),
            });

        write_codex_model_catalog(&dir, &provider).unwrap();

        let xy_file = dir.join("xy-switch-model-catalog.json");
        let cc_file = dir.join("cc-switch-model-catalog.json");
        assert!(xy_file.exists());
        assert!(!cc_file.exists());

        let catalog: Value = serde_json::from_slice(&fs::read(&xy_file).unwrap()).unwrap();
        let models = catalog["models"].as_array().unwrap();
        assert_eq!(models.len(), 2);
        assert_eq!(models[0]["slug"], "gpt-5.6-sol");
        assert_eq!(models[0]["display_name"], "gpt-5.6-sol");
        assert_eq!(models[1]["slug"], "deepseek-v3");
        assert_eq!(models[1]["display_name"], "DeepSeek V3");
        assert_eq!(models[1]["context_window"], 64_000);

        fs::remove_dir_all(dir).unwrap();
    }
    include!("takeover_regression_tests.rs");
}
