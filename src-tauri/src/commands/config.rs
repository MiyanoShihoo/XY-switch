use crate::database::{Database, Prompt, Provider};
use crate::mcp::{McpManager, McpServer};
use crate::proxy_manager::{ProxyManager, ProxySettings};
use serde::{Deserialize, Serialize};
use tauri::State;

const CONFIG_VERSION: u32 = 1;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConfigBundle {
    pub version: u32,
    #[serde(default)]
    pub providers: Vec<Provider>,
    #[serde(default)]
    pub prompts: Vec<Prompt>,
    #[serde(default)]
    pub mcp_servers: Vec<McpServer>,
    #[serde(default)]
    pub proxy: Option<ProxySettings>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub providers: usize,
    pub prompts: usize,
    pub mcp_servers: usize,
    pub proxy_imported: bool,
}

fn sanitize_mcp(mut server: McpServer) -> McpServer {
    server.headers.clear();
    server.secrets_configured = false;
    server
}

#[tauri::command]
pub fn export_config(
    database: State<'_, Database>,
    mcp: State<'_, McpManager>,
    proxy: State<'_, ProxyManager>,
) -> Result<String, String> {
    let bundle = ConfigBundle {
        version: CONFIG_VERSION,
        providers: database.list_providers()?,
        prompts: database.list_prompts()?,
        mcp_servers: mcp.list_servers()?.into_iter().map(sanitize_mcp).collect(),
        proxy: Some(proxy.settings()?),
    };
    serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn import_config(
    content: String,
    database: State<'_, Database>,
    mcp: State<'_, McpManager>,
    proxy: State<'_, ProxyManager>,
) -> Result<ImportResult, String> {
    let bundle: ConfigBundle = serde_json::from_str(&content)
        .map_err(|error| format!("Invalid configuration: {error}"))?;
    if bundle.version != CONFIG_VERSION {
        return Err(format!(
            "Unsupported configuration version: {}",
            bundle.version
        ));
    }

    for provider in &bundle.providers {
        database.save_provider(provider)?;
    }
    for prompt in &bundle.prompts {
        database.save_prompt(prompt)?;
    }
    for server in bundle.mcp_servers.iter().cloned().map(sanitize_mcp) {
        mcp.save_server(server)?;
    }

    let proxy_imported = if let Some(settings) = bundle.proxy.clone() {
        proxy.apply(settings).await?;
        true
    } else {
        false
    };

    Ok(ImportResult {
        providers: bundle.providers.len(),
        prompts: bundle.prompts.len(),
        mcp_servers: bundle.mcp_servers.len(),
        proxy_imported,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versioned_bundle() {
        let value = r#"{"version":1,"providers":[],"prompts":[],"mcpServers":[],"proxy":null}"#;
        let bundle: ConfigBundle = serde_json::from_str(value).unwrap();
        assert_eq!(bundle.version, CONFIG_VERSION);
    }
}
