mod codex;
mod opencode;
mod pool;

pub use pool::{BindNativeAccountRequest, NativePoolCandidate};

use crate::{client_config::{ClientSettings, ClientTargets}, database::Database, takeover::AppTakeoverManager};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf, sync::{Arc, Mutex}};

const SETTINGS_KEY: &str = "client_connections_v1";

#[derive(Clone, Copy)]
enum NativeRead {
    Observe,
    Refresh,
    CompletedLogin,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClientConnection {
    pub id: String,
    pub label: String,
    pub config_owner: String,
    pub config_paths: Vec<String>,
    pub config_detected: bool,
    pub takeover_target: Option<String>,
    pub mcp_target: String,
    pub login_method: String,
    pub login_instructions: String,
    pub inference_mode: String,
    pub native_verification: String,
    pub setup_error: Option<String>,
    pub configuration_pending: bool,
    pub login_status: Option<String>,
    pub account: Option<NativeAccountSnapshot>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeQuota {
    pub source: String,
    pub used_percent: Option<f64>,
    pub reset_at: Option<i64>,
    pub window_minutes: Option<i64>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeAccountSnapshot {
    pub client_id: String,
    pub auth_mode: String,
    pub plan: Option<String>,
    pub checked_at: i64,
    pub quota: Option<NativeQuota>,
    pub quotas: Vec<NativeQuota>,
    pub providers: Vec<NativeProviderConnection>,
    pub pool_candidates: Vec<NativePoolCandidate>,
    #[serde(skip_serializing)]
    identities: Vec<pool::NativeIdentity>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeProviderConnection {
    pub provider_id: String,
    pub label: String,
    pub connection_id: Option<String>,
    pub disconnectable: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLoginMethod {
    pub provider_id: String,
    pub provider_label: String,
    pub method_id: String,
    pub label: String,
    pub prompts: Vec<NativeLoginPrompt>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLoginPrompt {
    pub kind: String,
    pub key: String,
    pub message: String,
    pub options: Vec<NativeLoginOption>,
    pub when: Option<NativeLoginCondition>,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NativeLoginOption {
    pub label: String,
    pub value: String,
}

#[derive(Clone, Serialize, Deserialize)]
pub struct NativeLoginCondition {
    pub key: String,
    pub op: String,
    pub value: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLoginProgress {
    pub client_id: String,
    pub status: String,
    pub verification_url: Option<String>,
    pub user_code: Option<String>,
    pub provider_id: Option<String>,
    pub instructions: Option<String>,
    pub requires_code: bool,
}

#[derive(Clone)]
pub struct ClientManager {
    database: Database,
    home: PathBuf,
    codex_home: PathBuf,
    targets: ClientTargets,
    loaded_settings: ClientSettings,
    takeover: AppTakeoverManager,
    driver: Arc<Mutex<Option<codex::CodexDriver>>>,
    access: Arc<Mutex<Option<crate::client_config::ClientConfigLease>>>,
    snapshots: Arc<Mutex<BTreeMap<String, NativeAccountSnapshot>>>,
    opencode_driver: Arc<Mutex<Option<opencode::OpenCodeDriver>>>,
    opencode_access: Arc<Mutex<Option<crate::client_config::ClientConfigLease>>>,
    native_pool: Arc<pool::NativePoolBridge>,
    codex_operations: Arc<pool::NativeOperations>,
    opencode_operations: Arc<pool::NativeOperations>,
}

impl ClientManager {
    pub fn new(database: Database, home: PathBuf, codex_home: PathBuf, targets: ClientTargets, settings: ClientSettings, takeover: AppTakeoverManager, account_pool: crate::accounts::AccountPool) -> Result<Self, String> {
        let native_pool = Arc::new(pool::NativePoolBridge::new(database.clone(), account_pool)?);
        Ok(Self { database, home, codex_home, targets, loaded_settings: settings, takeover, driver: Arc::new(Mutex::new(None)), access: Arc::new(Mutex::new(None)), snapshots: Arc::new(Mutex::new(BTreeMap::new())), opencode_driver: Arc::new(Mutex::new(None)), opencode_access: Arc::new(Mutex::new(None)), native_pool, codex_operations: Arc::default(), opencode_operations: Arc::default() })
    }

    pub fn settings(database: &Database) -> Result<ClientSettings, String> {
        crate::governance::load(database, SETTINGS_KEY)
    }

    pub fn connections(&self) -> Vec<ClientConnection> {
        let configuration_pending = self.takeover.client_paths_pending();
        let login_status = self.driver.lock().ok().and_then(|driver| driver.as_ref().and_then(|driver| driver.login_status().map(str::to_owned)));
        let opencode_login_status = self.opencode_driver.lock().ok().and_then(|driver| driver.as_ref().and_then(|driver| driver.login_status().map(str::to_owned)));
        let codex_paths = vec![self.codex_home.join("config.toml")];
        let claude_paths = vec![self.home.join(".claude/settings.json")];
        let opencode_paths = self.targets.opencode.as_ref().map(|target| vec![target.path.clone()]).unwrap_or_default();
        let rows = [
            ("codex-desktop", "Codex Desktop", "codex", codex_paths.clone(), Some("codex"), "codex", "codex_device_code", "通过 Codex 官方设备授权登录；桌面与 CLI 共用此 CODEX_HOME。", "responses_gateway", None),
            ("codex-cli", "Codex CLI", "codex", codex_paths, Some("codex"), "codex", "codex_device_code", "通过 Codex 官方设备授权登录；桌面与 CLI 共用此 CODEX_HOME。", "responses_gateway", None),
            ("claude-desktop", "Claude Desktop", "claude-desktop", vec![self.targets.claude_desktop_mcp.clone()], None, "claude-desktop", "native_app", "在 Claude Desktop 原生界面完成订阅登录；通过本地 MCP 连接共享工具。", "native_subscription", None),
            ("claude-desktop-3p", "Claude Desktop 3P", "claude-desktop-3p", self.targets.claude_desktop_profile.clone().into_iter().collect(), Some("claude-desktop-3p"), "claude-desktop", "native_gateway_sso", "在 Claude Desktop 中保存并激活第三方网关配置，选择该配置文件，重启客户端后生效。", "messages_gateway", self.targets.claude_desktop_profile.is_none().then(|| "请先选择原生应用保存的 3P 配置文件".into())),
            ("claude-code", "Claude Code", "claude", claude_paths, Some("claude"), "claude", "native_terminal", "在 Claude Code 原生终端完成登录；网关账号可从本地账号池绑定。", "messages_gateway", None),
            ("opencode-desktop", "OpenCode Desktop", "opencode", opencode_paths.clone(), Some("opencode"), "opencode", "opencode_oauth", "通过已安装 OpenCode 的本地受保护接口完成提供商登录；桌面与终端共用当前系统用户的原生账号。", "chat_gateway", self.targets.opencode.as_ref().err().cloned()),
            ("opencode-cli", "OpenCode CLI", "opencode", opencode_paths, Some("opencode"), "opencode", "opencode_oauth", "通过已安装 OpenCode 的本地受保护接口完成提供商登录；项目配置可能覆盖全局模型配置。", "chat_gateway", self.targets.opencode.as_ref().err().cloned()),
            ("gemini-cli", "Gemini CLI", "gemini", vec![self.home.join(".gemini/settings.json"),self.home.join(".gemini/.env")], Some("gemini"), "gemini", "native_terminal", "在 Gemini CLI 原生终端完成登录；网关账号可从本地账号池绑定。", "configured_gateway", None),
        ];
        let snapshots = self.snapshots.lock().ok();
        rows.into_iter().map(|(id,label,owner,paths,takeover,mcp,login,instructions,mode,error)| ClientConnection {
            id:id.into(), label:label.into(), config_owner:owner.into(),
            config_detected:paths.iter().any(|path| path.is_file()),
            config_paths:paths.into_iter().map(|path| path.display().to_string()).collect(),
            takeover_target:takeover.map(str::to_owned), mcp_target:mcp.into(), login_method:login.into(),
            login_instructions:instructions.into(), inference_mode:mode.into(), native_verification:"pending_native".into(), setup_error:error,
            configuration_pending, login_status: match owner { "codex" => login_status.clone(), "opencode" => opencode_login_status.clone(), _ => None },
            account:snapshots.as_ref().and_then(|state| state.get(owner)).cloned().map(|mut account| {account.client_id=id.into();account}),
        }).collect()
    }

    fn authorized_codex(&self, client: &str, confirmed: bool) -> Result<(), String> {
        if !matches!(client,"codex-cli"|"codex-desktop") { return Err("此客户端使用原生登录界面，尚未开放可验证的账号读取接口".into()); }
        if !confirmed { return Err("读取、登录或刷新客户端账号需要对当前 CODEX_HOME 的明确授权".into()); }
        Ok(())
    }

    fn with_driver<T>(&self, operation: impl FnOnce(&mut codex::CodexDriver) -> Result<T,String>) -> Result<T,String> {
        let mut guard = self.driver.lock().map_err(|_| "客户端登录操作锁不可用")?;
        let mut access = self.access.lock().map_err(|_| "客户端配置授权锁不可用")?;
        if access.is_none() { *access = Some(self.takeover.native_config_guard("codex")?); }
        if guard.is_none() {
            match codex::CodexDriver::spawn(self.loaded_settings.codex_binary.as_deref(), &self.codex_home) {
                Ok(driver) => *guard = Some(driver),
                Err(error) => { *access = None; return Err(error); }
            }
        }
        let result = operation(guard.as_mut().unwrap());
        if guard.as_ref().is_some_and(codex::CodexDriver::terminated)
            || !guard.as_ref().is_some_and(codex::CodexDriver::login_pending) {
            *guard = None;
            *access = None;
        }
        result
    }

    pub fn read_account(&self, client: &str, confirmed: bool, refresh: bool) -> Result<NativeAccountSnapshot,String> {
        self.authorized_native(client, confirmed)?;
        let action = if refresh { NativeRead::Refresh } else { NativeRead::Observe };
        self.operations(client).observe(refresh, || self.read_account_locked(client, action, None)).map(|mut snapshot| {
            snapshot.client_id = client.into();
            snapshot
        })
    }

    fn authorized_native(&self, client: &str, confirmed: bool) -> Result<(), String> {
        if Self::is_opencode(client) {
            self.authorized_opencode(client, confirmed)
        } else {
            self.authorized_codex(client, confirmed)
        }
    }

    fn operations(&self, client: &str) -> &pool::NativeOperations {
        if Self::is_opencode(client) { &self.opencode_operations } else { &self.codex_operations }
    }

    fn native_scope(&self, client: &str) -> Result<String, String> {
        let owner = pool::owner(client)?;
        if Self::is_opencode(client) {
            let target = self.targets.opencode.as_ref().map_err(Clone::clone)?;
            self.native_pool.scope(owner, &[&self.home, &target.path])
        } else {
            self.native_pool.scope(owner, &[&self.codex_home])
        }
    }

    fn record_snapshot(&self, client: &str, scope: &str, snapshot: NativeAccountSnapshot, renew: bool) -> Result<NativeAccountSnapshot, String> {
        let owner = pool::owner(client)?;
        let snapshot = self.native_pool.observe(owner, scope, snapshot, renew)?;
        self.snapshots.lock().map_err(|_| "账号状态锁不可用")?.insert(owner.into(), snapshot.clone());
        Ok(snapshot)
    }

    fn read_account_locked(&self, client: &str, action: NativeRead, bound_id: Option<&str>) -> Result<NativeAccountSnapshot, String> {
        let scope = self.native_scope(client)?;
        let refresh = matches!(action, NativeRead::Refresh);
        let result = if Self::is_opencode(client) {
            self.with_opencode(|driver| {
                if driver.login_pending() {
                    return Err("请先完成或取消 OpenCode 登录".into());
                }
                let snapshot = driver.account(client)?;
                let binding = bound_id.map(|id| self.native_pool.bound_client(id, Some(&scope), Some(&snapshot))).transpose();
                let snapshot = self.record_snapshot(client, &scope, snapshot, false)?;
                binding?;
                Ok(snapshot)
            })
        } else {
            self.with_driver(|driver| {
                if driver.login_pending() {
                    return Err("请先完成或取消 Codex 登录".into());
                }
                if let Some(id) = bound_id {
                    let current = driver.account(client, false)?;
                    if let Err(error) = self.native_pool.bound_client(id, Some(&scope), Some(&current)) {
                        self.record_snapshot(client, &scope, current, false)?;
                        return Err(error);
                    }
                }
                if refresh { self.native_pool.invalidate(&scope)?; }
                let snapshot = driver.account(client, refresh)?;
                let renewed = matches!(action, NativeRead::Refresh | NativeRead::CompletedLogin)
                    && snapshot.auth_mode == "chatgpt";
                self.record_snapshot(client, &scope, snapshot, renewed)
            })
        };
        if result.is_err() {
            self.native_pool.invalidate(&scope)?;
            self.snapshots.lock().map_err(|_| "账号状态锁不可用")?.remove(pool::owner(client)?);
        }
        result
    }

    pub fn bind_account(&self, request: BindNativeAccountRequest) -> Result<crate::accounts::AccountInfo, String> {
        self.authorized_native(&request.client_id, request.confirmed)?;
        let client = request.client_id.clone();
        let scope = self.native_scope(&client)?;
        self.operations(&client).mutate(|| {
            self.native_pool.invalidate(&scope)?;
            let bind = |snapshot: NativeAccountSnapshot| {
                let result = self.native_pool.bind(&scope, &snapshot, request);
                self.record_snapshot(&client, &scope, snapshot, false)?;
                result
            };
            let result = if Self::is_opencode(&client) {
                self.with_opencode(|driver| {
                    if driver.login_pending() { return Err("请先完成或取消 OpenCode 登录".into()); }
                    bind(driver.account(&client)?)
                })
            } else {
                self.with_driver(|driver| {
                    if driver.login_pending() { return Err("请先完成或取消 Codex 登录".into()); }
                    bind(driver.account(&client, false)?)
                })
            };
            if result.is_err() {
                self.native_pool.invalidate(&scope)?;
                self.snapshots.lock().map_err(|_| "账号状态锁不可用")?.remove(pool::owner(&client)?);
            }
            result
        })
    }

    pub fn refresh_pool_account(&self, id: &str, confirmed: bool) -> Result<crate::accounts::AccountInfo, String> {
        if !confirmed { return Err(crate::accounts::AccountError::AuthorizationRequired.to_string()); }
        let client = self.native_pool.bound_client(id, None, None)?;
        let refresh = !Self::is_opencode(&client);
        let action = if refresh { NativeRead::Refresh } else { NativeRead::Observe };
        self.operations(&client).observe_for(id, refresh, || self.read_account_locked(&client, action, Some(id)))?;
        self.native_pool.account(id)
    }

    pub fn start_login(&self, client: &str, confirmed: bool) -> Result<NativeLoginProgress,String> {
        self.authorized_codex(client,confirmed)?;
        self.operations(client).mutate(|| {
            self.native_pool.invalidate(&self.native_scope(client)?)?;
            self.snapshots.lock().map_err(|_| "账号状态锁不可用")?.remove("codex");
            let progress = self.with_driver(|driver| driver.start_login(client))?;
            if progress.status == "completed" { let _ = self.read_account_locked(client, NativeRead::CompletedLogin, None); }
            Ok(progress)
        })
    }

    pub fn poll_login(&self, client: &str, confirmed: bool) -> Result<NativeLoginProgress,String> {
        self.authorized_native(client, confirmed)?;
        self.operations(client).mutate(|| {
            let progress = if Self::is_opencode(client) { self.with_opencode(|driver| driver.login_progress(client))? }
                else { self.with_driver(|driver| driver.login_progress(client))? };
            if progress.status == "completed" { let _ = self.read_account_locked(client, NativeRead::CompletedLogin, None); }
            Ok(progress)
        })
    }

    pub fn cancel_login(&self, client: &str, confirmed: bool) -> Result<(),String> {
        self.authorized_native(client, confirmed)?;
        self.operations(client).mutate(|| if Self::is_opencode(client) { self.with_opencode(opencode::OpenCodeDriver::cancel_login) }
            else { self.with_driver(codex::CodexDriver::cancel_login) })
    }

    pub fn logout(&self, client: &str, confirmed: bool) -> Result<(),String> {
        self.authorized_codex(client,confirmed)?;
        self.operations(client).mutate(|| {
            let scope = self.native_scope(client)?;
            self.native_pool.invalidate(&scope)?;
            self.with_driver(codex::CodexDriver::logout)?;
            self.snapshots.lock().map_err(|_| "账号状态锁不可用")?.remove("codex");
            self.native_pool.signed_out(&scope, None)
        })
    }

    pub fn stop_owned_driver(&self) {
        for client in ["codex-cli", "opencode-cli"] {
            let _ = self.operations(client).mutate(|| {
                if Self::is_opencode(client) {
                    if let Ok(mut guard) = self.opencode_driver.lock() { *guard = None; }
                    if let Ok(mut access) = self.opencode_access.lock() { *access = None; }
                } else {
                    if let Ok(mut guard) = self.driver.lock() { *guard = None; }
                    if let Ok(mut access) = self.access.lock() { *access = None; }
                }
                if let Ok(scope) = self.native_scope(client) { self.native_pool.invalidate(&scope)?; }
                self.snapshots.lock().map_err(|_| "账号状态锁不可用")?.remove(pool::owner(client)?);
                Ok(())
            });
        }
    }

    fn is_opencode(client: &str) -> bool {
        matches!(client, "opencode-cli" | "opencode-desktop")
    }

    fn authorized_opencode(&self, client: &str, confirmed: bool) -> Result<(), String> {
        if !Self::is_opencode(client) || !confirmed {
            return Err("OpenCode 账号操作需要对所选配置和当前系统用户原生账号的明确授权".into());
        }
        Ok(())
    }

    fn with_opencode<T>(&self, operation: impl FnOnce(&mut opencode::OpenCodeDriver) -> Result<T, String>) -> Result<T, String> {
        let mut guard = self.opencode_driver.lock().map_err(|_| "OpenCode 登录操作锁不可用")?;
        let mut access = self.opencode_access.lock().map_err(|_| "OpenCode 配置授权锁不可用")?;
        if access.is_none() { *access = Some(self.takeover.native_config_guard("opencode")?); }
        if guard.is_none() {
            let target = match self.targets.opencode.as_ref() {
                Ok(target) => target,
                Err(error) => { *access = None; return Err(error.clone()); }
            };
            match opencode::OpenCodeDriver::spawn(self.loaded_settings.opencode_binary.as_deref(), &self.home, target) {
                Ok(driver) => *guard = Some(driver),
                Err(error) => { *access = None; return Err(error); }
            }
        }
        let result = operation(guard.as_mut().unwrap());
        if guard.as_ref().is_some_and(opencode::OpenCodeDriver::terminated)
            || !guard.as_ref().is_some_and(opencode::OpenCodeDriver::login_pending) {
            *guard = None;
            *access = None;
        }
        result
    }

    pub fn login_methods(&self, client: &str, confirmed: bool) -> Result<Vec<NativeLoginMethod>, String> {
        self.authorized_opencode(client, confirmed)?;
        self.operations(client).mutate(|| self.with_opencode(opencode::OpenCodeDriver::login_methods))
    }

    pub fn start_opencode_login(&self, client: &str, confirmed: bool, provider_id: &str, method_id: &str, inputs: BTreeMap<String, String>) -> Result<NativeLoginProgress, String> {
        let mut inputs = opencode::Inputs(inputs);
        self.authorized_opencode(client, confirmed)?;
        self.operations(client).mutate(|| {
            self.native_pool.invalidate(&self.native_scope(client)?)?;
            self.snapshots.lock().map_err(|_| "账号状态锁不可用")?.remove("opencode");
            self.with_opencode(|driver| driver.start_login(client, provider_id, method_id, std::mem::take(&mut inputs.0)))
        })
    }

    pub fn complete_login(&self, client: &str, confirmed: bool, code: &str) -> Result<NativeLoginProgress, String> {
        self.authorized_opencode(client, confirmed)?;
        self.operations(client).mutate(|| {
            let progress = self.with_opencode(|driver| driver.complete_login(client, code))?;
            if progress.status == "completed" { let _ = self.read_account_locked(client, NativeRead::CompletedLogin, None); }
            Ok(progress)
        })
    }

    pub fn disconnect_provider(&self, client: &str, confirmed: bool, provider_id: &str, connection_id: Option<&str>) -> Result<(), String> {
        self.authorized_opencode(client, confirmed)?;
        self.operations(client).mutate(|| {
            let scope = self.native_scope(client)?;
            self.native_pool.invalidate(&scope)?;
            self.with_opencode(|driver| driver.disconnect(provider_id, connection_id))?;
            self.snapshots.lock().map_err(|_| "账号状态锁不可用")?.remove("opencode");
            self.native_pool.signed_out(&scope, Some(&NativeProviderConnection {
                provider_id: provider_id.into(), label: String::new(), connection_id: connection_id.map(str::to_owned), disconnectable: true,
            }))
        })
    }
}

#[tauri::command]
pub async fn bind_native_account(manager: tauri::State<'_, ClientManager>, request: BindNativeAccountRequest) -> Result<crate::accounts::AccountInfo, String> {
    crate::platform::require_desktop("原生账号绑定")?;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.bind_account(request)).await.map_err(|_| "原生账号绑定失败".to_string())?
}

#[tauri::command]
pub async fn refresh_native_account(manager: tauri::State<'_, ClientManager>, id: String, confirmed: bool) -> Result<crate::accounts::AccountInfo, String> {
    crate::platform::require_desktop("原生账号同步")?;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.refresh_pool_account(&id, confirmed)).await.map_err(|_| "原生账号同步失败".to_string())?
}

#[tauri::command]
pub async fn list_client_login_methods(manager: tauri::State<'_, ClientManager>, client_id: String, confirmed: bool) -> Result<Vec<NativeLoginMethod>, String> {
    crate::platform::require_desktop("客户端登录")?;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.login_methods(&client_id, confirmed)).await.map_err(|_| "查询原生登录方式失败".to_string())?
}

#[tauri::command]
pub async fn start_opencode_client_login(manager: tauri::State<'_, ClientManager>, client_id: String, confirmed: bool, provider_id: String, method_id: String, inputs: BTreeMap<String, String>) -> Result<NativeLoginProgress, String> {
    let mut inputs = opencode::Inputs(inputs);
    crate::platform::require_desktop("客户端登录")?;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.start_opencode_login(&client_id, confirmed, &provider_id, &method_id, std::mem::take(&mut inputs.0))).await.map_err(|_| "启动原生授权失败".to_string())?
}

#[tauri::command]
pub async fn complete_client_login(manager: tauri::State<'_, ClientManager>, client_id: String, confirmed: bool, code: String) -> Result<NativeLoginProgress, String> {
    let code = zeroize::Zeroizing::new(code);
    crate::platform::require_desktop("客户端登录")?;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.complete_login(&client_id, confirmed, &code)).await.map_err(|_| "提交原生授权码失败".to_string())?
}

#[tauri::command]
pub async fn disconnect_client_provider(manager: tauri::State<'_, ClientManager>, client_id: String, confirmed: bool, provider_id: String, connection_id: Option<String>) -> Result<(), String> {
    crate::platform::require_desktop("客户端账号")?;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.disconnect_provider(&client_id, confirmed, &provider_id, connection_id.as_deref())).await.map_err(|_| "退出原生提供商账号失败".to_string())?
}

#[tauri::command]
pub fn list_client_connections(manager: tauri::State<'_,ClientManager>) -> Vec<ClientConnection> {
    if cfg!(mobile) { Vec::new() } else { manager.connections() }
}

#[tauri::command]
pub fn get_client_settings(manager: tauri::State<'_,ClientManager>) -> Result<ClientSettings,String> {
    ClientManager::settings(&manager.database)
}

#[tauri::command]
pub fn save_client_settings(manager: tauri::State<'_,ClientManager>, takeover: tauri::State<'_,AppTakeoverManager>, settings: ClientSettings) -> Result<ClientSettings,String> {
    crate::platform::require_desktop("客户端接入")?;
    let _guard = takeover.inactive_guard()?;
    ClientTargets::validate_settings(&manager.home,&settings)?;
    crate::governance::save(&manager.database,SETTINGS_KEY,&settings)?;
    manager.stop_owned_driver();
    if settings != manager.loaded_settings { takeover.client_paths_changed()?; }
    Ok(settings)
}

#[tauri::command]
pub async fn read_client_account(manager: tauri::State<'_,ClientManager>, client_id:String, confirmed:bool, refresh:bool) -> Result<NativeAccountSnapshot,String> {
    crate::platform::require_desktop("客户端账号")?;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.read_account(&client_id,confirmed,refresh)).await.map_err(|_| "读取客户端账号任务失败".to_string())?
}

#[tauri::command]
pub async fn start_client_login(manager: tauri::State<'_,ClientManager>, takeover: tauri::State<'_,AppTakeoverManager>, client_id:String, confirmed:bool) -> Result<NativeLoginProgress,String> {
    crate::platform::require_desktop("客户端登录")?;
    let _ = takeover;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.start_login(&client_id,confirmed)).await.map_err(|_| "客户端登录任务失败".to_string())?
}

#[tauri::command]
pub async fn poll_client_login(manager: tauri::State<'_,ClientManager>, client_id:String, confirmed:bool) -> Result<NativeLoginProgress,String> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.poll_login(&client_id,confirmed)).await.map_err(|_| "客户端登录查询失败".to_string())?
}

#[tauri::command]
pub async fn cancel_client_login(manager: tauri::State<'_,ClientManager>, client_id:String, confirmed:bool) -> Result<(),String> {
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.cancel_login(&client_id,confirmed)).await.map_err(|_| "取消客户端登录失败".to_string())?
}

#[tauri::command]
pub async fn logout_client_account(manager: tauri::State<'_,ClientManager>, takeover: tauri::State<'_,AppTakeoverManager>, client_id:String, confirmed:bool) -> Result<(),String> {
    crate::platform::require_desktop("客户端账号")?;
    let _ = takeover;
    let manager = manager.inner().clone();
    tauri::async_runtime::spawn_blocking(move || manager.logout(&client_id,confirmed)).await.map_err(|_| "退出客户端账号失败".to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn desktop_and_cli_have_independent_rows_and_explicit_shared_owners() {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let home = std::env::temp_dir().join(format!("xy-client-{}",uuid::Uuid::new_v4()));
        let takeover = AppTakeoverManager::new(database.clone(), crate::proxy_manager::ProxyManager::new(database.clone()), home.clone(), home.join("data"));
        let account_pool = crate::accounts::AccountPool::test_pool(database.clone()).unwrap();
        let manager = ClientManager::new(database,home.clone(),home.join(".codex"),ClientTargets::for_home(&home),ClientSettings::default(),takeover,account_pool).unwrap();
        let rows = manager.connections();
        assert_eq!(rows.len(),8);
        assert_eq!(rows.iter().filter(|row| row.config_owner == "codex").count(),2);
        assert_eq!(rows.iter().filter(|row| row.config_owner == "opencode").count(),2);
        assert!(rows.iter().all(|row| row.account.is_none()));
        assert!(manager.read_account("codex-cli",false,false).is_err());
        assert!(manager.read_account("claude-desktop",true,false).is_err());
        assert!(manager.login_methods("opencode-cli", false).err().unwrap().contains("授权"));
        assert!(manager.start_opencode_login("opencode-desktop", false, "openai", "oauth", BTreeMap::from([("tenant".into(), "synthetic-private-input".into())])).err().unwrap().contains("授权"));
        assert!(manager.complete_login("opencode-cli", false, "synthetic-code").err().unwrap().contains("授权"));
        assert!(manager.disconnect_provider("opencode-cli", false, "openai", None).unwrap_err().contains("授权"));
        assert!(!home.exists());
    }
}
