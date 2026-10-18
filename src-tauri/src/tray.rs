use crate::database::{Database, Provider};
use tauri::{
    App, AppHandle, Emitter, Manager,
    menu::{IsMenuItem, Menu, MenuItem, Submenu},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

const PROVIDER_PREFIX: &str = "provider:";
const SHOW_WINDOW_ID: &str = "show-window";
const QUIT_ID: &str = "quit";
const TRAY_ID: &str = "xy-switch-tray";

fn provider_menu_id(provider_id: &str) -> String {
    format!("{PROVIDER_PREFIX}{provider_id}")
}

fn provider_id_from_menu_id(menu_id: &str) -> Option<&str> {
    menu_id
        .strip_prefix(PROVIDER_PREFIX)
        .filter(|provider_id| !provider_id.is_empty())
}

fn enabled_providers(providers: Vec<Provider>) -> Vec<Provider> {
    providers
        .into_iter()
        .filter(|provider| provider.enabled)
        .collect()
}

pub fn is_available(app: &AppHandle) -> bool {
    app.tray_by_id(TRAY_ID).is_some()
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // 静默启动或"关闭到托盘"时，主窗口是以 skip_taskbar(true) 隐藏的。
        // 如果唤回时不先复位这个标志，窗口就没有任务栏按钮，一最小化就彻底找不回来。
        // 所有唤回入口（托盘菜单、托盘左键、二次启动、URL 打开事件）都应走这个函数。
        if let Err(error) = window.set_skip_taskbar(false) {
            tracing::error!(%error, "failed to restore taskbar button for main window");
        }
        if window.is_minimized().unwrap_or(false) {
            if let Err(error) = window.unminimize() {
                tracing::error!(%error, "failed to unminimize main window");
            }
        }
        if let Err(error) = window.show() {
            tracing::error!(%error, "failed to show main window from tray");
        }
        if let Err(error) = window.set_focus() {
            tracing::error!(%error, "failed to focus main window from tray");
        }
    }
}

fn build_provider_submenu<M: Manager<tauri::Wry>>(
    manager: &M,
    providers: &[Provider],
    current_id: Option<&str>,
) -> tauri::Result<Option<Submenu<tauri::Wry>>> {
    if providers.is_empty() {
        return Ok(None);
    }

    let mut items = Vec::with_capacity(providers.len());
    for provider in providers {
        let selected = current_id == Some(provider.id.as_str());
        let label = if selected {
            format!("✓ {}", provider.name)
        } else {
            provider.name.clone()
        };
        items.push(MenuItem::with_id(
            manager,
            provider_menu_id(&provider.id),
            label,
            true,
            None::<&str>,
        )?);
    }
    let item_refs: Vec<&dyn IsMenuItem<tauri::Wry>> = items
        .iter()
        .map(|item| item as &dyn IsMenuItem<tauri::Wry>)
        .collect();
    Ok(Some(Submenu::with_items(
        manager,
        "供应商",
        true,
        &item_refs,
    )?))
}

/// 按数据库里**当前**的供应商列表构建托盘菜单。
///
/// 之前菜单只在启动时构建一次，之后新增/改名/删除供应商，托盘里都看不到变化——
/// 新加的卡片在托盘里永远切不到，删掉的卡片点了还会报错。
fn build_menu<M: Manager<tauri::Wry>>(
    manager: &M,
    database: &Database,
) -> Result<Menu<tauri::Wry>, String> {
    let providers = enabled_providers(database.list_providers()?);
    let current = database.current_provider()?;
    let current_id = current.as_ref().map(|provider| provider.id.as_str());

    let show = MenuItem::with_id(manager, SHOW_WINDOW_ID, "显示主窗口", true, None::<&str>)
        .map_err(|error| error.to_string())?;
    let quit = MenuItem::with_id(manager, QUIT_ID, "退出 XY Switch", true, None::<&str>)
        .map_err(|error| error.to_string())?;

    let submenus = build_provider_submenu(manager, &providers, current_id)
        .map_err(|error| error.to_string())?;

    let mut menu_items: Vec<&dyn IsMenuItem<tauri::Wry>> = Vec::new();
    menu_items.push(&show);
    if let Some(submenu) = &submenus {
        menu_items.push(submenu);
    }
    menu_items.push(&quit);
    Menu::with_items(manager, &menu_items).map_err(|error| error.to_string())
}

/// 供应商发生变化后重建托盘菜单。托盘不存在（创建失败或系统不支持）时静默跳过。
pub fn refresh(app: &AppHandle, database: &Database) {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        return;
    };
    match build_menu(app, database) {
        Ok(menu) => {
            if let Err(error) = tray.set_menu(Some(menu)) {
                tracing::error!(%error, "failed to update tray menu");
            }
        }
        Err(error) => tracing::error!(%error, "failed to rebuild tray menu"),
    }
}

pub fn setup(app: &App, database: Database) -> Result<(), String> {
    let menu = build_menu(app, &database)?;

    let database_for_menu = database.clone();
    TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .tooltip("XY Switch")
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let menu_id = event.id().as_ref();
            if menu_id == SHOW_WINDOW_ID {
                show_main_window(app);
            } else if menu_id == QUIT_ID {
                app.exit(0);
            } else if let Some(provider_id) = provider_id_from_menu_id(menu_id) {
                let app = app.clone();
                let database = database_for_menu.clone();
                let provider_id = provider_id.to_string();
                tauri::async_runtime::spawn(async move {
                    let takeover = app
                        .state::<crate::takeover::AppTakeoverManager>()
                        .inner()
                        .clone();
                    match crate::commands::providers::select_provider(
                        &database,
                        &takeover,
                        &provider_id,
                    )
                    .await
                    {
                        Ok(()) => {
                            // 重建菜单，让 ✓ 跟着移动到刚切过去的那张卡片上。
                            refresh(&app, &database);
                            if let Err(error) = app.emit("provider-changed", provider_id) {
                                tracing::error!(%error, "failed to emit provider change from tray");
                            }
                        }
                        Err(error) => {
                            tracing::error!(%error, %provider_id, "tray provider switch failed");
                            let _ = app.emit("provider-switch-error", error);
                            show_main_window(&app);
                        }
                    }
                });
            }
        })
        .on_tray_icon_event(|tray, event| {
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)
        .map_err(|error| error.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{provider_id_from_menu_id, provider_menu_id};

    #[test]
    fn provider_menu_ids_round_trip_without_using_provider_names() {
        for id in ["openai", "duplicate-name-a", "provider:with:colons"] {
            let menu_id = provider_menu_id(id);
            assert_eq!(provider_id_from_menu_id(&menu_id), Some(id));
        }
    }

    #[test]
    fn unrelated_menu_ids_are_not_treated_as_provider_ids() {
        assert_eq!(provider_id_from_menu_id("show-window"), None);
        assert_eq!(provider_id_from_menu_id("provider:"), None);
    }
}
