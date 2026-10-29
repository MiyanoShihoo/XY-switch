use crate::{database::Database, proxy};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex as StdMutex},
};
use tokio::{net::TcpListener, sync::Mutex, task::JoinHandle};

const PROXY_SETTINGS_KEY: &str = "proxy_settings";

fn default_route_targets() -> Vec<String> {
    ["claude", "codex", "gemini", "grok", "opencode"]
        .into_iter()
        .map(str::to_string)
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings {
    pub enabled: bool,
    pub show_in_home: bool,
    pub host: String,
    pub port: u16,
    #[serde(default = "default_route_targets")]
    pub route_targets: Vec<String>,
}

impl Default for ProxySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            show_in_home: false,
            host: "127.0.0.1".to_string(),
            port: 15_721,
            route_targets: default_route_targets(),
        }
    }
}

impl ProxySettings {
    pub(crate) fn validate(&self) -> Result<(), String> {
        self.socket_addr()?;
        let supported = ["claude", "codex", "gemini", "grok", "opencode"];
        if self.route_targets.len() > supported.len()
            || self.route_targets.iter().any(|target| !supported.iter().any(|known| target.eq_ignore_ascii_case(known))) {
            return Err("路由目标包含不支持的客户端或数量超过限制".into());
        }
        Ok(())
    }

    fn socket_addr(&self) -> Result<SocketAddr, String> {
        let ip = self
            .host
            .parse::<IpAddr>()
            .map_err(|_| "监听地址必须是回环 IP 地址".to_string())?;
        if !ip.is_loopback() {
            return Err("为保护本机凭据，代理只允许监听回环地址".to_string());
        }
        if self.port == 0 {
            return Err("监听端口必须在 1 到 65535 之间".to_string());
        }
        Ok(SocketAddr::new(ip, self.port))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyStatus {
    pub settings: ProxySettings,
    pub running: bool,
    pub address: String,
    pub last_error: Option<String>,
}

#[derive(Clone)]
pub struct ProxyManager {
    database: Database,
    task: Arc<Mutex<Option<JoinHandle<()>>>>,
    last_error: Arc<StdMutex<Option<String>>>,
}

impl ProxyManager {
    pub fn new(database: Database) -> Self {
        Self {
            database,
            task: Arc::new(Mutex::new(None)),
            last_error: Arc::new(StdMutex::new(None)),
        }
    }

    pub fn settings(&self) -> Result<ProxySettings, String> {
        match self.database.get_setting(PROXY_SETTINGS_KEY)? {
            Some(value) => {
                serde_json::from_str(&value).map_err(|error| format!("路由设置损坏：{error}"))
            }
            None => Ok(ProxySettings::default()),
        }
    }

    pub fn prepare_for_startup(&self) -> Result<(), String> {
        let mut settings = self.settings().unwrap_or_default();
        settings.enabled = false;
        self.save_settings(&settings)
    }

    fn save_settings(&self, settings: &ProxySettings) -> Result<(), String> {
        settings.validate()?;
        let value = serde_json::to_string(settings).map_err(|error| error.to_string())?;
        self.database.set_setting(PROXY_SETTINGS_KEY, &value)
    }

    fn set_last_error(&self, error: Option<String>) {
        if let Ok(mut value) = self.last_error.lock() {
            *value = error;
        }
    }

    async fn start_with(&self, settings: &ProxySettings) -> Result<(), String> {
        let mut task = self.task.lock().await;
        if task.as_ref().is_some_and(|handle| !handle.is_finished()) {
            return Ok(());
        }
        task.take();
        let address = settings.socket_addr()?;
        let listener = TcpListener::bind(address).await.map_err(|error| {
            let message = format!("无法监听 {address}：{error}");
            self.set_last_error(Some(message.clone()));
            message
        })?;
        self.set_last_error(None);
        let database = self.database.clone();
        let last_error = self.last_error.clone();
        // 路由目标决定 router 挂哪些端点，必须在起服务时传进去。
        let route_targets = settings.route_targets.clone();
        *task = Some(tokio::spawn(async move {
            if let Err(error) = proxy::serve(listener, database, route_targets).await {
                tracing::error!(%error, "local proxy stopped");
                if let Ok(mut value) = last_error.lock() {
                    *value = Some(error.to_string());
                }
            }
        }));
        Ok(())
    }

    pub async fn stop(&self) {
        let handle = self.task.lock().await.take();
        if let Some(handle) = handle {
            handle.abort();
            let _ = handle.await;
        }
    }

    pub async fn apply(&self, settings: ProxySettings) -> Result<ProxyStatus, String> {
        settings.validate()?;
        let previous = self.settings()?;
        let was_running = self.is_running().await;
        let address_changed = previous.host != settings.host || previous.port != settings.port;
        // 路由目标改了要重建 router，只保存不重启的话勾选要等下次开关代理才生效。
        let targets_changed = previous.route_targets != settings.route_targets;

        if was_running && (address_changed || targets_changed || !settings.enabled) {
            self.stop().await;
        }
        self.save_settings(&settings)?;

        if settings.enabled
            && (!was_running || address_changed || targets_changed)
            && let Err(error) = self.start_with(&settings).await
        {
            let _ = self.save_settings(&previous);
            if was_running {
                let _ = self.start_with(&previous).await;
            }
            return Err(error);
        }
        self.status().await
    }

    pub async fn set_enabled(&self, enabled: bool) -> Result<ProxyStatus, String> {
        let mut settings = self.settings()?;
        settings.enabled = enabled;
        self.apply(settings).await
    }

    async fn is_running(&self) -> bool {
        self.task
            .lock()
            .await
            .as_ref()
            .is_some_and(|handle| !handle.is_finished())
    }

    pub async fn status(&self) -> Result<ProxyStatus, String> {
        let settings = self.settings()?;
        let running = self.is_running().await;
        let address = format!("http://{}", settings.socket_addr()?);
        let last_error = self
            .last_error
            .lock()
            .map_err(|_| "proxy status lock poisoned".to_string())?
            .clone();
        Ok(ProxyStatus {
            settings,
            running,
            address,
            last_error,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{ProxyManager, ProxySettings};
    use crate::database::Database;

    fn manager() -> (ProxyManager, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("xy-switch-manager-{}.db", uuid::Uuid::new_v4()));
        (ProxyManager::new(Database::open(&path).unwrap()), path)
    }

    #[test]
    fn validates_loopback_address_and_port() {
        let mut settings = ProxySettings::default();
        assert!(!settings.enabled);
        assert!(settings.socket_addr().is_ok());
        settings.host = "0.0.0.0".to_string();
        assert!(settings.socket_addr().unwrap_err().contains("回环"));
        settings.host = "127.0.0.1".to_string();
        settings.port = 0;
        assert!(settings.socket_addr().unwrap_err().contains("端口"));
    }

    #[tokio::test]
    async fn starts_stops_and_persists_settings() {
        let (manager, path) = manager();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let settings = ProxySettings {
            enabled: true,
            port,
            ..ProxySettings::default()
        };

        let status = manager.apply(settings.clone()).await.unwrap();
        assert!(status.running);
        assert_eq!(status.settings, settings);
        let status = manager.set_enabled(false).await.unwrap();
        assert!(!status.running);
        assert!(!status.settings.enabled);

        drop(manager);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn startup_always_resets_proxy_to_stopped() {
        let (manager, path) = manager();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        manager
            .save_settings(&ProxySettings {
                enabled: true,
                port,
                ..ProxySettings::default()
            })
            .unwrap();

        manager.prepare_for_startup().unwrap();
        let status = manager.status().await.unwrap();
        assert!(!status.settings.enabled);
        assert!(!status.running);

        drop(manager);
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn ipv6_status_is_a_valid_bracketed_url() {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let manager = ProxyManager::new(database);
        let settings = ProxySettings { host: "::1".into(), enabled: false, ..Default::default() };
        let status = manager.apply(settings).await.unwrap();
        assert_eq!(status.address, "http://[::1]:15721");
        assert!(reqwest::Url::parse(&status.address).is_ok());
    }

    #[tokio::test]
    async fn invalid_route_targets_do_not_change_saved_settings() {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let manager = ProxyManager::new(database);
        let invalid = ProxySettings { route_targets: vec!["not-a-client".into()], ..Default::default() };
        assert!(manager.apply(invalid).await.is_err());
        assert_eq!(manager.settings().unwrap(), ProxySettings::default());
        assert!(ProxySettings { route_targets: vec![], ..Default::default() }.validate().is_ok());
    }

}
