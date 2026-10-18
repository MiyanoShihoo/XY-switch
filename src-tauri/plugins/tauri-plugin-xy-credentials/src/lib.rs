//! Private Rust-to-Android bridge; no credential-read command is exported to JS.
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tauri::{plugin::{Builder, PluginHandle, TauriPlugin}, Wry};

static HANDLE: OnceLock<PluginHandle<Wry>> = OnceLock::new();
#[derive(Serialize)]
struct Request<'a> { operation: &'a str, target: &'a str, value: Option<&'a str> }
#[derive(Deserialize)]
struct Response { value: Option<String> }

pub fn init() -> TauriPlugin<Wry> {
    Builder::new("xy-credentials").setup(|_, api| {
        #[cfg(target_os = "android")]
        HANDLE.set(api.register_android_plugin("com.xyswitch.credentials", "CredentialPlugin")?)
            .map_err(|_| "Android credential plugin already initialized")?;
        #[cfg(not(target_os = "android"))]
        let _ = api;
        Ok(())
    }).invoke_handler(|invoke| {
        invoke.resolver.reject("Credential operations are private to the Rust backend");
        true
    }).build()
}
fn access(operation: &str, target: &str, value: Option<&str>) -> Result<Option<String>, String> {
    let handle = HANDLE.get().ok_or("Android Keystore plugin is not initialized")?;
    #[cfg(target_os = "android")]
    { handle.run_mobile_plugin::<Response>("access", Request { operation, target, value })
        .map(|response| response.value).map_err(|error| format!("Android Keystore: {error}")) }
    #[cfg(not(target_os = "android"))]
    { let _ = (handle, operation, target, value); Err("Android Keystore requires Android".into()) }
}
pub fn set(target: &str, value: &str) -> Result<(), String> { access("set", target, Some(value)).map(|_| ()) }
pub fn get(target: &str) -> Result<Option<String>, String> { access("get", target, None) }
pub fn delete(target: &str) -> Result<(), String> { access("delete", target, None).map(|_| ()) }
