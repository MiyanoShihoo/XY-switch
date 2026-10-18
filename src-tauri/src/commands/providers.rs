use crate::credential_store;
use crate::database::{Database, Provider};
use crate::takeover::AppTakeoverManager;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;
use tauri::State;

const MAX_MODEL_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelDiscoveryRequest {
    pub provider_id: String,
    pub provider_type: String,
    #[serde(default)]
    pub upstream_format: String,
    pub endpoint: String,
    #[serde(default)]
    pub api_key: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DiscoveredModel {
    pub id: String,
    pub name: String,
}

fn ends_with_version_segment(endpoint: &str) -> bool {
    endpoint
        .rsplit('/')
        .next()
        .and_then(|part| part.strip_prefix('v'))
        .is_some_and(|digits| {
            !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
        })
}

fn model_list_urls(endpoint: &str, provider_type: &str) -> Result<Vec<reqwest::Url>, String> {
    let endpoint_trimmed = endpoint.trim();
    let path_part = endpoint_trimmed
        .split_once(['?', '#'])
        .map_or(endpoint_trimmed, |(head, _)| head);
    let mut endpoint = path_part.trim_end_matches('/');
    let lower = endpoint.to_ascii_lowercase();
    if lower.ends_with("/chat/completions") {
        endpoint = &endpoint[..endpoint.len() - 17];
    } else if lower.ends_with("/responses") {
        endpoint = &endpoint[..endpoint.len() - 10];
    } else if lower.ends_with("/messages") {
        endpoint = &endpoint[..endpoint.len() - 9];
    }
    let endpoint = endpoint.trim_end_matches('/');
    let candidates = match provider_type {
        "anthropic" if endpoint.ends_with("/v1") => vec![format!("{endpoint}/models")],
        "anthropic" => vec![format!("{endpoint}/v1/models")],
        "gemini" if endpoint.ends_with("/v1beta") => vec![format!("{endpoint}/models")],
        "gemini" => vec![format!("{endpoint}/v1beta/models")],
        _ if ends_with_version_segment(endpoint) => vec![format!("{endpoint}/models")],
        _ => vec![
            format!("{endpoint}/v1/models"),
            format!("{endpoint}/models"),
        ],
    };
    let mut urls = Vec::with_capacity(candidates.len());
    for candidate in candidates {
        let url =
            reqwest::Url::parse(&candidate).map_err(|error| format!("无效的 API 端点：{error}"))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err("API 端点必须使用 HTTP 或 HTTPS".to_string());
        }
        urls.push(url);
    }
    Ok(urls)
}

/// `/models` 返回值的三种形状。
#[derive(Clone, Copy)]
enum ModelListShape {
    /// Gemini：`models[].name`，值带 `models/` 前缀。
    Gemini,
    /// OpenAI / Anthropic：`data[].id`。
    OpenAi,
    /// Codex 远程目录：`models[].slug`。
    ///
    /// 智谱的 OpenAI Responses 端点（`https://open.bigmodel.cn/api/v1`，也就是
    /// 智谱 GLM Codex 预设的 Base URL）返回的就是这个形状。之前只认 `data[].id`，
    /// 所以在 Codex 供应商表单里点「获取模型」永远是空列表。
    CodexCatalog,
}

fn parse_models(provider_type: &str, payload: Value) -> Result<Vec<DiscoveredModel>, String> {
    // 有 data 字段时行为与之前完全一致；只有在没有 data 时才回退去读 models[].slug。
    let (items, shape) = if provider_type == "gemini" {
        (
            payload.get("models").and_then(Value::as_array),
            ModelListShape::Gemini,
        )
    } else if let Some(data) = payload.get("data").and_then(Value::as_array) {
        (Some(data), ModelListShape::OpenAi)
    } else {
        (
            payload.get("models").and_then(Value::as_array),
            ModelListShape::CodexCatalog,
        )
    };
    let items = items.ok_or_else(|| "供应商返回了无法识别的模型列表".to_string())?;

    let mut models = items
        .iter()
        .filter_map(|item| {
            let raw_id = match shape {
                ModelListShape::Gemini => {
                    let name = item.get("name")?.as_str()?;
                    name.strip_prefix("models/").unwrap_or(name)
                }
                ModelListShape::OpenAi => item.get("id")?.as_str()?,
                // 目录条目以 slug 为准；个别条目只给 id 时兜底用 id。
                ModelListShape::CodexCatalog => item
                    .get("slug")
                    .or_else(|| item.get("id"))
                    .and_then(Value::as_str)?,
            };
            if raw_id.trim().is_empty() {
                return None;
            }
            let id = raw_id.trim();
            let display_name = item
                .get("display_name")
                .or_else(|| item.get("displayName"))
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .unwrap_or(id);
            Some(DiscoveredModel {
                id: id.to_string(),
                name: display_name.to_string(),
            })
        })
        .collect::<Vec<_>>();
    models.sort_by(|left, right| {
        left.id
            .to_lowercase()
            .cmp(&right.id.to_lowercase())
            .then_with(|| left.id.cmp(&right.id))
    });
    models.dedup_by(|left, right| left.id.eq_ignore_ascii_case(&right.id));
    if models.is_empty() {
        return Err("供应商没有返回可用模型".to_string());
    }
    Ok(models)
}

fn discovery_provider_type<'a>(
    provider_type: &'a str,
    upstream_format: &str,
) -> Result<&'a str, String> {
    match upstream_format {
        "anthropic" => Ok("anthropic"),
        "gemini" => Ok("gemini"),
        "openai_chat" | "openai_responses" => Ok("openai"),
        "" | "auto" => Ok(provider_type),
        value => Err(format!("不支持的上游格式：{value}")),
    }
}

#[tauri::command]
pub fn list_providers(database: State<'_, Database>) -> Result<Vec<Provider>, String> {
    database.list_providers()
}

#[tauri::command]
pub async fn save_provider(
    app: tauri::AppHandle,
    database: State<'_, Database>,
    provider: Provider,
) -> Result<(), String> {
    database.save_provider(&provider)?;
    // 托盘菜单只在启动时构建一次，新增/改名/停用的卡片不刷新就永远不会出现在托盘里。
    crate::tray::refresh(&app, database.inner());
    Ok(())
}

#[tauri::command]
pub fn reorder_providers(
    app: tauri::AppHandle,
    database: State<'_, Database>,
    provider_ids: Vec<String>,
) -> Result<(), String> {
    database.reorder_providers(&provider_ids)?;
    crate::tray::refresh(&app, database.inner());
    Ok(())
}

#[tauri::command]
pub fn get_current_provider(database: State<'_, Database>) -> Result<Option<Provider>, String> {
    database.current_provider()
}

#[tauri::command]
pub async fn set_current_provider(
    app: tauri::AppHandle,
    database: State<'_, Database>,
    takeover: State<'_, AppTakeoverManager>,
    id: String,
) -> Result<(), String> {
    select_provider(database.inner(), takeover.inner(), &id).await?;
    // 让托盘里的 ✓ 跟着界面上的选择一起走。
    crate::tray::refresh(&app, database.inner());
    Ok(())
}

pub(crate) async fn select_provider(
    database: &Database,
    takeover: &AppTakeoverManager,
    id: &str,
) -> Result<(), String> {
    let provider = database
        .get_provider(id)?
        .ok_or_else(|| "供应商不存在".to_string())?;
    if !provider.enabled {
        return Err("请先启用此供应商".to_string());
    }
    // Do not report success or change selection if synchronization fails.
    let previous = database.current_provider()?;
    takeover.sync_active_provider(&provider).await?;
    if let Err(error) = database.set_current_provider(id) {
        if let Some(previous) = previous {
            if let Err(rollback) = takeover.sync_active_provider(&previous).await {
                return Err(format!("{error}；客户端同步回滚失败：{rollback}"));
            }
        }
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
pub async fn refresh_codex_provider(
    database: State<'_, Database>,
    takeover: State<'_, AppTakeoverManager>,
    id: String,
) -> Result<(), String> {
    // Keep the IPC name for compatibility; synchronize only explicitly managed clients.
    if database.get_setting("current_provider")?.as_deref() != Some(id.as_str()) {
        return Ok(());
    }
    let provider = database.get_provider(&id)?.ok_or("供应商不存在")?;
    if provider.enabled {
        takeover.sync_active_provider(&provider).await?;
    }
    Ok(())
}

#[tauri::command]
pub fn delete_provider(
    app: tauri::AppHandle,
    database: State<'_, Database>,
    id: String,
) -> Result<(), String> {
    database.delete_provider(&id)?;
    credential_store::delete_api_key(&id)?;
    // 删掉的卡片必须同时从托盘菜单里消失，否则点它只会报"供应商不存在"。
    crate::tray::refresh(&app, database.inner());
    Ok(())
}

#[tauri::command]
pub async fn discover_provider_models(
    request: ModelDiscoveryRequest,
) -> Result<Vec<DiscoveredModel>, String> {
    let discovery_type = discovery_provider_type(&request.provider_type, &request.upstream_format)?;
    let api_key = match request.api_key.filter(|key| !key.trim().is_empty()) {
        Some(key) => key,
        None => credential_store::get_api_key(&request.provider_id)?
            .ok_or_else(|| "请先配置 API Key".to_string())?,
    };
    let urls = model_list_urls(&request.endpoint, discovery_type)?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|error| error.to_string())?;
    let mut last_status = None;
    for url in urls {
        let request_builder = match discovery_type {
            "anthropic" => client
                .get(url)
                .header("x-api-key", &api_key)
                .header("anthropic-version", "2023-06-01"),
            "gemini" => client.get(url).header("x-goog-api-key", &api_key),
            _ => client.get(url).bearer_auth(&api_key),
        };
        let response = request_builder.send().await.map_err(|error| {
            format!(
                "模型探索请求失败：{}",
                crate::network::describe_error(error)
            )
        })?;
        let status = response.status();
        if matches!(
            status,
            reqwest::StatusCode::NOT_FOUND | reqwest::StatusCode::METHOD_NOT_ALLOWED
        ) {
            last_status = Some(status.as_u16());
            continue;
        }
        if !status.is_success() {
            return Err(format!("模型探索失败：HTTP {}", status.as_u16()));
        }
        let bytes = response
            .bytes()
            .await
            .map_err(|error| format!("读取模型列表失败：{error}"))?;
        if bytes.len() > MAX_MODEL_RESPONSE_BYTES {
            return Err("供应商返回的模型列表过大".to_string());
        }
        let payload = serde_json::from_slice(&bytes)
            .map_err(|error| format!("模型列表不是有效 JSON：{error}"))?;
        return parse_models(discovery_type, payload);
    }
    Err(format!(
        "模型探索失败：所有候选地址均返回 HTTP {}",
        last_status.unwrap_or(404)
    ))
}

#[cfg(test)]
mod tests {
    use super::{
        ModelDiscoveryRequest, discover_provider_models, discovery_provider_type, model_list_urls,
        parse_models,
    };
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::get,
    };
    use serde_json::json;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn builds_protocol_specific_model_urls() {
        assert_eq!(
            model_list_urls("https://api.openai.com/v1", "openai").unwrap()[0].as_str(),
            "https://api.openai.com/v1/models"
        );
        assert_eq!(
            model_list_urls("https://api.anthropic.com", "anthropic").unwrap()[0].as_str(),
            "https://api.anthropic.com/v1/models"
        );
        assert_eq!(
            model_list_urls("https://generativelanguage.googleapis.com", "gemini").unwrap()[0]
                .as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models"
        );
        let custom = model_list_urls("https://api.example.com", "custom").unwrap();
        assert_eq!(custom[0].as_str(), "https://api.example.com/v1/models");
        assert_eq!(custom[1].as_str(), "https://api.example.com/models");
        assert_eq!(
            model_list_urls("https://open.bigmodel.cn/api/paas/v4", "custom").unwrap()[0].as_str(),
            "https://open.bigmodel.cn/api/paas/v4/models"
        );
        assert_eq!(
            discovery_provider_type("custom", "anthropic").unwrap(),
            "anthropic"
        );
        assert_eq!(
            discovery_provider_type("custom", "gemini").unwrap(),
            "gemini"
        );
        assert!(discovery_provider_type("custom", "unknown").is_err());
    }

    #[test]
    fn parses_openai_and_gemini_models() {
        let openai =
            parse_models("openai", json!({"data":[{"id":"gpt-b"},{"id":"gpt-a"}]})).unwrap();
        assert_eq!(openai[0].id, "gpt-a");
        let gemini = parse_models(
            "gemini",
            json!({"models":[{"name":"models/gemini-pro","displayName":"Gemini Pro"}]}),
        )
        .unwrap();
        assert_eq!(gemini[0].id, "gemini-pro");
        assert_eq!(gemini[0].name, "Gemini Pro");

        let normalized = parse_models(
            "openai",
            json!({"data":[
                {"id":" GPT-5 ","display_name":"  GPT Five  "},
                {"id":"gpt-5","display_name":"duplicate"},
                {"id":"model-b","display_name":"   "}
            ]}),
        )
        .unwrap();
        assert_eq!(normalized.len(), 2);
        assert_eq!(normalized[0].id, "GPT-5");
        assert_eq!(normalized[0].name, "GPT Five");
        assert_eq!(normalized[1].id, "model-b");
        assert_eq!(normalized[1].name, "model-b");
    }

    #[test]
    fn falls_back_to_codex_catalog_slug_when_there_is_no_data_field() {
        // 智谱的 Responses 端点返回 Codex 远程目录格式，没有 data 字段。
        let zhipu = parse_models(
            "openai",
            json!({"models":[
                {"slug":"glm-4.6","display_name":"GLM-4.6"},
                {"slug":"glm-4.5-air"}
            ]}),
        )
        .unwrap();
        assert_eq!(zhipu.len(), 2);
        assert_eq!(zhipu[0].id, "glm-4.5-air");
        assert_eq!(zhipu[0].name, "glm-4.5-air");
        assert_eq!(zhipu[1].id, "glm-4.6");
        assert_eq!(zhipu[1].name, "GLM-4.6");

        // 同时有 data 时仍以 data 为准，行为不变。
        let both = parse_models(
            "openai",
            json!({"data":[{"id":"gpt-a"}], "models":[{"slug":"ignored"}]}),
        )
        .unwrap();
        assert_eq!(both.len(), 1);
        assert_eq!(both[0].id, "gpt-a");
    }

    #[tokio::test]
    async fn falls_back_to_models_endpoint_with_bearer_auth() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_handler = calls.clone();
        let router = Router::new()
            .route("/v1/models", get(|| async { StatusCode::NOT_FOUND }))
            .route(
                "/models",
                get(move |headers: HeaderMap| {
                    let calls = calls_for_handler.clone();
                    async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(headers.get("authorization").unwrap(), "Bearer secret-key");
                        Json(json!({"data":[{"id":"model-b"},{"id":"model-a"}]}))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

        let models = discover_provider_models(ModelDiscoveryRequest {
            provider_id: "mock".to_string(),
            provider_type: "custom".to_string(),
            upstream_format: "auto".to_string(),
            endpoint: format!("http://{address}"),
            api_key: Some("secret-key".to_string()),
        })
        .await
        .unwrap();
        assert_eq!(models[0].id, "model-a");
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        task.abort();
        let _ = task.await;
    }
}
