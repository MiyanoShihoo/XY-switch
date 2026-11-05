use super::{NativeAccountSnapshot, NativeLoginCondition, NativeLoginMethod, NativeLoginOption, NativeLoginProgress, NativeLoginPrompt, NativeProviderConnection};
use crate::{cancellation::Cancellation, client_config::{OpenCodeSchema, OpenCodeTarget}};
use futures_util::StreamExt;
use reqwest::{Client, Method, Url};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, BTreeSet}, io::{BufRead, BufReader}, path::Path, process::{Child, Command, Stdio}, sync::{Arc, mpsc::{self, Receiver}}, thread::JoinHandle, time::{Duration, Instant}};
use zeroize::{Zeroize, Zeroizing};

const MAX_RESPONSE: usize = 1024 * 1024;
const MAX_LINE: usize = 8192;
const LOGIN_TIMEOUT: Duration = Duration::from_secs(600);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(test)]
#[path = "opencode_tests.rs"]
mod tests;

#[derive(Debug, PartialEq)]
enum ApiError { Transport, Status(u16), Invalid }

impl ApiError {
    fn message(self) -> String {
        match self {
            Self::Transport => "无法连接 OpenCode 自有本地服务，请重试".into(),
            Self::Status(_) => "OpenCode 原生账号接口拒绝了请求".into(),
            Self::Invalid => "OpenCode 返回了不支持的账号数据".into(),
        }
    }
}

struct Payload(Value);

fn wipe(value: &mut Value) {
    match value {
        Value::String(text) => text.zeroize(),
        Value::Array(items) => items.iter_mut().for_each(wipe),
        Value::Object(items) => items.values_mut().for_each(wipe),
        _ => {},
    }
}

impl Drop for Payload { fn drop(&mut self) { wipe(&mut self.0); } }

#[derive(Clone)]
struct LocalApi { client: Client, endpoint: Url, password: Arc<Zeroizing<String>> }

impl LocalApi {
    fn new(endpoint: Url, password: String) -> Result<Self, String> {
        let client = Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2)).timeout(REQUEST_TIMEOUT).build()
            .map_err(|_| "无法创建 OpenCode 本地账号连接")?;
        Ok(Self { client, endpoint, password: Arc::new(Zeroizing::new(password)) })
    }

    fn url(&self, segments: &[&str]) -> Result<Url, ApiError> {
        let mut url = self.endpoint.clone();
        {
            let mut path = url.path_segments_mut().map_err(|_| ApiError::Invalid)?;
            path.clear();
            for segment in segments { path.push(segment); }
        }
        Ok(url)
    }

    async fn send(&self, method: Method, segments: &[&str], body: Option<&Value>, timeout: Duration) -> Result<Payload, ApiError> {
        let mut request = self.client.request(method, self.url(segments)?)
            .basic_auth("opencode", Some(self.password.as_str())).timeout(timeout);
        let encoded = body.map(|value| serde_json::to_string(value).map(Zeroizing::new)).transpose().map_err(|_| ApiError::Invalid)?;
        if let Some(encoded) = encoded.as_ref() {
            request = request.header(reqwest::header::CONTENT_TYPE, "application/json").body(encoded.as_bytes().to_vec());
        }
        let response = request.send().await.map_err(|_| ApiError::Transport)?;
        if !response.status().is_success() { return Err(ApiError::Status(response.status().as_u16())); }
        if response.status() == reqwest::StatusCode::NO_CONTENT { return Ok(Payload(Value::Null)); }
        if response.content_length().is_some_and(|size| size > MAX_RESPONSE as u64) { return Err(ApiError::Invalid); }
        let mut bytes = Zeroizing::new(Vec::new());
        let mut stream = response.bytes_stream();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|_| ApiError::Transport)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE { return Err(ApiError::Invalid); }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map(Payload).map_err(|_| ApiError::Invalid)
    }

    fn call(&self, method: Method, segments: &[&str], body: Option<&Value>) -> Result<Payload, ApiError> {
        tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| ApiError::Transport)?
            .block_on(self.send(method, segments, body, REQUEST_TIMEOUT))
    }

    fn callback(&self, provider: &str, method: usize, cancellation: &Cancellation) -> Result<(), String> {
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|_| "无法启动 OpenCode 授权等待")?;
        runtime.block_on(async {
            let body = json!({"method":method});
            let path = ["provider", provider, "oauth", "callback"];
            tokio::select! {
                biased;
                _ = cancellation.cancelled() => Err("OpenCode 授权已取消".into()),
                result = self.send(Method::POST, &path, Some(&body), LOGIN_TIMEOUT) => {
                    let result = result.map_err(ApiError::message)?;
                    if result.0 == Value::Bool(true) { Ok(()) } else { Err(ApiError::Invalid.message()) }
                }
            }
        })
    }
}

struct Automatic { result: Receiver<Result<(), String>>, cancellation: Cancellation, worker: Option<JoinHandle<()>> }

impl Drop for Automatic {
    fn drop(&mut self) {
        self.cancellation.cancel();
        if let Some(worker) = self.worker.take() { let _ = worker.join(); }
    }
}

struct LoginFlow {
    provider: String,
    method: String,
    attempt: Option<String>,
    url: String,
    instructions: String,
    requires_code: bool,
    status: String,
    started: Instant,
    automatic: Option<Automatic>,
}

impl Drop for LoginFlow {
    fn drop(&mut self) { self.url.zeroize(); self.instructions.zeroize(); }
}

pub(super) struct OpenCodeDriver {
    child: Option<Child>,
    api: LocalApi,
    schema: OpenCodeSchema,
    login: Option<LoginFlow>,
    failed: bool,
}

fn endpoint(text: &str) -> Result<Url, String> {
    let url = Url::parse(text).map_err(|_| "OpenCode 本地服务地址无效")?;
    if url.scheme() != "http" || url.host_str() != Some("127.0.0.1") || url.port().is_none_or(|port| port == 0)
        || !url.username().is_empty() || url.password().is_some() || url.query().is_some() || url.fragment().is_some()
        || !matches!(url.path(), "" | "/") { return Err("OpenCode 本地服务地址不符合安全要求".into()); }
    Ok(url)
}

fn authorization_url(value: &Value) -> Result<String, String> {
    let text = value.as_str().ok_or("OpenCode 未返回授权链接")?;
    let url = Url::parse(text).map_err(|_| "OpenCode 授权链接无效")?;
    if text.len() > 8192 || url.scheme() != "https" || url.host_str().is_none() || !url.username().is_empty()
        || url.password().is_some() || url.port().is_some_and(|port| port != 443) {
        return Err("OpenCode 返回了不支持的授权链接".into());
    }
    Ok(url.into())
}

fn identifier(value: &Value) -> Result<String, String> {
    let text = value.as_str().ok_or("OpenCode 标识无效")?;
    if text.is_empty() || text.len() > 256 || text.chars().any(|character| character.is_control())
        || matches!(text, "." | "..") { return Err("OpenCode 标识无效".into()); }
    Ok(text.into())
}

fn text(value: Option<&Value>, maximum: usize) -> Result<String, String> {
    let value = value.and_then(Value::as_str).ok_or("OpenCode 登录说明无效")?;
    if value.len() > maximum || value.contains('\0') { return Err("OpenCode 登录说明超出限制".into()); }
    Ok(crate::privacy::redact_diagnostic(value))
}

fn prompts(value: Option<&Value>) -> Result<Vec<NativeLoginPrompt>, String> {
    let Some(value) = value.filter(|value| !value.is_null()) else { return Ok(Vec::new()); };
    let items = value.as_array().filter(|items| items.len() <= 24).ok_or("OpenCode 登录表单不受支持")?;
    let mut result = Vec::new();
    let mut keys = BTreeSet::new();
    for item in items {
        let key = identifier(&item["key"])?;
        if keys.contains(&key) || matches!(key.as_str(), "__proto__" | "constructor" | "prototype") { return Err("OpenCode 登录表单字段无效或重复".into()); }
        let kind = item["type"].as_str().filter(|kind| matches!(*kind, "text" | "select")).ok_or("OpenCode 登录字段不受支持")?.to_owned();
        let mut options = Vec::new();
        if kind == "select" {
            for option in item["options"].as_array().filter(|options| !options.is_empty() && options.len() <= 128).ok_or("OpenCode 登录选项无效")? {
                let value = option["value"].as_str().filter(|value| value.len() <= 1024 && !value.contains('\0')).ok_or("OpenCode 登录选项超出限制")?;
                options.push(NativeLoginOption { label: text(option.get("label"), 2048)?, value: value.into() });
            }
        }
        let when = if let Some(condition) = item.get("when").filter(|value| !value.is_null()) {
            let condition_key = identifier(&condition["key"])?;
            let op = condition["op"].as_str().filter(|op| matches!(*op, "eq" | "neq")).ok_or("OpenCode 登录条件无效")?;
            let value = condition["value"].as_str().filter(|value| value.len() <= 1024 && !value.contains('\0')).ok_or("OpenCode 登录条件无效")?;
            if !keys.contains(&condition_key) { return Err("OpenCode 登录条件必须引用前面的字段".into()); }
            Some(NativeLoginCondition { key: condition_key, op: op.into(), value: value.into() })
        } else { None };
        keys.insert(key.clone());
        result.push(NativeLoginPrompt { kind, key, message: text(item.get("message"), 2048)?, options, when });
    }
    Ok(result)
}

fn active(prompt: &NativeLoginPrompt, inputs: &BTreeMap<String, String>) -> bool {
    prompt.when.as_ref().is_none_or(|condition| inputs.get(&condition.key).is_some_and(|value| {
        if condition.op == "eq" { value == &condition.value } else { value != &condition.value }
    }))
}

fn validate_inputs(method: &NativeLoginMethod, inputs: &BTreeMap<String, String>) -> Result<(), String> {
    if inputs.len() > 24 || inputs.values().any(|value| value.len() > 4096 || value.contains('\0'))
        || inputs.values().map(String::len).sum::<usize>() > 65536 { return Err("OpenCode 登录输入超出限制".into()); }
    for key in inputs.keys() {
        if !method.prompts.iter().any(|prompt| &prompt.key == key && active(prompt, inputs)) { return Err("OpenCode 登录输入不匹配所选方式".into()); }
    }
    for prompt in method.prompts.iter().filter(|prompt| active(prompt, inputs)) {
        let value = inputs.get(&prompt.key).ok_or("请填写当前登录方式要求的字段")?;
        if prompt.kind == "select" && !prompt.options.iter().any(|option| &option.value == value) { return Err("OpenCode 登录选项不匹配".into()); }
    }
    Ok(())
}

fn read_endpoint(reader: &mut impl BufRead) -> Result<Url, String> {
    let mut total = 0usize;
    loop {
        let mut line = Zeroizing::new(Vec::new());
        loop {
            let bytes = reader.fill_buf().map_err(|_| "无法读取 OpenCode 启动状态")?;
            if bytes.is_empty() { return Err("OpenCode 在启动完成前已退出".into()); }
            let count = bytes.iter().position(|byte| *byte == b'\n').map_or(bytes.len(), |index| index + 1);
            total = total.saturating_add(count);
            if line.len().saturating_add(count) > MAX_LINE || total > MAX_RESPONSE { return Err("OpenCode 启动输出超出限制".into()); }
            line.extend_from_slice(&bytes[..count]);
            reader.consume(count);
            if line.last() == Some(&b'\n') { break; }
        }
        if let Ok(line) = std::str::from_utf8(&line) {
            if let Some(address) = line.trim().strip_prefix("opencode server listening on ") { return endpoint(address); }
        }
    }
}

fn discard_output(reader: &mut impl std::io::Read) {
    let mut buffer = Zeroizing::new([0u8; 8192]);
    loop {
        match reader.read(buffer.as_mut_slice()) {
            Ok(0) | Err(_) => return,
            Ok(_) => buffer.zeroize(),
        }
    }
}

fn data(payload: &Payload, schema: OpenCodeSchema) -> Result<&Value, String> {
    match schema {
        OpenCodeSchema::V1 => Ok(&payload.0),
        OpenCodeSchema::V2 => payload.0.get("data").ok_or_else(|| ApiError::Invalid.message()),
    }
}

fn parse_methods(value: &Value, schema: OpenCodeSchema) -> Result<Vec<NativeLoginMethod>, String> {
    let mut result = Vec::new();
    let providers: Vec<(String, String, &Vec<Value>)> = match schema {
        OpenCodeSchema::V1 => value.as_object().filter(|providers| providers.len() <= 512).ok_or("OpenCode 登录提供商列表无效")?
            .iter().map(|(id, methods)| {
                let provider = identifier(&Value::String(id.clone()))?;
                let methods = methods.as_array().filter(|methods| methods.len() <= 32).ok_or("OpenCode 登录方式超出限制")?;
                Ok((provider.clone(), crate::privacy::redact_diagnostic(&provider), methods))
            }).collect::<Result<_, String>>()?,
        OpenCodeSchema::V2 => value.as_array().filter(|providers| providers.len() <= 512).ok_or("OpenCode 集成列表无效")?
            .iter().map(|provider| {
                let id = identifier(&provider["id"])?;
                let methods = provider["methods"].as_array().filter(|methods| methods.len() <= 32).ok_or("OpenCode 登录方式超出限制")?;
                Ok((id, text(provider.get("name"), 256)?, methods))
            }).collect::<Result<_, String>>()?,
    };
    for (provider_id, provider_label, methods) in providers {
        for (index, method) in methods.iter().enumerate() {
            if method["type"] != "oauth" { continue; }
            let method_id = if schema == OpenCodeSchema::V1 { index.to_string() } else { identifier(&method["id"])? };
            result.push(NativeLoginMethod {
                provider_id: provider_id.clone(), provider_label: provider_label.clone(), method_id,
                label: text(method.get("label"), 2048)?, prompts: prompts(method.get("prompts"))?,
            });
        }
    }
    Ok(result)
}

fn parse_connections(value: &Value, schema: OpenCodeSchema) -> Result<Vec<NativeProviderConnection>, String> {
    let mut result = Vec::new();
    match schema {
        OpenCodeSchema::V1 => {
            for provider in value["connected"].as_array().filter(|items| items.len() <= 512).ok_or("OpenCode 连接列表无效")? {
                let provider_id = identifier(provider)?;
                result.push(NativeProviderConnection { label: crate::privacy::redact_diagnostic(&provider_id), provider_id, connection_id: None, disconnectable: true });
            }
        }
        OpenCodeSchema::V2 => {
            for provider in value.as_array().filter(|items| items.len() <= 512).ok_or("OpenCode 集成列表无效")? {
                let provider_id = identifier(&provider["id"])?;
                let label = text(provider.get("name"), 256)?;
                let connections = provider["connections"].as_array().filter(|items| items.len() <= 128).ok_or("OpenCode 连接数量超出限制")?;
                for connection in connections {
                    let (connection_id, disconnectable) = match connection["type"].as_str() {
                        Some("credential") => (Some(identifier(&connection["id"])?), true),
                        Some("env") => (None, false),
                        _ => return Err("OpenCode 连接类型不受支持".into()),
                    };
                    result.push(NativeProviderConnection { provider_id: provider_id.clone(), label: label.clone(), connection_id, disconnectable });
                }
            }
        }
    }
    Ok(result)
}

impl OpenCodeDriver {
    pub(super) fn spawn(binary: Option<&Path>, home: &Path, target: &OpenCodeTarget) -> Result<Self, String> {
        if !home.is_absolute() { return Err("OpenCode 用户目录必须是绝对路径".into()); }
        crate::client_config::validate_path(&target.path)?;
        if let Some(binary) = binary { crate::client_config::validate_opencode_binary(binary)?; }
        let password = Zeroizing::new(format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple()));
        let mut command = Command::new(binary.unwrap_or_else(|| Path::new("opencode")));
        command.args(["serve", "--hostname=127.0.0.1", "--port=0"]).current_dir(home)
            .env("HOME", home).env("USERPROFILE", home)
            .env("OPENCODE_CONFIG", &target.path).env("OPENCODE_DISABLE_PROJECT_CONFIG", "true")
            .env("OPENCODE_SERVER_USERNAME", "opencode").env("OPENCODE_SERVER_PASSWORD", password.as_str())
            .env_remove("OPENCODE_CONFIG_CONTENT").env_remove("OPENCODE_CONFIG_DIR").env_remove("OPENCODE_TEST_HOME")
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|_| "无法启动已安装的 OpenCode，请在客户端设置选择原生可执行文件")?;
        let Some(output) = child.stdout.take() else {
            let _ = child.kill(); let _ = child.wait(); return Err("无法连接 OpenCode 启动输出".into());
        };
        let (sender, receiver) = mpsc::sync_channel(1);
        if std::thread::Builder::new().name("xy-opencode-startup".into()).spawn(move || {
            let mut reader = BufReader::new(output);
            let address = read_endpoint(&mut reader);
            let ready = address.is_ok();
            if sender.send(address).is_ok() && ready {
                discard_output(&mut reader);
            }
        }).is_err() {
            let _ = child.kill(); let _ = child.wait(); return Err("无法读取 OpenCode 启动状态".into());
        }
        let address = match receiver.recv_timeout(Duration::from_secs(20)) {
            Ok(Ok(address)) => address,
            _ => { let _ = child.kill(); let _ = child.wait(); return Err("OpenCode 本地服务启动失败或超时".into()); },
        };
        let api = match LocalApi::new(address, password.to_string()) {
            Ok(api) => api,
            Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); },
        };
        let mut driver = Self { child: Some(child), api, schema: target.schema.unwrap_or(OpenCodeSchema::V1), login: None, failed: false };
        if target.schema.is_none() {
            match driver.api.call(Method::GET, &["api", "integration"], None) {
                Ok(response) if data(&response, OpenCodeSchema::V2)?.is_array() => driver.schema = OpenCodeSchema::V2,
                Err(ApiError::Status(404)) => {},
                Err(error) => return Err(error.message()),
                _ => return Err(ApiError::Invalid.message()),
            }
        }
        Ok(driver)
    }

    fn alive(&mut self) -> Result<(), String> {
        if self.failed || self.child.as_mut().is_some_and(|child| !matches!(child.try_wait(), Ok(None))) {
            self.failed = true;
            return Err("OpenCode 自有本地服务已退出，请重试".into());
        }
        Ok(())
    }

    pub(super) fn terminated(&self) -> bool { self.failed }
    pub(super) fn login_status(&self) -> Option<&str> { self.login.as_ref().map(|flow| flow.status.as_str()) }
    pub(super) fn login_pending(&self) -> bool { self.login_status() == Some("pending") }

    pub(super) fn login_methods(&mut self) -> Result<Vec<NativeLoginMethod>, String> {
        self.alive()?;
        let path = if self.schema == OpenCodeSchema::V1 { vec!["provider", "auth"] } else { vec!["api", "integration"] };
        let response = self.api.call(Method::GET, &path, None).map_err(ApiError::message)?;
        parse_methods(data(&response, self.schema)?, self.schema)
    }

    pub(super) fn account(&mut self, client: &str) -> Result<NativeAccountSnapshot, String> {
        self.alive()?;
        let path = if self.schema == OpenCodeSchema::V1 { vec!["provider"] } else { vec!["api", "integration"] };
        let response = self.api.call(Method::GET, &path, None).map_err(ApiError::message)?;
        let providers = parse_connections(data(&response, self.schema)?, self.schema)?;
        Ok(NativeAccountSnapshot {
            client_id: client.into(), auth_mode: if providers.is_empty() { "not_logged_in" } else { "connected_providers" }.into(),
            plan: None, checked_at: crate::governance::now(), quota: None, quotas: Vec::new(),
            identities: providers.iter().filter_map(super::pool::NativeIdentity::opencode).collect(),
            providers, pool_candidates: Vec::new(),
        })
    }

    pub(super) fn disconnect(&mut self, provider: &str, connection: Option<&str>) -> Result<(), String> {
        if self.login_pending() { return Err("请先完成或取消 OpenCode 登录".into()); }
        let snapshot = self.account("opencode-cli")?;
        if !snapshot.providers.iter().any(|item| item.provider_id == provider && item.connection_id.as_deref() == connection && item.disconnectable) {
            return Err("所选原生连接已变化或不能由 XY Switch 退出".into());
        }
        let response = match self.schema {
            OpenCodeSchema::V1 => self.api.call(Method::DELETE, &["auth", provider], None),
            OpenCodeSchema::V2 => self.api.call(Method::DELETE, &["api", "credential", connection.ok_or("请选择具体的原生凭据连接")?], None),
        }.map_err(ApiError::message)?;
        if self.schema == OpenCodeSchema::V1 && response.0 != Value::Bool(true) { return Err(ApiError::Invalid.message()); }
        Ok(())
    }
}

impl Drop for OpenCodeDriver {
    fn drop(&mut self) {
        self.login = None;
        if let Some(mut child) = self.child.take() { let _ = child.kill(); let _ = child.wait(); }
    }
}

pub(super) struct Inputs(pub(super) BTreeMap<String, String>);

impl Inputs {
    pub(super) fn clear(&mut self) { self.0.values_mut().for_each(Zeroize::zeroize); }
}

impl Drop for Inputs {
    fn drop(&mut self) { self.clear(); }
}

impl OpenCodeDriver {
    pub(super) fn start_login(&mut self, client: &str, provider: &str, method: &str, inputs: BTreeMap<String, String>) -> Result<NativeLoginProgress, String> {
        let inputs = Inputs(inputs);
        if self.login_pending() { return Err("OpenCode 登录正在进行，请先完成或取消".into()); }
        let selected = self.login_methods()?.into_iter().find(|item| item.provider_id == provider && item.method_id == method)
            .ok_or("所选 OpenCode 登录方式已经变化，请重新查询")?;
        validate_inputs(&selected, &inputs.0)?;
        let body = Payload(match self.schema {
            OpenCodeSchema::V1 => json!({"method":method.parse::<usize>().map_err(|_| "OpenCode 登录方法索引无效")?,"inputs":inputs.0}),
            OpenCodeSchema::V2 => json!({"methodID":method,"inputs":inputs.0}),
        });
        let response = match self.schema {
            OpenCodeSchema::V1 => self.api.call(Method::POST, &["provider", provider, "oauth", "authorize"], Some(&body.0)),
            OpenCodeSchema::V2 => self.api.call(Method::POST, &["api", "integration", provider, "connect", "oauth"], Some(&body.0)),
        }.map_err(ApiError::message)?;
        let attempt = data(&response, self.schema)?;
        let attempt_id = if self.schema == OpenCodeSchema::V2 { Some(identifier(&attempt["attemptID"])?)} else { None };
        let details = (|| {
            let url = authorization_url(&attempt["url"])?;
            let instructions = text(attempt.get("instructions"), 16384)?;
            let mode = attempt[if self.schema == OpenCodeSchema::V1 { "method" } else { "mode" }].as_str()
                .filter(|mode| matches!(*mode, "auto" | "code")).ok_or("OpenCode 授权模式不受支持")?;
            Ok::<_, String>((url, instructions, mode == "code"))
        })();
        let (url, instructions, requires_code) = match details {
            Ok(details) => details,
            Err(error) => {
                if let Some(attempt) = attempt_id.as_deref() { let _ = self.api.call(Method::DELETE, &["api", "integration", "attempt", attempt], None); }
                return Err(error);
            }
        };
        let mut flow = LoginFlow {
            provider: provider.into(), method: method.into(), attempt: attempt_id, url, instructions, requires_code,
            status: "pending".into(), started: Instant::now(), automatic: None,
        };
        if self.schema == OpenCodeSchema::V1 && !requires_code {
            let api = self.api.clone();
            let provider = provider.to_owned();
            let method = method.parse::<usize>().map_err(|_| "OpenCode 登录方法索引无效")?;
            let cancellation = Cancellation::default();
            let worker_cancellation = cancellation.clone();
            let (sender, result) = mpsc::sync_channel(1);
            let worker = std::thread::Builder::new().name("xy-opencode-oauth".into()).spawn(move || {
                let _ = sender.send(api.callback(&provider, method, &worker_cancellation));
            }).map_err(|_| "无法启动 OpenCode 授权等待")?;
            flow.automatic = Some(Automatic { result, cancellation, worker: Some(worker) });
        }
        self.login = Some(flow);
        self.progress(client)
    }

    fn progress(&self, client: &str) -> Result<NativeLoginProgress, String> {
        let flow = self.login.as_ref().ok_or("没有进行中的 OpenCode 登录")?;
        let pending = flow.status == "pending";
        Ok(NativeLoginProgress {
            client_id: client.into(), status: flow.status.clone(),
            verification_url: pending.then(|| flow.url.clone()), user_code: None,
            provider_id: Some(flow.provider.clone()), instructions: pending.then(|| flow.instructions.clone()),
            requires_code: pending && flow.requires_code,
        })
    }

    fn finish(flow: &mut LoginFlow, status: &str) {
        flow.status = status.into();
        flow.url.zeroize(); flow.instructions.zeroize();
        flow.automatic = None;
    }

    pub(super) fn login_progress(&mut self, client: &str) -> Result<NativeLoginProgress, String> {
        self.alive()?;
        let flow = self.login.as_mut().ok_or("没有进行中的 OpenCode 登录")?;
        if flow.status != "pending" { return self.progress(client); }
        if flow.started.elapsed() >= LOGIN_TIMEOUT {
            if let Some(attempt) = flow.attempt.as_deref() { let _ = self.api.call(Method::DELETE, &["api", "integration", "attempt", attempt], None); }
            Self::finish(flow, "expired");
        } else if self.schema == OpenCodeSchema::V2 {
            let response = self.api.call(Method::GET, &["api", "integration", "attempt", flow.attempt.as_deref().ok_or("OpenCode 授权标识缺失")?], None).map_err(ApiError::message)?;
            let status = data(&response, self.schema)?["status"].as_str().ok_or("OpenCode 授权状态无效")?;
            match status {
                "pending" => {},
                "complete" => Self::finish(flow, "completed"),
                "failed" => Self::finish(flow, "failed"),
                "expired" => Self::finish(flow, "expired"),
                _ => return Err("OpenCode 授权状态不受支持".into()),
            }
        } else if let Some(automatic) = flow.automatic.as_ref() {
            match automatic.result.try_recv() {
                Ok(Ok(())) => Self::finish(flow, "completed"),
                Ok(Err(_)) | Err(mpsc::TryRecvError::Disconnected) => Self::finish(flow, "failed"),
                Err(mpsc::TryRecvError::Empty) => {},
            }
        }
        self.progress(client)
    }

    pub(super) fn complete_login(&mut self, client: &str, code: &str) -> Result<NativeLoginProgress, String> {
        self.alive()?;
        let flow = self.login.as_ref().filter(|flow| flow.status == "pending" && flow.requires_code).ok_or("当前 OpenCode 登录不需要提交授权码")?;
        if flow.started.elapsed() >= LOGIN_TIMEOUT { return self.login_progress(client); }
        if code.trim().is_empty() || code.len() > 8192 || code.chars().any(|character| character.is_control()) {
            return Err("请填写有效的 OpenCode 授权码".into());
        }
        let body = Payload(if self.schema == OpenCodeSchema::V1 {
            json!({"method":flow.method.parse::<usize>().map_err(|_| "OpenCode 登录方法索引无效")?,"code":code.trim()})
        } else { json!({"code":code.trim()}) });
        let response = match self.schema {
            OpenCodeSchema::V1 => self.api.call(Method::POST, &["provider", &flow.provider, "oauth", "callback"], Some(&body.0)),
            OpenCodeSchema::V2 => self.api.call(Method::POST, &["api", "integration", "attempt", flow.attempt.as_deref().ok_or("OpenCode 授权标识缺失")?, "complete"], Some(&body.0)),
        }.map_err(ApiError::message)?;
        if self.schema == OpenCodeSchema::V1 {
            if response.0 != Value::Bool(true) { return Err(ApiError::Invalid.message()); }
            Self::finish(self.login.as_mut().unwrap(), "completed");
            self.progress(client)
        } else { self.login_progress(client) }
    }

    pub(super) fn cancel_login(&mut self) -> Result<(), String> {
        let Some(flow) = self.login.as_mut() else { return Ok(()); };
        let result = if flow.status == "pending" {
            if let Some(attempt) = flow.attempt.as_deref() {
                self.api.call(Method::DELETE, &["api", "integration", "attempt", attempt], None).map(|_| ()).map_err(ApiError::message)
            } else { Ok(()) }
        } else { Ok(()) };
        Self::finish(flow, "cancelled");
        result
    }
}
