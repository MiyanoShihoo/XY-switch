use super::{http::{self, RpcRequest}, manager::bridge_credential, types::*};
use crate::credential_store;
use futures_util::StreamExt;
use serde_json::Value;
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::{io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader}, task::{AbortHandle, JoinSet}};
use zeroize::Zeroizing;

#[derive(Clone)]
struct BridgeTransport {
    client: reqwest::Client,
    endpoint: reqwest::Url,
    token: Arc<Zeroizing<String>>,
}

struct InFlight { abort: AbortHandle, ticket: u64, method: String }
struct BridgeResponse { value: Option<Value>, session: Option<String>, version: Option<String> }

pub async fn run_stdio_bridge(id: &str) -> Result<(), &'static str> {
    if !valid_connection_id(id) { return Err("shared_mcp_connection"); }
    let secret = Zeroizing::new(credential_store::get_secret("mcp", &secret_id(id)).map_err(|_| "shared_mcp_credentials")?
        .ok_or("shared_mcp_credentials")?);
    let (endpoint, token) = bridge_credential(&secret).map_err(|_| "shared_mcp_credentials")?;
    if endpoint.path() != format!("/mcp/{id}") { return Err("shared_mcp_credentials"); }
    let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(130)).build().map_err(|_| "shared_mcp_transport")?;
    let transport = BridgeTransport { client, endpoint, token: Arc::new(token) };
    run_io(BufReader::new(tokio::io::stdin()), tokio::io::stdout(), transport).await
}

async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R, frame: &mut Zeroizing<Vec<u8>>) -> Result<Option<Zeroizing<Vec<u8>>>, &'static str> {
    loop {
        let available = reader.fill_buf().await.map_err(|_| "shared_mcp_input")?;
        if available.is_empty() { return if frame.is_empty() { Ok(None) } else { Err("shared_mcp_frame") }; }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let length = newline.map_or(available.len(), |index| index + 1);
        if frame.len().saturating_add(length) > MAX_MESSAGE_BYTES + 2 { return Err("shared_mcp_limit"); }
        frame.extend_from_slice(&available[..length]);
        reader.consume(length);
        if newline.is_some() {
            frame.pop();
            if frame.last() == Some(&b'\r') { frame.pop(); }
            if frame.len() > MAX_MESSAGE_BYTES { return Err("shared_mcp_limit"); }
            return Ok(Some(std::mem::take(frame)));
        }
    }
}

async fn write_frame<W: AsyncWrite + Unpin>(writer: &mut W, value: &Value) -> Result<(), &'static str> {
    let encoded = serde_json::to_vec(value).map_err(|_| "shared_mcp_output")?;
    if encoded.len() > MAX_REPLY_BYTES { return Err("shared_mcp_limit"); }
    writer.write_all(&encoded).await.map_err(|_| "shared_mcp_output")?;
    writer.write_all(b"\n").await.map_err(|_| "shared_mcp_output")?;
    writer.flush().await.map_err(|_| "shared_mcp_output")
}

fn verified_reply(text: &str, id: &Value, secret: &str) -> Result<Value, &'static str> {
    let value = crate::plugins::validation::strict_json(text, MAX_REPLY_BYTES).map_err(|_| "shared_mcp_response")?;
    let object = value.as_object().ok_or("shared_mcp_response")?;
    if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") || object.get("id") != Some(id)
        || object.contains_key("result") == object.contains_key("error")
        || object.keys().any(|name| !["jsonrpc", "id", "result", "error"].contains(&name.as_str())) { return Err("shared_mcp_response"); }
    let checked = crate::privacy::inspect(value, &Default::default(), &[secret]).map_err(|_| "shared_mcp_privacy")?;
    if checked.blocked_code.is_some() || checked.redacted.get("id") != Some(id) { return Err("shared_mcp_privacy"); }
    Ok(checked.redacted)
}

async fn response_value(response: reqwest::Response, id: &Value, secret: &str) -> Result<Value, &'static str> {
    let content_type = response.headers().get("content-type").and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next()).unwrap_or("").to_owned();
    if !["application/json", "text/event-stream"].contains(&content_type.as_str()) { return Err("shared_mcp_response"); }
    let mut stream = response.bytes_stream();
    let mut buffer = Zeroizing::new(Vec::<u8>::new());
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| "shared_mcp_transport")?;
        if buffer.len().saturating_add(chunk.len()) > MAX_REPLY_BYTES + 8192 { return Err("shared_mcp_limit"); }
        buffer.extend_from_slice(&chunk);
        if content_type == "text/event-stream" {
            loop {
                let boundary = buffer.windows(2).position(|bytes| bytes == b"\n\n").map(|index| (index, 2))
                    .or_else(|| buffer.windows(4).position(|bytes| bytes == b"\r\n\r\n").map(|index| (index, 4)));
                let Some((position, delimiter)) = boundary else { break; };
                let event = std::str::from_utf8(&buffer[..position]).map_err(|_| "shared_mcp_response")?;
                let data = event.lines().filter_map(|line| line.strip_prefix("data:").map(|text| text.strip_prefix(' ').unwrap_or(text))).collect::<Vec<_>>().join("\n");
                if !data.is_empty() { return verified_reply(&data, id, secret); }
                buffer.drain(..position + delimiter);
            }
        }
    }
    if content_type == "text/event-stream" { return Err("shared_mcp_response"); }
    verified_reply(std::str::from_utf8(&buffer).map_err(|_| "shared_mcp_response")?, id, secret)
}

impl BridgeTransport {
    async fn forward(&self, rpc: RpcRequest, previous_version: Option<&str>, session: Option<&str>) -> Result<BridgeResponse, &'static str> {
        let declared = rpc.params.get("_meta").and_then(|meta| meta.get(http::VERSION_META)).and_then(Value::as_str);
        let version = declared.unwrap_or(previous_version.unwrap_or("2025-03-26"));
        if version.len() != 10 || !version.bytes().all(|byte| byte.is_ascii_digit() || byte == b'-') { return Err("shared_mcp_version"); }
        let modern = version == MODERN_VERSION;
        let mut request = self.client.post(self.endpoint.clone()).bearer_auth(self.token.as_ref().as_str())
            .header("accept", "application/json, text/event-stream").header("mcp-protocol-version", version).header("mcp-method", &rpc.method);
        if !modern && let Some(session) = session { request = request.header("mcp-session-id", session); }
        let name = match rpc.method.as_str() { "tools/call" | "prompts/get" => rpc.params.get("name"), "resources/read" => rpc.params.get("uri"), _ => None };
        if let Some(name) = name.and_then(Value::as_str) { request = request.header("mcp-name", http::encode_name(name)?); }
        let response = request.json(&rpc.value).send().await.map_err(|_| "shared_mcp_transport")?;
        if rpc.id.is_none() {
            return if response.status() == reqwest::StatusCode::ACCEPTED { Ok(BridgeResponse { value: None, session: None, version: None }) } else { Err("shared_mcp_response") };
        }
        let new_session = if !modern { response.headers().get("mcp-session-id").and_then(|value| value.to_str().ok()).map(str::to_owned) } else { None };
        if new_session.as_ref().is_some_and(|value| !valid_connection_id(value)) { return Err("shared_mcp_session"); }
        let value = response_value(response, rpc.id.as_ref().unwrap(), self.token.as_ref().as_str()).await?;
        let selected = if rpc.method == "initialize" { value.pointer("/result/protocolVersion").and_then(Value::as_str).map(str::to_owned) } else { None };
        if let Some(selected) = &selected {
            if !SUPPORTED_VERSIONS.contains(&selected.as_str()) || selected == MODERN_VERSION || new_session.is_none() { return Err("shared_mcp_version"); }
        }
        Ok(BridgeResponse { value: Some(value), session: new_session, version: selected })
    }

    async fn close(&self, version: Option<&str>, session: Option<&str>) {
        if let (Some(version), Some(session)) = (version, session) {
            let request = self.client.delete(self.endpoint.clone()).bearer_auth(self.token.as_ref().as_str())
                .header("mcp-protocol-version", version).header("mcp-session-id", session).send();
            let _ = tokio::time::timeout(Duration::from_secs(2), request).await;
        }
    }
}

async fn run_io<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(mut reader: R, mut writer: W, transport: BridgeTransport) -> Result<(), &'static str> {
    let mut tasks = JoinSet::new();
    let mut in_flight = BTreeMap::<String, InFlight>::new();
    let mut session: Option<String> = None;
    let mut version: Option<String> = None;
    let mut ticket = 0u64;
    let mut frame_buffer = Zeroizing::new(Vec::new());
    let outcome = loop {
        enum Incoming<T> { Frame(Result<Option<Zeroizing<Vec<u8>>>, &'static str>), Reply(T) }
        let next = tokio::select! {
            frame = read_frame(&mut reader, &mut frame_buffer) => Incoming::Frame(frame),
            reply = tasks.join_next(), if !tasks.is_empty() => Incoming::Reply(reply),
        };
        match next {
            Incoming::Frame(Ok(None)) => break Ok(()),
            Incoming::Frame(Err(code)) => break Err(code),
            Incoming::Frame(Ok(Some(frame))) => {
                let rpc = match std::str::from_utf8(&frame).map_err(|_| "shared_mcp_json").and_then(|text| http::parse_request(text, transport.token.as_ref().as_str())) {
                    Ok(rpc) => rpc,
                    Err(code) => {
                        if let Err(code) = write_frame(&mut writer, &http::parse_error_value(code)).await { break Err(code); }
                        continue;
                    }
                };
                if rpc.method == "notifications/cancelled" {
                    if let Some(key) = rpc.params.get("requestId").and_then(http::request_key) {
                        if in_flight.get(&key).is_some_and(|entry| entry.method != "initialize") {
                            if let Some(entry) = in_flight.remove(&key) { entry.abort.abort(); }
                        }
                    }
                    continue;
                }
                if rpc.method == "initialize" || rpc.id.is_none() {
                    if rpc.method == "initialize" && session.is_some() {
                        if let Err(code) = write_frame(&mut writer, &http::error_value(rpc.id.as_ref(), -32600, "shared_mcp_already_initialized")).await { break Err(code); }
                        continue;
                    }
                    let id = rpc.id.clone();
                    let response = tokio::time::timeout(Duration::from_secs(10), transport.forward(rpc, version.as_deref(), session.as_deref())).await;
                    match response {
                        Ok(Ok(response)) => {
                            if response.session.is_some() { session = response.session; }
                            if response.version.is_some() { version = response.version; }
                            if let Some(value) = response.value { if let Err(code) = write_frame(&mut writer, &value).await { break Err(code); } }
                        },
                        _ => if let Some(id) = id { if let Err(code) = write_frame(&mut writer, &http::error_value(Some(&id), -32603, "shared_mcp_transport")).await { break Err(code); } },
                    }
                    continue;
                }
                let id = rpc.id.clone().unwrap();
                let key = http::request_key(&id).unwrap();
                if in_flight.contains_key(&key) || in_flight.len() >= 16 {
                    if let Err(code) = write_frame(&mut writer, &http::error_value(Some(&id), -32600, "shared_mcp_limit")).await { break Err(code); }
                    continue;
                }
                ticket = ticket.wrapping_add(1);
                let current_ticket = ticket;
                let transport = transport.clone();
                let session = session.clone();
                let version = version.clone();
                let return_key = key.clone();
                let method = rpc.method.clone();
                let abort = tasks.spawn(async move { (return_key, current_ticket, id, transport.forward(rpc, version.as_deref(), session.as_deref()).await) });
                in_flight.insert(key, InFlight { abort, ticket: current_ticket, method });
            },
            Incoming::Reply(Some(Ok((key, completed_ticket, id, result)))) => {
                if !in_flight.get(&key).is_some_and(|entry| entry.ticket == completed_ticket) { continue; }
                in_flight.remove(&key);
                let value = match result {
                    Ok(response) => response.value.unwrap_or_else(|| http::error_value(Some(&id), -32603, "shared_mcp_response")),
                    Err(code) => http::error_value(Some(&id), -32603, code),
                };
                if let Err(code) = write_frame(&mut writer, &value).await { break Err(code); }
            },
            Incoming::Reply(_) => {},
        }
    };
    tasks.abort_all();
    tasks.shutdown().await;
    in_flight.clear();
    transport.close(version.as_deref(), session.as_deref()).await;
    outcome
}

#[cfg(test)]
pub(super) async fn run_test_io<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin>(reader: R, writer: W, endpoint: reqwest::Url, token: String) -> Result<(), &'static str> {
    let client = reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(130))
        .build().map_err(|_| "shared_mcp_transport")?;
    run_io(reader, writer, BridgeTransport { client, endpoint, token: Arc::new(Zeroizing::new(token)) }).await
}
