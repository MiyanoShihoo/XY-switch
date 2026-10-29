//! Linux Secret Service and Apple Keychain. Never fall back to plaintext storage.
use super::{MAX_API_KEY_BYTES, TARGET_PREFIX, credential_target, scoped_credential_target};
use keyring::{Entry, Error};
use zeroize::Zeroizing;

fn entry(target: &str) -> Result<Entry, String> {
    Entry::new(TARGET_PREFIX, target).map_err(store_error)
}

fn store_error(error: Error) -> String {
    format!("native credential store unavailable; unlock your system keychain or Secret Service: {error}")
}

fn set(target: String, value: &str) -> Result<(), String> {
    if value.is_empty() {
        return Err("credential cannot be empty".to_string());
    }
    if value.len() > MAX_API_KEY_BYTES {
        return Err(format!("credential exceeds {MAX_API_KEY_BYTES} bytes"));
    }
    entry(&target)?.set_password(value).map_err(store_error)
}

fn get(target: String) -> Result<Option<String>, String> {
    match entry(&target)?.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(Error::NoEntry) => Ok(None),
        Err(error) => Err(store_error(error)),
    }
}

fn delete(target: String) -> Result<(), String> {
    match entry(&target)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(error) => Err(store_error(error)),
    }
}

pub fn set_api_key(provider_id: &str, value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() { return Err("api key cannot be empty".to_string()); }
    set(credential_target(provider_id)?, value)
}
pub fn has_api_key(provider_id: &str) -> Result<bool, String> {
    Ok(get(credential_target(provider_id)?)?.map(Zeroizing::new).is_some())
}
pub fn get_api_key(provider_id: &str) -> Result<Option<String>, String> {
    get(credential_target(provider_id)?)
}
pub fn delete_api_key(provider_id: &str) -> Result<(), String> {
    delete(credential_target(provider_id)?)
}
pub fn set_secret(scope: &str, item_id: &str, value: &str) -> Result<(), String> {
    set(scoped_credential_target(scope, item_id)?, value)
}
pub fn get_secret(scope: &str, item_id: &str) -> Result<Option<String>, String> {
    get(scoped_credential_target(scope, item_id)?)
}
pub fn delete_secret(scope: &str, item_id: &str) -> Result<(), String> {
    delete(scoped_credential_target(scope, item_id)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_invalid_input_before_contacting_the_keychain() {
        assert!(set_api_key("provider", "  ").is_err());
        assert!(set_api_key("../provider", "fixture").is_err());
        assert!(set_secret("unknown", "item", "fixture").is_err());
        assert!(set_secret("mcp", "item", "").is_err());
        assert!(set_secret("mcp", "item", &"x".repeat(MAX_API_KEY_BYTES + 1)).is_err());
    }

    #[test]
    #[ignore = "requires an unlocked native keychain / Secret Service session"]
    fn native_keychain_round_trip() {
        let id = format!("test-{}", uuid::Uuid::new_v4().simple());
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = delete_api_key(&self.0);
                let _ = delete_secret("mcp", &self.0);
            }
        }
        let _cleanup = Cleanup(id.clone());
        assert!(!has_api_key(&id).unwrap());
        set_api_key(&id, "  xy-switch-test-secret  ").unwrap();
        assert!(has_api_key(&id).unwrap());
        assert_eq!(get_api_key(&id).unwrap().as_deref(), Some("xy-switch-test-secret"));
        set_api_key(&id, "xy-switch-updated-fixture").unwrap();
        assert_eq!(get_api_key(&id).unwrap().as_deref(), Some("xy-switch-updated-fixture"));
        set_secret("mcp", &id, "fixture-header").unwrap();
        assert_eq!(get_secret("mcp", &id).unwrap().as_deref(), Some("fixture-header"));
        delete_api_key(&id).unwrap();
        delete_api_key(&id).unwrap();
        assert!(!has_api_key(&id).unwrap());
        delete_secret("mcp", &id).unwrap();
        assert_eq!(get_secret("mcp", &id).unwrap(), None);
    }
}
