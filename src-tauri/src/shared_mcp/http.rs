use super::{manager::SharedMcpManager, types::*};
use crate::{cancellation::Cancellation, plugins::VerifiedTool};
use axum::{Router, body::Bytes, extract::{DefaultBodyLimit, Path, State}, http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response, Sse, sse::{Event, KeepAlive}}, routing::post, Json};
use futures_util::stream;
use serde_json::{Value, json};
use std::{collections::BTreeMap, convert::Infallible, time::Duration};
use zeroize::Zeroizing;

pub(crate) const VERSION_META: &str = "io.modelcontextprotocol/protocolVersion";
pub(crate) const CAPABILITIES_META: &str = "io.modelcontextprotocol/clientCapabilities";

pub(crate) struct RpcRequest {
    pub value: Value,
    pub id: Option<Value>,
    pub method: String,
    pub params: Value,
}

pub(crate) fn request_key(id: &Value) -> Option<String> {
    let valid = match id {
        Value::String(value) => value.len() <= 128 && !value.chars().any(char::is_control),
        Value::Number(value) => value.as_i64().is_some_and(|number| number.unsigned_abs() <= 9_007_199_254_740_991)
            || value.as_u64().is_some_and(|number| number <= 9_007_199_254_740_991),
        _ => false,
    };
    valid.then(|| id.to_string())
}

pub(crate) fn parse_request(text: &str, secret: &str) -> Result<RpcRequest, &'static str> {
    let value = crate::plugins::validation::strict_json(text, MAX_MESSAGE_BYTES).map_err(|_| {
        if serde_json::from_str::<serde::de::IgnoredAny>(text).is_err() { "shared_mcp_json" } else { "shared_mcp_rpc" }
    })?;
    let object = value.as_object().ok_or("shared_mcp_rpc")?;
    if object.keys().any(|key| !["jsonrpc", "id", "method", "params"].contains(&key.as_str()))
        || object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") { return Err("shared_mcp_rpc"); }
    let method = object.get("method").and_then(Value::as_str).ok_or("shared_mcp_rpc")?;
    if method.is_empty() || method.len() > 80 || !method.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"/_-.".contains(&byte)) { return Err("shared_mcp_rpc"); }
    let id = object.get("id").cloned();
    if let Some(id) = &id {
        if request_key(id).is_none() { return Err("shared_mcp_rpc"); }
        let checked = crate::privacy::inspect(json!({"id":id}), &Default::default(), &[secret]).map_err(|_| "shared_mcp_rpc")?;
        if checked.blocked_code.is_some() || checked.redacted.get("id") != Some(id) { return Err("shared_mcp_rpc"); }
    }
    if method.starts_with("notifications/") != id.is_none() { return Err("shared_mcp_rpc"); }
    let params = object.get("params").cloned().unwrap_or_else(|| json!({}));
    if !params.is_object() || params.get("_meta").is_some_and(|meta| !meta.is_object()) { return Err("shared_mcp_rpc"); }
    Ok(RpcRequest { method: method.to_owned(), id, params, value })
}

pub(crate) fn error_value(id: Option<&Value>, code: i64, message: &str) -> Value {
    let mut value = json!({"jsonrpc":"2.0", "id":null, "error":{"code":code,"message":message}});
    if let Some(id) = id { value["id"] = id.clone(); }
    value
}

pub(crate) fn parse_error_value(code: &'static str) -> Value {
    error_value(Some(&Value::Null), if code == "shared_mcp_json" { -32700 } else { -32600 }, code)
}

fn implementation(value: &Value) -> bool {
    value.is_object() && ["name", "version"].iter().all(|field| value.get(field).and_then(Value::as_str)
        .is_some_and(|text| !text.is_empty() && text.len() <= 128 && !text.chars().any(char::is_control)))
}

fn error(status: StatusCode, id: Option<&Value>, code: i64, message: &'static str) -> Response {
    (status, Json(error_value(id, code, message))).into_response()
}

pub(crate) fn result_value(id: &Value, mut result: Value, modern: bool) -> Value {
    if modern { result["resultType"] = Value::String("complete".into()); }
    json!({"jsonrpc":"2.0", "id":id, "result":result})
}

pub(crate) fn one_header<'a>(headers: &'a HeaderMap, name: &str) -> Result<Option<&'a str>, &'static str> {
    let values = headers.get_all(name);
    if values.iter().count() > 1 { return Err("shared_mcp_headers"); }
    values.iter().next().map(|value| value.to_str().map_err(|_| "shared_mcp_headers")).transpose()
}

fn authorize(manager: &SharedMcpManager, id: &str, headers: &HeaderMap) -> Result<(SharedMcpConnection, Zeroizing<String>), Response> {
    let status = manager.status().map_err(|_| error(StatusCode::SERVICE_UNAVAILABLE, None, -32603, "shared_mcp_state"))?;
    let valid_origin = one_header(headers, "origin").is_ok_and(|origin| origin.is_none_or(|origin| origin == status.address));
    let valid_host = one_header(headers, "host").is_ok_and(|host| host.is_none_or(|host| Some(host) == status.address.strip_prefix("http://")));
    if !valid_origin || !valid_host { return Err(error(StatusCode::FORBIDDEN, None, -32603, "shared_mcp_origin")); }
    let authorization = one_header(headers, "authorization").ok().flatten().and_then(|value| value.split_once(' '))
        .filter(|(scheme, _)| scheme.eq_ignore_ascii_case("bearer")).map(|(_, token)| token).unwrap_or("");
    let connection = manager.authenticate(id, authorization).map_err(|code| {
        let status = if code == "shared_mcp_rate_limit" { StatusCode::TOO_MANY_REQUESTS }
            else if code == "shared_mcp_disabled" { StatusCode::SERVICE_UNAVAILABLE } else { StatusCode::UNAUTHORIZED };
        let mut response = error(status, None, -32603, code);
        if status == StatusCode::UNAUTHORIZED { response.headers_mut().insert(header::WWW_AUTHENTICATE, "Bearer".parse().unwrap()); }
        if status == StatusCode::TOO_MANY_REQUESTS { response.headers_mut().insert(header::RETRY_AFTER, "30".parse().unwrap()); }
        response
    })?;
    Ok((connection, Zeroizing::new(authorization.to_owned())))
}

fn date_version(value: &str) -> bool {
    value.len() == 10 && value.bytes().enumerate().all(|(index, byte)| if index == 4 || index == 7 { byte == b'-' } else { byte.is_ascii_digit() })
}

fn version(headers: &HeaderMap, rpc: &RpcRequest) -> Result<String, Response> {
    let mismatch = || error(StatusCode::BAD_REQUEST, rpc.id.as_ref(), -32020, "shared_mcp_header_mismatch");
    let header_version = one_header(headers, "mcp-protocol-version").map_err(|_| mismatch())?;
    let meta = rpc.params.get("_meta").and_then(|meta| meta.get(VERSION_META));
    if (meta.is_some() || header_version == Some(MODERN_VERSION)) && !meta.and_then(Value::as_str).is_some_and(date_version) {
        return Err(error(StatusCode::BAD_REQUEST, rpc.id.as_ref(), -32602, "shared_mcp_metadata"));
    }
    let requested = if meta.is_some() || header_version == Some(MODERN_VERSION) {
        let value = meta.and_then(Value::as_str).ok_or_else(mismatch)?;
        if header_version != Some(value) || one_header(headers, "mcp-method").map_err(|_| mismatch())? != Some(rpc.method.as_str()) { return Err(mismatch()); }
        if matches!(rpc.method.as_str(), "tools/call" | "prompts/get" | "resources/read") {
            let field = if rpc.method == "resources/read" { "uri" } else { "name" };
            let name = rpc.params.get(field).and_then(Value::as_str).ok_or_else(mismatch)?;
            let mirrored = one_header(headers, "mcp-name").map_err(|_| mismatch())?.ok_or_else(mismatch)?;
            if decode_name(mirrored).map_err(|_| mismatch())? != name { return Err(mismatch()); }
        }
        value
    } else { header_version.unwrap_or("2025-03-26") };
    if !date_version(requested) { return Err(mismatch()); }
    if !SUPPORTED_VERSIONS.contains(&requested) {
        let mut value = error_value(rpc.id.as_ref(), -32022, "Unsupported protocol version");
        value["error"]["data"] = json!({"supported":SUPPORTED_VERSIONS,"requested":requested});
        return Err((StatusCode::BAD_REQUEST, Json(value)).into_response());
    }
    Ok(requested.to_owned())
}

pub(crate) fn tool_name(tool: &VerifiedTool) -> String { format!("{}.{}", tool.plugin_id, tool.tool.name) }

fn tool_definition(tool: VerifiedTool) -> Value {
    let properties: BTreeMap<_, _> = tool.tool.input_schema.properties.iter().map(|(name, property)| {
        let mut value = json!({"type":property.value_type});
        if let Some(text) = &property.description { value["description"] = json!(text); }
        if let Some(length) = property.max_length { value["maxLength"] = json!(length); }
        if let Some(values) = &property.enum_values { value["enum"] = json!(values); }
        (name.clone(), value)
    }).collect();
    json!({"name":tool_name(&tool),"description":tool.tool.description,"inputSchema":{"type":"object", "properties":properties,
        "required":tool.tool.input_schema.required,"additionalProperties":tool.tool.input_schema.additional_properties},
        "annotations":{"readOnlyHint":!matches!(tool.tool.operation, crate::plugins::ToolOperation::HttpGet {..}),"openWorldHint":matches!(tool.tool.operation, crate::plugins::ToolOperation::HttpGet {..})}})
}

pub(crate) fn router(manager: SharedMcpManager) -> Router {
    Router::new().route("/mcp/{id}", post(handle_post).delete(handle_delete)).layer(DefaultBodyLimit::max(MAX_MESSAGE_BYTES)).with_state(manager)
}

async fn handle_delete(State(manager): State<SharedMcpManager>, Path(id): Path<String>, headers: HeaderMap) -> Response {
    let (connection, _) = match authorize(&manager, &id, &headers) { Ok(value) => value, Err(response) => return response };
    let requested = one_header(&headers, "mcp-protocol-version").ok().flatten().unwrap_or("2025-03-26");
    if requested == MODERN_VERSION || !SUPPORTED_VERSIONS.contains(&requested) { return StatusCode::METHOD_NOT_ALLOWED.into_response(); }
    let session = match one_header(&headers, "mcp-session-id").ok().flatten() { Some(id) => id, None => return StatusCode::BAD_REQUEST.into_response() };
    match manager.close_session(&connection, session) { Ok(()) => StatusCode::NO_CONTENT.into_response(), Err(_) => StatusCode::NOT_FOUND.into_response() }
}

async fn handle_post(State(manager): State<SharedMcpManager>, Path(id): Path<String>, headers: HeaderMap, body: Bytes) -> Response {
    let (connection, secret) = match authorize(&manager, &id, &headers) { Ok(value) => value, Err(response) => return response };
    let content_type = one_header(&headers, "content-type").ok().flatten().and_then(|value| value.split(';').next());
    let accepts = one_header(&headers, "accept").ok().flatten().unwrap_or("");
    if content_type != Some("application/json") { return error(StatusCode::UNSUPPORTED_MEDIA_TYPE, None, -32600, "shared_mcp_content_type"); }
    if !accepts.split(',').any(|value| value.split(';').next().is_some_and(|value| value.trim() == "application/json"))
        || !accepts.split(',').any(|value| value.split(';').next().is_some_and(|value| value.trim() == "text/event-stream")) {
        return error(StatusCode::NOT_ACCEPTABLE, None, -32600, "shared_mcp_accept");
    }
    let rpc = match std::str::from_utf8(&body).map_err(|_| "shared_mcp_json").and_then(|text| parse_request(text, &secret)) {
        Ok(value) => value, Err(code) => return (StatusCode::BAD_REQUEST, Json(parse_error_value(code))).into_response(),
    };
    let version = match version(&headers, &rpc) { Ok(value) => value, Err(response) => return response };
    let modern = version == MODERN_VERSION;
    if modern && (!rpc.params.get("_meta").and_then(|meta| meta.get(CAPABILITIES_META)).is_some_and(Value::is_object)
        || rpc.params.get("_meta").and_then(|meta| meta.get("io.modelcontextprotocol/clientInfo")).is_some_and(|value| !implementation(value))) {
        return error(StatusCode::BAD_REQUEST, rpc.id.as_ref(), -32602, "shared_mcp_metadata");
    }
    let mut session_id = String::new();
    let mut session_cancel = Cancellation::default();
    if !modern && rpc.method == "initialize" {
        if headers.contains_key("mcp-session-id") { return error(StatusCode::BAD_REQUEST, rpc.id.as_ref(), -32600, "shared_mcp_already_initialized"); }
        let requested = rpc.params.get("protocolVersion").and_then(Value::as_str).unwrap_or("");
        if !date_version(requested) || !rpc.params.get("capabilities").is_some_and(Value::is_object)
            || !rpc.params.get("clientInfo").is_some_and(implementation) {
            return error(StatusCode::BAD_REQUEST, rpc.id.as_ref(), -32602, "shared_mcp_initialize");
        }
        let selected = if requested != MODERN_VERSION && SUPPORTED_VERSIONS.contains(&requested) { requested } else { "2025-11-25" };
        let session = match manager.create_session(&connection, selected) { Ok(value) => value, Err(code) => return error(StatusCode::SERVICE_UNAVAILABLE, rpc.id.as_ref(), -32603, code) };
        let result = json!({"protocolVersion":selected,"serverInfo":{"name":"xy-switch-shared-plugins","version":env!("CARGO_PKG_VERSION")},
            "capabilities":{"tools":{},"prompts":{}},"instructions":"Tools require explicit approval in XY Switch. Only approved, redacted arguments are used."});
        let mut response = Json(result_value(rpc.id.as_ref().unwrap(), result, false)).into_response();
        response.headers_mut().insert("mcp-session-id", session.parse().unwrap());
        return response;
    }
    if !modern {
        session_id = match one_header(&headers, "mcp-session-id").ok().flatten() { Some(value) => value.to_owned(), None => return error(StatusCode::BAD_REQUEST, rpc.id.as_ref(), -32603, "shared_mcp_session") };
        session_cancel = match manager.session(&connection, &session_id, &version, rpc.method == "notifications/initialized") {
            Ok(value) => value, Err(code) => return error(StatusCode::NOT_FOUND, rpc.id.as_ref(), -32603, code),
        };
        if rpc.method == "notifications/initialized" { return StatusCode::ACCEPTED.into_response(); }
        if rpc.method == "notifications/cancelled" {
            let request = match rpc.params.get("requestId").and_then(request_key) { Some(value) => value, None => return StatusCode::BAD_REQUEST.into_response() };
            let _ = manager.cancel_request(&connection, &session_id, &request);
            return StatusCode::ACCEPTED.into_response();
        }
    }
    if rpc.id.is_none() { return StatusCode::BAD_REQUEST.into_response(); }
    let id = rpc.id.as_ref().unwrap();
    let result = match rpc.method.as_str() {
        "server/discover" if modern => Ok(json!({"supportedVersions":SUPPORTED_VERSIONS,"capabilities":{"tools":{},"prompts":{}},
            "_meta":{"io.modelcontextprotocol/serverInfo":{"name":"xy-switch-shared-plugins","version":env!("CARGO_PKG_VERSION")}},
            "instructions":"Tools require explicit approval in XY Switch. Only approved, redacted arguments are used."})),
        "ping" => Ok(json!({})),
        "tools/list" => {
            if rpc.params.get("cursor").is_some() { return error(StatusCode::BAD_REQUEST, Some(id), -32602, "shared_mcp_cursor"); }
            manager.tools(&connection).map(|tools| json!({"tools":tools.into_iter().map(tool_definition).collect::<Vec<_>>()}))
        },
        "prompts/list" => {
            if rpc.params.get("cursor").is_some() { return error(StatusCode::BAD_REQUEST, Some(id), -32602, "shared_mcp_cursor"); }
            manager.skills(&connection).map(|skills| json!({"prompts":skills.into_iter().map(|skill| json!({"name":skill.plugin_id,"description":"Verified plugin skill"})).collect::<Vec<_>>()}))
        },
        "prompts/get" => {
            if rpc.params.get("arguments").is_some_and(|arguments| !arguments.is_object() || !arguments.as_object().unwrap().is_empty()) {
                return error(StatusCode::BAD_REQUEST, Some(id), -32602, "shared_mcp_prompt_arguments");
            }
            let name = rpc.params.get("name").and_then(Value::as_str).unwrap_or("");
            manager.skills(&connection).and_then(|skills| skills.into_iter().find(|skill| skill.plugin_id == name).ok_or("shared_mcp_prompt"))
                .map(|skill| json!({"messages":[{"role":"user","content":{"type":"text","text":skill.content}}]}))
        },
        "tools/call" => {
            let name = match rpc.params.get("name").and_then(Value::as_str) { Some(value) => value.to_owned(), None => return error(StatusCode::BAD_REQUEST, Some(id), -32602, "shared_mcp_tool") };
            let arguments = rpc.params.get("arguments").cloned().unwrap_or_else(|| json!({}));
            if !arguments.is_object() { return error(StatusCode::BAD_REQUEST, Some(id), -32602, "shared_mcp_tool_input"); }
            let key = request_key(id).unwrap();
            let id = id.clone();
            let future = async move {
                let result = manager.call(connection, session_id, key, session_cancel, name, arguments, secret).await
                    .unwrap_or_else(|code| json!({"isError":true,"content":[{"type":"text","text":code}]}));
                let value = result_value(&id, result, modern);
                let value = if value.to_string().len() <= MAX_REPLY_BYTES { value } else { error_value(Some(&id), -32603, "shared_mcp_limit") };
                Ok::<_, Infallible>(Event::default().json_data(value).unwrap())
            };
            let mut response = Sse::new(stream::once(future)).keep_alive(KeepAlive::new().interval(Duration::from_secs(10)).text("waiting")).into_response();
            response.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            response.headers_mut().insert("x-accel-buffering", "no".parse().unwrap());
            return response;
        },
        _ => return error(if modern { StatusCode::NOT_FOUND } else { StatusCode::OK }, Some(id), -32601, "Method not found"),
    };
    match result {
        Ok(result) => {
            let value = result_value(id, result, modern);
            if value.to_string().len() > MAX_REPLY_BYTES { return error(StatusCode::SERVICE_UNAVAILABLE, Some(id), -32603, "shared_mcp_limit"); }
            let mut response = Json(value).into_response();
            response.headers_mut().insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
            response
        },
        Err(code) => error(StatusCode::BAD_REQUEST, Some(id), -32602, code),
    }
}

const BASE64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn encode64(value: &[u8]) -> String {
    let mut result = String::with_capacity(value.len().div_ceil(3) * 4);
    for chunk in value.chunks(3) {
        let bits = (u32::from(chunk[0]) << 16) | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8) | u32::from(*chunk.get(2).unwrap_or(&0));
        result.push(BASE64[((bits >> 18) & 63) as usize] as char);
        result.push(BASE64[((bits >> 12) & 63) as usize] as char);
        result.push(if chunk.len() > 1 { BASE64[((bits >> 6) & 63) as usize] as char } else { '=' });
        result.push(if chunk.len() > 2 { BASE64[(bits & 63) as usize] as char } else { '=' });
    }
    result
}

pub(crate) fn encode_name(value: &str) -> Result<String, &'static str> {
    if value.len() > 2048 || value.chars().any(char::is_control) { return Err("shared_mcp_headers"); }
    Ok(if value.is_ascii() && value.trim() == value && !value.starts_with("=?base64?") { value.to_owned() } else { format!("=?base64?{}?=", encode64(value.as_bytes())) })
}

pub(crate) fn decode_name(value: &str) -> Result<String, &'static str> {
    if value.len() > 4096 || !value.is_ascii() || value.chars().any(char::is_control) { return Err("shared_mcp_headers"); }
    let Some(encoded) = value.strip_prefix("=?base64?").and_then(|value| value.strip_suffix("?=")) else {
        return if value.starts_with("=?base64?") { Err("shared_mcp_headers") } else { Ok(value.to_owned()) };
    };
    if !encoded.len().is_multiple_of(4) { return Err("shared_mcp_headers"); }
    let mut decoded = Vec::with_capacity(encoded.len() / 4 * 3);
    for chunk in encoded.as_bytes().chunks(4) {
        let mut bits = 0u32;
        for byte in chunk { bits = (bits << 6) | if *byte == b'=' { 0 } else { BASE64.iter().position(|value| value == byte).ok_or("shared_mcp_headers")? as u32 }; }
        decoded.push((bits >> 16) as u8);
        if chunk[2] != b'=' { decoded.push((bits >> 8) as u8); }
        if chunk[3] != b'=' { decoded.push(bits as u8); }
    }
    if encode64(&decoded) != encoded { return Err("shared_mcp_headers"); }
    let result = String::from_utf8(decoded).map_err(|_| "shared_mcp_headers")?;
    if result.len() > 2048 || result.chars().any(char::is_control) { return Err("shared_mcp_headers"); }
    Ok(result)
}
