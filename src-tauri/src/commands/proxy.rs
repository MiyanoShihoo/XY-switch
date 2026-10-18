use crate::proxy_manager::{ProxyManager, ProxySettings, ProxyStatus};
use crate::takeover::AppTakeoverManager;
use tauri::State;

#[tauri::command]
pub async fn get_proxy_status(manager: State<'_, ProxyManager>) -> Result<ProxyStatus, String> {
    manager.status().await
}

#[tauri::command]
pub async fn save_proxy_settings(
    manager: State<'_, ProxyManager>,
    takeover: State<'_, AppTakeoverManager>,
    settings: ProxySettings,
) -> Result<ProxyStatus, String> {
    let current = manager.status().await?;
    let address_changed =
        current.settings.host != settings.host || current.settings.port != settings.port;
    if address_changed && takeover.has_active() {
        return Err("修改监听地址或端口前，请先关闭所有应用接管".to_string());
    }
    if !settings.enabled {
        takeover.disable_all()?;
    }
    let status = manager.apply(settings).await?;
    if status.running {
        takeover.sync_all_active_apps(Some(&status.address))?;
    }
    Ok(status)
}

#[tauri::command]
pub async fn set_proxy_enabled(
    manager: State<'_, ProxyManager>,
    takeover: State<'_, AppTakeoverManager>,
    enabled: bool,
) -> Result<ProxyStatus, String> {
    set_proxy_enabled_inner(manager.inner(), takeover.inner(), enabled).await
}

async fn set_proxy_enabled_inner(
    manager: &ProxyManager,
    takeover: &AppTakeoverManager,
    enabled: bool,
) -> Result<ProxyStatus, String> {
    if !enabled {
        takeover.disable_all()?;
    }
    let status = manager.set_enabled(enabled).await?;
    if enabled {
        takeover.sync_all_active_apps(Some(&status.address))?;
    }
    Ok(status)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use std::{fs, path::PathBuf};

    fn temp_root() -> PathBuf {
        std::env::temp_dir().join(format!("xy-switch-proxy-command-{}", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn stopping_proxy_restores_takeover_before_releasing_listener() {
        let root = temp_root();
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(&root.join("test.db")).unwrap();
        let proxy = ProxyManager::new(database.clone());
        let probe = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        proxy
            .apply(ProxySettings {
                enabled: true,
                port,
                ..Default::default()
            })
            .await
            .unwrap();

        let takeover = AppTakeoverManager::new(
            database,
            proxy.clone(),
            root.join("home"),
            root.join("data"),
        );
        let claude_path = root.join("home/.claude/settings.json");
        fs::create_dir_all(claude_path.parent().unwrap()).unwrap();
        let original = b"{\"keep\":true}\n";
        fs::write(&claude_path, original).unwrap();
        takeover.set_enabled("claude", true).await.unwrap();

        let status = set_proxy_enabled_inner(&proxy, &takeover, false)
            .await
            .unwrap();

        assert!(!status.running);
        assert!(!status.settings.enabled);
        assert!(!takeover.has_active());
        assert_eq!(fs::read(&claude_path).unwrap(), original);
        let rebound = std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
        drop(rebound);

        drop(takeover);
        drop(proxy);
        fs::remove_dir_all(root).unwrap();
    }
}
