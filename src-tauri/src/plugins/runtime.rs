use super::{
    PluginManager,
    manager::{active_verified, now, revision},
    types::*,
    validation::*,
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    future::{Future, poll_fn},
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    task::Poll,
    time::{Duration, Instant},
};

const TOOL_PREVIEW_TTL: Duration = Duration::from_secs(120);
const MAX_TOOL_PREVIEWS: usize = 32;
const MAX_IN_FLIGHT: usize = 4;

fn error(code: &'static str) -> PluginError {
    PluginError::new(code)
}

pub(crate) struct PendingToolCall {
    preview: ToolCallPreview,
    arguments: Value,
    revision: i64,
    deadline: Instant,
}

struct ExecutionLease(Arc<Mutex<super::manager::HostState>>);

impl Drop for ExecutionLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.0.lock() {
            state.in_flight = state.in_flight.saturating_sub(1);
        }
    }
}

fn tool_named(entry: &PluginEntry, name: &str) -> PluginResult<DeclarativeTool> {
    let PluginEntry::Mcp(bundle) = entry else {
        return Err(error("plugin_tool_not_found"));
    };
    bundle
        .tools
        .iter()
        .find(|tool| tool.name == name)
        .cloned()
        .ok_or_else(|| error("plugin_tool_not_found"))
}

fn safe_url(text: &str, permissions: &PluginPermissions) -> PluginResult<reqwest::Url> {
    if text.len() > 2048 || text.contains('\\') || text.chars().any(char::is_control) {
        return Err(error("plugin_network"));
    }
    let url = reqwest::Url::parse(text).map_err(|_| error("plugin_network"))?;
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
        || url.fragment().is_some()
    {
        return Err(error("plugin_network"));
    }
    let origin = approved_origin(&url.origin().ascii_serialization())
        .map_err(|_| error("plugin_network"))?;
    if !permissions.network.contains(&origin) {
        return Err(error("plugin_network"));
    }
    inspect_package_privacy(Value::String(text.to_owned()))?;
    let transport =
        crate::privacy::inspect_transport(&url, &Default::default(), "", &Default::default(), &[])
            .map_err(|_| error("plugin_privacy"))?;
    if transport.blocked_code.is_some()
        || transport.report.secret_redactions > 0
        || transport.report.personal_redactions > 0
    {
        return Err(error("plugin_privacy"));
    }
    let query: Vec<_> = url
        .query_pairs()
        .map(|(name, value)| serde_json::json!([name, value]))
        .collect();
    inspect_package_privacy(serde_json::json!(query))?;
    Ok(url)
}

fn required_effect(
    published: &PublishedPlugin,
    tool: &DeclarativeTool,
    arguments: &Value,
) -> PluginResult<(PluginPermissions, String)> {
    validate_tool_arguments(&tool.input_schema, arguments)?;
    let mut required = PluginPermissions::default();
    let effect = match &tool.operation {
        ToolOperation::JsonPassthrough => {
            "Return the approved JSON input without external access.".to_owned()
        }
        ToolOperation::ReadText { path_argument } => {
            let path = arguments
                .get(path_argument)
                .and_then(Value::as_str)
                .ok_or_else(|| error("plugin_tool_arguments"))?;
            validate_package_path(path)?;
            let text = published
                .package
                .files
                .get(path)
                .ok_or_else(|| error("plugin_not_found"))?;
            if text.len() > MAX_TOOL_BYTES {
                return Err(error("plugin_tool_arguments"));
            }
            if !published
                .package
                .manifest
                .permissions
                .read_paths
                .iter()
                .any(|scope| package_scope_allows(scope, path))
            {
                return Err(error("plugin_permissions"));
            }
            required.read_paths.push(format!("package:{path}"));
            format!("Read verified package text: {path}")
        }
        ToolOperation::HttpGet { url_argument } => {
            let text = arguments
                .get(url_argument)
                .and_then(Value::as_str)
                .ok_or_else(|| error("plugin_tool_arguments"))?;
            let url = safe_url(text, &published.package.manifest.permissions)?;
            required.network.push(url.origin().ascii_serialization());
            format!("HTTPS GET {url}; no credentials, redirects or proxy.")
        }
    };
    Ok((required, effect))
}

impl PluginManager {
    fn visible_packages(&self, client: &str) -> PluginResult<Vec<super::manager::VerifiedPackage>> {
        validate_client(client)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(|_| error("plugin_database"))?;
        let mut statement = connection.prepare("SELECT plugin_id FROM xy_plugin_installations WHERE installed=1 ORDER BY plugin_id").map_err(|_| error("plugin_database"))?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|_| error("plugin_database"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| error("plugin_database"))?;
        Ok(ids
            .iter()
            .filter_map(|id| active_verified(&connection, &self.store, id, client).ok())
            .collect())
    }

    pub fn tools_for_client(&self, client: &str) -> PluginResult<Vec<VerifiedTool>> {
        let mut tools = Vec::new();
        for verified in self.visible_packages(client)? {
            if let PluginEntry::Mcp(bundle) = verified.validated.entry {
                let manifest = verified.published.package.manifest;
                for tool in bundle.tools {
                    tools.push(VerifiedTool {
                        plugin_id: manifest.id.clone(),
                        version: manifest.version.clone(),
                        digest: verified.published.digest.clone(),
                        client: client.to_owned(),
                        tool,
                    });
                }
            }
        }
        Ok(tools)
    }

    pub fn skills_for_client(&self, client: &str) -> PluginResult<Vec<VerifiedSkill>> {
        Ok(self
            .visible_packages(client)?
            .into_iter()
            .filter_map(|verified| {
                if let PluginEntry::Skill(content) = verified.validated.entry {
                    let manifest = verified.published.package.manifest;
                    Some(VerifiedSkill {
                        plugin_id: manifest.id,
                        version: manifest.version,
                        digest: verified.published.digest,
                        content,
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn provider_mappings_for_client(
        &self,
        client: &str,
    ) -> PluginResult<Vec<VerifiedProviderMapping>> {
        Ok(self
            .visible_packages(client)?
            .into_iter()
            .filter_map(|verified| {
                if let PluginEntry::Provider(mapping) = verified.validated.entry {
                    let manifest = verified.published.package.manifest;
                    Some(VerifiedProviderMapping {
                        plugin_id: manifest.id,
                        version: manifest.version,
                        digest: verified.published.digest,
                        mapping,
                    })
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn preview_tool_call_json(
        &self,
        id: &str,
        client: &str,
        tool: &str,
        arguments_json: &str,
    ) -> PluginResult<ToolCallPreview> {
        self.preview_tool_call(
            id,
            client,
            tool,
            strict_json(arguments_json, MAX_TOOL_BYTES)?,
        )
    }

    pub fn cancel_tool_preview(&self, preview_id: &str) -> PluginResult<()> {
        if preview_id.len() != 36
            || uuid::Uuid::parse_str(preview_id)
                .map(|id| id.hyphenated().to_string() != preview_id)
                .unwrap_or(true)
        {
            return Err(error("plugin_confirmation"));
        }
        let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
        state.tool_previews.remove(preview_id);
        Ok(())
    }

    pub fn preview_tool_call(
        &self,
        id: &str,
        client: &str,
        name: &str,
        arguments: Value,
    ) -> PluginResult<ToolCallPreview> {
        let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(|_| error("plugin_database"))?;
        let current_revision = revision(&connection)?;
        state.tool_previews.retain(|_, preview| {
            preview.deadline > Instant::now() && preview.revision == current_revision
        });
        if state.tool_previews.len() >= MAX_TOOL_PREVIEWS {
            return Err(error("plugin_limit"));
        }
        let verified = active_verified(&connection, &self.store, id, client)?;
        let tool = tool_named(&verified.validated.entry, name)?;
        let (required_permissions, effect) =
            required_effect(&verified.published, &tool, &arguments)?;
        let arguments_digest = sha256_hex(
            &serde_json::to_vec(&arguments).map_err(|_| error("plugin_tool_arguments"))?,
        );
        let preview = ToolCallPreview {
            preview_id: uuid::Uuid::new_v4().to_string(),
            expires_at: now() + TOOL_PREVIEW_TTL.as_millis() as i64,
            plugin_id: id.to_owned(),
            version: verified.published.package.manifest.version.clone(),
            plugin_digest: verified.published.digest,
            client: client.to_owned(),
            tool: name.to_owned(),
            arguments_digest,
            required_permissions,
            effect,
        };
        state.tool_previews.insert(
            preview.preview_id.clone(),
            PendingToolCall {
                preview: preview.clone(),
                arguments,
                revision: current_revision,
                deadline: Instant::now() + TOOL_PREVIEW_TTL,
            },
        );
        Ok(preview)
    }

    pub async fn execute_tool_json(
        &self,
        client: &str,
        confirmation_json: &str,
    ) -> PluginResult<ToolExecutionResult> {
        let confirmation = serde_json::from_value(strict_json(confirmation_json, 8192)?)
            .map_err(|_| error("plugin_confirmation"))?;
        self.execute_tool(client, confirmation).await
    }

    pub async fn execute_tool(
        &self,
        client: &str,
        confirmation: ToolCallConfirmation,
    ) -> PluginResult<ToolExecutionResult> {
        self.execute_tool_for_clients(&[client.to_owned()], confirmation)
            .await
    }

    pub async fn execute_tool_for_clients(
        &self,
        clients: &[String],
        confirmation: ToolCallConfirmation,
    ) -> PluginResult<ToolExecutionResult> {
        if clients.is_empty() || clients.len() > SUPPORTED_CLIENTS.len() {
            return Err(error("plugin_client"));
        }
        let mut unique = BTreeSet::new();
        for client in clients {
            validate_client(client)?;
            if !unique.insert(client) {
                return Err(error("plugin_client"));
            }
        }
        let (pending, verified, tool, _lease) = {
            let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
            let pending = state
                .tool_previews
                .remove(&confirmation.preview_id)
                .ok_or_else(|| error("plugin_preview_expired"))?;
            let connection = self.database.conn();
            let connection = connection.lock().map_err(|_| error("plugin_database"))?;
            let preview = &pending.preview;
            if !confirmation.confirmed
                || !clients.contains(&preview.client)
                || confirmation.plugin_digest != preview.plugin_digest
                || confirmation.arguments_digest != preview.arguments_digest
                || confirmation.accepted_permissions != preview.required_permissions
            {
                return Err(error("plugin_confirmation"));
            }
            let verified = self.verify_tool_scope(&connection, &pending, clients)?;
            let tool = tool_named(&verified.validated.entry, &preview.tool)?;
            let (permissions, _) = required_effect(&verified.published, &tool, &pending.arguments)?;
            if permissions != preview.required_permissions {
                return Err(error("plugin_confirmation"));
            }
            if state.in_flight >= MAX_IN_FLIGHT {
                return Err(error("plugin_limit"));
            }
            state.in_flight += 1;
            (pending, verified, tool, ExecutionLease(self.state.clone()))
        };
        let value = match tool.operation {
            ToolOperation::JsonPassthrough => pending.arguments.clone(),
            ToolOperation::ReadText { path_argument } => {
                let path = pending
                    .arguments
                    .get(&path_argument)
                    .and_then(Value::as_str)
                    .ok_or_else(|| error("plugin_tool_arguments"))?;
                Value::String(
                    verified
                        .published
                        .package
                        .files
                        .get(path)
                        .ok_or_else(|| error("plugin_not_found"))?
                        .clone(),
                )
            }
            ToolOperation::HttpGet { url_argument } => {
                let text = pending
                    .arguments
                    .get(&url_argument)
                    .and_then(Value::as_str)
                    .ok_or_else(|| error("plugin_tool_arguments"))?;
                let url = safe_url(text, &verified.published.package.manifest.permissions)?;
                tokio::select! {
                    biased;
                    failure = self.watch_revision(pending.revision) => return Err(failure),
                    result = tokio::time::timeout(Duration::from_secs(10), self.poll_tool_authorized(&pending, clients, http_get(url))) => result.map_err(|_| error("plugin_http"))??,
                }
            }
        };
        let (value, redacted) = inspect_output(value)?;
        {
            let _state = self.state.lock().map_err(|_| error("plugin_database"))?;
            let connection = self.database.conn();
            let connection = connection.lock().map_err(|_| error("plugin_database"))?;
            self.verify_tool_scope(&connection, &pending, clients)?;
        }
        let client = pending.preview.client.clone();
        Ok(ToolExecutionResult {
            plugin_id: pending.preview.plugin_id,
            version: pending.preview.version,
            digest: pending.preview.plugin_digest,
            client,
            tool: pending.preview.tool,
            value,
            redacted,
        })
    }

    pub(super) fn verify_tool_scope(
        &self,
        connection: &rusqlite::Connection,
        pending: &PendingToolCall,
        clients: &[String],
    ) -> PluginResult<super::manager::VerifiedPackage> {
        if pending.deadline <= Instant::now() || pending.revision != revision(connection)? {
            return Err(error("plugin_preview_expired"));
        }
        if !clients.contains(&pending.preview.client) {
            return Err(error("plugin_confirmation"));
        }
        let mut anchor = None;
        for client in clients {
            let verified =
                active_verified(connection, &self.store, &pending.preview.plugin_id, client)?;
            if verified.published.digest != pending.preview.plugin_digest
                || verified.published.package.manifest.version != pending.preview.version
            {
                return Err(error("plugin_digest"));
            }
            if client == &pending.preview.client {
                anchor = Some(verified);
            }
        }
        anchor.ok_or_else(|| error("plugin_confirmation"))
    }

    pub(super) async fn poll_tool_authorized<Output>(
        &self,
        pending: &PendingToolCall,
        clients: &[String],
        future: impl Future<Output = PluginResult<Output>>,
    ) -> PluginResult<Output> {
        let mut future = std::pin::pin!(future);
        poll_fn(|context| {
            let _state = match self.state.lock() {
                Ok(state) => state,
                Err(_) => return Poll::Ready(Err(error("plugin_database"))),
            };
            let connection = self.database.conn();
            let connection = match connection.lock() {
                Ok(connection) => connection,
                Err(_) => return Poll::Ready(Err(error("plugin_database"))),
            };
            if let Err(failure) = self.verify_tool_scope(&connection, pending, clients) {
                return Poll::Ready(Err(failure));
            }
            future.as_mut().poll(context)
        })
        .await
    }

    async fn watch_revision(&self, expected: i64) -> PluginError {
        loop {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let matches = (|| {
                let connection = self.database.conn();
                let connection = connection.lock().map_err(|_| error("plugin_database"))?;
                Ok::<_, PluginError>(revision(&connection)? == expected)
            })();
            match matches {
                Ok(true) => {}
                Ok(false) => return error("plugin_preview_expired"),
                Err(failure) => return failure,
            }
        }
    }
}

pub(super) fn inspect_output(value: Value) -> PluginResult<(Value, bool)> {
    let bytes = serde_json::to_vec(&value).map_err(|_| error("plugin_privacy"))?;
    if bytes.len() > MAX_TOOL_BYTES {
        return Err(error("plugin_tool_arguments"));
    }
    inspect_embedded_json(&value)?;
    let inspected = crate::privacy::inspect(
        serde_json::json!({"output":value}),
        &Default::default(),
        &[],
    )
    .map_err(|_| error("plugin_privacy"))?;
    if inspected.blocked_code.is_some() {
        return Err(error("plugin_privacy"));
    }
    let redacted =
        inspected.report.secret_redactions > 0 || inspected.report.personal_redactions > 0;
    let value = inspected
        .redacted
        .get("output")
        .cloned()
        .ok_or_else(|| error("plugin_privacy"))?;
    if serde_json::to_vec(&value)
        .map_err(|_| error("plugin_privacy"))?
        .len()
        > MAX_TOOL_BYTES
    {
        return Err(error("plugin_tool_arguments"));
    }
    inspect_package_privacy(value.clone())?;
    Ok((value, redacted))
}

pub(crate) fn public_address(address: IpAddr) -> bool {
    match address {
        IpAddr::V4(address) => {
            let octets = address.octets();
            !matches!(octets[0], 0 | 10 | 127 | 224..=255)
                && !(octets[0] == 100 && (64..=127).contains(&octets[1]))
                && !(octets[0] == 169 && octets[1] == 254)
                && !(octets[0] == 172 && (16..=31).contains(&octets[1]))
                && !(octets[0] == 192
                    && (octets[1] == 168 || octets[1] == 0 || octets[1] == 88 && octets[2] == 99))
                && !(octets[0] == 198
                    && ((18..=19).contains(&octets[1]) || octets[1] == 51 && octets[2] == 100))
                && !(octets[0] == 203 && octets[1] == 0 && octets[2] == 113)
        }
        IpAddr::V6(address) => {
            let segments = address.segments();
            (0x2000..=0x2fff).contains(&segments[0])
                && segments[0] != 0x2002
                && !(segments[0] == 0x2001 && (segments[1] < 0x0200 || segments[1] == 0x0db8))
        }
    }
}

async fn http_get(url: reqwest::Url) -> PluginResult<Value> {
    let host = url
        .host_str()
        .ok_or_else(|| error("plugin_network"))?
        .to_owned();
    let resolved = tokio::net::lookup_host((host.as_str(), 443))
        .await
        .map_err(|_| error("plugin_network"))?;
    let mut addresses = BTreeSet::<SocketAddr>::new();
    for address in resolved {
        if address.port() != 443 || !public_address(address.ip()) || addresses.len() >= 16 {
            return Err(error("plugin_network"));
        }
        addresses.insert(address);
    }
    if addresses.is_empty() {
        return Err(error("plugin_network"));
    }
    let addresses: Vec<_> = addresses.into_iter().collect();
    let client = reqwest::Client::builder()
        .https_only(true)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .referer(false)
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(10))
        .pool_max_idle_per_host(0)
        .resolve_to_addrs(&host, &addresses)
        .build()
        .map_err(|_| error("plugin_http"))?;
    let mut response = client
        .get(url)
        .header(
            reqwest::header::ACCEPT,
            "application/json, text/plain, text/markdown",
        )
        .header(reqwest::header::ACCEPT_ENCODING, "identity")
        .send()
        .await
        .map_err(|_| error("plugin_http"))?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > MAX_TOOL_BYTES as u64)
    {
        return Err(error("plugin_http"));
    }
    if response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .is_some_and(|encoding| encoding != "identity")
    {
        return Err(error("plugin_http"));
    }
    let content_type = response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|header| header.to_str().ok())
        .and_then(|header| header.split(';').next())
        .map(str::trim)
        .ok_or_else(|| error("plugin_http"))?
        .to_ascii_lowercase();
    let json = content_type == "application/json"
        || content_type.starts_with("application/") && content_type.ends_with("+json");
    if !json && !matches!(content_type.as_str(), "text/plain" | "text/markdown") {
        return Err(error("plugin_http"));
    }
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| error("plugin_http"))? {
        if bytes.len().saturating_add(chunk.len()) > MAX_TOOL_BYTES {
            return Err(error("plugin_http"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| error("plugin_http"))?;
    if text.contains('\0') {
        return Err(error("plugin_http"));
    }
    let body = if json {
        strict_json(text, MAX_TOOL_BYTES)?
    } else {
        Value::String(text.to_owned())
    };
    Ok(serde_json::json!({"status": status, "body": body}))
}
