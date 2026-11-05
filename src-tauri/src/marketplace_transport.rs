use crate::cancellation::Cancellation;
use reqwest::{Client, Method, Url, header};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, future::Future, sync::{Arc, Mutex}, task::Poll, time::{Duration, Instant}};

const MAX_ENVELOPE_BYTES: usize = crate::plugins::MAX_PACKAGE_BYTES + 64 * 1024;
const MAX_REQUESTS: usize = 64;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TransportOptions {
    origin: String,
    #[serde(default)]
    allow_development_http: bool,
    timeout_ms: u64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct TransportRequest {
    method: String,
    path: String,
    body: Option<String>,
    bearer: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TransportResponse {
    status: u16,
    content_type: Option<String>,
    body: String,
}

#[derive(Clone)]
struct PendingRequest {
    origin: Url,
    deadline: Instant,
    cancellation: Cancellation,
    started: bool,
}

#[derive(Clone)]
pub(crate) struct MarketplaceTransport {
    client: Client,
    requests: Arc<Mutex<HashMap<String, PendingRequest>>>,
}

struct RequestLease {
    id: String,
    pending: PendingRequest,
    requests: Arc<Mutex<HashMap<String, PendingRequest>>>,
}

impl Drop for RequestLease {
    fn drop(&mut self) {
        if let Ok(mut requests) = self.requests.lock() { requests.remove(&self.id); }
        self.pending.cancellation.cancel();
    }
}

fn origin(input: &str, allow_development_http: bool) -> Result<Url, String> {
    let invalid = || "invalid_origin".to_owned();
    if input.len() > 256 || input.chars().any(|character| character.is_control() || character.is_whitespace() || character == '\\') {
        return Err(invalid());
    }
    let raw = input.strip_suffix('/').unwrap_or(input);
    let (scheme, authority) = raw.split_once("://").ok_or_else(invalid)?;
    if authority.is_empty() || authority.contains(['/', '?', '#', '@']) { return Err(invalid()); }
    let url = Url::parse(input).map_err(|_| invalid())?;
    let exact_loopback = ["localhost", "127.0.0.1", "[::1]"].iter().any(|host| {
        authority.strip_prefix(host).is_some_and(|suffix| suffix.is_empty()
            || suffix.strip_prefix(':').is_some_and(|port| !port.is_empty() && port.bytes().all(|digit| digit.is_ascii_digit())))
    });
    if !(scheme == "https" || scheme == "http" && allow_development_http && exact_loopback)
        || url.host().is_none() || !url.username().is_empty() || url.password().is_some()
        || url.path() != "/" || url.query().is_some() || url.fragment().is_some() || url.port() == Some(0)
    { return Err(invalid()); }
    Ok(url)
}

fn prune(requests: &mut HashMap<String, PendingRequest>) {
    let now = Instant::now();
    requests.retain(|_, request| {
        if request.deadline <= now { request.cancellation.cancel(); false } else { true }
    });
}

impl MarketplaceTransport {
    pub(crate) fn new() -> Result<Self, String> {
        let client = Client::builder().no_proxy().referer(false)
            .redirect(reqwest::redirect::Policy::none()).build().map_err(|_| "network".to_owned())?;
        Ok(Self { client, requests: Arc::new(Mutex::new(HashMap::new())) })
    }

    pub(crate) fn begin(&self, options: TransportOptions) -> Result<String, String> {
        let origin = origin(&options.origin, options.allow_development_http)?;
        if !(100..=60_000).contains(&options.timeout_ms) { return Err("invalid_request".into()); }
        let mut requests = self.requests.lock().map_err(|_| "network".to_owned())?;
        prune(&mut requests);
        if requests.len() >= MAX_REQUESTS { return Err("rate_limited".into()); }
        let id = uuid::Uuid::new_v4().to_string();
        requests.insert(id.clone(), PendingRequest { origin, deadline: Instant::now() + Duration::from_millis(options.timeout_ms),
            cancellation: Cancellation::default(), started: false });
        Ok(id)
    }

    fn claim(&self, id: &str) -> Result<RequestLease, String> {
        if id.len() != 36 || uuid::Uuid::parse_str(id).is_err() { return Err("invalid_request".into()); }
        let mut requests = self.requests.lock().map_err(|_| "network".to_owned())?;
        prune(&mut requests);
        let request = requests.get_mut(id).ok_or_else(|| "session_changed".to_owned())?;
        if request.started { return Err("invalid_request".into()); }
        request.started = true;
        Ok(RequestLease { id: id.into(), pending: request.clone(), requests: self.requests.clone() })
    }

    pub(crate) fn cancel(&self, id: &str) -> Result<(), String> {
        let request = self.requests.lock().map_err(|_| "network".to_owned())?.remove(id);
        if let Some(request) = request { request.cancellation.cancel(); }
        Ok(())
    }

    pub(crate) fn cancel_all(&self) {
        let pending: Vec<_> = match self.requests.lock() {
            Ok(mut requests) => requests.drain().map(|(_, request)| request).collect(),
            Err(_) => return,
        };
        for request in pending { request.cancellation.cancel(); }
    }

    pub(crate) async fn request(&self, id: &str, request: TransportRequest) -> Result<TransportResponse, String> {
        let lease = self.claim(id)?;
        let cancellation = &lease.pending.cancellation;
        let mut operation = Box::pin(self.send(&lease.pending.origin, request));
        let guarded = std::future::poll_fn(|context| {
            let Ok(_gate) = cancellation.0.gate.lock() else { return Poll::Ready(Err("network".to_owned())); };
            if cancellation.is_cancelled() { return Poll::Ready(Err("session_changed".into())); }
            operation.as_mut().poll(context)
        });
        let result = tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err("session_changed".into()),
            result = tokio::time::timeout_at(tokio::time::Instant::from_std(lease.pending.deadline), guarded) => {
                result.map_err(|_| "network".to_owned())?
            }
        };
        result
    }

    async fn send(&self, origin: &Url, request: TransportRequest) -> Result<TransportResponse, String> {
        if request.path.len() > 2048 || !request.path.starts_with("/v1/")
            || !request.path.bytes().all(|character| (0x21..=0x7e).contains(&character) && !b"\\#".contains(&character))
        { return Err("invalid_request".into()); }
        let url = origin.join(&request.path).map_err(|_| "invalid_request".to_owned())?;
        if url.origin() != origin.origin() || !url.path().starts_with("/v1/") || url.fragment().is_some() {
            return Err("invalid_request".into());
        }
        let method = match request.method.as_str() {
            "GET" if request.body.is_none() => Method::GET,
            "POST" if request.body.is_some() => Method::POST,
            _ => return Err("invalid_request".into()),
        };
        let mut outgoing = self.client.request(method, url).header(header::ACCEPT, "application/json")
            .header(header::CACHE_CONTROL, "no-store");
        if let Some(body) = request.body {
            if body.len() > MAX_ENVELOPE_BYTES || serde_json::from_str::<serde_json::Value>(&body).is_err() {
                return Err("invalid_request".into());
            }
            outgoing = outgoing.header(header::CONTENT_TYPE, "application/json").body(body);
        }
        if let Some(bearer) = request.bearer {
            if bearer.is_empty() || bearer.len() > 4096
                || !bearer.bytes().all(|character| character.is_ascii_alphanumeric() || b"._~+/=-".contains(&character))
            { return Err("invalid_request".into()); }
            let mut value = header::HeaderValue::from_str(&format!("Bearer {bearer}")).map_err(|_| "invalid_request".to_owned())?;
            value.set_sensitive(true);
            outgoing = outgoing.header(header::AUTHORIZATION, value);
        }
        let mut response = outgoing.send().await.map_err(|_| "network".to_owned())?;
        let status = response.status().as_u16();
        if status < 200 || response.status().is_redirection() { return Err("invalid_response".into()); }
        if let Some(length) = response.headers().get(header::CONTENT_LENGTH) {
            let length = length.to_str().map_err(|_| "invalid_response".to_owned())?;
            if length.is_empty() || !length.bytes().all(|digit| digit.is_ascii_digit())
                || length.parse::<usize>().map_or(true, |size| size > MAX_ENVELOPE_BYTES || status == 204 && size != 0)
            { return Err("invalid_response".into()); }
        }
        if status == 204 { return Ok(TransportResponse { status, content_type: None, body: String::new() }); }
        let content_type = response.headers().get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok())
            .filter(|value| value.len() <= 256 && value.split(';').next().is_some_and(|kind| kind.trim().eq_ignore_ascii_case("application/json")))
            .ok_or_else(|| "invalid_response".to_owned())?.to_owned();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "invalid_response".to_owned())? {
            if bytes.len().saturating_add(chunk.len()) > MAX_ENVELOPE_BYTES { return Err("invalid_response".into()); }
            bytes.extend_from_slice(&chunk);
        }
        let body = String::from_utf8(bytes).map_err(|_| "invalid_response".to_owned())?;
        if serde_json::from_str::<serde_json::Value>(&body).is_err() { return Err("invalid_response".into()); }
        Ok(TransportResponse { status, content_type: Some(content_type), body })
    }
}

#[tauri::command]
pub(crate) fn marketplace_begin_request(state: tauri::State<'_, MarketplaceTransport>, options: TransportOptions) -> Result<String, String> {
    state.begin(options)
}

#[tauri::command]
pub(crate) async fn marketplace_request(state: tauri::State<'_, MarketplaceTransport>, request_id: String,
    request: TransportRequest) -> Result<TransportResponse, String> {
    state.request(&request_id, request).await
}

#[tauri::command]
pub(crate) fn marketplace_cancel_request(state: tauri::State<'_, MarketplaceTransport>, request_id: String) -> Result<(), String> {
    state.cancel(&request_id)
}

#[cfg(test)]
#[path = "marketplace_transport_tests.rs"]
mod tests;
