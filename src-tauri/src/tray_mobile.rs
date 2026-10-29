//! Mobile platforms have no system tray. Provider changes still use normal IPC.
pub fn refresh(_: &tauri::AppHandle, _: &crate::database::Database) {}
