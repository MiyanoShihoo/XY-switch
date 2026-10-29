use crate::takeover::{AppTakeoverManager, AppTakeoverStatus};
use tauri::State;

#[tauri::command]
pub fn list_app_takeovers(manager: State<'_, AppTakeoverManager>) -> Vec<AppTakeoverStatus> {
    if cfg!(mobile) { Vec::new() } else { manager.statuses() }
}

#[tauri::command]
pub async fn set_app_takeover_enabled(
    manager: State<'_, AppTakeoverManager>,
    app: String,
    enabled: bool,
) -> Result<AppTakeoverStatus, String> {
    crate::platform::require_desktop("客户端接管")?;
    manager.set_enabled(&app, enabled).await
}
