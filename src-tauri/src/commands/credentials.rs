use crate::credential_store;

#[tauri::command]
pub fn set_provider_api_key(provider_id: String, api_key: String) -> Result<(), String> {
    credential_store::set_api_key(&provider_id, &api_key)
}

#[tauri::command]
pub fn has_provider_api_key(provider_id: String) -> Result<bool, String> {
    credential_store::has_api_key(&provider_id)
}

#[tauri::command]
pub fn delete_provider_api_key(provider_id: String) -> Result<(), String> {
    credential_store::delete_api_key(&provider_id)
}
