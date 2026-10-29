use crate::{
    adapters, credential_store,
    database::{Database, Provider, RequestRecordResult, RequestUsage},
    preferences,
    streaming::{self, ClientFormat, UpstreamFormat},
    xai_responses,
};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use futures_util::TryStreamExt;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::{
    io,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_REQUEST_BYTES: usize = 10 * 1024 * 1024;
#[cfg(not(test))]
const RETRY_BASE_DELAY_MS: u64 = 200;
#[cfg(test)]
const RETRY_BASE_DELAY_MS: u64 = 1;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClientProtocol {
    Chat,
    Responses,
    Anthropic,
    Gemini,
}

impl ClientProtocol {
    fn shadow_protocol(self) -> &'static str {
        match self {
            Self::Chat => "chat",
            Self::Responses => "responses",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EffectiveUpstream {
    OpenAiChat,
    OpenAiResponses,
    Anthropic,
    Gemini,
}

impl EffectiveUpstream {
    fn shadow_protocol(self) -> &'static str {
        match self {
            Self::OpenAiChat => "chat",
            Self::OpenAiResponses => "responses",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }
    fn audit_format(self) -> UpstreamFormat {
        match self {
            Self::OpenAiChat => UpstreamFormat::OpenAi,
            Self::OpenAiResponses => UpstreamFormat::Responses,
            Self::Anthropic => UpstreamFormat::Anthropic,
            Self::Gemini => UpstreamFormat::Gemini,
        }
    }
}

impl ClientProtocol {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResponseTransform {
    None,
    ResponsesToResponses,
    ChatToResponses,
    ResponsesToChat,
    ResponsesToAnthropic,
    ResponsesToGemini,
    AnthropicToChat,
    AnthropicToResponses,
    ChatToAnthropic,
    GeminiToChat,
    GeminiToAnthropic,
    GeminiToResponses,
    ChatToGemini,
    AnthropicToGemini,
}

trait ApiKeyProvider: Send + Sync {
    fn get(&self, provider_id: &str) -> Result<Option<String>, String>;
}

struct WindowsApiKeyProvider;

impl ApiKeyProvider for WindowsApiKeyProvider {
    fn get(&self, provider_id: &str) -> Result<Option<String>, String> {
        credential_store::get_api_key(provider_id)
    }
}

#[derive(Clone)]
struct ProxyState {
    database: Database,
    client: reqwest::Client,
    api_keys: Arc<dyn ApiKeyProvider>,
    local_addr: Option<std::net::SocketAddr>,
}

impl ProxyState {
    fn new(database: Database, local_addr: Option<std::net::SocketAddr>) -> Result<Self, String> {
        // 整体超时取「设置 → 高级」里的值（默认 180 秒，和修复前写死的一致）。
        // 客户端在这里等的是上游出第一个 token 到最后一个 token 的全过程，
        // 长上下文 + 高推理强度时 180 秒真的会不够，所以这项必须可调。
        let timeout = preferences::load(&database).request_timeout();
        let client = crate::network::client_builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(timeout)
            .build()
            .map_err(|error| format!("failed to create HTTP client: {error}"))?;
        Ok(Self {
            database,
            client,
            api_keys: Arc::new(WindowsApiKeyProvider),
            local_addr,
        })
    }
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

#[derive(Debug)]
struct ProxyError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

impl ProxyError {
    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(json!({
                "error": {
                    "type": self.code,
                    "message": self.message,
                }
            })),
        )
            .into_response()
    }
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "xy-switch-proxy",
    })
}

/// Redacts sensitive information from URLs for safe logging.
/// Strips query parameters that may contain API keys, and masks userinfo.
#[allow(dead_code)]
fn redact_url_for_log(url: &str) -> String {
    let Ok(mut parsed) = reqwest::Url::parse(url) else {
        return "<invalid-url>".to_string();
    };
    parsed.set_query(None);
    parsed.set_fragment(None);
    if parsed.has_host() {
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);
    }
    parsed.to_string()
}

const TERMINAL_SUFFIXES: &[&str] = &[
    "/chat/completions",
    "/responses/compact",
    "/responses",
    "/messages",
    "/embeddings",
];

fn build_upstream_url(endpoint: &str, route: &str) -> Result<reqwest::Url, ProxyError> {
    let mut url = reqwest::Url::parse(endpoint).map_err(|_| {
        ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_provider_endpoint",
            "The selected provider endpoint is invalid.",
        )
    })?;

    let original_path = url.path().trim_end_matches('/').to_string();
    let route_clean = route.trim_matches('/');

    // Check if the route ends with any known terminal suffix
    let route_suffix = TERMINAL_SUFFIXES
        .iter()
        .find(|&&suffix| {
            let s = suffix.trim_start_matches('/');
            route_clean.eq_ignore_ascii_case(s) || route_clean.ends_with(s)
        })
        .copied();

    let combined = if let Some(suffix) = route_suffix {
        let orig_lower = original_path.to_ascii_lowercase();
        if orig_lower.ends_with(suffix) {
            // The endpoint path ALREADY ends with this exact terminal suffix
            // (e.g. endpoint is ".../v1/chat/completions" and route is "/v1/chat/completions").
            // Use original_path directly to avoid double appending!
            original_path
        } else {
            // The endpoint path does not end with this suffix.
            // But check if it ends with a DIFFERENT terminal suffix (e.g. user pasted ".../chat/completions"
            // but the route is "/v1/responses" or vice versa). Strip that conflicting suffix.
            let stripped_base = if let Some(conflicting) =
                TERMINAL_SUFFIXES.iter().find(|&&s| orig_lower.ends_with(s))
            {
                &original_path[..original_path.len() - conflicting.len()]
            } else {
                original_path.as_str()
            };

            let base = stripped_base.trim_end_matches('/');
            if base.is_empty() {
                format!("/{}", route_clean)
            } else {
                format!(
                    "{}/{}",
                    base,
                    route_clean.strip_prefix("v1/").unwrap_or(route_clean)
                )
            }
        }
    } else {
        // Route is not a known terminal suffix
        let orig_lower = original_path.to_ascii_lowercase();
        let stripped_base = if let Some(conflicting) =
            TERMINAL_SUFFIXES.iter().find(|&&s| orig_lower.ends_with(s))
        {
            &original_path[..original_path.len() - conflicting.len()]
        } else {
            original_path.as_str()
        };

        let base = stripped_base.trim_end_matches('/');
        if base.is_empty() {
            format!("/{}", route_clean)
        } else {
            format!(
                "{}/{}",
                base,
                route_clean.strip_prefix("v1/").unwrap_or(route_clean)
            )
        }
    };

    let mut combined = combined;
    while combined.contains("/v1/v1/") {
        combined = combined.replace("/v1/v1/", "/v1/");
    }
    while combined.contains("/v1beta/v1beta/") {
        combined = combined.replace("/v1beta/v1beta/", "/v1beta/");
    }

    url.set_path(&combined);
    url.set_fragment(None);
    Ok(url)
}

/// Artifact 工具在各协议里的名字都是这一个。
const ARTIFACT_TOOL_NAME: &str = "Artifact";

/// 取工具定义里的名字：Chat 是 `tools[].function.name`，Anthropic / Responses 是 `tools[].name`。
fn tool_entry_name(tool: &Value) -> Option<&str> {
    tool.get("function")
        .and_then(|function| function.get("name"))
        .or_else(|| tool.get("name"))
        .and_then(Value::as_str)
}

fn is_artifact_tool(tool: &Value) -> bool {
    tool_entry_name(tool).is_some_and(|name| name.eq_ignore_ascii_case(ARTIFACT_TOOL_NAME))
}

/// 把 Artifact 工具从请求里摘掉（对应「禁用 Artifact 工具」开关）。
///
/// 三种客户端协议的 tools 形状都覆盖；摘完以后如果一个工具都不剩，就把 `tools`
/// 整个删掉——空数组在部分网关上会被判成非法参数。`tool_choice` 如果正好指着
/// Artifact，也必须一起删掉，否则上游会因为"指定了不存在的工具"再报一次 400。
fn strip_artifact_tool(object: &mut Map<String, Value>) {
    let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    let before = tools.len();
    tools.retain(|tool| !is_artifact_tool(tool));
    if tools.len() == before {
        return;
    }
    if tools.is_empty() {
        object.remove("tools");
    }
    if object
        .get("tool_choice")
        .is_some_and(|choice| is_artifact_tool(choice))
    {
        object.remove("tool_choice");
    }
}

fn prepare_payload(body: &[u8], provider: &Provider) -> Result<(Value, String), ProxyError> {
    let mut payload: Value = serde_json::from_slice(body).map_err(|_| {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "Request body must be a JSON object.",
        )
    })?;
    let object = payload.as_object_mut().ok_or_else(|| {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "Request body must be a JSON object.",
        )
    })?;
    let requested_model = object.get("model").and_then(Value::as_str);
    let model = requested_model
        .and_then(|requested| {
            provider.advanced.model_mappings.iter().find(|mapping| {
                mapping.model.eq_ignore_ascii_case(requested)
                    || (!mapping.display_name.is_empty()
                        && mapping.display_name.eq_ignore_ascii_case(requested))
            })
        })
        .map(|mapping| mapping.model.trim().to_string())
        // DEBUG.txt 第 35 条：没命中映射表时原本一律改写成供应商配置的模型，
        // 于是在 xAI 卡上请求 grok-4.6-fast 会被悄悄换成卡里配的 grok-4.5——
        // 用户以为换了模型，实际能力和账单都还是旧的。只对 xAI/Grok 放行，
        // 判断条件见 xai_responses::passthrough_model（别的卡上发 grok-* 不放行，
        // 否则就成了绕过模型锁定的后门）。
        .or_else(|| {
            xai_responses::passthrough_model(&provider.endpoint, &provider.model, requested_model)
        })
        .unwrap_or_else(|| provider.model.clone());
    object.insert("model".to_string(), Value::String(model.clone()));
    if provider.advanced.claude.disable_artifact_tool {
        strip_artifact_tool(object);
    }
    Ok((payload, model))
}

fn adapter_error(error: String) -> ProxyError {
    ProxyError::new(StatusCode::BAD_REQUEST, "invalid_request", error)
}

fn effective_upstream(provider: &Provider) -> Result<EffectiveUpstream, ProxyError> {
    let endpoint_trimmed = provider.endpoint.trim();
    let path_part = endpoint_trimmed
        .split_once(['?', '#'])
        .map_or(endpoint_trimmed, |(head, _)| head);
    let endpoint_lower = path_part.trim_end_matches('/').to_ascii_lowercase();

    let format = match provider.advanced.upstream_format.as_str() {
        "openai_chat" => EffectiveUpstream::OpenAiChat,
        "openai_responses" => {
            if endpoint_lower.ends_with("/chat/completions") {
                EffectiveUpstream::OpenAiChat
            } else {
                EffectiveUpstream::OpenAiResponses
            }
        }
        "anthropic" => EffectiveUpstream::Anthropic,
        _ => {
            return Err(ProxyError::new(
                StatusCode::UNPROCESSABLE_ENTITY,
                "protocol_not_supported",
                "The selected upstream format is not supported.",
            ));
        }
    };
    Ok(format)
}

fn apply_body_overrides(provider: &Provider, payload: &mut Value) {
    if let Some(object) = payload.as_object_mut() {
        object.extend(provider.advanced.body_overrides.clone());
    }
}

/// Use the same protocol decision for Codex export and HTTP forwarding.
pub(crate) fn requires_codex_proxy(provider: &Provider) -> Result<bool, String> {
    effective_upstream(provider)
        .map(|_| true)
        .map_err(|error| error.message)
}

pub(crate) fn requires_protocol_conversion(provider: &Provider) -> Result<bool, String> {
    match effective_upstream(provider) {
        Ok(EffectiveUpstream::OpenAiResponses) => Ok(false),
        Ok(_) => Ok(true),
        Err(e) => Err(e.message),
    }
}

pub(crate) fn codex_direct_base_url(provider: &Provider) -> Result<String, String> {
    let mut url =
        build_upstream_url(&provider.endpoint, "/v1/responses").map_err(|error| error.message)?;
    let path = url
        .path()
        .trim_end_matches('/')
        .strip_suffix("/responses")
        .ok_or_else(|| "Responses 地址必须以 /responses 结尾".to_string())?
        .to_string();
    url.set_path(&path);
    Ok(url.to_string().trim_end_matches('/').to_string())
}

struct PreparedRequest {
    payload: Value,
    url: reqwest::Url,
    transform: ResponseTransform,
    stream: bool,
    upstream: EffectiveUpstream,
    model: String,
    audit: crate::model_audit::Observation,
}

fn prepare_upstream_request(
    provider: &Provider,
    protocol: ClientProtocol,
    body: &[u8],
) -> Result<PreparedRequest, ProxyError> {
    let (source, model) = prepare_payload(body, provider)?;
    let stream = source
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let upstream = effective_upstream(provider)?;
    let (mut payload, route, transform) = match upstream {
        EffectiveUpstream::OpenAiChat => match protocol {
            ClientProtocol::Chat => (source, "/v1/chat/completions", ResponseTransform::None),
            ClientProtocol::Responses => (
                adapters::responses_to_openai_chat(&source).map_err(adapter_error)?,
                "/v1/chat/completions",
                ResponseTransform::ChatToResponses,
            ),
            ClientProtocol::Anthropic => (
                adapters::anthropic_to_openai_chat(source, &model).map_err(adapter_error)?,
                "/v1/chat/completions",
                ResponseTransform::ChatToAnthropic,
            ),
            ClientProtocol::Gemini => (
                adapters::gemini_to_openai_chat(&source, &model).map_err(adapter_error)?,
                "/v1/chat/completions",
                ResponseTransform::ChatToGemini,
            ),
        },
        EffectiveUpstream::OpenAiResponses => match protocol {
            ClientProtocol::Responses => (
                source,
                "/v1/responses",
                ResponseTransform::ResponsesToResponses,
            ),
            ClientProtocol::Chat => (
                adapters::openai_chat_to_responses(source, &model).map_err(adapter_error)?,
                "/v1/responses",
                ResponseTransform::ResponsesToChat,
            ),
            ClientProtocol::Anthropic => {
                let chat =
                    adapters::anthropic_to_openai_chat(source, &model).map_err(adapter_error)?;
                (
                    adapters::openai_chat_to_responses(chat, &model).map_err(adapter_error)?,
                    "/v1/responses",
                    ResponseTransform::ResponsesToAnthropic,
                )
            }
            ClientProtocol::Gemini => {
                let chat =
                    adapters::gemini_to_openai_chat(&source, &model).map_err(adapter_error)?;
                (
                    adapters::openai_chat_to_responses(chat, &model).map_err(adapter_error)?,
                    "/v1/responses",
                    ResponseTransform::ResponsesToGemini,
                )
            }
        },
        EffectiveUpstream::Anthropic => match protocol {
            ClientProtocol::Anthropic => (source, "/v1/messages", ResponseTransform::None),
            ClientProtocol::Chat => (
                adapters::openai_chat_to_anthropic(source, &model).map_err(adapter_error)?,
                "/v1/messages",
                ResponseTransform::AnthropicToChat,
            ),
            ClientProtocol::Responses => {
                let chat = adapters::responses_to_openai_chat(&source).map_err(adapter_error)?;
                (
                    adapters::openai_chat_to_anthropic(chat, &model).map_err(adapter_error)?,
                    "/v1/messages",
                    ResponseTransform::AnthropicToResponses,
                )
            }
            ClientProtocol::Gemini => {
                let chat =
                    adapters::gemini_to_openai_chat(&source, &model).map_err(adapter_error)?;
                (
                    adapters::openai_chat_to_anthropic(chat, &model).map_err(adapter_error)?,
                    "/v1/messages",
                    ResponseTransform::AnthropicToGemini,
                )
            }
        },
        EffectiveUpstream::Gemini => {
            let route = if stream {
                "/v1beta/models/streamGenerateContent"
            } else {
                "/v1beta/models/generateContent"
            };
            if protocol == ClientProtocol::Gemini {
                let mut payload = source;
                if let Some(obj) = payload.as_object_mut() {
                    obj.remove("model");
                }
                (payload, route, ResponseTransform::None)
            } else {
                let chat = match protocol {
                    ClientProtocol::Chat => source,
                    ClientProtocol::Responses => {
                        adapters::responses_to_openai_chat(&source).map_err(adapter_error)?
                    }
                    ClientProtocol::Anthropic => {
                        adapters::anthropic_to_openai_chat(source, &model).map_err(adapter_error)?
                    }
                    ClientProtocol::Gemini => unreachable!(),
                };
                let transform = match protocol {
                    ClientProtocol::Chat => ResponseTransform::GeminiToChat,
                    ClientProtocol::Responses => ResponseTransform::GeminiToResponses,
                    ClientProtocol::Anthropic => ResponseTransform::GeminiToAnthropic,
                    ClientProtocol::Gemini => unreachable!(),
                };
                let payload = adapters::openai_chat_to_gemini(&chat).map_err(adapter_error)?;
                (payload, route, transform)
            }
        }
    };

    // DEBUG.txt 第 35 条：xAI 的原生 Responses 端点比 OpenAI 严格得多，
    // 根级 oneOf/anyOf、历史 function_call 参数里的 1.0 这种整数浮点、
    // agent_message / encrypted_content 这些 OpenAI 私有输入项，以及没夹过的
    // reasoning.effort，都会让它直接 400。清洗统一收在 xai_responses 里。
    //
    // 位置很关键：必须在协议转换之后（此时 payload 一定已经是 Responses 形状），
    // 在 apply_body_overrides 之前（用户在高级设置里显式写的覆盖值优先级最高，
    // 不该被我们改回去）。闸门也很关键：DeepSeek 官方 Codex 同样走原生 Responses，
    // is_xai_target 必须把它排除在外。
    if upstream == EffectiveUpstream::OpenAiResponses
        && xai_responses::is_xai_target(&provider.endpoint, &model)
    {
        xai_responses::sanitize_request(&mut payload, &model).map_err(adapter_error)?;
    }

    apply_body_overrides(provider, &mut payload);

    let url = if provider.advanced.is_full_url {
        reqwest::Url::parse(&provider.endpoint).map_err(|_| {
            ProxyError::new(
                StatusCode::BAD_GATEWAY,
                "invalid_provider_endpoint",
                "The selected provider endpoint is invalid.",
            )
        })?
    } else if upstream == EffectiveUpstream::Gemini {
        let operation = if stream {
            "streamGenerateContent"
        } else {
            "generateContent"
        };
        build_gemini_url(&provider.endpoint, &model, operation, stream)?
    } else {
        build_upstream_url(&provider.endpoint, route)?
    };
    // Capture the original identity separately from the exact model/limit dispatched
    // after mappings, protocol conversion, sanitization, and body overrides.
    let requested_model = serde_json::from_slice::<Value>(body).ok().and_then(|body| {
        body["model"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
    });
    let forwarded_model = if upstream == EffectiveUpstream::Gemini {
        url.path()
            .rsplit_once("/models/")
            .and_then(|(_, operation)| operation.rsplit_once(':'))
            .map(|(model, _)| model.to_owned())
    } else {
        payload["model"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .map(str::to_owned)
    };
    let max_tokens = match upstream {
        EffectiveUpstream::OpenAiChat => payload
            .get("max_completion_tokens")
            .or_else(|| payload.get("max_tokens")),
        EffectiveUpstream::OpenAiResponses => payload.get("max_output_tokens"),
        EffectiveUpstream::Anthropic => payload.get("max_tokens"),
        EffectiveUpstream::Gemini => payload.pointer("/generationConfig/maxOutputTokens"),
    }
    .and_then(Value::as_u64);
    let audit = crate::model_audit::Observation {
        requested_model,
        forwarded_model,
        // Usage is aggregate for n choices, whereas a request limit is per choice.
        max_tokens: if payload["n"].as_u64().unwrap_or(1) > 1 {
            None
        } else {
            max_tokens
        },
        ..Default::default()
    };
    Ok(PreparedRequest {
        payload,
        url,
        transform,
        stream,
        upstream,
        model,
        audit,
    })
}

/// One bounded diagnostic request using the provider's real routing/auth path.
/// It never activates a provider, starts the proxy, retries, or falls back.
pub(crate) struct PreparedModelProbe {
    pub request: reqwest::RequestBuilder,
    pub format: UpstreamFormat,
    pub protocol: &'static str,
    pub model: String,
    pub observation: crate::model_audit::Observation,
}

pub(crate) fn response_model_protocol(provider: &Provider) -> Result<&'static str, String> {
    effective_upstream(provider).map(|value|value.shadow_protocol()).map_err(|e|e.message)
}

#[cfg(test)]
pub(crate) fn prepare_response_model_probe(
    client: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
    model: &str,
) -> Result<PreparedModelProbe, String> {
    prepare_bounded_model_probe(client,provider,api_key,model,8)
}

/// Protocol cost capability, independent of the user's separate per-route approval.
pub(crate) fn check_model_probe_cost(provider:&Provider,model:&str)->Result<(),String>{
    let upstream=effective_upstream(provider).map_err(|e|e.message)?;
    if matches!(upstream,EffectiveUpstream::Gemini) {
        let mut bounded=provider.clone();
        bounded.model=model.to_owned();
        bounded.advanced.body_overrides.retain(|key,_|key=="model");
        let (_,forwarded)=prepare_payload(&serde_json::to_vec(&json!({"model":model})).map_err(|_|"cost_unknown_reasoning")?,&bounded).map_err(|_|"cost_unknown_reasoning")?;
        // Only these known routes support disabling thinking with a zero budget.
        // Unknown aliases, Pro, and thinking-level-only models fail closed.
        if !matches!(forwarded.strip_prefix("models/").unwrap_or(&forwarded),"gemini-2.5-flash"|"gemini-2.5-flash-lite") {
            return Err("cost_unknown_reasoning".into());
        }
    }
    Ok(())
}

pub(crate) fn prepare_bounded_model_probe(client:&reqwest::Client,provider:&Provider,api_key:&str,model:&str,output_limit:u32)->Result<PreparedModelProbe,String> {
    if !(1..=64).contains(&output_limit) {return Err("无效的输出上限".into());}
    check_model_probe_cost(provider,model)?;
    let mut bounded = provider.clone();
    // The diagnostic model is explicit and does not change the saved provider.
    bounded.model = model.to_owned();
    // Honour a model override, but never let overrides inject tools, history,
    // multiple choices, or an unbounded output budget into a diagnostic request.
    bounded
        .advanced
        .body_overrides
        .retain(|key, _| key == "model");
    let upstream = effective_upstream(&bounded).map_err(|e| e.message)?;
    let prompt = "Reply OK.";
    let (protocol, body) = match upstream {
        EffectiveUpstream::OpenAiResponses => (
            ClientProtocol::Responses,
            json!({
                "model": model, "input": prompt, "max_output_tokens": output_limit,
                "stream": true, "store": false,
            }),
        ),
        EffectiveUpstream::OpenAiChat => (
            ClientProtocol::Chat,
            json!({
                "model": model, "messages": [{"role":"user","content":prompt}],
                "max_completion_tokens": output_limit, "stream": true, "stream_options":{"include_usage":true}, "n":1,
            }),
        ),
        EffectiveUpstream::Anthropic => (
            ClientProtocol::Anthropic,
            json!({
                "model": model, "messages": [{"role":"user","content":prompt}],
                "max_tokens": output_limit, "stream": true,
            }),
        ),
        EffectiveUpstream::Gemini => (
            ClientProtocol::Gemini,
            json!({
                "model": model, "contents":[{"role":"user","parts":[{"text":prompt}]}],
                "generationConfig":{"maxOutputTokens":output_limit,"thinkingConfig":{"thinkingBudget":0}}, "stream": true,
            }),
        ),
    };
    let prepared = prepare_upstream_request(
        &bounded,
        protocol,
        &serde_json::to_vec(&body).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.message)?;
    if !matches!(prepared.url.scheme(), "http" | "https")
        || !prepared.url.username().is_empty()
        || prepared.url.password().is_some()
    {
        return Err("探测端点必须是无内嵌凭据的 HTTP / HTTPS URL".into());
    }
    let format = prepared.upstream.audit_format();
    let protocol = prepared.upstream.shadow_protocol();
    let request = request_builder(
        client,
        &bounded,
        api_key,
        prepared.url,
        &prepared.payload,
        prepared.upstream,
    );
    Ok(PreparedModelProbe {
        request,
        format,
        protocol,
        model: prepared.model,
        observation: prepared.audit,
    })
}

fn build_gemini_url(
    endpoint: &str,
    model: &str,
    operation: &str,
    stream: bool,
) -> Result<reqwest::Url, ProxyError> {
    let mut url = reqwest::Url::parse(endpoint).map_err(|_| {
        ProxyError::new(
            StatusCode::BAD_GATEWAY,
            "invalid_provider_endpoint",
            "The selected provider endpoint is invalid.",
        )
    })?;
    let base = url.path().trim_end_matches('/');
    let model = model.strip_prefix("models/").unwrap_or(model);
    url.set_path(&format!("{base}/v1beta/models/{model}:{operation}"));
    url.set_query(stream.then_some("alt=sse"));
    Ok(url)
}

fn transform_response(transform: ResponseTransform, body: Value, model: &str) -> Value {
    match transform {
        ResponseTransform::None | ResponseTransform::ResponsesToResponses => body,
        ResponseTransform::ChatToResponses => adapters::openai_chat_response_to_responses(body),
        ResponseTransform::ResponsesToChat => adapters::responses_response_to_openai_chat(body),
        ResponseTransform::ResponsesToAnthropic => adapters::openai_response_to_anthropic(
            adapters::responses_response_to_openai_chat(body),
        ),
        ResponseTransform::ResponsesToGemini => {
            adapters::openai_response_to_gemini(adapters::responses_response_to_openai_chat(body))
        }
        ResponseTransform::AnthropicToChat => adapters::anthropic_response_to_openai(body),
        ResponseTransform::AnthropicToResponses => adapters::openai_chat_response_to_responses(
            adapters::anthropic_response_to_openai(body),
        ),
        ResponseTransform::ChatToAnthropic => adapters::openai_response_to_anthropic(body),
        ResponseTransform::GeminiToChat => adapters::gemini_response_to_openai(body, model),
        ResponseTransform::GeminiToAnthropic => {
            adapters::openai_response_to_anthropic(adapters::gemini_response_to_openai(body, model))
        }
        ResponseTransform::GeminiToResponses => adapters::openai_chat_response_to_responses(
            adapters::gemini_response_to_openai(body, model),
        ),
        ResponseTransform::ChatToGemini => adapters::openai_response_to_gemini(body),
        ResponseTransform::AnthropicToGemini => {
            adapters::openai_response_to_gemini(adapters::anthropic_response_to_openai(body))
        }
    }
}

fn stream_formats(transform: ResponseTransform) -> Option<(UpstreamFormat, ClientFormat)> {
    match transform {
        ResponseTransform::None => None,
        ResponseTransform::ResponsesToResponses => {
            Some((UpstreamFormat::Responses, ClientFormat::Responses))
        }
        ResponseTransform::ChatToResponses => {
            Some((UpstreamFormat::OpenAi, ClientFormat::Responses))
        }
        ResponseTransform::ResponsesToChat => Some((UpstreamFormat::Responses, ClientFormat::Chat)),
        ResponseTransform::ResponsesToAnthropic => {
            Some((UpstreamFormat::Responses, ClientFormat::Anthropic))
        }
        ResponseTransform::ResponsesToGemini => {
            Some((UpstreamFormat::Responses, ClientFormat::Gemini))
        }
        ResponseTransform::AnthropicToChat => Some((UpstreamFormat::Anthropic, ClientFormat::Chat)),
        ResponseTransform::AnthropicToResponses => {
            Some((UpstreamFormat::Anthropic, ClientFormat::Responses))
        }
        ResponseTransform::ChatToAnthropic => {
            Some((UpstreamFormat::OpenAi, ClientFormat::Anthropic))
        }
        ResponseTransform::GeminiToChat => Some((UpstreamFormat::Gemini, ClientFormat::Chat)),
        ResponseTransform::GeminiToAnthropic => {
            Some((UpstreamFormat::Gemini, ClientFormat::Anthropic))
        }
        ResponseTransform::GeminiToResponses => {
            Some((UpstreamFormat::Gemini, ClientFormat::Responses))
        }
        ResponseTransform::ChatToGemini => Some((UpstreamFormat::OpenAi, ClientFormat::Gemini)),
        ResponseTransform::AnthropicToGemini => {
            Some((UpstreamFormat::Anthropic, ClientFormat::Gemini))
        }
    }
}

fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn route_name(protocol: ClientProtocol) -> &'static str {
    match protocol {
        ClientProtocol::Chat => "/v1/chat/completions",
        ClientProtocol::Responses => "/v1/responses",
        ClientProtocol::Anthropic => "/v1/messages",
        ClientProtocol::Gemini => "/v1beta/models/:generateContent",
    }
}

fn is_retryable(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

fn should_failover(status: StatusCode) -> bool {
    is_retryable(status) || status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN
}

struct RequestRecord<'a> {
    provider_id: &'a str,
    protocol: ClientProtocol,
    status: Option<StatusCode>,
    latency_ms: i64,
    retry_count: usize,
    error_kind: Option<&'a str>,
    affects_health: bool,
}

fn record_request(database: &Database, record: RequestRecord<'_>) -> Option<RequestRecordResult> {
    let event = crate::database::RequestEvent {
        provider_id: record.provider_id.to_string(),
        route: route_name(record.protocol).to_string(),
        status_code: record.status.map(|status| status.as_u16()),
        success: record.status.is_some_and(|status| status.is_success()),
        affects_health: record.affects_health,
        latency_ms: record.latency_ms,
        retry_count: record.retry_count as i64,
        error_kind: record.error_kind.map(str::to_string),
        model: None,
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        cost: None,
        price_source: None,
        price_version: None,
        created_at: now_millis(),
    };
    match database.record_request(&event) {
        Ok(result) => Some(result),
        Err(error) => {
            tracing::warn!(%error, provider_id = record.provider_id, "failed to record proxy request");
            None
        }
    }
}

fn request_builder(
    client: &reqwest::Client,
    provider: &Provider,
    api_key: &str,
    upstream_url: reqwest::Url,
    payload: &Value,
    upstream: EffectiveUpstream,
) -> reqwest::RequestBuilder {
    let mut request = client
        .post(upstream_url)
        .header(header::ACCEPT, "application/json, text/event-stream");
    request = match upstream {
        EffectiveUpstream::Anthropic => request
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01"),
        EffectiveUpstream::Gemini => request.header("x-goog-api-key", api_key),
        _ => request.bearer_auth(api_key),
    };
    if !provider.advanced.custom_user_agent.is_empty() {
        request = request.header(
            header::USER_AGENT,
            provider.advanced.custom_user_agent.as_str(),
        );
    }
    for (name, value) in &provider.advanced.header_overrides {
        // Authentication is owned by the selected provider credential. Never allow
        // persisted custom headers to replace it with a stale token from another provider.
        if name.eq_ignore_ascii_case(header::AUTHORIZATION.as_str())
            || name.eq_ignore_ascii_case("x-api-key")
            || name.eq_ignore_ascii_case("x-goog-api-key")
        {
            continue;
        }
        request = request.header(name.as_str(), value.as_str());
    }
    request.header("x-xy-switch-loopback", "1").json(payload)
}

async fn buffered_failure(upstream: reqwest::Response) -> Result<Response, ProxyError> {
    let status = upstream.status();
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let request_id = upstream.headers().get("x-request-id").cloned();
    let body = upstream.bytes().await.map_err(|error| {
        ProxyError::new(StatusCode::BAD_GATEWAY, "response_error", error.to_string())
    })?;
    let mut response = Response::builder().status(status);
    if let Some(content_type) = content_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(request_id) = request_id {
        response = response.header("x-request-id", request_id);
    }
    response.body(Body::from(body)).map_err(|error| {
        ProxyError::new(StatusCode::BAD_GATEWAY, "response_error", error.to_string())
    })
}

fn non_negative_i64(value: Option<&Value>) -> Option<i64> {
    value.and_then(Value::as_i64).filter(|value| *value >= 0)
}

fn extract_non_stream_usage(body: &Value, fallback_model: &str) -> RequestUsage {
    let model = body
        .get("model")
        .or_else(|| body.get("modelVersion"))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(fallback_model)
        .to_string();
    let usage = body.get("usage");
    let metadata = body.get("usageMetadata");
    let input_tokens = non_negative_i64(
        usage
            .and_then(|v| v.get("prompt_tokens"))
            .or_else(|| usage.and_then(|v| v.get("input_tokens")))
            .or_else(|| metadata.and_then(|v| v.get("promptTokenCount"))),
    );
    let output_tokens = non_negative_i64(
        usage
            .and_then(|v| v.get("completion_tokens"))
            .or_else(|| usage.and_then(|v| v.get("output_tokens")))
            .or_else(|| metadata.and_then(|v| v.get("candidatesTokenCount"))),
    );
    let cache_creation_tokens =
        non_negative_i64(usage.and_then(|v| v.get("cache_creation_input_tokens")));
    let cache_read_tokens = non_negative_i64(
        usage
            .and_then(|v| v.get("cache_read_input_tokens"))
            .or_else(|| {
                usage
                    .and_then(|v| v.get("prompt_tokens_details"))
                    .and_then(|v| v.get("cached_tokens"))
            })
            .or_else(|| {
                usage
                    .and_then(|v| v.get("input_tokens_details"))
                    .and_then(|v| v.get("cached_tokens"))
            })
            .or_else(|| metadata.and_then(|v| v.get("cachedContentTokenCount"))),
    );
    let cost = crate::session_usage_codex::estimate_codex_cost(
        &model,
        input_tokens.unwrap_or(0),
        output_tokens.unwrap_or(0),
        cache_read_tokens.unwrap_or(0),
    );
    RequestUsage {
        model: Some(model),
        input_tokens,
        output_tokens,
        cache_creation_tokens,
        cache_read_tokens,
        cost: (cost > 0.0).then_some(cost),
        price_source: (cost > 0.0).then(|| "standard".to_string()),
        price_version: (cost > 0.0).then(|| "v1".to_string()),
        ..Default::default()
    }
}

fn persist_non_stream_usage(
    database: &Database,
    event_id: Option<i64>,
    body: &Value,
    fallback_model: &str,
    format: UpstreamFormat,
    observation: crate::model_audit::Observation,
    shadow: Option<crate::shadow_audit_runtime::ShadowCapture>,
) {
    let Some(event_id) = event_id else {
        return;
    };
    let mut usage = extract_non_stream_usage(body, fallback_model);
    let collector =
        streaming::audit_capture::Collector::new(format, observation).capture_non_stream(body);
    collector.publish_shadow(database, Some(event_id), shadow);
    usage.audit = Some(crate::model_audit::audit(collector.observation));
    if let Err(error) = database.update_request_usage(event_id, &usage) {
        eprintln!("failed to update request usage for event {event_id}: {error}");
    }
}
async fn finalize_upstream(
    database: Database,
    event_id: Option<i64>,
    model: String,
    transform: ResponseTransform,
    stream: bool,
    upstream_kind: EffectiveUpstream,
    observation: crate::model_audit::Observation,
    started: Instant,
    shadow: Option<crate::shadow_audit_runtime::ShadowCapture>,
    upstream: reqwest::Response,
    tool_bridge: crate::tool_bridge::ToolBridge,
) -> Result<Response, ProxyError> {
    let status = upstream.status();
    let mut observation = observation;
    if observation.verification_capture.as_ref().is_some_and(|c|c.store_diagnostic_ids) {
        observation.provider_request_id = ["x-request-id","request-id","x-goog-request-id"].iter()
            .find_map(|key|upstream.headers().get(*key).and_then(|v|v.to_str().ok()).and_then(crate::model_verification::safe_id));
    }
    let request_id = upstream.headers().get("x-request-id").cloned();
    let upstream_format = upstream_kind.audit_format();
    if stream
        && status.is_success()
        && let Some((upstream_format, client_format)) = stream_formats(transform)
    {
        let transformed = streaming::transform_with_tools(
            streaming::audit_capture::observe_stream_with_shadow(
                upstream.bytes_stream(),
                upstream_format,
                observation,
                model.clone(),
                database.clone(),
                event_id,
                started,
                shadow,
            ),
            upstream_format,
            client_format,
            model.clone(),
            tool_bridge.clone(),
            None,
            None,
        );
        let mut response = Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache");
        if let Some(request_id) = request_id {
            response = response.header("x-request-id", request_id);
        }
        return response
            .body(Body::from_stream(transformed))
            .map_err(|error| {
                ProxyError::new(StatusCode::BAD_GATEWAY, "response_error", error.to_string())
            });
    }
    if !stream && status.is_success() {
        let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
        let bytes = match upstream.bytes().await {
            Ok(bytes) => bytes,
            Err(error) => {
                persist_non_stream_usage(
                    &database,
                    event_id,
                    &Value::Null,
                    &model,
                    upstream_format,
                    observation,
                    shadow,
                );
                return Err(ProxyError::new(
                    StatusCode::BAD_GATEWAY,
                    "response_error",
                    error.to_string(),
                ));
            }
        };
        match serde_json::from_slice::<Value>(&bytes) {
            Ok(upstream_body) => {
                persist_non_stream_usage(
                    &database,
                    event_id,
                    &upstream_body,
                    &model,
                    upstream_format,
                    observation,
                    shadow,
                );
                if transform != ResponseTransform::None {
                    let mut body = transform_response(transform, upstream_body, &model);
                    tool_bridge
                        .restore_response(&mut body)
                        .map_err(adapter_error)?;
                    let mut response = Json(body).into_response();
                    *response.status_mut() = status;
                    if let Some(request_id) = request_id {
                        response.headers_mut().insert("x-request-id", request_id);
                    }
                    return Ok(response);
                }
            }
            Err(_) if transform != ResponseTransform::None => {
                persist_non_stream_usage(
                    &database,
                    event_id,
                    &Value::Null,
                    &model,
                    upstream_format,
                    observation,
                    shadow,
                );
                return Err(ProxyError::new(
                    StatusCode::BAD_GATEWAY,
                    "invalid_upstream_response",
                    "The provider returned invalid JSON.",
                ));
            }
            Err(_) => {
                persist_non_stream_usage(
                    &database,
                    event_id,
                    &Value::Null,
                    &model,
                    upstream_format,
                    observation,
                    shadow,
                );
            }
        }
        let mut response = Response::builder().status(status);
        if let Some(content_type) = content_type {
            response = response.header(header::CONTENT_TYPE, content_type);
        }
        if let Some(request_id) = request_id {
            response = response.header("x-request-id", request_id);
        }
        return response.body(Body::from(bytes)).map_err(|error| {
            ProxyError::new(StatusCode::BAD_GATEWAY, "response_error", error.to_string())
        });
    }
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let stream = streaming::audit_capture::observe_stream_with_shadow(
        upstream.bytes_stream(),
        upstream_format,
        observation,
        model,
        database,
        event_id,
        started,
        shadow,
    )
    .map_err(|error| io::Error::other(error.to_string()));
    let mut response = Response::builder().status(status);
    if let Some(content_type) = content_type {
        response = response.header(header::CONTENT_TYPE, content_type);
    }
    if let Some(request_id) = request_id {
        response = response.header("x-request-id", request_id);
    }
    response.body(Body::from_stream(stream)).map_err(|error| {
        ProxyError::new(StatusCode::BAD_GATEWAY, "response_error", error.to_string())
    })
}

async fn forward(
    state: ProxyState,
    protocol: ClientProtocol,
    headers: axum::http::HeaderMap,
    body: Bytes,
    requested_model: Option<String>,
) -> Result<Response, ProxyError> {
    if headers.contains_key("x-xy-switch-loopback") {
        return Err(ProxyError::new(
            StatusCode::LOOP_DETECTED,
            "loop_detected",
            "Infinite proxy loop detected",
        ));
    }
    let selected = state
        .database
        .current_provider()
        .map_err(|error| {
            ProxyError::new(StatusCode::INTERNAL_SERVER_ERROR, "database_error", error)
        })?
        .ok_or_else(|| {
            ProxyError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                "provider_not_selected",
                "No provider is selected.",
            )
        })?;
    if !selected.enabled {
        return Err(ProxyError::new(
            StatusCode::SERVICE_UNAVAILABLE,
            "provider_disabled",
            "The selected provider is disabled.",
        ));
    }

    let mut candidates: Vec<Provider> = state
        .database
        .list_providers()
        .map_err(|error| {
            ProxyError::new(StatusCode::INTERNAL_SERVER_ERROR, "database_error", error)
        })?
        .into_iter()
        .filter(|provider| provider.enabled)
        .collect();
    candidates.sort_by_key(|provider| if provider.id == selected.id { 0 } else { 1 });

    let mut last_failure = None;
    let mut last_error = None;
    let mut found_key = false;
    let mut circuits_open = 0;
    for (provider_index, provider) in candidates.into_iter().enumerate() {
        if let (Some(local), Ok(endpoint_url)) =
            (state.local_addr, reqwest::Url::parse(&provider.endpoint))
        {
            let host = endpoint_url.host_str().unwrap_or("");
            let port = endpoint_url
                .port()
                .unwrap_or_else(|| match endpoint_url.scheme() {
                    "https" => 443,
                    _ => 80,
                });
            let is_local_host = host == "localhost"
                || host == "127.0.0.1"
                || host == "::1"
                || host == "0.0.0.0"
                || host == local.ip().to_string();

            if is_local_host && port == local.port() {
                tracing::warn!(provider=%provider.id, "Proxy loopback detected, skipping provider");
                last_error = Some("配置错误：供应商端点指向了本地代理自身，构成死循环".to_string());
                continue;
            }
        }

        if state
            .database
            .provider_health(&provider.id)
            .map_err(|error| {
                ProxyError::new(StatusCode::INTERNAL_SERVER_ERROR, "database_error", error)
            })?
            .and_then(|health| health.circuit_open_until)
            .is_some_and(|until| until > now_millis())
        {
            circuits_open += 1;
            continue;
        }
        let Some(api_key) = state.api_keys.get(&provider.id).map_err(|error| {
            ProxyError::new(StatusCode::INTERNAL_SERVER_ERROR, "credential_error", error)
        })?
        else {
            continue;
        };
        found_key = true;
        let client_shadow =
            state
                .database
                .shadow
                .begin_capture(&provider.id, protocol.shadow_protocol(), &body);
        let mut prepared = prepare_upstream_request(&provider, protocol, &body)?;
        if requested_model.is_some() {
            prepared.audit.requested_model = requested_model.clone();
        }
        prepared.audit.verification_capture = state.database.verification.capture(
            &provider, &api_key, prepared.upstream.shadow_protocol(),
            prepared.audit.requested_model.as_deref().unwrap_or(&provider.model));
        let mut shadow = client_shadow.and_then(|client| {
            prepared.audit.forwarded_model.as_deref().and_then(|model| {
                state.database.shadow.capture(
                    client,
                    &provider.id,
                    model,
                    prepared.audit.requested_model.as_deref(),
                    prepared.url.as_str(),
                    prepared.upstream.shadow_protocol(),
                    &prepared.payload,
                )
            })
        });
        let tool_bridge = if protocol == ClientProtocol::Responses
            && prepared.transform != ResponseTransform::None
        {
            crate::tool_bridge::ToolBridge::from_request(
                &serde_json::from_slice::<Value>(&body)
                    .map_err(|e| adapter_error(e.to_string()))?,
            )
            .map_err(adapter_error)?
        } else {
            crate::tool_bridge::ToolBridge::default()
        };
        let mut retry_count = 0usize;
        loop {
            let started = Instant::now();
            let result = request_builder(
                &state.client,
                &provider,
                &api_key,
                prepared.url.clone(),
                &prepared.payload,
                prepared.upstream,
            )
            .send()
            .await;
            let latency_ms = started.elapsed().as_millis() as i64;
            if (provider_index > 0 || retry_count > 0)
                && let Some(capture) = shadow.as_mut()
            {
                capture.mark_unreliable();
            }
            match result {
                Ok(upstream) => {
                    let status = upstream.status();
                    if is_retryable(status) && retry_count < 2 {
                        retry_count += 1;
                        tokio::time::sleep(Duration::from_millis(
                            RETRY_BASE_DELAY_MS * retry_count as u64,
                        ))
                        .await;
                        continue;
                    }
                    let affects_health = status.is_success() || should_failover(status);
                    let record_result = record_request(
                        &state.database,
                        RequestRecord {
                            provider_id: &provider.id,
                            protocol,
                            status: Some(status),
                            latency_ms,
                            retry_count,
                            error_kind: (!status.is_success()).then_some("upstream_status"),
                            affects_health,
                        },
                    );
                    if !status.is_success() {
                        persist_non_stream_usage(
                            &state.database,
                            record_result.map(|result| result.event_id),
                            &Value::Null,
                            &prepared.model,
                            prepared.upstream.audit_format(),
                            prepared.audit.clone(),
                            shadow.take(),
                        );
                    }
                    if status.is_success() {
                        let _ = state.database.set_circuit_open_until(&provider.id, None);
                        return finalize_upstream(
                            state.database.clone(),
                            record_result.map(|result| result.event_id),
                            prepared.model,
                            prepared.transform,
                            prepared.stream,
                            prepared.upstream,
                            prepared.audit,
                            started,
                            shadow,
                            upstream,
                            tool_bridge.clone(),
                        )
                        .await;
                    }
                    let failures = record_result
                        .map(|result| result.consecutive_failures)
                        .unwrap_or(0);
                    if failures >= 3 && affects_health {
                        let _ = state
                            .database
                            .set_circuit_open_until(&provider.id, Some(now_millis() + 60_000));
                    }
                    if !should_failover(status) {
                        return finalize_upstream(
                            state.database.clone(),
                            record_result.map(|result| result.event_id),
                            prepared.model,
                            prepared.transform,
                            prepared.stream,
                            prepared.upstream,
                            prepared.audit,
                            started,
                            shadow,
                            upstream,
                            tool_bridge.clone(),
                        )
                        .await;
                    }
                    last_failure = Some(buffered_failure(upstream).await?);
                    break;
                }
                Err(error) => {
                    if retry_count < 2 {
                        retry_count += 1;
                        tokio::time::sleep(Duration::from_millis(
                            RETRY_BASE_DELAY_MS * retry_count as u64,
                        ))
                        .await;
                        continue;
                    }
                    let error_kind = if error.is_timeout() {
                        "timeout"
                    } else if error.is_connect() {
                        "connect"
                    } else {
                        "request"
                    };
                    let failures = record_request(
                        &state.database,
                        RequestRecord {
                            provider_id: &provider.id,
                            protocol,
                            status: None,
                            latency_ms,
                            retry_count,
                            error_kind: Some(error_kind),
                            affects_health: true,
                        },
                    );
                    persist_non_stream_usage(
                        &state.database,
                        failures.map(|result| result.event_id),
                        &Value::Null,
                        &prepared.model,
                        prepared.upstream.audit_format(),
                        prepared.audit.clone(),
                        shadow.take(),
                    );
                    let failures = failures
                        .map(|result| result.consecutive_failures)
                        .unwrap_or(0);
                    if failures >= 3 {
                        let _ = state
                            .database
                            .set_circuit_open_until(&provider.id, Some(now_millis() + 60_000));
                    }
                    last_error = Some(crate::network::describe_error(error));
                    break;
                }
            }
        }
    }

    if let Some(response) = last_failure {
        return Ok(response);
    }
    if !found_key && circuits_open == 0 {
        return Err(ProxyError::new(
            StatusCode::UNAUTHORIZED,
            "api_key_missing",
            "No enabled provider has an API key configured.",
        ));
    }
    Err(ProxyError::new(
        StatusCode::SERVICE_UNAVAILABLE,
        if circuits_open > 0 {
            "circuit_open"
        } else {
            "upstream_unavailable"
        },
        last_error.unwrap_or_else(|| "No provider is currently available.".to_string()),
    ))
}

async fn chat_completions(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Response, ProxyError> {
    forward(state, ClientProtocol::Chat, headers, body, None).await
}

async fn responses(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Response, ProxyError> {
    forward(state, ClientProtocol::Responses, headers, body, None).await
}

async fn messages(
    State(state): State<ProxyState>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Response, ProxyError> {
    forward(state, ClientProtocol::Anthropic, headers, body, None).await
}

fn gemini_operation(operation: &str) -> Result<(&str, bool), ProxyError> {
    let invalid = || ProxyError::new(StatusCode::NOT_FOUND, "unsupported_operation", "Only generateContent and streamGenerateContent are supported.");
    let (model, method) = operation.rsplit_once(':').ok_or_else(invalid)?;
    if model.is_empty() || !model.bytes().all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c)) { return Err(invalid()); }
    let stream = match method { "generateContent" => false, "streamGenerateContent" => true, _ => return Err(invalid()) };
    Ok((model, stream))
}

async fn gemini_generate(
    State(state): State<ProxyState>,
    Path(operation): Path<String>,
    headers: axum::http::HeaderMap,
    body: Bytes,
) -> Result<Response, ProxyError> {
    let (requested_model, stream) = gemini_operation(&operation)?;
    let requested_model = Some(requested_model.to_owned());
    let mut payload: Value = serde_json::from_slice(&body).map_err(|_| {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "Request body must be a JSON object.",
        )
    })?;
    let object = payload.as_object_mut().ok_or_else(|| {
        ProxyError::new(
            StatusCode::BAD_REQUEST,
            "invalid_json",
            "Request body must be a JSON object.",
        )
    })?;
    object.insert(
        "stream".to_string(),
        Value::Bool(stream),
    );
    let bytes = serde_json::to_vec(&payload).map_err(|error| {
        ProxyError::new(StatusCode::BAD_REQUEST, "invalid_json", error.to_string())
    })?;

    forward(
        state,
        ClientProtocol::Gemini,
        headers,
        Bytes::from(bytes),
        requested_model,
    )
    .await
}

async fn list_models(State(state): State<ProxyState>) -> Result<Response, ProxyError> {
    let mut model_ids = Vec::new();
    if let Some(provider) = state.database.current_provider()
        .map_err(|error| ProxyError::new(StatusCode::INTERNAL_SERVER_ERROR, "database_error", error))? {
        if !provider.model.trim().is_empty() {
            model_ids.push(provider.model.clone());
        }
        for mapping in &provider.advanced.model_mappings {
            if !mapping.model.trim().is_empty() && !model_ids.contains(&mapping.model) {
                model_ids.push(mapping.model.clone());
            }
        }
    }
    let data: Vec<Value> = model_ids
        .into_iter()
        .map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 1700000000,
                "owned_by": "xy-switch"
            })
        })
        .collect();
    let body = json!({
        "object": "list",
        "data": data
    });
    Ok(Json(body).into_response())
}

/// 按「协议路由目标」组装 axum 路由表。
///
/// routeTargets 以前只是原样存进数据库，router 无条件注册了全部端点，
/// 设置页上的那几个勾选框点了等于没点。这里按目标裁剪端点：
/// 被取消勾选的协议不再挂载，客户端打过来会收到 404，
/// 这也正是"不接管该协议"应有的表现。
fn router_with_targets(state: ProxyState, targets: &[String]) -> Router {
    // 空列表视为全开：旧数据、单测和 `serve` 的兼容入口都走这一条，
    // 免得升级后有人的代理突然一个端点都不提供。
    let enable_all = targets.is_empty();
    let has = |name: &str| enable_all || targets.iter().any(|item| item.eq_ignore_ascii_case(name));

    let mut router = Router::new()
        // /health 与模型列表始终提供：它们是客户端探活和模型下拉的依赖，
        // 关掉只会让人以为代理没起来。
        .route("/health", get(health))
        .route("/v1/models", get(list_models))
        .route("/models", get(list_models));

    // OpenAI Chat Completions：Grok Build、OpenCode 等客户端走这里。
    if has("grok") || has("opencode") {
        router = router
            .route("/v1/chat/completions", post(chat_completions))
            .route("/chat/completions", post(chat_completions));
    }
    // OpenAI Responses：Codex / ChatGPT 桌面端走这里。
    if has("codex") {
        router = router
            .route("/v1/responses", post(responses))
            .route("/responses", post(responses));
    }
    // Anthropic Messages：Claude Code 走这里。
    if has("claude") {
        router = router
            .route("/v1/messages", post(messages))
            .route("/messages", post(messages));
    }
    // Gemini generateContent / streamGenerateContent。
    if has("gemini") {
        router = router.route("/v1beta/models/{*operation}", post(gemini_generate));
    }

    router
        .layer(axum::middleware::from_fn_with_state(state.local_addr, crate::proxy_request_guard::protect))
        .layer(DefaultBodyLimit::max(MAX_REQUEST_BYTES))
        .with_state(state)
}

/// 全端点 router，仅供单测使用（`serve` 走 `router_with_targets`）。
#[cfg(test)]
fn router_with_state(state: ProxyState) -> Router {
    router_with_targets(state, &[])
}

pub async fn serve(
    listener: tokio::net::TcpListener,
    database: Database,
    route_targets: Vec<String>,
) -> Result<(), io::Error> {
    let addr = listener.local_addr()?;
    let state = ProxyState::new(database, Some(addr)).map_err(io::Error::other)?;
    tracing::info!(%addr, targets = ?route_targets, "local proxy listening");
    axum::serve(listener, router_with_targets(state, &route_targets)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;

    struct FakeApiKeyProvider;

    impl ApiKeyProvider for FakeApiKeyProvider {
        fn get(&self, _: &str) -> Result<Option<String>, String> {
            Ok(Some("test-api-key".to_string()))
        }
    }

    struct ProviderScopedApiKeyProvider;

    impl ApiKeyProvider for ProviderScopedApiKeyProvider {
        fn get(&self, provider_id: &str) -> Result<Option<String>, String> {
            Ok(Some(format!("{provider_id}-test-token")))
        }
    }
    fn test_database() -> (Database, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("xy-switch-proxy-{}.db", uuid::Uuid::new_v4()));
        (Database::open(&path).unwrap(), path)
    }

    /// routeTargets 以前只存不用，设置页勾选框是摆设。这里锁住裁剪行为。
    #[tokio::test]
    async fn route_targets_gate_the_exposed_protocol_endpoints() {
        let (database, path) = test_database();
        // Database::open seeds live endpoints. This routing-only test must not
        // make requests to those endpoints with its placeholder credentials.
        for provider in database.list_providers().unwrap() {
            database.delete_provider(&provider.id).unwrap();
        }
        assert!(database.list_providers().unwrap().is_empty());
        let state = ProxyState {
            database: database.clone(),
            client: crate::network::test_client(),
            api_keys: Arc::new(FakeApiKeyProvider),
            local_addr: None,
        };
        // 只勾选 codex：Responses 端点保留，Messages / Chat 应当整体消失。
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = router_with_targets(state, &["codex".to_string()]);
        let task = tokio::spawn(async move { axum::serve(listener, router).await });

        let client = crate::network::test_client();
        // 探活和模型列表不受路由目标影响，否则客户端会误判代理没起来。
        let health = client
            .get(format!("http://{address}/health"))
            .send()
            .await
            .unwrap();
        assert_eq!(health.status(), StatusCode::OK);

        for suffix in [
            "/v1/messages",
            "/messages",
            "/v1/chat/completions",
            "/chat/completions",
        ] {
            let response = client
                .post(format!("http://{address}{suffix}"))
                .json(&json!({"model":"x"}))
                .send()
                .await
                .unwrap();
            assert_eq!(
                response.status(),
                StatusCode::NOT_FOUND,
                "{suffix} 未被路由目标关闭"
            );
        }

        // Responses 仍然挂载：没配供应商所以会报业务错误，但绝不该是 404。
        let responses = client
            .post(format!("http://{address}/v1/responses"))
            .json(&json!({"model":"x","input":"hi"}))
            .send()
            .await
            .unwrap();
        assert_eq!(responses.status(), StatusCode::SERVICE_UNAVAILABLE);
        let error = responses.json::<Value>().await.unwrap();
        assert_eq!(error["error"]["type"], "provider_not_selected");

        task.abort();
        let _ = task.await;
        drop(database);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn extracts_real_usage_and_preserves_unknown_values() {
        let cases = [
            (
                json!({"model":"gpt-real","usage":{"prompt_tokens":11,"completion_tokens":7,"prompt_tokens_details":{"cached_tokens":3}}}),
                "fallback",
                ("gpt-real", Some(11), Some(7), None, Some(3)),
            ),
            (
                json!({"model":"responses-real","usage":{"input_tokens":13,"output_tokens":5,"input_tokens_details":{"cached_tokens":2}}}),
                "fallback",
                ("responses-real", Some(13), Some(5), None, Some(2)),
            ),
            (
                json!({"model":"claude-real","usage":{"input_tokens":17,"output_tokens":9,"cache_creation_input_tokens":4,"cache_read_input_tokens":6}}),
                "fallback",
                ("claude-real", Some(17), Some(9), Some(4), Some(6)),
            ),
            (
                json!({"modelVersion":"gemini-real","usageMetadata":{"promptTokenCount":19,"candidatesTokenCount":8,"cachedContentTokenCount":5}}),
                "fallback",
                ("gemini-real", Some(19), Some(8), None, Some(5)),
            ),
        ];
        for (body, fallback, expected) in cases {
            let usage = extract_non_stream_usage(&body, fallback);
            assert_eq!(usage.model.as_deref(), Some(expected.0));
            assert_eq!(usage.input_tokens, expected.1);
            assert_eq!(usage.output_tokens, expected.2);
            assert_eq!(usage.cache_creation_tokens, expected.3);
            assert_eq!(usage.cache_read_tokens, expected.4);
            assert_eq!(usage.cost, None);
        }

        let unknown = extract_non_stream_usage(&json!({}), "sent-model");
        assert_eq!(unknown.model.as_deref(), Some("sent-model"));
        assert_eq!(unknown.input_tokens, None);
        assert_eq!(unknown.output_tokens, None);
        assert_eq!(unknown.cache_creation_tokens, None);
        assert_eq!(unknown.cache_read_tokens, None);
        assert_eq!(unknown.cost, None);
        assert_eq!(unknown.price_source, None);
        assert_eq!(unknown.price_version, None);

        let zero = extract_non_stream_usage(
            &json!({"usage":{"input_tokens":0,"output_tokens":0,"cache_creation_input_tokens":0,"cache_read_input_tokens":0}}),
            "zero-model",
        );
        assert_eq!(zero.input_tokens, Some(0));
        assert_eq!(zero.output_tokens, Some(0));
        assert_eq!(zero.cache_creation_tokens, Some(0));
        assert_eq!(zero.cache_read_tokens, Some(0));

        let invalid = extract_non_stream_usage(
            &json!({"usage":{"input_tokens":-1,"output_tokens":"2"}}),
            "fallback",
        );
        assert_eq!(invalid.input_tokens, None);
        assert_eq!(invalid.output_tokens, None);
    }
    #[test]
    fn provider_switch_uses_current_provider_credential_and_preserves_previous_credential() {
        let (database, path) = test_database();
        for (id, endpoint, model, sort_order) in [
            ("grok", "https://api.x.ai/v1", "grok-test-model", 1),
            (
                "openai",
                "https://api.openai.com/v1",
                "openai-test-model",
                2,
            ),
        ] {
            database
                .save_provider(&Provider {
                    id: id.to_string(),
                    name: id.to_string(),
                    provider_type: "openai_compatible".to_string(),
                    endpoint: endpoint.to_string(),
                    model: model.to_string(),
                    app_group: "Codex".to_string(),
                    enabled: true,
                    sort_order,
                    advanced: Default::default(),
                })
                .unwrap();
        }

        let credentials = ProviderScopedApiKeyProvider;
        for (selected_id, expected_token) in [
            ("grok", "Bearer grok-test-token"),
            ("openai", "Bearer openai-test-token"),
            ("grok", "Bearer grok-test-token"),
        ] {
            database.set_current_provider(selected_id).unwrap();
            let selected = database.current_provider().unwrap().unwrap();
            assert_eq!(selected.id, selected_id);

            let api_key = credentials.get(&selected.id).unwrap().unwrap();
            let request = request_builder(
                &crate::network::test_client(),
                &selected,
                &api_key,
                build_upstream_url(&selected.endpoint, "/v1/chat/completions").unwrap(),
                &json!({"model": selected.model, "messages": []}),
                EffectiveUpstream::OpenAiChat,
            )
            .build()
            .unwrap();
            assert_eq!(
                request.headers().get(header::AUTHORIZATION).unwrap(),
                expected_token
            );
        }

        assert!(database.get_provider("grok").unwrap().is_some());
        assert!(credentials.get("grok").unwrap().is_some());
        drop(database);
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn builds_paths_for_root_and_versioned_endpoints() {
        // Root endpoint: route appended as-is
        assert_eq!(
            build_upstream_url("https://api.example.com", "/v1/chat/completions")
                .unwrap()
                .as_str(),
            "https://api.example.com/v1/chat/completions"
        );
        // Custom path prefix: /v1 from route is preserved (not stripped)
        assert_eq!(
            build_upstream_url("https://api.example.com/v4/", "/v1/chat/completions")
                .unwrap()
                .as_str(),
            "https://api.example.com/v4/chat/completions"
        );
        // Deduplication: /v1 endpoint + /v1 route should not produce /v1/v1
        assert_eq!(
            build_upstream_url("https://api.example.com/v1", "/v1/chat/completions")
                .unwrap()
                .as_str(),
            "https://api.example.com/v1/chat/completions"
        );
        // Gemini /v1beta deduplication
        assert_eq!(
            build_upstream_url(
                "https://generativelanguage.googleapis.com/v1beta",
                "/v1beta/models/streamGenerateContent"
            )
            .unwrap()
            .as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/streamGenerateContent"
        );
        // Gemini root endpoint: v1beta route appended correctly
        assert_eq!(
            build_upstream_url(
                "https://generativelanguage.googleapis.com",
                "/v1beta/models/generateContent"
            )
            .unwrap()
            .as_str(),
            "https://generativelanguage.googleapis.com/v1beta/models/generateContent"
        );
        // Query parameters in endpoint are preserved (e.g. Azure OpenAI)
        assert_eq!(
            build_upstream_url(
                "https://my-corp.openai.azure.com/openai/deployments/gpt-4o?api-version=2024-02-15",
                "/v1/chat/completions"
            )
            .unwrap()
            .as_str(),
            "https://my-corp.openai.azure.com/openai/deployments/gpt-4o/chat/completions?api-version=2024-02-15"
        );
        // Endpoint ending in full /chat/completions does NOT duplicate
        assert_eq!(
            build_upstream_url(
                "https://gateway.example.com/v1/chat/completions",
                "/v1/chat/completions"
            )
            .unwrap()
            .as_str(),
            "https://gateway.example.com/v1/chat/completions"
        );
        // Endpoint ending in /chat/completions with conflicting route /v1/responses strips the conflicting suffix
        assert_eq!(
            build_upstream_url(
                "https://gateway.example.com/v1/chat/completions",
                "/v1/responses"
            )
            .unwrap()
            .as_str(),
            "https://gateway.example.com/v1/responses"
        );
        // Anthropic /v1/messages endpoint does not duplicate
        assert_eq!(
            build_upstream_url("https://api.anthropic.com/v1/messages", "/v1/messages")
                .unwrap()
                .as_str(),
            "https://api.anthropic.com/v1/messages"
        );
    }

    #[test]
    fn replaces_client_model_with_selected_provider_model() {
        let provider = Provider {
            id: "mock".to_string(),
            name: "Mock".to_string(),
            provider_type: "custom".to_string(),
            endpoint: "https://example.com".to_string(),
            model: "configured-model".to_string(),
            app_group: "Codex".to_string(),
            enabled: true,
            sort_order: 1,
            advanced: Default::default(),
        };
        let (payload, model) =
            prepare_payload(br#"{"model":"client-model","messages":[]}"#, &provider).unwrap();
        assert_eq!(payload["model"], "configured-model");
        assert_eq!(model, "configured-model");
    }

    /// 造一个原生 Responses 供应商，用来验证 xAI 清洗闸门。
    fn responses_provider(endpoint: &str, model: &str) -> Provider {
        let mut provider = Provider {
            id: "p".to_string(),
            name: "P".to_string(),
            provider_type: "custom".to_string(),
            endpoint: endpoint.to_string(),
            model: model.to_string(),
            app_group: "Codex".to_string(),
            enabled: true,
            sort_order: 1,
            advanced: Default::default(),
        };
        provider.advanced.upstream_format = "openai_responses".to_string();
        provider
    }

    #[test]
    fn xai_sanitizer_runs_only_for_xai_native_responses_targets() {
        // 一个同时踩中「根级 oneOf」和「max 档」的请求体。
        let body = br#"{
            "model":"grok-4.5",
            "reasoning":{"effort":"max"},
            "tools":[{"type":"function","name":"f","parameters":{"oneOf":[
                {"type":"object","properties":{"a":{"type":"string"}},"required":["a"]},
                {"type":"object","properties":{"b":{"type":"string"}},"required":["a","b"]}
            ]}}]
        }"#;

        // xAI 卡：折叠 + 夹档都要发生。
        let xai = responses_provider("https://api.x.ai/v1", "grok-4.5");
        let prepared = prepare_upstream_request(&xai, ClientProtocol::Responses, body).unwrap();
        let parameters = &prepared.payload["tools"][0]["parameters"];
        assert!(parameters.get("oneOf").is_none());
        assert_eq!(parameters["required"], json!(["a"]));
        assert_eq!(prepared.payload["reasoning"]["effort"], json!("xhigh"));

        // DeepSeek 官方 Codex 同样是原生 Responses，但不该被这套 xAI 专用清洗碰到。
        let deepseek = responses_provider("https://api.deepseek.com/v1", "deepseek-v4-pro");
        let prepared =
            prepare_upstream_request(&deepseek, ClientProtocol::Responses, body).unwrap();
        assert!(
            prepared.payload["tools"][0]["parameters"]
                .get("oneOf")
                .is_some(),
            "非 xAI 的原生 Responses 供应商不应被改写"
        );
    }

    #[test]
    fn xai_card_passes_grok_models_through_but_still_locks_other_models() {
        let xai = responses_provider("https://api.x.ai/v1", "grok-4.5");
        // 映射表里没有 grok-4.6-fast，但它是 grok-*，原样放行。
        let (payload, model) =
            prepare_payload(br#"{"model":"grok-4.6-fast","input":[]}"#, &xai).unwrap();
        assert_eq!(model, "grok-4.6-fast");
        assert_eq!(payload["model"], "grok-4.6-fast");
        // 非 grok 模型仍然锁回卡里配置的模型。
        let (_, model) = prepare_payload(br#"{"model":"gpt-5.6","input":[]}"#, &xai).unwrap();
        assert_eq!(model, "grok-4.5");
        // 别的卡上发 grok-* 不放行，不能拿它绕过模型锁定。
        let other = responses_provider("https://api.deepseek.com/v1", "deepseek-v4-pro");
        let (_, model) = prepare_payload(br#"{"model":"grok-4.5","input":[]}"#, &other).unwrap();
        assert_eq!(model, "deepseek-v4-pro");
    }

    #[test]
    fn disable_artifact_tool_removes_only_the_artifact_tool() {
        let mut provider = Provider {
            id: "claude".to_string(),
            name: "Claude".to_string(),
            provider_type: "anthropic".to_string(),
            endpoint: "https://example.com".to_string(),
            model: "configured-model".to_string(),
            app_group: "Claude".to_string(),
            enabled: true,
            sort_order: 1,
            advanced: crate::database::ProviderAdvancedConfig {
                upstream_format: "anthropic".to_string(),
                ..Default::default()
            },
        };
        // 三种协议的 tools 形状：Anthropic / Responses 扁平，Chat 嵌在 function 里。
        let anthropic = br#"{"model":"m","tools":[{"name":"Artifact","input_schema":{}},{"name":"Bash","input_schema":{}}],"tool_choice":{"type":"tool","name":"Artifact"},"messages":[]}"#;
        let chat = br#"{"model":"m","tools":[{"type":"function","function":{"name":"Artifact"}},{"type":"function","function":{"name":"Bash"}}],"messages":[]}"#;
        let only_artifact =
            br#"{"model":"m","tools":[{"name":"artifact","input_schema":{}}],"messages":[]}"#;

        // 关闭开关（默认）时请求原样通过，一个工具都不能少。
        for body in [
            anthropic.as_slice(),
            chat.as_slice(),
            only_artifact.as_slice(),
        ] {
            let (payload, _) = prepare_payload(body, &provider).unwrap();
            let original: Value = serde_json::from_slice(body).unwrap();
            assert_eq!(payload["tools"], original["tools"]);
            assert_eq!(payload["tool_choice"], original["tool_choice"]);
        }

        provider.advanced.claude.disable_artifact_tool = true;

        let (payload, _) = prepare_payload(anthropic, &provider).unwrap();
        assert_eq!(payload["tools"].as_array().unwrap().len(), 1);
        assert_eq!(payload["tools"][0]["name"], "Bash");
        // 指向已删工具的 tool_choice 必须一起摘掉，否则上游还是 400。
        assert!(payload.get("tool_choice").is_none());

        let (payload, _) = prepare_payload(chat, &provider).unwrap();
        assert_eq!(payload["tools"].as_array().unwrap().len(), 1);
        assert_eq!(payload["tools"][0]["function"]["name"], "Bash");

        // 摘完没有工具剩下时删掉整个 tools 字段，而不是留一个空数组。
        let (payload, _) = prepare_payload(only_artifact, &provider).unwrap();
        assert!(payload.get("tools").is_none());
    }

    #[test]
    fn applies_advanced_model_protocol_body_and_headers() {
        let mut advanced = crate::database::ProviderAdvancedConfig {
            upstream_format: "anthropic".to_string(),
            custom_user_agent: "xy-switch-test/1.0".to_string(),
            ..Default::default()
        };
        advanced
            .header_overrides
            .insert("x-workspace".to_string(), "demo".to_string());
        advanced
            .body_overrides
            .insert("temperature".to_string(), json!(0.2));
        advanced
            .model_mappings
            .push(crate::database::ProviderModelMapping {
                model: "upstream-model".to_string(),
                display_name: "Fast".to_string(),
                context_window: Some(128_000),
            });
        let provider = Provider {
            id: "advanced".to_string(),
            name: "Advanced".to_string(),
            provider_type: "custom".to_string(),
            endpoint: "https://example.com".to_string(),
            model: "fallback-model".to_string(),
            app_group: "Codex".to_string(),
            enabled: true,
            sort_order: 1,
            advanced,
        };
        let prepared = prepare_upstream_request(
            &provider,
            ClientProtocol::Chat,
            br#"{"model":"Fast","messages":[{"role":"user","content":"Hi"}]}"#,
        )
        .unwrap();
        assert_eq!(prepared.model, "upstream-model");
        assert_eq!(prepared.url.path(), "/v1/messages");
        assert_eq!(prepared.payload["model"], "upstream-model");
        assert_eq!(prepared.payload["temperature"], 0.2);

        let request = request_builder(
            &crate::network::test_client(),
            &provider,
            "secret",
            prepared.url,
            &prepared.payload,
            prepared.upstream,
        )
        .build()
        .unwrap();
        assert_eq!(request.headers()["x-api-key"], "secret");
        assert_eq!(request.headers()[header::USER_AGENT], "xy-switch-test/1.0");
        assert_eq!(request.headers()["x-workspace"], "demo");
        assert!(request.headers().get(header::AUTHORIZATION).is_none());
    }

    #[test]
    fn xai_paths_and_protocols_are_isolated() {
        let mut chat_provider = Provider {
            id: "grok-chat".to_string(),
            name: "Grok Chat".to_string(),
            provider_type: "openai_compatible".to_string(),
            endpoint: "https://api.x.ai/v1".to_string(),
            model: "grok-4".to_string(),
            app_group: "Codex".to_string(),
            enabled: true,
            sort_order: 1,
            advanced: Default::default(),
        };
        chat_provider.advanced.upstream_format = "openai_chat".to_string();

        let chat = prepare_upstream_request(
            &chat_provider,
            ClientProtocol::Chat,
            br#"{"model":"old","messages":[]}"#,
        )
        .unwrap();
        assert_eq!(chat.url.as_str(), "https://api.x.ai/v1/chat/completions");
        assert_eq!(chat.upstream, EffectiveUpstream::OpenAiChat);
        assert_eq!(chat.transform, ResponseTransform::None);

        let responses_via_chat = prepare_upstream_request(
            &chat_provider,
            ClientProtocol::Responses,
            br#"{"model":"old","input":"hello"}"#,
        )
        .unwrap();
        assert_eq!(
            responses_via_chat.url.as_str(),
            "https://api.x.ai/v1/chat/completions"
        );
        assert_eq!(responses_via_chat.upstream, EffectiveUpstream::OpenAiChat);
        assert_eq!(
            responses_via_chat.transform,
            ResponseTransform::ChatToResponses
        );

        let mut responses_provider = chat_provider.clone();
        responses_provider.id = "grok-responses".to_string();
        responses_provider.advanced.upstream_format = "openai_responses".to_string();
        let native_responses = prepare_upstream_request(
            &responses_provider,
            ClientProtocol::Responses,
            br#"{"model":"old","input":"hello"}"#,
        )
        .unwrap();
        assert_eq!(
            native_responses.url.as_str(),
            "https://api.x.ai/v1/responses"
        );
        assert_eq!(
            native_responses.upstream,
            EffectiveUpstream::OpenAiResponses
        );
        assert_eq!(
            native_responses.transform,
            ResponseTransform::ResponsesToResponses
        );

        // Preparing another provider must not mutate the original chat configuration.
        let chat_again = prepare_upstream_request(
            &chat_provider,
            ClientProtocol::Chat,
            br#"{"model":"old","messages":[]}"#,
        )
        .unwrap();
        assert_eq!(
            chat_again.url.as_str(),
            "https://api.x.ai/v1/chat/completions"
        );
        assert_eq!(chat_again.upstream, EffectiveUpstream::OpenAiChat);
    }

    #[test]
    fn selected_provider_credential_cannot_be_overridden_by_custom_auth_headers() {
        let mut provider = Provider {
            id: "grok".to_string(),
            name: "Grok".to_string(),
            provider_type: "openai_compatible".to_string(),
            endpoint: "https://api.x.ai/v1".to_string(),
            model: "grok-4".to_string(),
            app_group: "Codex".to_string(),
            enabled: true,
            sort_order: 1,
            advanced: Default::default(),
        };
        provider.advanced.upstream_format = "openai_chat".to_string();
        provider.advanced.header_overrides.insert(
            "Authorization".to_string(),
            "Bearer stale-client-token".to_string(),
        );
        provider
            .advanced
            .header_overrides
            .insert("X-API-Key".to_string(), "stale-anthropic-token".to_string());
        provider.advanced.header_overrides.insert(
            "X-Goog-Api-Key".to_string(),
            "stale-gemini-token".to_string(),
        );
        provider
            .advanced
            .header_overrides
            .insert("X-Workspace".to_string(), "xy".to_string());

        let request = request_builder(
            &crate::network::test_client(),
            &provider,
            "selected-xai-token",
            reqwest::Url::parse("https://api.x.ai/v1/chat/completions").unwrap(),
            &json!({"model":"grok-4","messages":[]}),
            EffectiveUpstream::OpenAiChat,
        )
        .build()
        .unwrap();

        assert_eq!(
            request.headers().get(header::AUTHORIZATION).unwrap(),
            "Bearer selected-xai-token"
        );
        assert!(request.headers().get("x-api-key").is_none());
        assert!(request.headers().get("x-goog-api-key").is_none());
        assert_eq!(request.headers().get("x-workspace").unwrap(), "xy");
    }

    #[tokio::test]
    async fn forwards_chat_request_to_mock_upstream() {
        async fn upstream(headers: HeaderMap, Json(body): Json<Value>) -> Response {
            if headers
                .get(header::AUTHORIZATION)
                .and_then(|value| value.to_str().ok())
                != Some("Bearer test-api-key")
            {
                return (StatusCode::UNAUTHORIZED, Json(json!({"error":"bad auth"})))
                    .into_response();
            }
            if body["stream"] == true {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "text/event-stream")
                    .body(Body::from("data: {\"ok\":true}\n\n"))
                    .unwrap();
            }
            (
                StatusCode::OK,
                Json(json!({"model": body["model"], "ok": true})),
            )
                .into_response()
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_address = mock_listener.local_addr().unwrap();
        let mock_task = tokio::spawn(async move {
            axum::serve(
                mock_listener,
                Router::new()
                    .route("/v1/chat/completions", post(upstream))
                    .route("/v1/responses", post(upstream)),
            )
            .await
        });

        let (database, path) = test_database();
        database
            .save_provider(&crate::database::Provider {
                id: "mock".to_string(),
                name: "Mock".to_string(),
                provider_type: "openai_compatible".to_string(),
                endpoint: format!("http://{mock_address}"),
                model: "configured-model".to_string(),
                app_group: "Codex".to_string(),
                enabled: true,
                sort_order: 1,
                advanced: crate::database::ProviderAdvancedConfig {
                    upstream_format: "openai_chat".into(),
                    ..Default::default()
                },
            })
            .unwrap();
        database.set_current_provider("mock").unwrap();
        let state = ProxyState {
            database: database.clone(),
            client: crate::network::test_client(),
            api_keys: Arc::new(FakeApiKeyProvider),
            local_addr: None,
        };
        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy_task =
            tokio::spawn(
                async move { axum::serve(proxy_listener, router_with_state(state)).await },
            );

        let response = crate::network::test_client()
            .post(format!("http://{proxy_address}/v1/chat/completions"))
            .json(&json!({"model":"ignored", "messages":[]}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body: Value = response.json().await.unwrap();
        assert_eq!(body["model"], "configured-model");
        assert_eq!(body["ok"], true);

        let responses = crate::network::test_client()
            .post(format!("http://{proxy_address}/v1/responses"))
            .json(&json!({"model":"ignored", "input":"hello"}))
            .send()
            .await
            .unwrap();
        assert_eq!(responses.status(), StatusCode::OK);
        assert_eq!(
            responses.json::<Value>().await.unwrap()["model"],
            "configured-model"
        );

        let stream = crate::network::test_client()
            .post(format!("http://{proxy_address}/v1/chat/completions"))
            .json(&json!({"model":"ignored", "messages":[], "stream":true}))
            .send()
            .await
            .unwrap();
        assert_eq!(
            stream
                .headers()
                .get(header::CONTENT_TYPE)
                .unwrap()
                .to_str()
                .unwrap(),
            "text/event-stream"
        );
        assert_eq!(stream.text().await.unwrap(), "data: {\"ok\":true}\n\n");

        proxy_task.abort();
        mock_task.abort();
        let _ = proxy_task.await;
        let _ = mock_task.await;
        drop(database);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn adapts_anthropic_and_gemini_protocols_through_proxy() {
        use axum::http::Uri;

        async fn adapted_upstream(
            uri: Uri,
            headers: HeaderMap,
            Json(body): Json<Value>,
        ) -> Response {
            if uri.path() == "/v1/messages" {
                if headers
                    .get("x-api-key")
                    .and_then(|value| value.to_str().ok())
                    != Some("test-api-key")
                    || headers
                        .get("anthropic-version")
                        .and_then(|value| value.to_str().ok())
                        != Some("2023-06-01")
                    || body["system"][0]["text"] != "Be concise"
                {
                    return StatusCode::BAD_REQUEST.into_response();
                }
                if body["stream"] == true {
                    let events = concat!(
                        "event: message_start\ndata: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_stream\",\"model\":\"claude-mock\",\"usage\":{\"input_tokens\":2}}}\n\n",
                        "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"stream-anthropic\"}}\n\n",
                        "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":1}}\n\n",
                        "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n"
                    );
                    return Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(Body::from(events))
                        .unwrap();
                }
                return Json(json!({
                    "id":"msg_mock", "type":"message", "role":"assistant", "model":"claude-mock",
                    "content":[{"type":"text", "text":"anthropic-ok"}], "stop_reason":"end_turn",
                    "usage":{"input_tokens":4, "output_tokens":2}
                }))
                .into_response();
            }
            if uri.path().contains("GenerateContent") || uri.path().contains(":generateContent") {
                if headers
                    .get("x-goog-api-key")
                    .and_then(|value| value.to_str().ok())
                    != Some("test-api-key")
                    || body["systemInstruction"]["parts"][0]["text"] != "Gemini system"
                {
                    return StatusCode::BAD_REQUEST.into_response();
                }
                if uri.path().contains("streamGenerateContent") {
                    return Response::builder()
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(Body::from("data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"stream-gemini\"}]},\"finishReason\":\"STOP\"}]}\n\n"))
                        .unwrap();
                }
                return Json(json!({
                    "candidates":[{"content":{"parts":[{"text":"gemini-ok"}]}, "finishReason":"STOP"}],
                    "usageMetadata":{"promptTokenCount":5, "candidatesTokenCount":3, "totalTokenCount":8}
                }))
                .into_response();
            }
            StatusCode::NOT_FOUND.into_response()
        }

        let mock_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let mock_address = mock_listener.local_addr().unwrap();
        let mock_task = tokio::spawn(async move {
            axum::serve(
                mock_listener,
                Router::new().fallback(post(adapted_upstream)),
            )
            .await
        });

        let (database, path) = test_database();
        for (id, provider_type, model, app_group) in [
            ("anthropic-codex-mock", "anthropic", "claude-mock", "Codex"),
            (
                "anthropic-gemini-mock",
                "anthropic",
                "claude-mock",
                "Gemini",
            ),
        ] {
            database
                .save_provider(&crate::database::Provider {
                    id: id.to_string(),
                    name: id.to_string(),
                    provider_type: provider_type.to_string(),
                    endpoint: format!("http://{mock_address}"),
                    model: model.to_string(),
                    app_group: app_group.to_string(),
                    enabled: true,
                    sort_order: 1,
                    advanced: crate::database::ProviderAdvancedConfig {
                        upstream_format: if provider_type == "anthropic" {
                            "anthropic".to_string()
                        } else {
                            "openai_chat".to_string()
                        },
                        ..Default::default()
                    },
                })
                .unwrap();
        }
        let state = ProxyState {
            database: database.clone(),
            client: crate::network::test_client(),
            api_keys: Arc::new(FakeApiKeyProvider),
            local_addr: None,
        };
        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy_task =
            tokio::spawn(
                async move { axum::serve(proxy_listener, router_with_state(state)).await },
            );

        database
            .set_current_provider("anthropic-codex-mock")
            .unwrap();
        let anthropic_response = crate::network::test_client()
            .post(format!("http://{proxy_address}/v1/chat/completions"))
            .json(&json!({
                "model":"ignored",
                "messages":[{"role":"system", "content":"Be concise"}, {"role":"user", "content":"Hello"}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(anthropic_response.status(), StatusCode::OK);
        let anthropic_body: Value = anthropic_response.json().await.unwrap();
        assert_eq!(anthropic_body["object"], "chat.completion");
        assert_eq!(
            anthropic_body["choices"][0]["message"]["content"],
            "anthropic-ok"
        );
        assert_eq!(anthropic_body["usage"]["total_tokens"], 6);

        let anthropic_stream = crate::network::test_client()
            .post(format!("http://{proxy_address}/v1/chat/completions"))
            .json(&json!({
                "model":"ignored", "stream":true,
                "messages":[{"role":"system", "content":"Be concise"}, {"role":"user", "content":"Hello"}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            anthropic_stream.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let anthropic_stream_body = anthropic_stream.text().await.unwrap();
        assert!(anthropic_stream_body.contains("stream-anthropic"));
        assert!(anthropic_stream_body.contains("data: [DONE]"));

        database
            .set_current_provider("anthropic-gemini-mock")
            .unwrap();
        let gemini_entry = crate::network::test_client()
            .post(format!(
                "http://{proxy_address}/v1beta/models/client-model:generateContent"
            ))
            .json(&json!({
                "systemInstruction":{"parts":[{"text":"Be concise"}]},
                "contents":[{"role":"user", "parts":[{"text":"Hello"}]}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(gemini_entry.status(), StatusCode::OK);
        let gemini_entry_body: Value = gemini_entry.json().await.unwrap();
        assert_eq!(
            gemini_entry_body["candidates"][0]["content"]["parts"][0]["text"],
            "anthropic-ok"
        );

        let gemini_entry_stream = crate::network::test_client()
            .post(format!(
                "http://{proxy_address}/v1beta/models/client-model:streamGenerateContent"
            ))
            .json(&json!({
                "systemInstruction":{"parts":[{"text":"Be concise"}]},
                "contents":[{"role":"user", "parts":[{"text":"Hello"}]}]
            }))
            .send()
            .await
            .unwrap();
        assert_eq!(
            gemini_entry_stream.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        let gemini_entry_stream_body = gemini_entry_stream.text().await.unwrap();
        assert!(gemini_entry_stream_body.contains("stream-anthropic"));
        assert!(gemini_entry_stream_body.contains("finishReason"));

        proxy_task.abort();
        mock_task.abort();
        let _ = proxy_task.await;
        let _ = mock_task.await;
        drop(database);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn retries_fails_over_and_opens_circuit() {
        use std::sync::atomic::{AtomicUsize, Ordering};

        async fn failing(State(calls): State<Arc<AtomicUsize>>) -> impl IntoResponse {
            calls.fetch_add(1, Ordering::SeqCst);
            (StatusCode::SERVICE_UNAVAILABLE, "temporary failure")
        }
        async fn backup() -> impl IntoResponse {
            Json(json!({
                "id":"chat_backup", "object":"chat.completion", "model":"backup-model",
                "choices":[{"index":0, "message":{"role":"assistant", "content":"backup-ok"}, "finish_reason":"stop"}],
                "usage":{"prompt_tokens":1, "completion_tokens":1, "total_tokens":2}
            }))
        }

        let primary_calls = Arc::new(AtomicUsize::new(0));
        let primary_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let primary_address = primary_listener.local_addr().unwrap();
        let counter = primary_calls.clone();
        let primary_task = tokio::spawn(async move {
            axum::serve(
                primary_listener,
                Router::new()
                    .route("/v1/chat/completions", post(failing))
                    .with_state(counter),
            )
            .await
        });
        let backup_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backup_address = backup_listener.local_addr().unwrap();
        let backup_task = tokio::spawn(async move {
            axum::serve(
                backup_listener,
                Router::new().route("/v1/chat/completions", post(backup)),
            )
            .await
        });

        let (database, path) = test_database();
        for (id, endpoint, sort_order) in [
            ("primary", format!("http://{primary_address}"), 1),
            ("backup", format!("http://{backup_address}"), 2),
        ] {
            database
                .save_provider(&crate::database::Provider {
                    id: id.to_string(),
                    name: id.to_string(),
                    provider_type: "openai_compatible".to_string(),
                    endpoint,
                    model: format!("{id}-model"),
                    app_group: if id == "backup" { "Claude" } else { "Codex" }.to_string(),
                    enabled: true,
                    sort_order,
                    advanced: crate::database::ProviderAdvancedConfig {
                        upstream_format: "openai_chat".into(),
                        ..Default::default()
                    },
                })
                .unwrap();
        }
        database.set_current_provider("primary").unwrap();
        let state = ProxyState {
            database: database.clone(),
            client: crate::network::test_client(),
            api_keys: Arc::new(FakeApiKeyProvider),
            local_addr: None,
        };
        let proxy_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let proxy_address = proxy_listener.local_addr().unwrap();
        let proxy_task =
            tokio::spawn(
                async move { axum::serve(proxy_listener, router_with_state(state)).await },
            );

        for _ in 0..4 {
            let response = crate::network::test_client()
                .post(format!("http://{proxy_address}/v1/chat/completions"))
                .json(&json!({"model":"ignored", "messages":[{"role":"user", "content":"hello"}]}))
                .send()
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.json::<Value>().await.unwrap()["choices"][0]["message"]["content"],
                "backup-ok"
            );
        }

        assert_eq!(primary_calls.load(Ordering::SeqCst), 9);
        let primary_health = database.provider_health("primary").unwrap().unwrap();
        assert_eq!(primary_health.status, "circuit_open");
        assert_eq!(primary_health.consecutive_failures, 3);
        assert!(primary_health.circuit_open_until.is_some());
        assert_eq!(database.request_summary().unwrap().total_requests, 7);

        proxy_task.abort();
        primary_task.abort();
        backup_task.abort();
        let _ = proxy_task.await;
        let _ = primary_task.await;
        let _ = backup_task.await;
        drop(database);
        std::fs::remove_file(path).unwrap();
    }

    #[tokio::test]
    async fn full_url_provider_does_not_append_route() {
        let (database, path) = test_database();
        let mut advanced: crate::database::ProviderAdvancedConfig = Default::default();
        advanced.is_full_url = true;
        database
            .save_provider(&crate::database::Provider {
                id: "full-url-test".to_string(),
                name: "Full URL Test".to_string(),
                provider_type: "openai_compatible".to_string(),
                endpoint: "https://example.com/custom/api/endpoint?debug=1".to_string(),
                model: "gpt-5.6-sol".to_string(),
                app_group: "Codex".to_string(),
                enabled: true,
                sort_order: 1,
                advanced,
            })
            .unwrap();

        let provider = database.get_provider("full-url-test").unwrap().unwrap();
        let prepared = prepare_upstream_request(
            &provider,
            ClientProtocol::Responses,
            br#"{"model":"ignored","input":"hi"}"#,
        )
        .unwrap();

        assert_eq!(
            prepared.url.as_str(),
            "https://example.com/custom/api/endpoint?debug=1"
        );
        drop(database);
        std::fs::remove_file(path).unwrap();
    }
    include!("proxy_regression_tests.rs");
    include!("model_verification/proxy_tests.rs");
    include!("codex_client_regression_tests.rs");
    #[test]
    fn gemini_operations_are_explicit_and_do_not_accept_model_path_injection() {
        assert_eq!(gemini_operation("gemini-2.5-pro:generateContent").unwrap(), ("gemini-2.5-pro", false));
        assert_eq!(gemini_operation("gemini-2.5-flash:streamGenerateContent").unwrap(), ("gemini-2.5-flash", true));
        for operation in ["model:countTokens", "model:embedContent", "model", ":generateContent", "models/model:generateContent", "model:other:generateContent", "model?x=1:generateContent", "模型:generateContent"] {
            assert!(gemini_operation(operation).is_err(), "{operation}");
        }
    }

    #[tokio::test]
    async fn production_router_blocks_browser_spend_and_unsupported_gemini_operations() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let upstream = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_address = upstream.local_addr().unwrap();
        let upstream_task = tokio::spawn(async move {
            axum::serve(upstream, Router::new().fallback(post(move |Json(body): Json<Value>| {
                let called = called.clone();
                async move {
                    called.fetch_add(1, Ordering::SeqCst);
                    Json(json!({"id":"qa-response","model":body["model"],"choices":[{"index":0,"message":{"role":"assistant","content":"ok"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}))
                }
            }))).await
        });
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        for provider in database.list_providers().unwrap() {
            database.delete_provider(&provider.id).unwrap();
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = ProxyState { database: database.clone(), client: crate::network::test_client(), api_keys: Arc::new(FakeApiKeyProvider), local_addr: Some(address) };
        let router = router_with_targets(state, &["gemini".into(), "codex".into(), "grok".into()]);
        let task = tokio::spawn(async move { axum::serve(listener, router).await });
        let client = crate::network::test_client();
        let base = format!("http://{address}");
        let models: Value = client.get(format!("{base}/v1/models")).send().await.unwrap().json().await.unwrap();
        assert_eq!(models["data"], json!([]), "No configured providers must not advertise a fabricated fallback model");
        database.save_provider(&crate::database::Provider {
            id: "qa-local".into(), name: "Isolated test upstream".into(), app_group: "Codex".into(), provider_type: "openai_compatible".into(),
            endpoint: format!("http://{upstream_address}"), model: "configured-model".into(), enabled: true, sort_order: 0,
            advanced: crate::database::ProviderAdvancedConfig { upstream_format: "openai_chat".into(), ..Default::default() },
        }).unwrap();
        database.set_current_provider("qa-local").unwrap();
        let models: Value = client.get(format!("{base}/models")).send().await.unwrap().json().await.unwrap();
        assert_eq!(models["data"].as_array().unwrap().len(), 1);
        assert_eq!(models["data"][0]["id"], "configured-model");
        for operation in ["model:countTokens", "model:embedContent", "model", "other/model:generateContent", "model:unknown"] {
            let response = client.post(format!("{base}/v1beta/models/{operation}")).json(&json!({"contents":[]})).send().await.unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{operation}");
        }
        for (name, value) in [("origin", "https://attacker.invalid"), ("origin", "null"), ("sec-fetch-site", "cross-site")] {
            let response = client.post(format!("{base}/v1/responses")).header(name, value).header("content-type", "text/plain").body("{\"input\":\"do not send\"}").send().await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        let response = client.post(format!("{base}/v1/chat/completions")).header("host", format!("rebind.invalid:{}", address.port())).json(&json!({"messages":[]})).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let response = client.get(format!("{base}/models")).header("origin", "https://attacker.invalid").send().await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(calls.load(Ordering::SeqCst), 0, "Rejected requests must not reach an upstream");
        let response = client.get(format!("{base}/health")).header("origin", "https://attacker.invalid").send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let response = client.post(format!("{base}/v1/chat/completions")).json(&json!({"messages":[{"role":"user","content":"test"}]})).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "Native requests must remain functional");
        task.abort(); upstream_task.abort();
        let _ = task.await; let _ = upstream_task.await;
    }

}
