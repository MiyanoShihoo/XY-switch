const TARGET_PREFIX: &str = "XY Switch";
const MAX_API_KEY_BYTES: usize = 2560;

fn credential_target(provider_id: &str) -> Result<String, String> {
    scoped_credential_target("provider", provider_id)
}

fn scoped_credential_target(scope: &str, item_id: &str) -> Result<String, String> {
    if !matches!(scope, "provider" | "mcp" | "internal") {
        return Err("invalid credential scope".to_string());
    }
    let id = item_id.trim();
    if id.is_empty()
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
    {
        return Err("invalid credential id".to_string());
    }
    Ok(format!("{TARGET_PREFIX}/{scope}/{id}"))
}

#[cfg(windows)]
mod platform {
    use super::{MAX_API_KEY_BYTES, credential_target, scoped_credential_target};
    use std::{io, ptr};
    use windows_sys::Win32::Security::Credentials::{
        CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW,
        CredDeleteW, CredFree, CredReadW, CredWriteW,
    };
    use zeroize::Zeroizing;

    const ERROR_NOT_FOUND: i32 = 1168;

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(Some(0)).collect()
    }

    fn windows_error(operation: &str) -> String {
        format!("{operation} failed: {}", io::Error::last_os_error())
    }

    struct CredentialGuard(*mut CREDENTIALW);

    impl Drop for CredentialGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { CredFree(self.0.cast()) };
            }
        }
    }

    fn set(target: String, value: &str) -> Result<(), String> {
        if value.is_empty() {
            return Err("credential cannot be empty".to_string());
        }
        if value.len() > MAX_API_KEY_BYTES || value.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err(format!("credential exceeds {MAX_API_KEY_BYTES} bytes"));
        }

        let mut target = wide(&target);
        let mut username = wide("XY Switch");
        let mut blob = Zeroizing::new(value.as_bytes().to_vec());
        let credential = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            CredentialBlobSize: blob.len() as u32,
            CredentialBlob: blob.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: username.as_mut_ptr(),
            ..Default::default()
        };

        let result = unsafe { CredWriteW(&credential, 0) };
        blob.fill(0);
        if result == 0 {
            return Err(windows_error("writing credential"));
        }
        Ok(())
    }

    fn has(target: String) -> Result<bool, String> {
        let target = wide(&target);
        let mut credential = ptr::null_mut();
        let result = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
        if result != 0 {
            drop(CredentialGuard(credential));
            return Ok(true);
        }
        match io::Error::last_os_error().raw_os_error() {
            Some(ERROR_NOT_FOUND) => Ok(false),
            _ => Err(windows_error("reading credential")),
        }
    }

    fn get(target: String) -> Result<Option<String>, String> {
        let target = wide(&target);
        let mut credential = ptr::null_mut();
        let result = unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut credential) };
        if result == 0 {
            return match io::Error::last_os_error().raw_os_error() {
                Some(ERROR_NOT_FOUND) => Ok(None),
                _ => Err(windows_error("reading credential")),
            };
        }

        let guard = CredentialGuard(credential);
        let credential = unsafe { &*guard.0 };
        let bytes = unsafe {
            std::slice::from_raw_parts(
                credential.CredentialBlob,
                credential.CredentialBlobSize as usize,
            )
        };
        String::from_utf8(bytes.to_vec())
            .map(Some)
            .map_err(|_| "stored api key is not valid UTF-8".to_string())
    }

    fn delete(target: String) -> Result<(), String> {
        let target = wide(&target);
        let result = unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) };
        if result != 0 {
            return Ok(());
        }
        match io::Error::last_os_error().raw_os_error() {
            Some(ERROR_NOT_FOUND) => Ok(()),
            _ => Err(windows_error("deleting credential")),
        }
    }

    pub fn set_api_key(provider_id: &str, api_key: &str) -> Result<(), String> {
        let key = api_key.trim();
        if key.is_empty() {
            return Err("api key cannot be empty".to_string());
        }
        set(credential_target(provider_id)?, key)
    }

    pub fn has_api_key(provider_id: &str) -> Result<bool, String> {
        has(credential_target(provider_id)?)
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
}

#[cfg(any(target_os = "linux", target_os = "macos", target_os = "ios"))]
#[path = "credential_store/native_keyring.rs"]
mod platform;

#[cfg(target_os = "android")]
#[path = "credential_store/android_keystore.rs"]
mod platform;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos", target_os = "ios", target_os = "android")))]
mod platform {
    pub fn set_api_key(_: &str, _: &str) -> Result<(), String> {
        Err("secure API key storage is unavailable on this platform".to_string())
    }
    pub fn has_api_key(_: &str) -> Result<bool, String> {
        Err("secure API key storage is unavailable on this platform".to_string())
    }
    pub fn get_api_key(_: &str) -> Result<Option<String>, String> {
        Err("secure API key storage is unavailable on this platform".to_string())
    }
    pub fn delete_api_key(_: &str) -> Result<(), String> {
        Err("secure API key storage is unavailable on this platform".to_string())
    }
    pub fn set_secret(_: &str, _: &str, _: &str) -> Result<(), String> {
        Err("secure credential storage is unavailable on this platform".to_string())
    }
    pub fn get_secret(_: &str, _: &str) -> Result<Option<String>, String> {
        Err("secure credential storage is unavailable on this platform".to_string())
    }
    pub fn delete_secret(_: &str, _: &str) -> Result<(), String> {
        Err("secure credential storage is unavailable on this platform".to_string())
    }
}

pub use platform::{
    delete_api_key, delete_secret, get_api_key, get_secret, has_api_key, set_api_key, set_secret,
};

#[cfg(test)]
mod tests {
    use super::{credential_target, scoped_credential_target};

    #[test]
    fn builds_scoped_target_for_valid_provider_id() {
        assert_eq!(
            credential_target("openai-main").unwrap(),
            "XY Switch/provider/openai-main"
        );
    }

    #[test]
    fn rejects_empty_or_unsafe_provider_ids() {
        assert!(credential_target("").is_err());
        assert!(credential_target("../openai").is_err());
        assert!(credential_target("provider id").is_err());
    }

    #[test]
    fn builds_scoped_mcp_target() {
        assert_eq!(
            scoped_credential_target("mcp", "context7.main").unwrap(),
            "XY Switch/mcp/context7.main"
        );
        assert!(scoped_credential_target("unknown", "context7").is_err());
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "requires an interactive Windows Credential Manager session"]
    fn stores_reads_and_deletes_windows_credential() {
        struct Cleanup(String);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let _ = super::delete_api_key(&self.0);
            }
        }

        let provider_id = format!("test-{}", uuid::Uuid::new_v4().simple());
        let _cleanup = Cleanup(provider_id.clone());
        let secret = "xy-switch-test-secret";

        super::set_api_key(&provider_id, secret).unwrap();
        assert!(super::has_api_key(&provider_id).unwrap());
        assert_eq!(
            super::get_api_key(&provider_id).unwrap().as_deref(),
            Some(secret)
        );
        super::delete_api_key(&provider_id).unwrap();
        assert!(!super::has_api_key(&provider_id).unwrap());
    }
}
