use crate::{
    credential_store,
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
    let Some(api_key) = credential_store::get_api_key(provider_id)? else {
        database.record_request(&RequestEvent {
            provider_id: provider_id.to_string(),
            route: "health_check".to_string(),
            status_code: None,
            success: false,
            affects_health: true,
            latency_ms: 0,
            retry_count: 0,
            error_kind: Some("api_key_missing".to_string()),
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
        return database
            .provider_health(provider_id)?
            .ok_or_else(|| "health result was not recorded".to_string());
    };

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client.get(&provider.endpoint);
    request = match provider.provider_type.as_str() {
        "anthropic" => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        "gemini" => request.header("x-goog-api-key", api_key),
        _ => request.bearer_auth(api_key),
    };
    let result = request.send().await;
    let latency_ms = started.elapsed().as_millis() as i64;
    let (status_code, success, error_kind) = match result {
        Ok(response) => {
            let status = response.status();
            let success = !status.is_server_error()
                && status != reqwest::StatusCode::UNAUTHORIZED
                && status != reqwest::StatusCode::FORBIDDEN;
            (
                Some(status.as_u16()),
                success,
                (!success).then(|| format!("http_{}", status.as_u16())),
            )
        }
        Err(error) => (
            None,
            false,
            Some(if error.is_timeout() {
                "timeout".to_string()
            } else if error.is_connect() {
                "connect".to_string()
            } else {
                "request".to_string()
            }),
        ),
    };
    database.record_request(&RequestEvent {
        provider_id: provider_id.to_string(),
        route: "health_check".to_string(),
        status_code,
        success,
        affects_health: true,
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
