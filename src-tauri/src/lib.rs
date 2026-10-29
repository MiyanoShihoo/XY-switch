#[cfg(test)]
mod acl_regression_tests;
mod adapters;
mod commands;
mod credential_store;
mod database;
mod governance;
mod mcp;
mod model_audit;
mod model_pricing;
mod model_audit_commands;
mod model_verification;
mod network;
mod response_model_probe;
mod preferences;
mod interface_language;
mod platform;
mod proxy;
mod proxy_manager;
mod proxy_request_guard;
mod sessions;
mod session_usage_codex;
mod session_usage_harness;
mod shadow_audit_commands;
mod shadow_audit_runtime;
mod skills_local;
mod streaming;
mod takeover;
mod tool_bridge;
#[cfg(desktop)]
mod tray;
#[cfg(mobile)]
#[path = "tray_mobile.rs"]
mod tray;
mod usage;
mod xai_responses;

use tauri::Manager;

// Generate the context once: on macOS this macro defines the embedded Info.plist symbol.
// ACL regression tests use this same factory without starting the application.
fn application_context() -> tauri::Context<tauri::Wry> {
    tauri::generate_context!()
}

#[cfg(debug_assertions)]
fn debug_path_override(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os(name)
        .filter(|value| !value.is_empty())
        .map(std::path::PathBuf::from)
}

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// 「高级设置」页展示依据当前用户环境动态推导的真实数据目录。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPaths {
    pub data_dir: String,
    pub database_path: String,
    pub home_dir: String,
    pub codex_home: String,
}

#[cfg_attr(target_os = "android", tauri::command(async))]
#[cfg_attr(not(target_os = "android"), tauri::command)]
fn app_paths(app: tauri::AppHandle) -> Result<AppPaths, String> {
    // 这里刻意重复 setup() 里的目录推导逻辑（含 debug 覆盖），
    // 保证界面显示的路径和实际打开的文件是同一个。
    #[cfg(debug_assertions)]
    let data_dir = match debug_path_override("XY_SWITCH_TEST_DATA_DIR") {
        Some(path) => path,
        None => app.path().app_data_dir().map_err(|e| e.to_string())?,
    };
    #[cfg(not(debug_assertions))]
    let data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    #[cfg(debug_assertions)]
    let home_dir = match debug_path_override("XY_SWITCH_TEST_HOME_DIR") {
        Some(path) => path,
        None => app.path().home_dir().map_err(|e| e.to_string())?,
    };
    #[cfg(not(debug_assertions))]
    let home_dir = app.path().home_dir().map_err(|e| e.to_string())?;

    let codex_home_override = std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty());
    #[cfg(debug_assertions)]
    let codex_home_override = if debug_path_override("XY_SWITCH_TEST_HOME_DIR").is_some() {
        None
    } else {
        codex_home_override
    };
    let codex_home = takeover::resolve_codex_home(&home_dir, codex_home_override)?;

    Ok(AppPaths {
        database_path: data_dir.join("xy-switch.db").to_string_lossy().into_owned(),
        data_dir: data_dir.to_string_lossy().into_owned(),
        home_dir: home_dir.to_string_lossy().into_owned(),
        codex_home: codex_home.to_string_lossy().into_owned(),
    })
}

#[cfg_attr(target_os = "android", tauri::command(async))]
#[cfg_attr(not(target_os = "android"), tauri::command)]
#[allow(deprecated)]
fn open_external_url(app: tauri::AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_shell::ShellExt;
    // 只允许打开 http(s) 链接：这个命令暴露给前端，
    // 如果放开 scheme，file:// 或自定义协议会变成本机命令执行入口。
    let trimmed = url.trim();
    let allowed = trimmed
        .split_once("://")
        .map(|(scheme, rest)| {
            matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https") && !rest.is_empty()
        })
        .unwrap_or(false);
    if !allowed {
        return Err("仅支持打开 http/https 链接".to_string());
    }
    app.shell().open(trimmed, None).map_err(|e| e.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "android")]
    let builder = builder.plugin(tauri_plugin_xy_credentials::init());
    builder
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            #[cfg(debug_assertions)]
            let data_dir = match debug_path_override("XY_SWITCH_TEST_DATA_DIR") {
                Some(path) => path,
                None => app
                    .path()
                    .app_data_dir()
                    .map_err(|error| error.to_string())?,
            };
            #[cfg(not(debug_assertions))]
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|error| error.to_string())?;
            std::fs::create_dir_all(&data_dir)?;
            let database = database::Database::open(&data_dir.join("xy-switch.db"))?;
            // 日志订阅器挪到这里初始化：等数据库打开后才知道用户选的等级。
            // 「设置 → 高级」里的日志等级以前完全没人读，恒定 info。
            // RUST_LOG 优先级更高，方便临时排查时覆盖界面上的选择。
            let log_filter = std::env::var("RUST_LOG")
                .unwrap_or_else(|_| preferences::load(&database).log_level);
            tracing_subscriber::fmt()
                .with_env_filter(tracing_subscriber::EnvFilter::new(log_filter.as_str()))
                .init();
            app.manage(database.clone());
            // 托盘创建失败（部分系统没有可用的通知区域）不应该让整个程序起不来，
            // 主窗口和代理功能与托盘无关。
            #[cfg(desktop)]
            if let Err(error) = tray::setup(app, database.clone()) {
                tracing::error!(%error, "failed to create tray icon; continuing without tray");
            }
            let proxy_manager = proxy_manager::ProxyManager::new(database.clone());
            proxy_manager.prepare_for_startup()?;
            #[cfg(debug_assertions)]
            let home_dir = match debug_path_override("XY_SWITCH_TEST_HOME_DIR") {
                Some(path) => path,
                None => app.path().home_dir().map_err(|error| error.to_string())?,
            };
            #[cfg(not(debug_assertions))]
            let home_dir = app.path().home_dir().map_err(|error| error.to_string())?;
            let mcp_manager = mcp::McpManager::new(database.clone(), home_dir.clone());
            let codex_home_override = std::env::var_os("CODEX_HOME").filter(|value| !value.is_empty());
            #[cfg(debug_assertions)]
            let codex_home_override = if debug_path_override("XY_SWITCH_TEST_HOME_DIR").is_some() {
                None
            } else {
                codex_home_override
            };
            let codex_home = takeover::resolve_codex_home(&home_dir, codex_home_override)?;
            let takeover_manager = takeover::AppTakeoverManager::new(
                database.clone(),
                proxy_manager.clone(),
                home_dir.clone(),
                data_dir,
            ).with_codex_home(codex_home);
            #[cfg(desktop)]
            takeover_manager.recover_on_startup();
            governance::monitor::start(database.clone(), proxy_manager.clone());
            app.manage(governance::mounts::MountManager::default());
            app.manage(proxy_manager);
            app.manage(takeover_manager);
            app.manage(mcp_manager);
            // 开机自启拉起时只驻留托盘：不显示主窗口，也不占任务栏。
            // tray::show_main_window 会在唤回时把 skip_taskbar 复位，
            // 所以这里可以放心把它设成 true。
            #[cfg(desktop)]
            if preferences::started_minimized()
                && tray::is_available(app.handle())
                && let Some(window) = app.get_webview_window("main")
            {
                if let Err(error) = window.set_skip_taskbar(true) {
                    tracing::error!(%error, "failed to hide taskbar button on silent start");
                }
                if let Err(error) = window.hide() {
                    tracing::error!(%error, "failed to start minimized to tray");
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            #[cfg(mobile)]
            let _ = (window, event);
            #[cfg(desktop)]
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                // 以前这里无条件隐藏窗口，「关闭窗口时最小化到系统托盘」那个开关
                // 关掉也照样隐藏，用户只能去托盘里点退出。现在按偏好决定：
                // 关掉开关就走正常关闭流程（最后一个窗口关掉后进程退出）。
                let minimize_to_tray = window
                    .app_handle()
                    .try_state::<database::Database>()
                    .map(|database| preferences::load(&database).minimize_to_tray)
                    // 读不到状态时按老行为隐藏，至少不会让人以为数据没保存就退了。
                    .unwrap_or(true);
                if minimize_to_tray && tray::is_available(window.app_handle()) {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_version,
            governance::access::get_access_policy,
            governance::access::save_access_policy,
            governance::access::create_access_token,
            governance::access::list_access_tokens,
            governance::access::revoke_access_token,
            governance::budget::get_hard_budget,
            governance::budget::save_hard_budget,
            governance::versions::create_config_version,
            governance::versions::list_config_versions,
            governance::versions::delete_config_version,
            governance::versions::preview_config_rollback,
            governance::versions::apply_config_rollback,
            governance::monitor::get_governance_status,
            governance::monitor::get_synthetic_monitor,
            governance::monitor::save_synthetic_monitor,
            governance::monitor::run_synthetic_check,
            governance::monitor::list_synthetic_runs,
            governance::mounts::choose_mount_root,
            governance::mounts::preview_explicit_mount,
            governance::mounts::preview_mount_rollback,
            governance::mounts::apply_explicit_mount,
            governance::mounts::list_mount_backups,
            governance::mounts::delete_mount_backup,
            governance::governance_events,
            app_paths,
            platform::app_platform,
            interface_language::get_interface_language,
            interface_language::set_interface_language,
            preferences::get_app_preferences,
            preferences::save_app_preferences,
            preferences::get_autostart_enabled,
            preferences::set_autostart_enabled,
            usage::sync_local_usage,
            usage::get_usage_summary,
            usage::get_usage_logs,
            model_audit_commands::analyze_model_audit,
            model_audit_commands::generate_model_reference,
            model_audit_commands::load_model_reference,
            model_audit_commands::analyze_model_reference,
            shadow_audit_commands::get_shadow_audit_settings,
            shadow_audit_commands::save_shadow_audit_setting,
            shadow_audit_commands::analyze_shadow_audit,
            usage::get_usage_trends,
            usage::get_provider_stats,
            usage::get_model_stats,
            usage::list_model_pricing,
            usage::save_model_pricing,
            usage::delete_model_pricing,
            usage::sync_models_dev_pricing,
            commands::config::export_config,
            commands::config::import_config,
            commands::providers::list_providers,
            commands::providers::save_provider,
            commands::providers::reorder_providers,
            commands::providers::delete_provider,
            commands::providers::get_current_provider,
            commands::providers::set_current_provider,
            commands::providers::refresh_codex_provider,
            commands::providers::discover_provider_models,
            response_model_probe::probe_provider_response_model,
            model_verification::model_verification_status,
            model_verification::set_model_verification_settings,
            model_verification::confirm_model_verification_cost,
            commands::credentials::set_provider_api_key,
            commands::credentials::has_provider_api_key,
            commands::credentials::delete_provider_api_key,
            commands::health::list_provider_health,
            commands::health::request_summary,
            commands::health::test_provider_health,
            commands::health::test_all_provider_health,
            commands::proxy::get_proxy_status,
            commands::proxy::save_proxy_settings,
            commands::proxy::set_proxy_enabled,
            commands::takeover::list_app_takeovers,
            commands::takeover::set_app_takeover_enabled,
            commands::mcp::list_mcp_servers,
            commands::mcp::list_mcp_apps,
            commands::mcp::save_mcp_server,
            commands::mcp::delete_mcp_server,
            commands::mcp::set_mcp_app_enabled,
            commands::mcp::import_mcp_from_apps,
            commands::skills::get_skill_catalog,
            commands::skills::get_skill_import_settings,
            commands::skills::save_skill_import_settings,
            commands::skills::get_installed_skills,
            commands::skills::install_skill_unified,
            commands::skills::uninstall_skill_unified,
            commands::skills::toggle_skill_app,
            commands::skills::discover_available_skills,
            commands::skills::check_skill_updates,
            commands::skills::update_skill,
            commands::skills::import_skill_folder,
            commands::skills::pick_and_import_skill_folder,
            commands::sessions::get_session_catalog,
            commands::sessions::get_session_detail,
            commands::sessions::list_sessions,
            commands::sessions::get_session_messages,
            commands::sessions::delete_sessions,
            commands::prompts::list_prompts,
            commands::prompts::save_prompt,
            commands::prompts::delete_prompt,
            commands::prompts::set_prompt_enabled,
            open_external_url
        ])
        .build(application_context())
        .expect("failed to build XY Switch")
        .run(|app, event| {
            if matches!(event, tauri::RunEvent::ExitRequested { .. }) {
                let takeover = app.state::<takeover::AppTakeoverManager>().inner().clone();
                let proxy = app.state::<proxy_manager::ProxyManager>().inner().clone();
                // 退出顺序：先把被接管的应用配置还原回用户原始内容，再释放监听端口。
                // 还原失败只能记录日志——不能因此提前 return，否则代理端口会一直被占用，
                // 下次启动会因端口冲突起不来。
                if let Err(error) = takeover.disable_all() {
                    tracing::error!(%error, "failed to restore application configs during shutdown");
                }
                // 这里不能再调用 sync_all_active_apps：那会在刚刚还原好的配置上
                // 重新写入托管内容（直连模式下还会把明文 API Key 落盘）。
                tauri::async_runtime::block_on(proxy.stop());
            }
        });
}
