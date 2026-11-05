use super::{http, manager::ConnectionSecrets, *};
use crate::{database::Database, mcp::McpManager, plugins::*};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}, sync::{Arc, Mutex, atomic::{AtomicBool, Ordering}}, time::Duration};
use tokio::{net::TcpListener, task::JoinHandle};

#[derive(Default)]
struct MemorySecrets {
    values: Mutex<BTreeMap<String, String>>,
    fail_set: AtomicBool,
    fail_delete: AtomicBool,
}

impl ConnectionSecrets for MemorySecrets {
    fn get(&self, id: &str) -> Result<Option<String>, String> { Ok(self.values.lock().unwrap().get(id).cloned()) }
    fn set(&self, id: &str, value: &str) -> Result<(), String> {
        if self.fail_set.load(Ordering::SeqCst) { return Err("synthetic-secret-must-not-leak".into()); }
        self.values.lock().unwrap().insert(id.into(), value.into());
        Ok(())
    }
    fn delete(&self, id: &str) -> Result<(), String> {
        if self.fail_delete.load(Ordering::SeqCst) { return Err("synthetic-secret-must-not-leak".into()); }
        self.values.lock().unwrap().remove(id);
        Ok(())
    }
}

struct Fixture {
    root: PathBuf,
    shared: Option<SharedMcpManager>,
    mcp: Option<McpManager>,
    secrets: Arc<MemorySecrets>,
}

impl Fixture {
    fn new() -> Self {
        let base = std::env::temp_dir();
        #[cfg(unix)]
        let base = fs::canonicalize(base).unwrap();
        let root = base.join(format!("xy-shared-mcp-tests-{}", uuid::Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let home = root.join("home");
        for directory in [home.join(".claude"), home.join(".codex"),
            crate::client_config::config_directory(&home, "Claude", false)] {
            fs::create_dir_all(directory).unwrap();
        }
        let database = Database::open(&root.join("metadata.sqlite")).unwrap();
        let plugins = PluginManager::new(database.clone(), root.join("packages")).unwrap();
        let secrets = Arc::new(MemorySecrets::default());
        let manager = SharedMcpManager::with_secrets(database.clone(), plugins, secrets.clone()).unwrap();
        {
            let mut state = manager.state.lock().unwrap();
            state.enabled = true;
            state.running = true;
        }
        Self { root, shared: Some(manager), mcp: Some(McpManager::new(database, home)), secrets }
    }

    fn manager(&self) -> &SharedMcpManager { self.shared.as_ref().unwrap() }
    fn mcp(&self) -> &McpManager { self.mcp.as_ref().unwrap() }

    fn connection(&self, owner: &str) -> (SharedMcpConnection, String) {
        let connection = self.manager().register(self.mcp(), owner, true, true).unwrap();
        let raw = self.secrets.get(&connection.id).unwrap().unwrap();
        let (_, token) = super::manager::bridge_credential(&raw).unwrap();
        (connection, token.to_string())
    }

    fn install_echo(&self, enabled_clients: &[&str]) {
        let pair = Ed25519KeyPair::from_seed_unchecked(&[42; 32]).unwrap();
        let hex = |bytes: &[u8]| bytes.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
        let input = TrustedKeyInput { id: "test.shared".into(), label: "Synthetic publisher".into(), public_key: hex(pair.public_key().as_ref()) };
        let key = self.manager().plugins.preview_trusted_key(&input).unwrap();
        self.manager().plugins.configure_trusted_key(input, &key.fingerprint, true).unwrap();
        let package = PluginPackage {
            manifest: PluginManifest {
                schema_version: 1, id: "test.shared-echo".into(), name: "Shared echo".into(), description: "Return reviewed JSON.".into(),
                author: "Synthetic publisher".into(), version: "1.0.0".into(), minimum_host_version: "0.0.0".into(), kind: PluginKind::Mcp,
                entry: "tools.json".into(), clients: vec!["codex-desktop".into(), "codex-cli".into(), "claude-desktop".into(), "claude-desktop-3p".into()],
                permissions: PluginPermissions::default(), dependencies: Vec::new(),
            },
            files: BTreeMap::from([("tools.json".into(), json!({"schemaVersion":1,"tools":[{"name":"echo","description":"Return reviewed JSON.",
                "inputSchema":{"type":"object","properties":{"message":{"type":"string","maxLength":4096}},"required":["message"],"additionalProperties":false},
                "operation":{"type":"json_passthrough"}}]}).to_string())]),
        };
        let digest = package_digest(&package).unwrap();
        let signature = hex(pair.sign(&signing_payload(&package.manifest.id, &package.manifest.version, &digest).unwrap()).as_ref());
        let preview = self.manager().plugins.preview_install(PublishedPlugin { package, digest, signature, signing_key_id: "test.shared".into() }).unwrap();
        self.manager().plugins.confirm_install(InstallConfirmation { preview_id: preview.preview_id,
            digest: preview.validation.digest, accepted_permissions: preview.validation.manifest.permissions,
            clients: enabled_clients.iter().map(|client| (*client).into()).collect(), confirmed: true, enable: true }).unwrap();
    }

    async fn pending(&self) -> SharedToolApproval {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if let Some(pending) = self.manager().pending().unwrap().into_iter().next() { break pending; }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("a tool call entered review")
    }

    async fn cleared(&self) {
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if self.manager().pending().unwrap().is_empty() && self.manager().status().unwrap().pending_count == 0 { break; }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("tool review was removed")
    }
}

fn cleanup(root: &Path, path: &Path) {
    if !path.starts_with(root) { return; }
    let Ok(metadata) = fs::symlink_metadata(path) else { return; };
    if metadata.file_type().is_symlink() { let _ = fs::remove_file(path); return; }
    if metadata.is_dir() {
        if let Ok(entries) = fs::read_dir(path) { for entry in entries.flatten() { cleanup(root, &entry.path()); } }
        let _ = fs::remove_dir(path);
    } else if metadata.is_file() {
        if metadata.permissions().readonly() {
            let mut permissions = metadata.permissions();
            #[cfg(unix)]
            { use std::os::unix::fs::PermissionsExt; permissions.set_mode(permissions.mode() | 0o200); }
            #[cfg(windows)]
            permissions.set_readonly(false);
            let _ = fs::set_permissions(path, permissions);
        }
        let _ = fs::remove_file(path);
    }
}

impl Drop for Fixture {
    fn drop(&mut self) { self.shared.take(); self.mcp.take(); cleanup(&self.root, &self.root); }
}

struct Server {
    fixture: Fixture,
    task: Option<JoinHandle<()>>,
    client: reqwest::Client,
}

impl Server {
    async fn new() -> Self {
        let fixture = Fixture::new();
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        fixture.manager().state.lock().unwrap().address = address;
        let router = http::router(fixture.manager().clone());
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
        Self { fixture, task: Some(task), client: reqwest::Client::builder().no_proxy().timeout(Duration::from_secs(6)).build().unwrap() }
    }

    fn endpoint(&self, connection: &SharedMcpConnection) -> String { format!("{}/mcp/{}", self.fixture.manager().status().unwrap().address, connection.id) }
    fn post(&self, connection: &SharedMcpConnection, token: &str, rpc: &Value) -> reqwest::RequestBuilder {
        let mut request = self.client.post(self.endpoint(connection)).bearer_auth(token).header("accept", "application/json, text/event-stream");
        if let Some(version) = rpc["params"].get("_meta").and_then(|meta| meta.get(http::VERSION_META)).and_then(Value::as_str) {
            request = request.header("mcp-protocol-version", version).header("mcp-method", rpc["method"].as_str().unwrap());
            if let Some(name) = rpc["params"].get("name").and_then(Value::as_str) { request = request.header("mcp-name", http::encode_name(name).unwrap()); }
        }
        request.json(rpc)
    }
    async fn shutdown(mut self) { if let Some(task) = self.task.take() { task.abort(); let _ = task.await; } }
}

impl Drop for Server { fn drop(&mut self) { if let Some(task) = &self.task { task.abort(); } } }

fn modern(id: i64, method: &str, mut params: Value) -> Value {
    params["_meta"] = json!({(http::VERSION_META):MODERN_VERSION,(http::CAPABILITIES_META):{}});
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}

fn legacy(id: i64, method: &str, params: Value) -> Value { json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}) }

#[path = "http_tests.rs"]
mod http_tests;
#[path = "manager_tests.rs"]
mod manager_tests;
#[path = "bridge_tests.rs"]
mod bridge_tests;
