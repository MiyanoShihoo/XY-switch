//! Real sockets exercise the same router, access middleware, adapters and ledger
//! used by the desktop proxy. No operating-system credentials are consulted.
use super::*;
use crate::governance::{self, access, budget, monitor, test_support as fixture};
use axum::http::HeaderMap;
use futures_util::StreamExt;
use std::{collections::VecDeque, sync::Mutex};

struct MemoryKey;
impl ApiKeyProvider for MemoryKey {
    fn get(&self, _: &str) -> Result<Option<String>, String> {
        Ok(Some("unit-upstream-key".into()))
    }
}
#[derive(Clone)]
enum Reply {
    Good,
    Error(u16),
    WrongText,
    Hold(Arc<tokio::sync::Semaphore>),
    Redirect(String),
}
#[derive(Clone)]
struct Captured { body: Value, authorization: Option<String> }
#[derive(Clone, Default)]
struct Mock {
    replies: Arc<Mutex<VecDeque<Reply>>>,
    captured: Arc<Mutex<Vec<Captured>>>,
}
fn chunk(content: &str) -> String {
    format!("data: {}\n\n", json!({"id":"gov-stream","object":"chat.completion.chunk","model":"gov-model","choices":[{"index":0,"delta":{"content":content},"finish_reason":null}]}))
}
fn terminal() -> String {
    format!("data: {}\n\ndata: [DONE]\n\n", json!({"id":"gov-stream","model":"gov-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}}))
}
async fn upstream(State(state): State<Mock>, headers: HeaderMap, Json(body): Json<Value>) -> Response {
    state.captured.lock().unwrap().push(Captured {
        body: body.clone(),
        authorization: headers.get(header::AUTHORIZATION).and_then(|v| v.to_str().ok()).map(str::to_owned),
    });
    let reply = state.replies.lock().unwrap().pop_front().unwrap_or(Reply::Good);
    match reply {
        Reply::Error(status) => return (StatusCode::from_u16(status).unwrap(), Json(json!({"error":{"message":"unit-upstream-key must not reach a receipt"}}))).into_response(),
        Reply::Redirect(url) => return (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, url)], "redirect").into_response(),
        _ => {}
    }
    if body["stream"] == true {
        let head = chunk("OK");
        let tail = terminal();
        let stream = futures_util::stream::once(async move { Ok::<_, io::Error>(Bytes::from(head)) })
            .chain(futures_util::stream::once(async move {
                if let Reply::Hold(gate) = reply { let _permit = gate.acquire().await.unwrap(); }
                Ok::<_, io::Error>(Bytes::from(tail))
            }));
        return Response::builder().header(header::CONTENT_TYPE, "text/event-stream").body(Body::from_stream(stream)).unwrap();
    }
    let tool = body.get("tools").is_some();
    let message = if tool { json!({"role":"assistant","content":null,"tool_calls":[{"id":"gov-call","type":"function","function":{"name":"xy_health_echo","arguments":"{\"value\":\"OK\"}"}}]}) }
        else { json!({"role":"assistant","content":if matches!(reply, Reply::WrongText) { "NOT OK" } else { "OK" }}) };
    Json(json!({"id":"gov-completion","object":"chat.completion","created":0,"model":body["model"],
        "choices":[{"index":0,"message":message,"finish_reason":if tool {"tool_calls"} else {"stop"}}],
        "usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}})).into_response()
}
struct Fixture {
    db: Database,
    base: String,
    upstream_base: String,
    token: String,
    mock: Mock,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Fixture { fn drop(&mut self) { for task in &self.tasks { task.abort(); } } }
impl Fixture {
    async fn new(replies: Vec<Reply>, access_policy: access::AccessPolicy) -> Self {
        let mock = Mock::default();
        mock.replies.lock().unwrap().extend(replies);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let upstream_base = format!("http://{}", listener.local_addr().unwrap());
        let router = Router::new().route("/v1/chat/completions", post(upstream)).with_state(mock.clone());
        let upstream_task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
        let db = fixture::database();
        db.save_provider(&fixture::provider(&upstream_base)).unwrap();
        db.set_current_provider("gov-test").unwrap();
        fixture::budget(&db);
        governance::save(&db, access::KEY, &access_policy).unwrap();
        let token = access::issue(&db, "Proxy test", "proxy", 1).unwrap().token;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let state = ProxyState { database: db.clone(), client: client(), api_keys: Arc::new(MemoryKey), local_addr: Some(address) };
        let router = router_with_targets(state, &[]);
        let proxy_task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap(); });
        Self { db, base: format!("http://{address}"), upstream_base, token, mock, tasks: vec![upstream_task, proxy_task] }
    }
    fn post(&self) -> reqwest::RequestBuilder { client().post(format!("{}/v1/chat/completions", self.base)).bearer_auth(&self.token).json(&body()) }
    fn policy(&self, mode: &str) -> monitor::MonitorPolicy {
        monitor::MonitorPolicy { provider_id:"gov-test".into(), model:"gov-model".into(), mode:mode.into(), ..Default::default() }
    }
    async fn check(&self, mode: &str) -> monitor::Run {
        monitor::execute_at(&self.db, &self.base, &self.token, &self.policy(mode), false).await.unwrap()
    }
    fn calls(&self) -> usize { self.mock.captured.lock().unwrap().len() }
}
fn strict() -> access::AccessPolicy { access::AccessPolicy { require_token: true, ..Default::default() } }
fn body() -> Value { json!({"model":"gov-model","messages":[{"role":"user","content":"OK"}],"max_tokens":16}) }
fn client() -> reqwest::Client { crate::network::client_builder().no_proxy().redirect(reqwest::redirect::Policy::none()).timeout(Duration::from_secs(5)).build().unwrap() }

#[tokio::test]
async fn governance_six_modes_complete_through_full_socket_proxy_and_adapters() {
    let f = Fixture::new(vec![], strict()).await;
    for mode in ["chat", "stream", "tools", "responses", "anthropic", "gemini"] {
        let run = f.check(mode).await;
        assert_eq!(run.status, "passed", "{mode}: {}", run.detail);
        assert_eq!(run.evidence.http_status, Some(200));
        assert_eq!(run.evidence.input_tokens, Some(4), "{mode}");
        assert_eq!(run.evidence.output_tokens, Some(1), "{mode}");
        let receipt = serde_json::to_string(&run).unwrap();
        assert!(!receipt.contains("unit-upstream-key"));
        assert!(!receipt.contains("Reply exactly"));
        assert!(!receipt.contains(&f.token));
    }
    assert_eq!(f.calls(), 6);
    for request in f.mock.captured.lock().unwrap().iter() {
        assert_eq!(request.authorization.as_deref(), Some("Bearer unit-upstream-key"));
        assert_eq!(request.body["model"], "gov-model");
        assert!(request.body["max_completion_tokens"].as_i64().unwrap() <= 48);
        assert_eq!(request.body["n"], 1);
    }
    let used = budget::status(&f.db).unwrap();
    assert_eq!((used.attempts, used.reserved_tokens, used.reserved_microusd), (6, 6272, 6400));
    assert_eq!(monitor::runs(&f.db).unwrap().len(), 6);
}

#[tokio::test]
async fn governance_auth_scope_revocation_expiry_ambiguity_host_and_origin_block_before_upstream() {
    let f = Fixture::new(vec![], strict()).await;
    let url = format!("{}/v1/chat/completions", f.base);
    for token in [None, Some("wrong-token"), Some("xy_unknown")] {
        let mut request = client().post(&url).json(&body());
        if let Some(token) = token { request = request.bearer_auth(token); }
        assert_eq!(request.send().await.unwrap().status(), 401);
    }
    let readonly = access::issue(&f.db, "Monitor", "monitor", 1).unwrap();
    assert_eq!(client().post(&url).bearer_auth(&readonly.token).json(&body()).send().await.unwrap().status(), 403);
    assert_eq!(client().get(format!("{}/xy/status",f.base)).bearer_auth(&readonly.token).send().await.unwrap().status(), 200);
    assert_eq!(client().get(format!("{}/xy/status",f.base)).send().await.unwrap().status(), 401);
    assert_eq!(client().get(format!("{}/health",f.base)).send().await.unwrap().status(), 200);
    for column in ["expires_at=0", "revoked=1", "scope='unknown'"] {
        let issued = access::issue(&f.db, "Invalidated", "proxy", 1).unwrap();
        { let c=f.db.conn(); let c=c.lock().unwrap(); c.execute(&format!("UPDATE access_tokens SET {column} WHERE id=?1"), [&issued.info.id]).unwrap(); }
        assert_eq!(client().post(&url).bearer_auth(&issued.token).json(&body()).send().await.unwrap().status(), 401);
    }
    for req in [f.post().header("x-api-key", "different"), f.post().header(header::AUTHORIZATION, format!("Bearer {}",f.token))] {
        assert_eq!(req.send().await.unwrap().status(), 401);
    }
    for req in [f.post().header(header::ORIGIN, "https://attacker.invalid"), f.post().header(header::HOST, "attacker.invalid"), f.post().header("sec-fetch-site", "cross-site")] {
        assert_eq!(req.send().await.unwrap().status(), 403);
    }
    assert_eq!(f.calls(), 0);
    assert_eq!(budget::status(&f.db).unwrap().attempts, 0);
    let c=f.db.conn(); let c=c.lock().unwrap();
    let digest:String = c.query_row("SELECT digest FROM access_tokens WHERE label='Proxy test'", [], |r|r.get(0)).unwrap();
    assert_eq!(digest, governance::digest(f.token.as_bytes()));
    assert!(!digest.contains(&f.token));
}

#[tokio::test]
async fn governance_legacy_compatibility_does_not_elevate_monitor_or_unknown_local_tokens() {
    let f = Fixture::new(vec![], access::AccessPolicy::default()).await;
    let readonly = access::issue(&f.db, "Monitor", "monitor", 1).unwrap();
    for (token,status) in [(&readonly.token[..],403), ("xy_unknown",401)] {
        assert_eq!(client().post(format!("{}/v1/chat/completions",f.base)).bearer_auth(token).json(&body()).send().await.unwrap().status(),status);
    }
    let response = client().post(format!("{}/v1/chat/completions",f.base)).json(&body()).send().await.unwrap();
    assert_eq!(response.status(),200);
    response.bytes().await.unwrap();
    assert_eq!(f.calls(),1);
    f.db.set_setting(access::KEY, "{broken").unwrap();
    assert_eq!(f.post().send().await.unwrap().status(),503);
    assert_eq!(f.calls(),1);
}

#[tokio::test]
async fn governance_all_tokens_share_rate_limit() {
    let f = Fixture::new(vec![], access::AccessPolicy { requests_per_minute:1, ..strict() }).await;
    let second = access::issue(&f.db,"Second","proxy",1).unwrap();
    let url = format!("{}/v1/models",f.base);
    client().get(&url).bearer_auth(&f.token).send().await.unwrap().error_for_status().unwrap().bytes().await.unwrap();
    let response = client().get(&url).bearer_auth(&second.token).send().await.unwrap();
    assert_eq!(response.status(),429);
    assert!(response.text().await.unwrap().contains("local_rate_limited"));
    assert_eq!(f.calls(),0);
}

#[tokio::test]
async fn governance_failed_authentication_counts_against_rate_limit_but_health_is_exempt() {
    let f = Fixture::new(vec![], access::AccessPolicy { requests_per_minute:1, ..strict() }).await;
    let url = format!("{}/v1/chat/completions", f.base);
    let denied = client().post(&url).json(&body()).send().await.unwrap();
    assert_eq!(denied.status(), 401);
    denied.bytes().await.unwrap();
    for token in ["xy_unknown", f.token.as_str()] {
        let response = client().post(&url).bearer_auth(token).json(&body()).send().await.unwrap();
        assert_eq!(response.status(), 429);
        assert!(response.text().await.unwrap().contains("local_rate_limited"));
    }
    assert_eq!(client().get(format!("{}/health", f.base)).send().await.unwrap().status(), 200);
    assert_eq!(f.calls(), 0);
    assert_eq!(budget::status(&f.db).unwrap().attempts, 0);
}

#[tokio::test]
async fn governance_stream_holds_concurrency_slot_until_body_finishes() {
    let gate = Arc::new(tokio::sync::Semaphore::new(0));
    let f = Fixture::new(vec![Reply::Hold(gate.clone())], access::AccessPolicy { max_in_flight:1, ..strict() }).await;
    let mut payload = body(); payload["stream"] = json!(true);
    let mut first = f.post().json(&payload).send().await.unwrap();
    assert_eq!(first.status(),200);
    first.chunk().await.unwrap().unwrap();
    let second = f.post().send().await.unwrap();
    assert_eq!(second.status(),429);
    assert!(second.text().await.unwrap().contains("local_concurrency_limited"));
    assert_eq!(f.calls(),1);
    gate.add_permits(1);
    first.bytes().await.unwrap();
    let response = client().get(format!("{}/v1/models",f.base)).bearer_auth(&f.token).send().await.unwrap();
    assert_eq!(response.status(),200);
}

#[tokio::test]
async fn governance_budget_rejects_without_call_and_reserves_every_retry_and_failover() {
    let f = Fixture::new(vec![Reply::Error(503), Reply::Good], strict()).await;
    let response = f.post().send().await.unwrap();
    assert_eq!(response.status(),200); response.bytes().await.unwrap();
    assert_eq!(f.calls(),2);
    assert_eq!(budget::status(&f.db).unwrap().attempts,2);

    let f = Fixture::new(vec![Reply::Error(503), Reply::Good], strict()).await;
    let mut p = fixture::budget(&f.db); p.daily_tokens=1040;
    governance::save(&f.db,budget::KEY,&p).unwrap();
    let response = f.post().send().await.unwrap();
    assert_eq!(response.status(),402);
    assert!(response.text().await.unwrap().contains("hard_token_budget_exhausted"));
    assert_eq!(f.calls(),1);
    assert_eq!(budget::status(&f.db).unwrap().attempts,1);
    assert_eq!(f.post().send().await.unwrap().status(),402);
    assert_eq!(f.calls(),1);

    let f = Fixture::new(vec![Reply::Error(503),Reply::Error(503),Reply::Error(503),Reply::Good],strict()).await;
    let mut fallback=fixture::provider(&f.upstream_base); fallback.id="fallback".into(); fallback.sort_order=2;
    f.db.save_provider(&fallback).unwrap();
    assert_eq!(f.post().send().await.unwrap().status(),200);
    assert_eq!(f.calls(),4);
    assert_eq!(budget::status(&f.db).unwrap().attempts,4);
    let c=f.db.conn(); let c=c.lock().unwrap();
    assert_eq!(c.query_row("SELECT COUNT(*) FROM budget_ledger WHERE provider_id='fallback'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
}

#[tokio::test]
async fn governance_acceptance_pins_model_ignores_overrides_and_never_retries_or_fails_over() {
    let f = Fixture::new(vec![Reply::Error(503)],strict()).await;
    let mut p=f.db.get_provider("gov-test").unwrap().unwrap();
    p.model="different-default".into();
    p.advanced.body_overrides.insert("max_completion_tokens".into(),json!(10000));
    p.advanced.body_overrides.insert("unknown_billable_feature".into(),json!(true));
    f.db.save_provider(&p).unwrap();
    let mut other=fixture::provider(&f.upstream_base); other.id="other".into();
    f.db.save_provider(&other).unwrap(); f.db.set_current_provider("other").unwrap();
    let run=f.check("chat").await;
    assert_eq!(run.status,"failed"); assert_eq!(run.evidence.http_status,Some(503));
    assert_eq!(f.calls(),1);
    let requests=f.mock.captured.lock().unwrap();
    assert_eq!(requests[0].body["model"],"gov-model");
    assert_eq!(requests[0].body["max_completion_tokens"],16);
    assert!(requests[0].body.get("unknown_billable_feature").is_none());
    assert_eq!(f.db.current_provider().unwrap().unwrap().id,"other");
    assert_eq!(f.db.get_provider("gov-test").unwrap().unwrap().model,"different-default");
    assert_eq!(budget::status(&f.db).unwrap().attempts,1);
    assert!(!serde_json::to_string(&run).unwrap().contains("unit-upstream-key"));
}

#[tokio::test]
async fn governance_schedule_and_shared_daily_cap_cannot_be_bypassed_with_drafts() {
    let f=Fixture::new(vec![],strict()).await;
    let mut p=f.policy("chat"); p.enabled=true; p.max_runs_per_day=2;
    governance::save(&f.db,monitor::KEY,&p).unwrap();
    assert_eq!(monitor::execute_at(&f.db,&f.base,&f.token,&p,true).await.unwrap().status,"passed");
    assert!(monitor::execute_at(&f.db,&f.base,&f.token,&p,true).await.err().unwrap().contains("interval"));
    let mut draft=p.clone(); draft.max_runs_per_day=100;
    assert_eq!(monitor::execute_at(&f.db,&f.base,&f.token,&draft,false).await.unwrap().status,"passed");
    assert!(monitor::execute_at(&f.db,&f.base,&f.token,&draft,false).await.err().unwrap().contains("daily request limit"));
    p.enabled=false; governance::save(&f.db,monitor::KEY,&p).unwrap();
    assert!(monitor::execute_at(&f.db,&f.base,&f.token,&draft,true).await.err().unwrap().contains("policy changed"));
    assert_eq!(f.calls(),2);
}

#[tokio::test]
async fn governance_monitor_alerts_only_on_failure_recovery_transitions_and_excludes_bodies() {
    let f=Fixture::new(vec![Reply::WrongText,Reply::WrongText,Reply::Good,Reply::Good],strict()).await;
    for expected in ["failed","failed","passed","passed"] { assert_eq!(f.check("chat").await.status,expected); }
    let c=f.db.conn(); let c=c.lock().unwrap();
    let kinds:Vec<String>=c.prepare("SELECT kind FROM governance_events WHERE kind LIKE 'monitor_%' ORDER BY id").unwrap().query_map([],|r|r.get(0)).unwrap().map(Result::unwrap).collect();
    assert_eq!(kinds,["monitor_failed","monitor_recovered"]);
    drop(c);
    let status=monitor::read_only_status(&f.db).unwrap().to_string();
    for secret in [&f.token[..],"unit-upstream-key","messages","NOT OK","endpoint"] { assert!(!status.contains(secret),"{secret}"); }
}

#[tokio::test]
async fn governance_upstream_redirects_are_not_followed() {
    let f=Fixture::new(vec![Reply::Redirect("http://127.0.0.1:1/credential-leak".into())],strict()).await;
    let run=f.check("chat").await;
    assert_eq!(run.status,"failed");
    assert_eq!(run.evidence.http_status,Some(307));
    assert_eq!(f.calls(),1);
    assert_eq!(budget::status(&f.db).unwrap().attempts,1);
}
