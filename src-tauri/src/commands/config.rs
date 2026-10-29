use crate::database::{Database, Prompt, Provider};
use crate::mcp::{McpManager, McpServer};
use crate::model_pricing::{self, PricingInput};
use crate::proxy_manager::{ProxyManager, ProxySettings};
use serde::{Deserialize, Serialize};
use tauri::State;

const CONFIG_VERSION: u32 = 2;
const MAX_CONFIG_BYTES: usize = 8 * 1024 * 1024;
const MAX_RECORDS_PER_KIND: usize = 1_000;

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
    #[serde(default)]
    pub model_pricing: Vec<PricingInput>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportResult {
    pub providers: usize,
    pub prompts: usize,
    pub mcp_servers: usize,
    pub proxy_imported: bool,
    pub model_pricing: usize,
}

fn sanitize_mcp(mut server: McpServer) -> McpServer {
    for value in server.env.values_mut().chain(server.headers.values_mut()) { value.clear(); }
    server.secrets_configured = false;
    server
}

#[tauri::command]
pub fn export_config(
    database: State<'_, Database>,
    mcp: State<'_, McpManager>,
    proxy: State<'_, ProxyManager>,
) -> Result<String, String> {
    export_content(&database, &mcp, &proxy)
}

fn export_content(database: &Database, mcp: &McpManager, proxy: &ProxyManager) -> Result<String, String> {
    let bundle = ConfigBundle {
        version: CONFIG_VERSION,
        providers: database.list_providers()?,
        prompts: database.list_prompts()?,
        mcp_servers: mcp.list_servers()?.into_iter().map(sanitize_mcp).collect(),
        proxy: Some(proxy.settings()?),
        // Public reference catalogs and immutable usage snapshots are not configuration.
        model_pricing: model_pricing::list(database)?.into_iter().filter(|row| row.source == "custom").map(|row| row.pricing).collect(),
    };
    let content = serde_json::to_string_pretty(&bundle).map_err(|error| error.to_string())?;
    if content.len() > MAX_CONFIG_BYTES { return Err("Configuration backup exceeds 8 MiB".into()); }
    Ok(content)
}

fn parse_bundle(content: &str) -> Result<ConfigBundle, String> {
    if content.len() > MAX_CONFIG_BYTES { return Err("Configuration backup exceeds 8 MiB".into()); }
    let bundle: ConfigBundle = serde_json::from_str(content)
        .map_err(|error| format!("Invalid configuration: {error}"))?;
    if !matches!(bundle.version, 1 | CONFIG_VERSION) {
        return Err(format!("Unsupported configuration version: {}", bundle.version));
    }
    if bundle.version == 1 && !bundle.model_pricing.is_empty() { return Err("Custom model pricing requires configuration version 2".into()); }
    fn unique_ids<'a>(kind: &str, limit: usize, ids: impl Iterator<Item = &'a str>) -> Result<(), String> {
        let mut seen = std::collections::BTreeSet::new();
        for id in ids {
            if seen.len() >= limit { return Err(format!("Too many {kind} records in configuration")); }
            if !seen.insert(id.trim()) { return Err(format!("Duplicate {kind} ID in configuration: {id}")); }
        }
        Ok(())
    }
    unique_ids("provider", MAX_RECORDS_PER_KIND, bundle.providers.iter().map(|record| record.id.as_str()))?;
    unique_ids("prompt", MAX_RECORDS_PER_KIND, bundle.prompts.iter().map(|record| record.id.as_str()))?;
    unique_ids("MCP", MAX_RECORDS_PER_KIND, bundle.mcp_servers.iter().map(|record| record.id.as_str()))?;
    unique_ids("model pricing", model_pricing::MAX_PRICES, bundle.model_pricing.iter().map(|record| record.id.as_str()))?;
    for price in &bundle.model_pricing { price.validate()?; }
    // Check every section before the first persistent mutation or client sync.
    crate::database::validate_config_records(&bundle.providers, &bundle.prompts)?;
    for server in &bundle.mcp_servers { crate::mcp::validate_server(server)?; }
    if let Some(settings) = &bundle.proxy { settings.validate()?; }
    Ok(bundle)
}

async fn import_content(
    content: &str, database: &Database, mcp: &McpManager, proxy: &ProxyManager,
) -> Result<ImportResult, String> {
    let bundle = parse_bundle(content)?;
    database.import_config_records(&bundle.providers, &bundle.prompts, &bundle.model_pricing)?;
    let partial = |completed_mcp: usize, error: String| format!(
        "配置导入未全部完成：已更新供应商 {} 个、提示词 {} 个、自定义定价 {} 条、MCP {} 个。请检查当前配置后重试；失败原因：{error}",
        bundle.providers.len(), bundle.prompts.len(), bundle.model_pricing.len(), completed_mcp,
    );
    // MCP synchronization and listener changes involve external resources, not
    // one SQLite transaction. Report progress on I/O failure rather than claim
    // the whole backup was applied (or silently imply a complete rollback).
    for (completed, server) in bundle.mcp_servers.iter().cloned().map(sanitize_mcp).enumerate() {
        mcp.import_server(server).map_err(|error| partial(completed, error))?;
    }
    let proxy_imported = if let Some(settings) = bundle.proxy.clone() {
        proxy.apply(settings).await.map_err(|error| partial(bundle.mcp_servers.len(), error))?;
        true
    } else { false };
    Ok(ImportResult { providers: bundle.providers.len(), prompts: bundle.prompts.len(), mcp_servers: bundle.mcp_servers.len(), proxy_imported, model_pricing: bundle.model_pricing.len() })
}

#[tauri::command]
pub async fn import_config(
    content: String,
    database: State<'_, Database>,
    mcp: State<'_, McpManager>,
    proxy: State<'_, ProxyManager>,
) -> Result<ImportResult, String> {
    import_content(&content, &database, &mcp, &proxy).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versioned_bundle() {
        let value = r#"{"version":1,"providers":[],"prompts":[],"mcpServers":[],"proxy":null}"#;
        let bundle: ConfigBundle = serde_json::from_str(value).unwrap();
        assert_eq!(bundle.version, 1);
        assert!(bundle.model_pricing.is_empty());
        assert!(parse_bundle(value).is_ok());
    }
    fn provider() -> Provider {
        Provider { id: "qa-provider".into(), name: "QA Provider".into(), provider_type: "custom".into(), endpoint: "https://example.invalid/v1".into(), model: "qa-model".into(), app_group: "Codex".into(), enabled: true, sort_order: 0, advanced: Default::default() }
    }
    fn value() -> serde_json::Value {
        serde_json::json!({"version":1,"providers":[provider()],"prompts":[],"mcpServers":[],"proxy":null})
    }

    #[tokio::test]
    async fn invalid_later_sections_cannot_partially_import_providers() {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let home = std::env::temp_dir().join(format!("xy-switch-invalid-import-{}", uuid::Uuid::new_v4()));
        let mcp = McpManager::new(database.clone(), home.clone());
        let proxy = ProxyManager::new(database.clone());
        let baseline = database.list_providers().unwrap().len();
        let mut invalid_prompt = value();
        invalid_prompt["prompts"] = serde_json::json!([{"id":"bad-prompt","name":"","content":"content","enabled":true}]);
        let mut invalid_mcp = value();
        invalid_mcp["mcpServers"] = serde_json::json!([{"id":"bad-mcp","name":"Broken","description":"","transport":"stdio","command":"","args":[],"env":{},"headers":{},"apps":{},"builtIn":false,"secretsConfigured":false}]);
        let mut invalid_proxy = value();
        invalid_proxy["proxy"] = serde_json::json!({"enabled":true,"showInHome":false,"host":"0.0.0.0","port":15721,"routeTargets":["codex"]});
        let mut invalid_pricing = value();
        invalid_pricing["version"] = serde_json::json!(2);
        invalid_pricing["modelPricing"] = serde_json::json!([{"id":"bad","inputPerMillion":-1,"outputPerMillion":2}]);
        for invalid in [invalid_prompt, invalid_mcp, invalid_proxy, invalid_pricing] {
            assert!(import_content(&invalid.to_string(), &database, &mcp, &proxy).await.is_err());
            assert_eq!(database.list_providers().unwrap().len(), baseline);
            assert!(database.list_prompts().unwrap().is_empty());
            assert!(!proxy.status().await.unwrap().running);
            assert!(!home.exists());
        }
    }

    #[test]
    fn import_rejects_duplicate_ids_oversized_backups_and_unknown_versions() {
        let mut duplicate = value();
        duplicate["providers"] = serde_json::json!([provider(), provider()]);
        assert!(parse_bundle(&duplicate.to_string()).unwrap_err().contains("Duplicate provider ID"));
        assert!(parse_bundle(&" ".repeat(MAX_CONFIG_BYTES + 1)).unwrap_err().contains("exceeds 8 MiB"));
        assert!(parse_bundle("{\"version\":999}").unwrap_err().contains("Unsupported configuration version"));
    }

    #[test]
    fn backup_sanitization_keeps_secret_names_but_never_values() {
        let server: McpServer = serde_json::from_value(serde_json::json!({"id":"qa-server","name":"QA","description":"","transport":"stdio","command":"node","args":[],"env":{"QA_TOKEN":"not-exported"},"headers":{"Authorization":"Bearer not-exported"},"apps":{},"builtIn":false,"secretsConfigured":true})).unwrap();
        let sanitized = sanitize_mcp(server);
        assert_eq!(sanitized.env.get("QA_TOKEN").map(String::as_str), Some(""));
        assert_eq!(sanitized.headers.get("Authorization").map(String::as_str), Some(""));
        assert!(!sanitized.secrets_configured);
        assert!(!serde_json::to_string(&sanitized).unwrap().contains("not-exported"));
    }

    fn custom_price() -> PricingInput {
        PricingInput { id: "qa/priced".into(), input_per_million: 2.0, output_per_million: 4.0, cache_read_per_million: Some(0.0), cache_creation_per_million: None }
    }

    #[test]
    fn version_two_prices_are_validated_before_any_import() {
        let mut content = value(); content["version"] = serde_json::json!(2);
        content["modelPricing"] = serde_json::json!([custom_price()]);
        assert_eq!(parse_bundle(&content.to_string()).unwrap().model_pricing, vec![custom_price()]);
        content["modelPricing"][0]["inputPerMillion"] = serde_json::json!(-1);
        assert!(parse_bundle(&content.to_string()).unwrap_err().contains("有限数值"));
        content["modelPricing"] = serde_json::json!([custom_price(), custom_price()]);
        assert!(parse_bundle(&content.to_string()).unwrap_err().contains("Duplicate model pricing ID"));
        content["modelPricing"] = serde_json::json!([custom_price()]);
        content["modelPricing"][0]["source"] = serde_json::json!("models.dev");
        assert!(parse_bundle(&content.to_string()).is_err());
        content["modelPricing"] = serde_json::json!([custom_price()]); content["version"] = serde_json::json!(1);
        assert!(parse_bundle(&content.to_string()).unwrap_err().contains("version 2"));
    }

    #[tokio::test]
    async fn custom_prices_round_trip_but_reference_catalog_and_usage_do_not() {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let home = std::env::temp_dir().join(format!("xy-switch-price-backup-{}", uuid::Uuid::new_v4()));
        let mcp = McpManager::new(database.clone(), home.clone());
        let proxy = ProxyManager::new(database.clone());
        model_pricing::save(&database, &custom_price()).unwrap();
        let reference = PricingInput { id: "reference/model".into(), ..custom_price() };
        model_pricing::save(&database, &reference).unwrap();
        database.conn().lock().unwrap().execute("UPDATE model_pricing SET source='models.dev' WHERE id=?1", [&reference.id]).unwrap();
        let exported = export_content(&database, &mcp, &proxy).unwrap();
        let bundle = parse_bundle(&exported).unwrap();
        assert_eq!(bundle.version, 2);
        assert_eq!(bundle.model_pricing, vec![custom_price()]);
        assert!(!exported.contains("reference/model"));
        assert!(!exported.contains("request_price_snapshots"));
        // Keep this test offline: configuration sections which touch clients or listeners
        // are independently covered; only pricing is transferred here.
        let price_only = serde_json::json!({"version":2,"modelPricing":bundle.model_pricing});
        let restored = Database::open(std::path::Path::new(":memory:")).unwrap();
        let restored_mcp = McpManager::new(restored.clone(), home.clone());
        let restored_proxy = ProxyManager::new(restored.clone());
        let result = import_content(&price_only.to_string(), &restored, &restored_mcp, &restored_proxy).await.unwrap();
        assert_eq!(result.model_pricing, 1);
        assert_eq!(model_pricing::list(&restored).unwrap()[0].pricing, custom_price());
        import_content(r#"{"version":1}"#, &restored, &restored_mcp, &restored_proxy).await.unwrap();
        assert_eq!(model_pricing::list(&restored).unwrap()[0].pricing, custom_price());
        assert!(!home.exists());
    }

    #[tokio::test]
    async fn pricing_write_failure_rolls_back_all_database_configuration_sections() {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        let home = std::env::temp_dir().join(format!("xy-switch-price-rollback-{}", uuid::Uuid::new_v4()));
        let mcp = McpManager::new(database.clone(), home.clone());
        let proxy = ProxyManager::new(database.clone());
        let original = provider(); database.save_provider(&original).unwrap();
        model_pricing::save(&database, &custom_price()).unwrap();
        let mut content = value(); content["version"] = serde_json::json!(2);
        content["providers"][0]["name"] = serde_json::json!("Changed");
        content["prompts"] = serde_json::json!([{"id":"qa-prompt","name":"QA","content":"offline","enabled":true}]);
        content["modelPricing"] = serde_json::json!([custom_price()]);
        content["modelPricing"][0]["inputPerMillion"] = serde_json::json!(3);
        database.conn().lock().unwrap().execute_batch("CREATE TRIGGER reject_price_import BEFORE INSERT ON model_pricing BEGIN SELECT RAISE(ABORT, 'synthetic import price failure'); END;").unwrap();
        assert!(import_content(&content.to_string(), &database, &mcp, &proxy).await.unwrap_err().contains("synthetic import price failure"));
        assert_eq!(database.list_providers().unwrap().iter().find(|p| p.id == original.id).unwrap().name, original.name);
        assert!(database.list_prompts().unwrap().is_empty());
        assert_eq!(model_pricing::list(&database).unwrap()[0].pricing, custom_price());
        assert!(!home.exists());
    }

}
