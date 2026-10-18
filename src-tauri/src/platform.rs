use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlatformCapabilities {
    pub os: &'static str,
    pub mobile: bool,
    pub system_tray: bool,
    pub autostart: bool,
    pub client_takeover: bool,
    pub credential_store: &'static str,
}

pub fn credential_store_name() -> &'static str {
    match std::env::consts::OS {
        "windows" => "Windows Credential Manager",
        "linux" => "Secret Service",
        "macos" | "ios" => "Apple Keychain",
        _ => "Unavailable",
    }
}

pub fn require_desktop(operation: &str) -> Result<(), String> {
    if cfg!(mobile) {
        Err(format!("{operation}仅适用于桌面端；iOS 不能访问其他应用的 CLI 配置或会话目录"))
    } else {
        Ok(())
    }
}

#[tauri::command]
pub fn app_platform(app: tauri::AppHandle) -> PlatformCapabilities {
    #[cfg(desktop)]
    let system_tray = crate::tray::is_available(&app);
    #[cfg(mobile)]
    let system_tray = { let _ = app; false };
    PlatformCapabilities {
        os: std::env::consts::OS,
        mobile: cfg!(mobile),
        system_tray,
        autostart: cfg!(windows),
        client_takeover: cfg!(desktop),
        credential_store: credential_store_name(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_backend_matches_the_compilation_target() {
        #[cfg(windows)] assert_eq!(credential_store_name(), "Windows Credential Manager");
        #[cfg(target_os = "linux")] assert_eq!(credential_store_name(), "Secret Service");
        #[cfg(any(target_os = "ios", target_os = "macos"))] assert_eq!(credential_store_name(), "Apple Keychain");
    }
    #[test]
    fn desktop_operations_are_not_advertised_on_mobile() {
        assert_eq!(require_desktop("客户端接管").is_ok(), !cfg!(mobile));
    }
}
