use super::{SharedMcpManager, SharedMcpStatus, SharedMcpConnection, SharedToolApproval};
use crate::mcp::McpManager;
use tauri::State;

#[tauri::command]
pub fn shared_mcp_status(manager: State<'_, SharedMcpManager>) -> Result<SharedMcpStatus, String> { manager.status() }

#[tauri::command]
pub async fn shared_mcp_set_enabled(manager: State<'_, SharedMcpManager>, enabled: bool, confirmed: bool) -> Result<SharedMcpStatus, String> {
    manager.set_enabled(enabled, confirmed).await
}

#[tauri::command]
pub fn shared_mcp_register(manager: State<'_, SharedMcpManager>, mcp: State<'_, McpManager>, owner: String, confirmed: bool, shared_scope_confirmed: bool) -> Result<SharedMcpConnection, String> {
    manager.register(&mcp, &owner, confirmed, shared_scope_confirmed)
}

#[tauri::command]
pub fn shared_mcp_revoke(manager: State<'_, SharedMcpManager>, mcp: State<'_, McpManager>, id: String, confirmed: bool) -> Result<(), String> {
    manager.revoke(&mcp, &id, confirmed)
}

#[tauri::command]
pub fn shared_mcp_pending(manager: State<'_, SharedMcpManager>) -> Result<Vec<SharedToolApproval>, String> { manager.pending() }

#[tauri::command]
pub fn shared_mcp_decide(manager: State<'_, SharedMcpManager>, id: String, approved: bool, confirmed: bool) -> Result<(), String> {
    manager.decide(&id, approved, confirmed)
}
