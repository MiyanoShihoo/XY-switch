use crate::{
    commands::providers::{ModelDiscoveryRequest, check_model_access},
    database::{Database, ProviderHealth, RequestEvent, RequestSummary},
};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

async fn probe(database: &Database, provider_id: &str) -> Result<ProviderHealth, String> {
    let provider = database
        .get_provider(provider_id)?
        .ok_or_else(|| "provider not found".to_string())?;
    let started = Instant::now();
    let result = check_model_access(ModelDiscoveryRequest {
        provider_id: provider_id.to_string(),
        provider_type: provider.provider_type,
        upstream_format: provider.advanced.upstream_format,
        endpoint: provider.endpoint,
        api_key: None,
    }, Duration::from_secs(10)).await;
    let latency_ms = started.elapsed().as_millis() as i64;
    let (status_code, success, error_kind) = match result {
        Ok(access) => (Some(access.status_code), true, None),
        Err(error) => (error.status_code, false, Some(error.kind)),
    };
    database.record_request(&RequestEvent {
        provider_id: provider_id.to_string(),
        route: "health_check".to_string(),
        status_code,
        success,
        affects_health: true,
        diagnostics: None,
        latency_ms,
        retry_count: 0,
        error_kind,
        model: None,
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        cost: None,
        price_source: None,
        price_version: None,
        created_at: now_millis(),
    })?;
    database
        .provider_health(provider_id)?
        .ok_or_else(|| "health result was not recorded".to_string())
}

#[tauri::command]
pub fn list_provider_health(database: State<'_, Database>) -> Result<Vec<ProviderHealth>, String> {
    database.list_provider_health()
}

#[tauri::command]
pub fn request_summary(database: State<'_, Database>) -> Result<RequestSummary, String> {
    database.request_summary()
}

#[tauri::command]
pub async fn test_provider_health(
    database: State<'_, Database>,
    provider_id: String,
) -> Result<ProviderHealth, String> {
    probe(&database, &provider_id).await
}

#[tauri::command]
pub async fn test_all_provider_health(
    database: State<'_, Database>,
) -> Result<Vec<ProviderHealth>, String> {
    let providers = database.list_providers()?;
    let mut results = Vec::new();
    for provider in providers.into_iter().filter(|provider| provider.enabled) {
        results.push(probe(&database, &provider.id).await?);
    }
    Ok(results)
}
