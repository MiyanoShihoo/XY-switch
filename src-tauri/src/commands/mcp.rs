use crate::mcp::{McpAppStatus, McpImportResult, McpManager, McpServer};
use tauri::State;

#[tauri::command]
pub fn list_mcp_servers(manager: State<'_, McpManager>) -> Result<Vec<McpServer>, String> {
    manager.list_servers()
}

#[tauri::command]
pub fn list_mcp_apps(manager: State<'_, McpManager>) -> Vec<McpAppStatus> {
    if cfg!(mobile) { Vec::new() } else { manager.app_statuses() }
}

#[tauri::command]
pub fn save_mcp_server(
    manager: State<'_, McpManager>,
    server: McpServer,
) -> Result<McpServer, String> {
    manager.save_server(server)
}

#[tauri::command]
pub fn delete_mcp_server(manager: State<'_, McpManager>, id: String) -> Result<(), String> {
    manager.delete_server(&id)
}

#[tauri::command]
pub fn set_mcp_app_enabled(
    manager: State<'_, McpManager>,
    server_id: String,
    app: String,
    enabled: bool,
) -> Result<McpServer, String> {
    crate::platform::require_desktop("MCP 客户端同步")?;
    manager.set_app_enabled(&server_id, &app, enabled)
}

#[tauri::command]
pub fn import_mcp_from_apps(manager: State<'_, McpManager>) -> Result<McpImportResult, String> {
    crate::platform::require_desktop("读取本机 MCP 配置")?;
    manager.import_from_apps()
}
