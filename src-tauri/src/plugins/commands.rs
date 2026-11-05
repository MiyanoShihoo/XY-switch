use super::{PluginManager, types::*, validation::strict_json};
use tauri::State;

fn key_input(text: &str) -> PluginResult<TrustedKeyInput> {
    serde_json::from_value(strict_json(text, 2048)?).map_err(|_| PluginError::new("plugin_schema"))
}

#[tauri::command]
pub fn plugins_validate_package(
    manager: State<'_, PluginManager>,
    package_json: String,
) -> PluginResult<PackageValidation> {
    manager.validate_local_package(&package_json)
}

#[tauri::command]
pub fn plugins_preview_trusted_key(
    manager: State<'_, PluginManager>,
    key_json: String,
) -> PluginResult<TrustedSigningKey> {
    manager.preview_trusted_key(&key_input(&key_json)?)
}

#[tauri::command]
pub fn plugins_configure_trusted_key(
    manager: State<'_, PluginManager>,
    key_json: String,
    fingerprint: String,
    confirmed: bool,
) -> PluginResult<TrustedSigningKey> {
    manager.configure_trusted_key(key_input(&key_json)?, &fingerprint, confirmed)
}

#[tauri::command]
pub fn plugins_list_trusted_keys(
    manager: State<'_, PluginManager>,
) -> PluginResult<Vec<TrustedSigningKey>> {
    manager.list_trusted_keys()
}

#[tauri::command]
pub fn plugins_revoke_trusted_key(
    manager: State<'_, PluginManager>,
    id: String,
) -> PluginResult<()> {
    manager.revoke_trusted_key(&id)
}

#[tauri::command]
pub fn plugins_preview_install(
    manager: State<'_, PluginManager>,
    published_json: String,
) -> PluginResult<InstallPreview> {
    manager.preview_install_json(&published_json)
}

#[tauri::command]
pub fn plugins_preview_installed(
    manager: State<'_, PluginManager>,
    id: String,
) -> PluginResult<InstallPreview> {
    manager.preview_installed(&id)
}

#[tauri::command]
pub fn plugins_confirm_install(
    manager: State<'_, PluginManager>,
    confirmation_json: String,
) -> PluginResult<InstalledPlugin> {
    manager.confirm_install_json(&confirmation_json)
}

#[tauri::command]
pub fn plugins_list_installed(
    manager: State<'_, PluginManager>,
) -> PluginResult<Vec<InstalledPlugin>> {
    manager.list_installed()
}

#[tauri::command]
pub fn plugins_disable(
    manager: State<'_, PluginManager>,
    id: String,
    client: Option<String>,
) -> PluginResult<InstalledPlugin> {
    manager.disable_plugin(&id, client.as_deref())
}

#[tauri::command]
pub fn plugins_enable_client(
    manager: State<'_, PluginManager>,
    id: String,
    client: String,
    digest: String,
    confirmed: bool,
) -> PluginResult<InstalledPlugin> {
    manager.enable_plugin_client(&id, &client, &digest, confirmed)
}

#[tauri::command]
pub fn plugins_uninstall(
    manager: State<'_, PluginManager>,
    id: String,
    confirmed: bool,
) -> PluginResult<InstalledPlugin> {
    manager.uninstall_plugin(&id, confirmed)
}

#[tauri::command]
pub fn plugins_preview_rollback(
    manager: State<'_, PluginManager>,
    id: String,
    version: String,
) -> PluginResult<InstallPreview> {
    manager.preview_rollback(&id, &version)
}

#[tauri::command]
pub fn plugins_apply_withdrawal(
    manager: State<'_, PluginManager>,
    withdrawal_json: String,
) -> PluginResult<InstalledPlugin> {
    manager.apply_withdrawal_json(&withdrawal_json)
}

#[tauri::command]
pub fn plugins_tools(
    manager: State<'_, PluginManager>,
    client: String,
) -> PluginResult<Vec<VerifiedTool>> {
    manager.tools_for_client(&client)
}

#[tauri::command]
pub fn plugins_skills(
    manager: State<'_, PluginManager>,
    client: String,
) -> PluginResult<Vec<VerifiedSkill>> {
    manager.skills_for_client(&client)
}

#[tauri::command]
pub fn plugins_provider_mappings(
    manager: State<'_, PluginManager>,
    client: String,
) -> PluginResult<Vec<VerifiedProviderMapping>> {
    manager.provider_mappings_for_client(&client)
}

#[tauri::command]
pub fn plugins_preview_tool(
    manager: State<'_, PluginManager>,
    id: String,
    client: String,
    tool: String,
    arguments_json: String,
) -> PluginResult<ToolCallPreview> {
    manager.preview_tool_call_json(&id, &client, &tool, &arguments_json)
}

#[tauri::command]
pub async fn plugins_execute_tool(
    manager: State<'_, PluginManager>,
    client: String,
    confirmation_json: String,
) -> PluginResult<ToolExecutionResult> {
    manager.execute_tool_json(&client, &confirmation_json).await
}
