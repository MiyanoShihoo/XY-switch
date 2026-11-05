use super::{http, types::*};
use crate::{cancellation::Cancellation, credential_store, database::Database,
    mcp::{McpManager, McpServer}, plugins::{PluginManager, ToolCallConfirmation, VerifiedSkill, VerifiedTool}, privacy};
use ring::{digest, rand::{SecureRandom, SystemRandom}};
use futures_util::future::poll_fn;
use std::future::Future;
use std::task::Poll;
use serde_json::{Value, json};
use std::{collections::{BTreeMap, VecDeque}, sync::{Arc, Mutex}, time::{Duration, Instant}};
use tokio::{net::TcpListener, sync::{Mutex as AsyncMutex, oneshot}, task::JoinHandle};
use zeroize::Zeroizing;

const SETTINGS_KEY: &str = "shared_mcp_settings_v1";
const CONNECTIONS_KEY: &str = "shared_mcp_connections_v1";
const MAX_CONNECTIONS: usize = 16;
const MAX_SESSIONS: usize = 128;
const MAX_PENDING: usize = 32;
const APPROVAL_TTL: Duration = Duration::from_secs(110);

pub(super) trait ConnectionSecrets: Send + Sync {
    fn get(&self, id: &str) -> Result<Option<String>, String>;
    fn set(&self, id: &str, value: &str) -> Result<(), String>;
    fn delete(&self, id: &str) -> Result<(), String>;
}

struct NativeConnectionSecrets;

impl ConnectionSecrets for NativeConnectionSecrets {
    fn get(&self, id: &str) -> Result<Option<String>, String> { credential_store::get_secret("mcp", &secret_id(id)) }
    fn set(&self, id: &str, value: &str) -> Result<(), String> { credential_store::set_secret("mcp", &secret_id(id), value) }
    fn delete(&self, id: &str) -> Result<(), String> { credential_store::delete_secret("mcp", &secret_id(id)) }
}

pub(crate) struct RuntimeConnection {
    pub view: SharedMcpConnection,
    pub token_hash: Vec<u8>,
    pub cancellation: Cancellation,
    pub requests: VecDeque<Instant>,
}

pub(crate) struct Session {
    pub connection_id: String,
    pub version: String,
    pub ready: bool,
    pub cancellation: Cancellation,
    pub last_seen: Instant,
}

struct PendingApproval {
    key: (String, String, String),
    view: SharedToolApproval,
    sender: oneshot::Sender<bool>,
    deadline: Instant,
}

pub(crate) struct Runtime {
    pub enabled: bool,
    pub running: bool,
    pub address: String,
    pub connections: BTreeMap<String, RuntimeConnection>,
    pub sessions: BTreeMap<String, Session>,
    pending: BTreeMap<String, PendingApproval>,
    active: BTreeMap<(String, String, String), Cancellation>,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct SharedMcpManager {
    pub(crate) database: Database,
    pub(crate) plugins: PluginManager,
    pub(crate) state: Arc<Mutex<Runtime>>,
    operation: Arc<AsyncMutex<()>>,
    task: Arc<AsyncMutex<Option<JoinHandle<()>>>>,
    registration: Arc<Mutex<()>>,
    secrets: Arc<dyn ConnectionSecrets>,
}

pub(crate) struct CallLease {
    manager: SharedMcpManager,
    key: (String, String, String),
    preview_id: String,
}

impl Drop for CallLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.manager.state.lock() {
            state.pending.remove(&self.preview_id);
            state.active.remove(&self.key);
        }
        let _ = self.manager.plugins.cancel_tool_preview(&self.preview_id);
    }
}

impl SharedMcpManager {
    pub fn new(database: Database, plugins: PluginManager) -> Result<Self, String> {
        Self::with_secrets(database, plugins, Arc::new(NativeConnectionSecrets))
    }

    pub(super) fn with_secrets(database: Database, plugins: PluginManager, secrets: Arc<dyn ConnectionSecrets>) -> Result<Self, String> {
        let settings = match database.get_setting(SETTINGS_KEY).map_err(|_| "shared_mcp_store")? {
            Some(raw) => serde_json::from_str::<SharedMcpSettings>(&raw).map_err(|_| "shared_mcp_store")?,
            None => SharedMcpSettings::default(),
        };
        let records: Vec<SharedMcpConnection> = match database.get_setting(CONNECTIONS_KEY).map_err(|_| "shared_mcp_store")? {
            Some(raw) => serde_json::from_str(&raw).map_err(|_| "shared_mcp_store")?, None => Vec::new(),
        };
        if records.len() > MAX_CONNECTIONS { return Err("shared_mcp_limit".into()); }
        let mut connections = BTreeMap::new();
        let mut scope_migrated = false;
        for mut view in records {
            let clients = clients_for_owner(&view.owner).ok_or("shared_mcp_scope")?;
            let legacy_desktop_scope = view.owner == "claude-desktop" && view.clients == ["claude-desktop"];
            if !valid_connection_id(&view.id) || (!legacy_desktop_scope && view.clients != clients.iter().map(|client| (*client).to_owned()).collect::<Vec<_>>())
                || view.server_id != format!("xy-shared-{}", view.id) || connections.contains_key(&view.id) {
                return Err("shared_mcp_store".into());
            }
            if legacy_desktop_scope {
                scope_migrated |= view.active;
                view.active = false;
            }
            let token_hash = if view.active {
                match secrets.get(&view.id).map_err(|_| "shared_mcp_credentials")? {
                    Some(raw) => { let raw = Zeroizing::new(raw); let (_, token) = bridge_credential(&raw)?; digest::digest(&digest::SHA256, token.as_bytes()).as_ref().to_vec() },
                    None => { view.active = false; Vec::new() },
                }
            } else { Vec::new() };
            connections.insert(view.id.clone(), RuntimeConnection { view, token_hash, cancellation: Cancellation::default(), requests: VecDeque::new() });
        }
        if scope_migrated {
            let records: Vec<_> = connections.values().map(|connection| &connection.view).collect();
            let raw = serde_json::to_string(&records).map_err(|_| "shared_mcp_store")?;
            database.set_setting(CONNECTIONS_KEY, &raw).map_err(|_| "shared_mcp_store")?;
        }
        Ok(Self { database, plugins, state: Arc::new(Mutex::new(Runtime {
            enabled: settings.enabled, running: false, address: format!("http://127.0.0.1:{DEFAULT_PORT}"),
            connections, sessions: BTreeMap::new(), pending: BTreeMap::new(), active: BTreeMap::new(), last_error: None,
        })), operation: Arc::new(AsyncMutex::new(())), task: Arc::new(AsyncMutex::new(None)), registration: Arc::new(Mutex::new(())), secrets })
    }

    pub fn status(&self) -> Result<SharedMcpStatus, String> {
        let state = self.state.lock().map_err(|_| "shared_mcp_state")?;
        Ok(SharedMcpStatus { enabled: state.enabled, running: state.running, address: state.address.clone(),
            versions: SUPPORTED_VERSIONS.iter().map(|version| (*version).to_owned()).collect(),
            connections: state.connections.values().map(|connection| connection.view.clone()).collect(),
            pending_count: state.pending.len(), last_error: state.last_error.clone() })
    }

    pub async fn start_saved(&self) -> Result<(), String> {
        if self.status()?.enabled { self.set_enabled(true, true).await?; }
        Ok(())
    }

    pub async fn set_enabled(&self, enabled: bool, confirmed: bool) -> Result<SharedMcpStatus, String> {
        if !confirmed { return Err("shared_mcp_confirmation".into()); }
        let _operation = self.operation.lock().await;
        let mut task = self.task.lock().await;
        if enabled && task.as_ref().is_none_or(JoinHandle::is_finished) {
            let listener = match TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, DEFAULT_PORT)).await {
                Ok(listener) => listener,
                Err(_) => {
                    self.state.lock().map_err(|_| "shared_mcp_state")?.last_error = Some("shared_mcp_bind".into());
                    return Err("shared_mcp_bind".into());
                }
            };
            self.database.set_setting(SETTINGS_KEY, r#"{"enabled":true}"#).map_err(|_| "shared_mcp_store")?;
            { let _registration = self.registration.lock().map_err(|_| "shared_mcp_state")?;
                let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
                state.enabled = true; state.running = true; state.last_error = None;
                for connection in state.connections.values_mut() { connection.cancellation = Cancellation::default(); }
            }
            let manager = self.clone();
            *task = Some(tokio::spawn(async move {
                let result = axum::serve(listener, http::router(manager.clone())).await;
                if let Ok(mut state) = manager.state.lock() {
                    state.running = false;
                    if result.is_err() { state.last_error = Some("shared_mcp_server".into()); }
                }
            }));
        } else if !enabled {
            self.database.set_setting(SETTINGS_KEY, r#"{"enabled":false}"#).map_err(|_| "shared_mcp_store")?;
            self.cancel_all();
            if let Some(handle) = task.take() { handle.abort(); let _ = handle.await; }
        }
        self.status()
    }

    pub fn cancel_all(&self) {
        let Ok(_registration) = self.registration.lock() else { return; };
        let cleanup = self.state.lock().ok().map(|mut state| {
            state.enabled = false;
            state.running = false;
            let mut signals: Vec<_> = state.connections.values().map(|connection| connection.cancellation.clone()).collect();
            signals.extend(state.sessions.values().map(|session| session.cancellation.clone()));
            signals.extend(state.active.values().cloned());
            state.sessions.clear();
            (signals, std::mem::take(&mut state.pending).into_keys().collect::<Vec<_>>())
        });
        if let Some((signals, previews)) = cleanup {
            for signal in signals { signal.cancel(); }
            for id in previews { let _ = self.plugins.cancel_tool_preview(&id); }
        }
    }

    pub async fn stop(&self) {
        let _operation = self.operation.lock().await;
        self.cancel_all();
        if let Some(handle) = self.task.lock().await.take() {
            handle.abort();
            let _ = handle.await;
        }
    }

    fn check_connection(state: &Runtime, connection: &SharedMcpConnection) -> Result<(), &'static str> {
        let current = state.connections.get(&connection.id).ok_or("shared_mcp_auth")?;
        if !state.enabled || !state.running || !current.view.active || &current.view != connection || current.cancellation.is_cancelled() {
            return Err("shared_mcp_cancelled");
        }
        Ok(())
    }

    pub fn register(&self, manager: &McpManager, owner: &str, confirmed: bool, shared_scope_confirmed: bool) -> Result<SharedMcpConnection, String> {
        let clients = clients_for_owner(owner).ok_or("shared_mcp_scope")?;
        if !confirmed || clients.len() > 1 && !shared_scope_confirmed { return Err("shared_mcp_confirmation".into()); }
        let _registration = self.registration.lock().map_err(|_| "shared_mcp_state")?;
        let status = self.status()?;
        if !status.running { return Err("shared_mcp_disabled".into()); }
        let config_target = config_target_for_owner(owner);
        if status.connections.iter().any(|connection| !connection.active && config_target_for_owner(&connection.owner) == config_target) {
            return Err("shared_mcp_cleanup_required".into());
        }
        if status.connections.iter().filter(|connection| connection.active).any(|connection| config_target_for_owner(&connection.owner) == config_target)
            || status.connections.len() >= MAX_CONNECTIONS { return Err("shared_mcp_limit".into()); }
        let id = uuid::Uuid::new_v4().to_string();
        let view = SharedMcpConnection { server_id: format!("xy-shared-{id}"), id: id.clone(), owner: owner.to_owned(),
            clients: clients.iter().map(|client| (*client).to_owned()).collect(), active: true };
        let executable = std::env::current_exe().map_err(|_| "shared_mcp_executable")?;
        let mut random = Zeroizing::new([0u8; 32]);
        SystemRandom::new().fill(random.as_mut()).map_err(|_| "shared_mcp_credentials")?;
        let token = Zeroizing::new(random.iter().map(|byte| format!("{byte:02x}")).collect::<String>());
        random.fill(0);
        let endpoint = format!("{}/mcp/{id}", status.address);
        let secret = Zeroizing::new(serde_json::to_string(&BridgeCredentialRef { endpoint: &endpoint, token: &token }).map_err(|_| "shared_mcp_credentials")?);
        self.secrets.set(&id, &secret).map_err(|_| "shared_mcp_credentials")?;
        let server = McpServer { id: view.server_id.clone(), name: "XY Switch shared plugins".into(),
            description: "Local verified plugins; each tool call requires approval in XY Switch.".into(), transport: "stdio".into(),
            command: Some(executable.to_string_lossy().into_owned()), args: vec!["--shared-mcp-stdio".into(), id.clone()], url: None,
            env: BTreeMap::new(), headers: BTreeMap::new(), apps: BTreeMap::from([(config_target.to_owned(), true)]), built_in: false, secrets_configured: false };
        if manager.save_server(server).is_err() {
            let _ = self.secrets.delete(&id); return Err("shared_mcp_config".into());
        }
        let mut records = status.connections;
        records.push(view.clone());
        if self.save_connections(&records).is_err() {
            let _ = manager.delete_server(&view.server_id);
            let _ = self.secrets.delete(&id); return Err("shared_mcp_store".into());
        }
        self.state.lock().map_err(|_| "shared_mcp_state")?.connections.insert(id, RuntimeConnection { view: view.clone(),
            token_hash: digest::digest(&digest::SHA256, token.as_bytes()).as_ref().to_vec(), cancellation: Cancellation::default(), requests: VecDeque::new() });
        Ok(view)
    }

    fn save_connections(&self, records: &[SharedMcpConnection]) -> Result<(), String> {
        let value = serde_json::to_string(records).map_err(|_| "shared_mcp_store")?;
        self.database.set_setting(CONNECTIONS_KEY, &value).map_err(|_| "shared_mcp_store".into())
    }

    pub fn revoke(&self, manager: &McpManager, id: &str, confirmed: bool) -> Result<(), String> {
        if !confirmed || !valid_connection_id(id) { return Err("shared_mcp_confirmation".into()); }
        let _registration = self.registration.lock().map_err(|_| "shared_mcp_state")?;
        let mut records = self.status()?.connections;
        let view = records.iter_mut().find(|connection| connection.id == id).ok_or("shared_mcp_connection")?;
        view.active = false;
        let server_id = view.server_id.clone();
        self.save_connections(&records)?;
        let (signals, previews) = {
            let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
            let mut signals = Vec::new();
            if let Some(connection) = state.connections.get_mut(id) {
                connection.view.active = false;
                signals.push(connection.cancellation.clone());
                connection.token_hash.clear();
            }
            state.sessions.retain(|_, session| {
                if session.connection_id == id { signals.push(session.cancellation.clone()); false } else { true }
            });
            signals.extend(state.active.iter().filter(|(key, _)| key.0 == id).map(|(_, signal)| signal.clone()));
            let previews: Vec<_> = state.pending.iter().filter(|(_, pending)| pending.key.0 == id).map(|(key, _)| key.clone()).collect();
            for preview in &previews { state.pending.remove(preview); }
            (signals, previews)
        };
        for signal in signals { signal.cancel(); }
        for preview in previews { let _ = self.plugins.cancel_tool_preview(&preview); }
        if self.database.get_mcp_record(&server_id).map_err(|_| "shared_mcp_store")?.is_some() {
            manager.delete_server(&server_id).map_err(|_| "shared_mcp_config_cleanup")?;
        }
        self.secrets.delete(id).map_err(|_| "shared_mcp_credentials_cleanup")?;
        records.retain(|connection| connection.id != id);
        self.save_connections(&records)?;
        self.state.lock().map_err(|_| "shared_mcp_state")?.connections.remove(id);
        Ok(())
    }

    pub(crate) fn authenticate(&self, id: &str, token: &str) -> Result<SharedMcpConnection, &'static str> {
        if !valid_connection_id(id) || token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) { return Err("shared_mcp_auth"); }
        let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
        if !state.enabled || !state.running { return Err("shared_mcp_disabled"); }
        let connection = state.connections.get_mut(id).ok_or("shared_mcp_auth")?;
        let hash = digest::digest(&digest::SHA256, token.as_bytes());
        #[allow(deprecated)]
        let valid = ring::constant_time::verify_slices_are_equal(&connection.token_hash, hash.as_ref()).is_ok();
        if !connection.view.active || !valid || connection.cancellation.is_cancelled() { return Err("shared_mcp_auth"); }
        let now = Instant::now();
        while connection.requests.front().is_some_and(|seen| now.duration_since(*seen) > Duration::from_secs(30)) { connection.requests.pop_front(); }
        if connection.requests.len() >= 120 { return Err("shared_mcp_rate_limit"); }
        connection.requests.push_back(now);
        Ok(connection.view.clone())
    }

    pub(crate) fn create_session(&self, connection: &SharedMcpConnection, version: &str) -> Result<String, &'static str> {
        if version == MODERN_VERSION || !SUPPORTED_VERSIONS.contains(&version) { return Err("shared_mcp_version"); }
        let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
        Self::check_connection(&state, connection)?;
        let stale: Vec<String> = state.sessions.iter().filter(|(_, session)| session.last_seen.elapsed() > Duration::from_secs(1800)).map(|(id, _)| id.clone()).collect();
        let expired: Vec<_> = stale.iter().filter_map(|id| state.sessions.remove(id).map(|session| session.cancellation)).collect();
        if state.sessions.len() >= MAX_SESSIONS { return Err("shared_mcp_limit"); }
        let id = uuid::Uuid::new_v4().to_string();
        state.sessions.insert(id.clone(), Session { connection_id: connection.id.clone(), version: version.into(), ready: false,
            cancellation: Cancellation::default(), last_seen: Instant::now() });
        drop(state);
        for signal in expired { signal.cancel(); }
        Ok(id)
    }

    pub(crate) fn session(&self, connection: &SharedMcpConnection, id: &str, version: &str, initialize: bool) -> Result<Cancellation, &'static str> {
        if !valid_connection_id(id) { return Err("shared_mcp_session"); }
        let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
        Self::check_connection(&state, connection)?;
        let session = state.sessions.get_mut(id).ok_or("shared_mcp_session")?;
        if session.connection_id != connection.id || session.version != version || session.cancellation.is_cancelled() { return Err("shared_mcp_session"); }
        if session.last_seen.elapsed() > Duration::from_secs(1800) {
            drop(state);
            self.close_session(connection, id)?;
            return Err("shared_mcp_session");
        }
        if initialize { session.ready = true; }
        if !session.ready { return Err("shared_mcp_not_initialized"); }
        session.last_seen = Instant::now();
        Ok(session.cancellation.clone())
    }

    pub(crate) fn close_session(&self, connection: &SharedMcpConnection, id: &str) -> Result<(), &'static str> {
        let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
        if state.sessions.get(id).is_none_or(|session| session.connection_id != connection.id) { return Err("shared_mcp_session"); }
        let mut signals = Vec::new();
        if let Some(session) = state.sessions.remove(id) { signals.push(session.cancellation); }
        signals.extend(state.active.iter().filter(|(key, _)| key.0 == connection.id && key.1 == id).map(|(_, signal)| signal.clone()));
        let previews: Vec<_> = state.pending.iter().filter(|(_, pending)| pending.key.0 == connection.id && pending.key.1 == id).map(|(key, _)| key.clone()).collect();
        for preview in &previews { state.pending.remove(preview); }
        drop(state);
        for signal in signals { signal.cancel(); }
        for preview in previews { let _ = self.plugins.cancel_tool_preview(&preview); }
        Ok(())
    }

    pub(crate) fn tools(&self, connection: &SharedMcpConnection) -> Result<Vec<VerifiedTool>, &'static str> {
        self.state.lock().map_err(|_| "shared_mcp_state").and_then(|state| Self::check_connection(&state, connection))?;
        let first = connection.clients.first().ok_or("shared_mcp_scope")?;
        let mut tools = self.plugins.tools_for_client(first).map_err(|_| "shared_mcp_plugins")?;
        for client in connection.clients.iter().skip(1) {
            let other = self.plugins.tools_for_client(client).map_err(|_| "shared_mcp_plugins")?;
            tools.retain(|tool| other.iter().any(|candidate| candidate.plugin_id == tool.plugin_id && candidate.digest == tool.digest && candidate.tool == tool.tool));
        }
        Ok(tools)
    }

    pub(crate) fn skills(&self, connection: &SharedMcpConnection) -> Result<Vec<VerifiedSkill>, &'static str> {
        self.state.lock().map_err(|_| "shared_mcp_state").and_then(|state| Self::check_connection(&state, connection))?;
        let first = connection.clients.first().ok_or("shared_mcp_scope")?;
        let mut skills = self.plugins.skills_for_client(first).map_err(|_| "shared_mcp_plugins")?;
        for client in connection.clients.iter().skip(1) {
            let other = self.plugins.skills_for_client(client).map_err(|_| "shared_mcp_plugins")?;
            skills.retain(|skill| other.iter().any(|candidate| candidate.plugin_id == skill.plugin_id && candidate.digest == skill.digest));
        }
        Ok(skills)
    }

    pub fn pending(&self) -> Result<Vec<SharedToolApproval>, String> {
        let (pending, expired) = {
            let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
            let expired: Vec<_> = state.pending.iter().filter(|(_, pending)| pending.deadline <= Instant::now() || pending.sender.is_closed()).map(|(id, _)| id.clone()).collect();
            for id in &expired { state.pending.remove(id); }
            (state.pending.values().map(|pending| pending.view.clone()).collect(), expired)
        };
        for id in expired { let _ = self.plugins.cancel_tool_preview(&id); }
        Ok(pending)
    }

    #[cfg(test)]
    pub(super) fn expire_test_approval(&self, id: &str) {
        self.state.lock().unwrap().pending.get_mut(id).unwrap().deadline = Instant::now() - Duration::from_secs(1);
    }

    pub fn decide(&self, id: &str, approved: bool, confirmed: bool) -> Result<(), String> {
        if !valid_connection_id(id) || approved && !confirmed { return Err("shared_mcp_confirmation".into()); }
        let pending = self.state.lock().map_err(|_| "shared_mcp_state")?.pending.remove(id).ok_or("shared_mcp_expired")?;
        if pending.deadline <= Instant::now() {
            let _ = self.plugins.cancel_tool_preview(id);
            return Err("shared_mcp_expired".into());
        }
        if !approved { let _ = self.plugins.cancel_tool_preview(id); }
        pending.sender.send(approved).map_err(|_| "shared_mcp_expired".into())
    }

    pub(crate) fn cancel_request(&self, connection: &SharedMcpConnection, session: &str, request: &str) -> Result<(), &'static str> {
        let key = (connection.id.clone(), session.to_owned(), request.to_owned());
        let (signal, previews) = {
            let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
            let signal = state.active.get(&key).cloned();
            let previews: Vec<_> = state.pending.iter().filter(|(_, pending)| pending.key == key).map(|(id, _)| id.clone()).collect();
            for id in &previews { state.pending.remove(id); }
            (signal, previews)
        };
        if let Some(signal) = signal { signal.cancel(); }
        for id in previews { let _ = self.plugins.cancel_tool_preview(&id); }
        Ok(())
    }

    pub(crate) async fn call(&self, connection: SharedMcpConnection, session_id: String, request_key: String,
        session_cancel: Cancellation, name: String, arguments: Value, known_secret: Zeroizing<String>) -> Result<Value, &'static str> {
        let tool = self.tools(&connection)?.into_iter().find(|tool| http::tool_name(tool) == name).ok_or("shared_mcp_tool")?;
        let settings = privacy::load(&self.database).map_err(|_| "shared_mcp_privacy")?;
        let checked = privacy::inspect(arguments, &settings, &[known_secret.as_str()]).map_err(|_| "shared_mcp_privacy")?;
        if checked.blocked_code.is_some() { return Err("shared_mcp_privacy"); }
        let key = (connection.id.clone(), session_id, request_key);
        let request_cancel = Cancellation::default();
        let (receiver, preview, lease, connection_cancel) = {
            let mut state = self.state.lock().map_err(|_| "shared_mcp_state")?;
            Self::check_connection(&state, &connection)?;
            let current = state.connections.get(&connection.id).ok_or("shared_mcp_auth")?;
            let connection_cancel = current.cancellation.clone();
            if !state.enabled || !current.view.active || connection_cancel.is_cancelled() || session_cancel.is_cancelled() { return Err("shared_mcp_cancelled"); }
            if state.pending.len() >= MAX_PENDING || state.active.keys().filter(|active| active.0 == connection.id).count() >= 4 || state.active.contains_key(&key) { return Err("shared_mcp_limit"); }
            let preview = self.plugins.preview_tool_call(&tool.plugin_id, &tool.client, &tool.tool.name, checked.redacted.clone()).map_err(|_| "shared_mcp_tool_input")?;
            let (sender, receiver) = oneshot::channel();
            let view = SharedToolApproval { id: preview.preview_id.clone(), owner: connection.owner.clone(), clients: connection.clients.clone(),
                preview: preview.clone(), arguments: checked.redacted, privacy: checked.report };
            state.active.insert(key.clone(), request_cancel.clone());
            state.pending.insert(view.id.clone(), PendingApproval { key: key.clone(), view, sender, deadline: Instant::now() + APPROVAL_TTL });
            let lease = CallLease { manager: self.clone(), key, preview_id: preview.preview_id.clone() };
            (receiver, preview, lease, connection_cancel)
        };
        let _lease = lease;
        let result = tokio::select! { biased;
            _ = connection_cancel.cancelled() => return Err("shared_mcp_cancelled"),
            _ = session_cancel.cancelled() => return Err("shared_mcp_cancelled"),
            _ = request_cancel.cancelled() => return Err("shared_mcp_cancelled"),
            result = tokio::time::timeout(APPROVAL_TTL, receiver) => result,
        };
        match result {
            Ok(Ok(true)) => {},
            Ok(Ok(false)) => return Err("shared_mcp_denied"),
            Ok(Err(_)) => return Err("shared_mcp_cancelled"),
            Err(_) => return Err("shared_mcp_approval_timeout"),
        }
        if !self.tools(&connection)?.iter().any(|current| current.plugin_id == tool.plugin_id && current.digest == preview.plugin_digest && current.tool.name == tool.tool.name) {
            return Err("shared_mcp_plugins");
        }
        let confirmation = ToolCallConfirmation { preview_id: preview.preview_id, plugin_digest: preview.plugin_digest,
            arguments_digest: preview.arguments_digest, accepted_permissions: preview.required_permissions, confirmed: true };
        let mut execution = Box::pin(self.plugins.execute_tool_for_clients(&connection.clients, confirmation));
        let guarded_execution = poll_fn(|context| {
            let Ok(_connection_gate) = connection_cancel.0.gate.lock() else { return Poll::Ready(Err("shared_mcp_state")); };
            let Ok(_session_gate) = session_cancel.0.gate.lock() else { return Poll::Ready(Err("shared_mcp_state")); };
            let Ok(_request_gate) = request_cancel.0.gate.lock() else { return Poll::Ready(Err("shared_mcp_state")); };
            if connection_cancel.is_cancelled() || session_cancel.is_cancelled() || request_cancel.is_cancelled() {
                return Poll::Ready(Err("shared_mcp_cancelled"));
            }
            let active = self.state.lock().map_err(|_| "shared_mcp_state")
                .and_then(|state| Self::check_connection(&state, &connection));
            if let Err(code) = active { return Poll::Ready(Err(code)); }
            execution.as_mut().poll(context).map(Ok)
        });
        let result = tokio::select! { biased;
            _ = connection_cancel.cancelled() => return Err("shared_mcp_cancelled"),
            _ = session_cancel.cancelled() => return Err("shared_mcp_cancelled"),
            _ = request_cancel.cancelled() => return Err("shared_mcp_cancelled"),
            result = tokio::time::timeout(Duration::from_secs(15), guarded_execution) => result,
        }.map_err(|_| "shared_mcp_timeout")??.map_err(|_| "shared_mcp_tool_failed")?;
        let checked = privacy::inspect(result.value, &settings, &[known_secret.as_str()]).map_err(|_| "shared_mcp_privacy")?;
        if checked.blocked_code.is_some() { return Err("shared_mcp_privacy"); }
        let text = serde_json::to_string(&checked.redacted).map_err(|_| "shared_mcp_privacy")?;
        if text.len() > MAX_REPLY_BYTES { return Err("shared_mcp_limit"); }
        Ok(json!({"content":[{"type":"text", "text":text}], "isError":false}))
    }
}

#[derive(serde::Serialize)]
struct BridgeCredentialRef<'a> { endpoint: &'a str, token: &'a str }

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct BridgeCredential {
    endpoint: String,
    #[serde(deserialize_with = "deserialize_secret")]
    token: Zeroizing<String>,
}

fn deserialize_secret<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Zeroizing<String>, D::Error> {
    <String as serde::Deserialize>::deserialize(deserializer).map(Zeroizing::new)
}

pub(crate) fn bridge_credential(raw: &str) -> Result<(reqwest::Url, Zeroizing<String>), String> {
    if raw.len() > 8192 { return Err("shared_mcp_credentials".into()); }
    let value: BridgeCredential = serde_json::from_str(raw).map_err(|_| "shared_mcp_credentials")?;
    let url = reqwest::Url::parse(&value.endpoint).map_err(|_| "shared_mcp_credentials")?;
    let id = url.path().strip_prefix("/mcp/").ok_or("shared_mcp_credentials")?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") || url.port().is_none()
        || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some() || !valid_connection_id(id)
        || value.token.len() != 64 || !value.token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("shared_mcp_credentials".into());
    }
    Ok((url, value.token))
}
