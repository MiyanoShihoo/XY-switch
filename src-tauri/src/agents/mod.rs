mod codex;
mod collector;
mod harness;
pub mod models;
mod privacy;
mod projection;
mod store;
#[cfg(test)]
mod tests;

use crate::database::Database;
use collector::{Collector, SourceRoot};
use models::*;
use projection::Projection;
use std::path::Path;
use std::sync::{Arc, Mutex, Weak, atomic::{AtomicBool, Ordering}};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use store::EventStore;
use tauri::Emitter;

pub fn now_ms() -> i64 { SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().min(i64::MAX as u128) as i64 }

struct ViewState {
    projection: Projection,
    sources: Vec<AgentSource>,
    history: AgentHistory,
    warnings: Vec<String>,
    revision: u64,
}

struct Shared {
    store: EventStore,
    roots: Vec<SourceRoot>,
    view: Mutex<ViewState>,
    started: AtomicBool,
}

#[derive(Clone)]
pub struct AgentMonitor { shared: Arc<Shared> }

pub struct AgentMonitorState(pub Result<AgentMonitor, String>);

#[derive(Clone, Copy, Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentEventScope {
    pub at: Option<i64>,
}

impl AgentMonitorState {
    fn ready(&self) -> Result<AgentMonitor, String> {
        self.0.clone().map_err(|error| format!("Agent 观测初始化失败：{}", privacy::text(&error, 160)))
    }
}

fn source_roots(home: &Path, codex_home: &Path) -> Result<Vec<SourceRoot>, String> {
    #[cfg(debug_assertions)]
    let isolated = crate::debug_path_override("XY_SWITCH_TEST_HOME_DIR").is_some();
    #[cfg(not(debug_assertions))]
    let isolated = false;
    let harness_home = if isolated { home.join(".dsh") } else { crate::skills_local::agent_home(home, "DSH_HOME", ".dsh")? };
    let mut codex_sessions = codex_home.join("sessions");
    let mut codex_archive = codex_home.join("archived_sessions");
    let mut harness_sessions = harness_home.join("sessions");
    #[cfg(debug_assertions)]
    {
        if let Some(root) = crate::debug_path_override("XY_SWITCH_TEST_AGENT_CODEX_ROOT") { codex_sessions = root; }
        if let Some(root) = crate::debug_path_override("XY_SWITCH_TEST_AGENT_CODEX_ARCHIVE_ROOT") { codex_archive = root; }
        if let Some(root) = crate::debug_path_override("XY_SWITCH_TEST_AGENT_DSH_ROOT") { harness_sessions = root; }
    }
    Ok(vec![
        SourceRoot { id: "codex-sessions".into(), client_id: "codex".into(), path: codex_sessions, adapter: codex::parse_record },
        SourceRoot { id: "codex-archives".into(), client_id: "codex".into(), path: codex_archive, adapter: codex::parse_record },
        SourceRoot { id: "deepseek-harness".into(), client_id: "deepseek_harness".into(), path: harness_sessions, adapter: harness::parse_record },
    ])
}

fn capabilities(source: &mut AgentSource) {
    let history = source.id == "codex-archives";
    let mode = if history || source.mode == "history" { "history" } else { "delayed" };
    if history { source.name = "Codex 已归档日志".into(); source.mode = "history".into(); }
    for (feature, reason) in [
        ("relationships", "仅使用明确的父线程/子会话元数据与协作工具接收者 ID"),
        ("tasks", "依据结构化任务或回合生命周期；缺失字段保持未知"),
        ("tools", "显示明确的调用与返回状态，不保存原始参数和输出"),
        ("messages", "仅公开消息的脱敏短摘要，不读取推理内容"),
        ("tokens", "逐响应去重或可靠累计差分；输入缓存及推理是子集"),
        ("context", "最近请求输入与来源报告窗口，不代表实时内部状态"),
    ] { source.capabilities.push(AgentCapability { feature: feature.into(), mode: mode.into(), reason: reason.into() }); }
    source.capabilities.push(AgentCapability { feature: "actual_billing".into(), mode: "unsupported".into(), reason: "当前日志没有账单数据；有精确本地模型价格时显示估算，否则费用未知".into() });
    source.capabilities.push(AgentCapability { feature: "internal_state".into(), mode: "unsupported".into(), reason: "客户端隐藏状态与无结构化记录的通信不可见".into() });
}

impl AgentMonitor {
    pub fn new(database: Database, home: &Path, codex_home: &Path) -> Result<Self, String> {
        Self::with_roots(database, source_roots(home, codex_home)?)
    }

    fn with_roots(database: Database, roots: Vec<SourceRoot>) -> Result<Self, String> {
        let store = EventStore::new(database)?;
        let history = store.history()?;
        let sources = roots.iter().map(|root| {
            let mut source = AgentSource { id: root.id.clone(), client_id: root.client_id.clone(),
                name: if root.client_id == "codex" { "Codex 会话日志" } else { "DeepSeek Harness 会话日志" }.into(),
                connection: "unavailable".into(), mode: "delayed".into(), detail: "等待首次只读检查。".into(), ..Default::default() };
            capabilities(&mut source); source
        }).collect();
        Ok(Self { shared: Arc::new(Shared { store, roots, started: AtomicBool::new(false),
            view: Mutex::new(ViewState { projection: Projection::default(), sources, history,
                warnings: vec!["正在载入已保存记录并检查本地日志。".into()], revision: 0 }) }) })
    }

    pub fn ensure_started(&self, app: tauri::AppHandle) -> Result<(), String> {
        if self.shared.started.compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire).is_err() { return Ok(()); }
        let weak = Arc::downgrade(&self.shared);
        if std::thread::Builder::new().name("agent-observation".into()).spawn(move || run(weak, app)).is_err() {
            self.shared.started.store(false, Ordering::Release);
            return Err("无法启动 Agent 只读采集器".into());
        }
        Ok(())
    }

    pub fn snapshot(&self, query: AgentSnapshotQuery) -> Result<AgentSnapshot, String> {
        if query.at.is_some_and(|time| time < 0 || time > now_ms().saturating_add(60_000)) { return Err("历史回放时间无效".into()); }
        let replay = query.at.map(|at| self.shared.store.load_events(Some(at)).map(|events| Projection::from_events(&events))).transpose()?;
        let state = self.shared.view.lock().map_err(|_| "Agent 观测视图暂不可用")?;
        Ok(replay.as_ref().unwrap_or(&state.projection).snapshot(&query, state.sources.clone(), state.history.clone(), state.revision, now_ms(), state.warnings.clone()))
    }

    pub fn events(&self, query: AgentEventQuery) -> Result<AgentEventPage, String> {
        let at = query.until;
        self.event_page(query, at)
    }

    pub fn events_in_scope(&self, query: AgentEventQuery, scope: AgentEventScope) -> Result<AgentEventPage, String> {
        if scope.at.is_some_and(|time| time < 0 || time > now_ms().saturating_add(60_000)) { return Err("历史回放时间无效".into()); }
        self.event_page(query, scope.at)
    }

    fn event_page(&self, mut query: AgentEventQuery, at: Option<i64>) -> Result<AgentEventPage, String> {
        if let Some(at) = at { query.until = Some(query.until.map_or(at, |until| until.min(at))); }
        let members = if let Some(team) = &query.team_id {
            Some(if let Some(at) = at {
                Projection::from_events(&self.shared.store.load_events(Some(at))?).members(team)
            } else {
                let state = self.shared.view.lock().map_err(|_| "Agent 观测视图暂不可用")?;
                state.projection.members(team)
            })
        } else { None };
        self.shared.store.event_page(&query, members.as_deref())
    }

    pub fn set_retention(&self, days: u32) -> Result<(), String> { self.shared.store.set_retention_days(days) }
}

fn run(weak: Weak<Shared>, app: tauri::AppHandle) {
    let mut collector = None;
    loop {
        let Some(shared) = weak.upgrade() else { return; };
        let result = (|| -> Result<bool, String> {
            if collector.is_none() {
                shared.store.prune(now_ms())?;
                let projection = Projection::from_events(&shared.store.load_events(None)?);
                let mut state = shared.view.lock().map_err(|_| "Agent 视图暂不可用")?;
                state.projection = projection;
                collector = Some(Collector::new(shared.roots.clone(), shared.store.clone())?);
            }
            let mut result = collector.as_mut().expect("initialized collector").poll(now_ms())?;
            for source in &mut result.sources { capabilities(source); }
            let history = shared.store.history()?;
            let rebuilt = if result.pruned_events > 0 { Some(Projection::from_events(&shared.store.load_events(None)?)) } else { None };
            let mut state = shared.view.lock().map_err(|_| "Agent 视图暂不可用")?;
            let changed = result.changed || state.revision == 0 || state.history.retention_days != history.retention_days || state.warnings != result.warnings;
            if let Some(projection) = rebuilt { state.projection = projection; } else { state.projection.extend(&result.events); }
            state.sources = result.sources; state.warnings = result.warnings; state.history = history;
            if changed { state.revision = state.revision.saturating_add(1); }
            Ok(changed)
        })();
        let changed = match result {
            Ok(changed) => changed,
            Err(error) => {
                let warning = format!("采集暂不可用，将重试：{}", privacy::text(&error, 160));
                if let Ok(mut state) = shared.view.lock() {
                    let changed = state.warnings != [warning.clone()];
                    state.warnings = vec![warning];
                    for source in &mut state.sources { source.connection = "disconnected".into(); }
                    if changed { state.revision = state.revision.saturating_add(1); }
                    changed
                } else { false }
            }
        };
        if changed { let _ = app.emit("agent-observation-changed", ()); }
        drop(shared);
        std::thread::sleep(Duration::from_secs(2));
    }
}

#[tauri::command]
pub async fn get_agent_snapshot(app: tauri::AppHandle, monitor: tauri::State<'_, AgentMonitorState>, query: Option<AgentSnapshotQuery>) -> Result<AgentSnapshot, String> {
    crate::platform::require_desktop("Agent 编队观测")?;
    let monitor = monitor.ready()?;
    monitor.ensure_started(app)?;
    tauri::async_runtime::spawn_blocking(move || monitor.snapshot(query.unwrap_or_default())).await.map_err(|_| "Agent 快照读取中断".to_string())?
}

#[tauri::command]
pub async fn get_agent_events(app: tauri::AppHandle, monitor: tauri::State<'_, AgentMonitorState>, query: AgentEventQuery, scope: Option<AgentEventScope>) -> Result<AgentEventPage, String> {
    crate::platform::require_desktop("Agent 编队观测")?;
    let monitor = monitor.ready()?;
    monitor.ensure_started(app)?;
    tauri::async_runtime::spawn_blocking(move || match scope {
        Some(scope) => monitor.events_in_scope(query, scope),
        None => monitor.events(query),
    }).await.map_err(|_| "Agent 事件读取中断".to_string())?
}

#[tauri::command]
pub async fn set_agent_retention(app: tauri::AppHandle, monitor: tauri::State<'_, AgentMonitorState>, retention_days: u32) -> Result<(), String> {
    crate::platform::require_desktop("Agent 编队观测")?;
    let monitor = monitor.ready()?;
    monitor.ensure_started(app)?;
    tauri::async_runtime::spawn_blocking(move || monitor.set_retention(retention_days)).await.map_err(|_| "Agent 保留设置中断".to_string())?
}
