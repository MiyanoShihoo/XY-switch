//! Android Keystore-backed AES-GCM records. No plaintext fallback.
use super::{MAX_API_KEY_BYTES, credential_target, scoped_credential_target};
use zeroize::Zeroizing;

fn set(target: String, value: &str) -> Result<(), String> {
    if value.is_empty() { return Err("credential cannot be empty".into()); }
    if value.len() > MAX_API_KEY_BYTES { return Err(format!("credential exceeds {MAX_API_KEY_BYTES} bytes")); }
    tauri_plugin_xy_credentials::set(&target, value)
}
pub fn set_api_key(id: &str, value: &str) -> Result<(), String> { set(credential_target(id)?, value.trim()) }
pub fn get_api_key(id: &str) -> Result<Option<String>, String> { tauri_plugin_xy_credentials::get(&credential_target(id)?) }
pub fn has_api_key(id: &str) -> Result<bool, String> { Ok(get_api_key(id)?.map(Zeroizing::new).is_some()) }
pub fn delete_api_key(id: &str) -> Result<(), String> { tauri_plugin_xy_credentials::delete(&credential_target(id)?) }
pub fn set_secret(scope: &str, id: &str, value: &str) -> Result<(), String> { set(scoped_credential_target(scope,id)?, value) }
pub fn get_secret(scope: &str, id: &str) -> Result<Option<String>, String> { tauri_plugin_xy_credentials::get(&scoped_credential_target(scope,id)?) }
pub fn delete_secret(scope: &str, id: &str) -> Result<(), String> { tauri_plugin_xy_credentials::delete(&scoped_credential_target(scope,id)?) }
