use super::{NativeAccountSnapshot, NativeLoginProgress, NativeQuota};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc::{self, Receiver, RecvTimeoutError, TryRecvError},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const MAX_FRAME: usize = 256 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

struct LoginFlow {
    id: String,
    verification_url: String,
    user_code: String,
    status: String,
}

pub(super) struct CodexDriver {
    child: Option<Child>,
    input: Box<dyn Write + Send>,
    messages: Receiver<Result<Value, ()>>,
    next_id: u64,
    failed: bool,
    login: Option<LoginFlow>,
    completed: BTreeMap<String, bool>,
    timeout: Duration,
}

fn read_frame(reader: &mut impl BufRead) -> Result<Option<Value>, ()> {
    let mut frame = zeroize::Zeroizing::new(Vec::new());
    loop {
        let available = reader.fill_buf().map_err(|_| ())?;
        if available.is_empty() {
            if frame.is_empty() { return Ok(None); }
            return Err(());
        }
        let length = available.iter().position(|byte| *byte == b'\n').map(|index| index + 1).unwrap_or(available.len());
        if frame.len().saturating_add(length) > MAX_FRAME { return Err(()); }
        frame.extend_from_slice(&available[..length]);
        reader.consume(length);
        if frame.last() == Some(&b'\n') {
            return serde_json::from_slice(&frame).map(Some).map_err(|_| ());
        }
    }
}

fn device_url(value: &Value) -> Result<String, String> {
    let text = value.as_str().ok_or("Codex 未返回有效的设备授权地址")?;
    let url = reqwest::Url::parse(text).map_err(|_| "Codex 设备授权地址无效")?;
    if url.scheme() != "https"
        || !matches!(url.host_str(), Some("auth.openai.com" | "chatgpt.com" | "www.chatgpt.com"))
        || !url.username().is_empty() || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || text.len() > 2048
    {
        return Err("Codex 返回了非官方设备授权地址".into());
    }
    Ok(url.into())
}

fn quota_window(value: &Value, name: &str) -> Option<NativeQuota> {
    if !value.is_object() { return None; }
    Some(NativeQuota {
        source: format!("codex.{name}"),
        used_percent: value.get("usedPercent").and_then(Value::as_f64).filter(|number| number.is_finite() && (0.0..=100.0).contains(number)),
        reset_at: value.get("resetsAt").and_then(Value::as_i64).filter(|number| (0..=253402300799).contains(number)),
        window_minutes: value.get("windowDurationMins").and_then(Value::as_i64).filter(|number| (1..=525600).contains(number)),
    })
}

impl CodexDriver {
    pub(super) fn spawn(binary: Option<&Path>, codex_home: &Path) -> Result<Self, String> {
        if !codex_home.is_absolute() { return Err("CODEX_HOME 必须是绝对路径".into()); }
        if let Some(binary) = binary { crate::client_config::validate_codex_binary(binary)?; }
        let mut command = Command::new(binary.unwrap_or_else(|| Path::new("codex")));
        command.arg("app-server")
            .env("CODEX_HOME", codex_home)
            .env_remove("OPENAI_API_KEY")
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x08000000);
        }
        let mut child = command.spawn().map_err(|_| "无法启动已安装的 Codex，请在客户端设置选择原生可执行文件")?;
        let input = child.stdin.take().ok_or("无法连接 Codex 登录输入")?;
        let output = child.stdout.take().ok_or("无法连接 Codex 登录输出")?;
        let (sender, messages) = mpsc::sync_channel(16);
        if std::thread::Builder::new().name("xy-codex-account".into()).spawn(move || {
            let mut reader = BufReader::new(output);
            loop {
                match read_frame(&mut reader) {
                    Ok(Some(value)) => if sender.send(Ok(value)).is_err() { break; },
                    Ok(None) => break,
                    Err(()) => { let _ = sender.send(Err(())); break; }
                }
            }
        }).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("无法启动 Codex 账号读取任务".into());
        }
        let mut driver = Self {
            child: Some(child), input: Box::new(input), messages, next_id: 1,
            failed: false, login: None, completed: BTreeMap::new(), timeout: REQUEST_TIMEOUT,
        };
        driver.initialize()?;
        Ok(driver)
    }

    fn initialize(&mut self) -> Result<(), String> {
        self.request("initialize", json!({"clientInfo":{"name":"xy_switch","title":"XY Switch","version":env!("CARGO_PKG_VERSION")}}))?;
        self.send(&json!({"method":"initialized","params":{}}))
    }

    pub(super) fn terminated(&self) -> bool { self.failed }

    pub(super) fn login_pending(&self) -> bool {
        self.login.as_ref().is_some_and(|flow| flow.status == "pending")
    }

    pub(super) fn login_status(&self) -> Option<&str> {
        self.login.as_ref().map(|flow| flow.status.as_str())
    }

    fn fail(&mut self, message: &str) -> String {
        self.failed = true;
        if let Some(child) = &mut self.child { let _ = child.kill(); let _ = child.wait(); }
        message.into()
    }

    fn send(&mut self, value: &Value) -> Result<(), String> {
        let mut bytes = zeroize::Zeroizing::new(serde_json::to_vec(value).map_err(|_| "无法编码 Codex 账号请求")?);
        bytes.push(b'\n');
        if self.input.write_all(&bytes).and_then(|()| self.input.flush()).is_err() {
            return Err(self.fail("Codex 登录连接已关闭"));
        }
        Ok(())
    }

    fn notification(&mut self, message: &Value) -> Result<(), String> {
        if message.get("id").is_some() && message.get("method").is_some() {
            return self.send(&json!({"id":message["id"],"error":{"code":-32601,"message":"This host supports managed account authentication only"}}));
        }
        if message.get("method").and_then(Value::as_str) == Some("account/login/completed") {
            let params = &message["params"];
            if let (Some(id), Some(success)) = (params.get("loginId").and_then(Value::as_str), params.get("success").and_then(Value::as_bool)) {
                if id.len() <= 512 && self.completed.len() < 8 {
                    self.completed.insert(id.into(), success);
                    if let Some(flow) = &mut self.login && flow.id == id {
                        flow.status = if success { "completed" } else { "failed" }.into();
                        flow.user_code.clear();
                    }
                }
            }
        }
        Ok(())
    }

    fn request(&mut self, method: &str, params: Value) -> Result<Value, String> {
        if self.failed { return Err("Codex 登录连接需要重新建立".into()); }
        let id = self.next_id;
        self.next_id = self.next_id.checked_add(1).ok_or("Codex 请求编号已用尽")?;
        self.send(&json!({"id":id,"method":method,"params":params}))?;
        let deadline = Instant::now() + self.timeout;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let message = match self.messages.recv_timeout(remaining) {
                Ok(Ok(message)) => message,
                Ok(Err(())) => return Err(self.fail("Codex 返回了无效或超限的账号响应")),
                Err(RecvTimeoutError::Timeout) => return Err(self.fail("Codex 账号操作超时，请在官方客户端检查网络或登录")),
                Err(RecvTimeoutError::Disconnected) => return Err(self.fail("Codex 登录进程已结束")),
            };
            if message.get("id").and_then(Value::as_u64) == Some(id) && message.get("method").is_none() {
                if message.get("error").is_some() {
                    return Err("Codex 拒绝了账号操作，请检查原生登录状态与客户端版本".into());
                }
                return message.get("result").cloned().ok_or_else(|| self.fail("Codex 账号响应缺少结果"));
            }
            self.notification(&message)?;
            if Instant::now() >= deadline { return Err(self.fail("Codex 账号操作超时")); }
        }
    }

    pub(super) fn account(&mut self, client: &str, refresh: bool) -> Result<NativeAccountSnapshot, String> {
        let response = self.request("account/read", json!({"refreshToken":refresh}))?;
        let account = &response["account"];
        let auth_mode = match account.get("type").and_then(Value::as_str) {
            Some("apiKey") => "api_key",
            Some("chatgpt") => "chatgpt",
            Some("chatgptAuthTokens") => "external_chatgpt",
            Some("amazonBedrock") => "amazon_bedrock",
            None if account.is_null() => "signed_out",
            _ => "unsupported",
        };
        let plan = account.get("planType").and_then(Value::as_str).map(str::to_ascii_lowercase)
            .filter(|value| matches!(value.as_str(), "free"|"go"|"plus"|"pro"|"team"|"business"|"enterprise"|"education"|"edu"|"unknown"));
        let mut quotas = Vec::new();
        if matches!(auth_mode, "chatgpt"|"external_chatgpt") {
            if let Ok(result) = self.request("account/rateLimits/read", Value::Null) {
                for name in ["primary", "secondary"] {
                    if let Some(window) = quota_window(&result["rateLimits"][name], name) { quotas.push(window); }
                }
            } else if self.failed { return Err("Codex 额度查询连接已中断，请重试".into()); }
        }
        Ok(NativeAccountSnapshot {
            client_id: client.into(), auth_mode: auth_mode.into(), plan,
            checked_at: SystemTime::now().duration_since(UNIX_EPOCH).map(|duration| duration.as_secs().min(i64::MAX as u64) as i64).unwrap_or(0),
            quota: quotas.first().cloned(), quotas, providers: Vec::new(), pool_candidates: Vec::new(),
            identities: super::pool::NativeIdentity::codex(account).into_iter().collect(),
        })
    }

    pub(super) fn start_login(&mut self, client: &str) -> Result<NativeLoginProgress, String> {
        if self.login_pending() { return Err("已有 Codex 设备授权正在等待完成".into()); }
        self.completed.clear();
        let result = self.request("account/login/start", json!({"type":"chatgptDeviceCode"}))?;
        let id = match result.get("loginId").and_then(Value::as_str).filter(|value| !value.is_empty() && value.len() <= 512) {
            Some(id) => id,
            None => return Err(self.fail("Codex 未返回有效的登录编号，已停止登录进程")),
        };
        let code = match result.get("userCode").and_then(Value::as_str)
            .filter(|value| !value.is_empty() && value.len() <= 32 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')) {
            Some(code) => code,
            None => {
                let _ = self.request("account/login/cancel", json!({"loginId":id}));
                return Err(self.fail("Codex 未返回有效的设备授权码，已停止登录进程"));
            }
        };
        let verification_url = match device_url(&result["verificationUrl"]) {
            Ok(url) => url,
            Err(error) => {
                let _ = self.request("account/login/cancel", json!({"loginId":id}));
                return Err(self.fail(&error));
            }
        };
        let status = self.completed.get(id).map(|success| if *success { "completed" } else { "failed" }).unwrap_or("pending");
        self.login = Some(LoginFlow { id: id.into(), verification_url, user_code: if status == "pending" { code.into() } else { String::new() }, status: status.into() });
        self.progress(client)
    }

    fn progress(&self, client: &str) -> Result<NativeLoginProgress, String> {
        let flow = self.login.as_ref().ok_or("没有正在进行的 Codex 设备授权")?;
        let pending = flow.status == "pending";
        Ok(NativeLoginProgress {
            client_id: client.into(), status: flow.status.clone(),
            verification_url: pending.then(|| flow.verification_url.clone()),
            user_code: pending.then(|| flow.user_code.clone()),
            provider_id: None, instructions: None, requires_code: false,
        })
    }

    pub(super) fn login_progress(&mut self, client: &str) -> Result<NativeLoginProgress, String> {
        for _ in 0..64 {
            match self.messages.try_recv() {
                Ok(Ok(message)) => self.notification(&message)?,
                Ok(Err(())) => return Err(self.fail("Codex 登录响应无效或超限")),
                Err(TryRecvError::Disconnected) => return Err(self.fail("Codex 登录进程已结束")),
                Err(TryRecvError::Empty) => break,
            }
        }
        self.progress(client)
    }

    pub(super) fn cancel_login(&mut self) -> Result<(), String> {
        if let Some(flow) = &self.login && flow.status == "pending" {
            let id = flow.id.clone();
            if self.request("account/login/cancel", json!({"loginId":id})).is_err() {
                return Err(self.fail("Codex 未确认取消授权，已停止登录进程"));
            }
        }
        if let Some(flow) = &mut self.login { flow.status = "cancelled".into(); flow.user_code.clear(); }
        self.completed.clear();
        Ok(())
    }

    pub(super) fn logout(&mut self) -> Result<(), String> {
        self.cancel_login()?;
        self.request("account/logout", Value::Null)?;
        Ok(())
    }
}

impl Drop for CodexDriver {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child { let _ = child.kill(); let _ = child.wait(); }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    #[derive(Clone)]
    struct Capture(Arc<Mutex<Vec<u8>>>);
    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> { self.0.lock().unwrap().extend_from_slice(bytes); Ok(bytes.len()) }
        fn flush(&mut self) -> std::io::Result<()> { Ok(()) }
    }
    fn fixture(messages: Vec<Value>) -> (CodexDriver, Arc<Mutex<Vec<u8>>>, mpsc::Sender<Result<Value, ()>>) {
        let (sender, receiver) = mpsc::channel();
        for message in messages { sender.send(Ok(message)).unwrap(); }
        let captured = Arc::new(Mutex::new(Vec::new()));
        (CodexDriver { child: None, input: Box::new(Capture(captured.clone())), messages: receiver, next_id: 1, failed: false, login: None, completed: BTreeMap::new(), timeout: Duration::from_millis(25) }, captured, sender)
    }
    #[test]
    fn initialization_precedes_account_and_private_fields_never_reach_snapshot() {
        let (mut driver, captured, _sender) = fixture(vec![
            json!({"id":1,"result":{"userAgent":"private-native-agent"}}),
            json!({"id":2,"result":{"account":{"type":"chatgpt","email":"private@example.test","planType":"plus","accessToken":"synthetic-private-token","accountId":"private-id"}}}),
            json!({"id":3,"result":{"rateLimits":{"primary":{"usedPercent":32.0,"windowDurationMins":180,"resetsAt":1793710000},"secondary":{"usedPercent":70.0,"windowDurationMins":10080,"resetsAt":1793900000}}}}),
        ]);
        driver.initialize().unwrap();
        let snapshot = driver.account("codex-desktop", true).unwrap();
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert_eq!(snapshot.quotas.len(), 2);
        for secret in ["private@example.test", "synthetic-private-token", "private-id", "private-native-agent"] { assert!(!encoded.contains(secret)); }
        let sent: Vec<Value> = String::from_utf8(captured.lock().unwrap().clone()).unwrap().lines().map(|line| serde_json::from_str(line).unwrap()).collect();
        assert_eq!(sent[0]["method"], "initialize");
        assert_eq!(sent[1]["method"], "initialized");
        assert_eq!(sent[2]["params"]["refreshToken"], true);
    }
    #[test]
    fn manager_rejects_account_reads_and_binding_during_native_login() {
        let (mut driver, captured, _sender) = fixture(vec![json!({"id":1,"result":{
            "loginId":"fixture-login","verificationUrl":"https://auth.openai.com/device","userCode":"TEST-CODE"
        }})]);
        driver.start_login("codex-cli").unwrap();
        let written = captured.lock().unwrap().len();
        let database = crate::database::Database::open(std::path::Path::new(":memory:")).unwrap();
        let home = std::env::temp_dir().join(format!("xy-client-pending-{}", uuid::Uuid::new_v4()));
        let takeover = crate::takeover::AppTakeoverManager::new(database.clone(),
            crate::proxy_manager::ProxyManager::new(database.clone()), home.clone(), home.join("data"));
        let account_pool = crate::accounts::AccountPool::test_pool(database.clone()).unwrap();
        let manager = crate::clients::ClientManager::new(database, home.clone(), home.join(".codex"),
            crate::client_config::ClientTargets::for_home(&home), crate::client_config::ClientSettings::default(),
            takeover, account_pool.clone()).unwrap();
        *manager.driver.lock().unwrap() = Some(driver);
        for refresh in [false, true] {
            assert!(manager.read_account("codex-cli", true, refresh).err().unwrap().contains("请先完成或取消"));
        }
        assert!(manager.bind_account(crate::clients::BindNativeAccountRequest {
            client_id: "codex-desktop".into(), candidate_id: uuid::Uuid::new_v4().to_string(),
            alias: "synthetic-native".into(), provider_id: "synthetic-provider".into(),
            approved_destination: "https://provider.example.test".into(), confirmed: true,
        }).err().unwrap().contains("请先完成或取消"));
        assert_eq!(captured.lock().unwrap().len(), written);
        assert!(account_pool.list().unwrap().is_empty());
        assert!(manager.connections().iter().filter(|row| row.config_owner == "codex")
            .all(|row| row.login_status.as_deref() == Some("pending") && row.account.is_none()));
        manager.stop_owned_driver();
        assert!(manager.driver.lock().unwrap().is_none());
        assert!(manager.access.lock().unwrap().is_none());
        assert!(!home.exists());
    }

    #[test]
    fn device_login_completion_can_arrive_before_start_response_and_code_is_removed() {
        let (mut driver, _, _sender) = fixture(vec![
            json!({"method":"account/login/completed","params":{"loginId":"fixture-login","success":true,"error":"private-details"}}),
            json!({"id":1,"result":{"loginId":"fixture-login","verificationUrl":"https://auth.openai.com/codex/device","userCode":"TEST-CODE"}}),
        ]);
        let progress = driver.start_login("codex-cli").unwrap();
        assert_eq!(progress.status, "completed");
        assert!(progress.user_code.is_none());
        assert!(progress.verification_url.is_none());
    }
    #[test]
    fn invalid_device_flows_are_cancelled_and_transport_is_stopped() {
        for result in [
            json!({"verificationUrl":"https://auth.openai.com/device", "userCode":"TEST-CODE"}),
            json!({"loginId":"fixture-login", "verificationUrl":"https://auth.openai.com/device", "userCode":"private@example.test"}),
            json!({"loginId":"fixture-login", "verificationUrl":"https://untrusted.example.test/device", "userCode":"TEST-CODE"}),
        ] {
            let has_id = result.get("loginId").is_some();
            let (mut driver, captured, _sender) = fixture(vec![json!({"id":1,"result":result}), json!({"id":2,"result":{}})]);
            let error = driver.start_login("codex-cli").err().unwrap();
            assert!(driver.terminated());
            assert!(!driver.login_pending());
            assert!(!error.contains("private@example.test"));
            assert_eq!(String::from_utf8(captured.lock().unwrap().clone()).unwrap().contains("account/login/cancel"), has_id);
        }
    }
    #[test]
    fn refused_cancellation_stops_owned_login_process_without_native_error_echo() {
        let (mut driver, _, _sender) = fixture(vec![
            json!({"id":1,"result":{"loginId":"fixture-login","verificationUrl":"https://auth.openai.com/device","userCode":"TEST-CODE"}}),
            json!({"id":2,"error":{"code":-1,"message":"private-native-token"}}),
        ]);
        driver.start_login("codex-cli").unwrap();
        let error = driver.cancel_login().unwrap_err();
        assert!(driver.terminated());
        assert!(!error.contains("private-native-token"));
    }
    #[test]
    fn rejects_nonofficial_urls_and_does_not_reflect_native_errors() {
        for url in ["http://auth.openai.com/device", "https://auth.openai.com.evil.test/device", "https://private@auth.openai.com/device", "https://auth.openai.com:8888/device"] { assert!(device_url(&json!(url)).is_err()); }
        let (mut driver, _, _sender) = fixture(vec![json!({"id":1,"error":{"code":401,"message":"secret-user@example.test bearer synthetic-secret"}})]);
        let error = driver.account("codex-cli", false).err().unwrap();
        assert!(!error.contains("synthetic-secret"));
        assert!(!error.contains("secret-user"));
    }
    #[test]
    fn oversized_partial_and_invalid_frames_are_rejected_before_unbounded_allocation() {
        assert!(read_frame(&mut std::io::Cursor::new(vec![b'x';MAX_FRAME+1])).is_err());
        assert!(read_frame(&mut std::io::Cursor::new(b"{}".to_vec())).is_err());
        assert!(read_frame(&mut std::io::Cursor::new(b"private log\n".to_vec())).is_err());
        assert!(read_frame(&mut std::io::Cursor::new(b"{}\n".to_vec())).unwrap().is_some());
    }
    #[test]
    fn timed_out_transport_is_discarded_and_unknown_quota_is_not_invented() {
        let (mut driver, _, _sender) = fixture(vec![]);
        assert!(driver.account("codex-cli", false).is_err());
        assert!(driver.terminated());
        assert!(quota_window(&json!({"usedPercent":120,"resetsAt":-1,"windowDurationMins":-2}), "primary").unwrap().used_percent.is_none());
    }
}
