use crate::{
    credential_store,
    database::{Database, Provider, RequestEvent},
    model_audit::AuditRecord,
    proxy,
    streaming::audit_capture::Collector,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::State;

const MAX_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
static PROBE_SLOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRequest {
    pub provider_id: String,
    pub model: Option<String>,
    pub anomaly_id: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeResult {
    pub request_id: String,
    pub provider_id: String,
    pub protocol: String,
    pub created_at: i64,
    pub http_status: Option<u16>,
    pub latency_ms: i64,
    pub success: bool,
    pub streaming: bool,
    pub received_bytes: usize,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cache_read_tokens: Option<i64>,
    pub cache_creation_tokens: Option<i64>,
    pub total_cost_usd: Option<f64>,
    pub audit: AuditRecord,
    pub error_message: Option<String>,
}

#[tauri::command]
pub async fn probe_provider_response_model(request:ProbeRequest,db:State<'_,Database>)->Result<ProbeResult,String>{
    ensure_active(&db)?;
    let anomaly=request.anomaly_id.as_deref().and_then(|id|id.parse::<i64>().ok()).ok_or("请先选择一条当前路由的被动异常响应")?;
    let provider=db.get_provider(&request.provider_id)?.ok_or("供应商不存在")?;
    let model=request.model.as_deref().unwrap_or(&provider.model);
    let key=zeroize::Zeroizing::new(credential_store::get_api_key(&provider.id)?.ok_or("此供应商尚未保存 API Key")?);
    guarded_probe(&db,&provider,&key,model,anomaly).await
}

pub(crate) async fn automatic_probe(db:Database,evidence:crate::model_verification::VerificationEvidence)->Result<(),String>{
    ensure_active(&db)?;
    let provider=db.get_provider(&evidence.upstream_id)?.ok_or("provider_missing")?;
    let model=evidence.requested_model.clone();
    let key=zeroize::Zeroizing::new(credential_store::get_api_key(&provider.id)?.ok_or("credential_unavailable")?);
    let anomaly=evidence.observation_id.parse().map_err(|_|"anomaly_invalid")?;
    guarded_probe(&db,&provider,&key,&model,anomaly).await.map(|_|())
}

fn ensure_active(db:&Database)->Result<(),String>{
    let settings=db.verification.settings()?;
    if !settings.enabled || !settings.active_probe_enabled {return Err("active_probing_disabled".into());}
    Ok(())
}

async fn guarded_probe(db:&Database,provider:&Provider,key:&str,model:&str,anomaly:i64)->Result<ProbeResult,String>{
    ensure_active(db)?;
    let protocol=proxy::response_model_protocol(provider)?;
    let capture=db.verification.capture(provider,key,protocol,model).ok_or("verification_disabled_or_invalid_route")?;
    let generation=capture.generation;
    if let Err(error)=proxy::check_model_probe_cost(provider,model) {
        db.verification.note_skipped_probe(anomaly,generation,&error);
        return Err(error);
    }
    let _slot=PROBE_SLOTS.try_acquire().map_err(|_|{db.verification.note_skipped_probe(anomaly,generation,"probe_global_busy");"probe_global_busy"})?;
    let reservation=match db.verification.reserve(capture,anomaly,crate::model_verification::now_ms()) {
        Ok(r)=>r,Err(error)=>{db.verification.note_skipped_probe(anomaly,generation,&error);return Err(error);}
    };
    let result=async {
        let current=db.get_provider(&provider.id)?.ok_or("provider_missing")?;
        let current_capture=db.verification.capture(&current,key,proxy::response_model_protocol(&current)?,model).ok_or("route_changed")?;
        if current_capture.context_key!=reservation.capture.context_key {return Err("route_changed".into());}
        let client=reqwest::Client::builder().connect_timeout(Duration::from_secs(reservation.settings.timeout_seconds.min(10)))
            .timeout(Duration::from_secs(reservation.settings.timeout_seconds)).redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .build().map_err(|_|"probe_client_unavailable")?;
        db.verification.mark_running(&reservation)?;
        db.verification.note_probe(reservation.anomaly_id,generation,"running",None);
        perform_probe(db,&client,provider,key,model,Some(reservation.capture.clone()),reservation.settings.max_output_tokens,Some(reservation.cancellation.clone())).await
    }.await;
    let billed=result.as_ref().ok().and_then(|r|crate::model_verification::usage_total(&r.audit.observation));
    let outcome=if result.as_ref().is_ok_and(|r|r.success){"completed"}else if !db.verification.valid(&reservation.capture){"cancelled"}else{"failed_or_unknown"};
    db.verification.settle(&reservation,billed,outcome)?;
    db.verification.note_probe(reservation.anomaly_id,generation,outcome,billed);
    result
}

async fn cancelled(receiver:&mut Option<tokio::sync::watch::Receiver<u64>>){
    match receiver {Some(receiver)=>{let _=receiver.changed().await;},None=>std::future::pending::<()>().await}
}

#[cfg(test)]
async fn run_probe(db:&Database,client:&reqwest::Client,provider:&Provider,key:&str,model:&str)->Result<ProbeResult,String>{
    let capture=db.verification.capture(provider,key,proxy::response_model_protocol(provider)?,model);
    perform_probe(db,client,provider,key,model,capture,8,None).await
}

fn network_message(error: &reqwest::Error) -> String {
    // URLs, custom headers and upstream error text may contain credentials.
    if error.is_timeout() {
        "本次复核已到配置的超时上限，未重试；用量不明时保留预算预留"
    } else if error.is_connect() {
        "本次探测连接失败，请检查 DNS、TLS、系统代理及上游服务"
    } else {
        "本次探测传输失败，已保留收到的模型证据"
    }
    .into()
}

fn http_message(status: u16, body: &[u8]) -> String {
    let value = serde_json::from_slice::<serde_json::Value>(body).ok();
    // Only known constant error codes are surfaced; never reflect arbitrary bodies.
    let code = value
        .as_ref()
        .and_then(|v| v.pointer("/error/code"))
        .and_then(|v| v.as_str());
    let hint = match code {
        Some("model_not_found") => "：上游没有此模型的可用渠道（model_not_found）",
        Some("insufficient_quota") => "：上游额度不足（insufficient_quota）",
        Some("invalid_api_key") => "：上游拒绝此凭据（invalid_api_key）",
        Some("rate_limit_exceeded") => "：上游限流（rate_limit_exceeded）",
        _ => "",
    };
    format!("上游 HTTP {status}{hint}；本次未取得可用的响应模型证据")
}

async fn perform_probe(
    db: &Database,
    client: &reqwest::Client,
    provider: &Provider,
    key: &str,
    model: &str,
    capture: Option<crate::model_verification::Capture>,
    output_limit: u32,
    mut cancellation: Option<tokio::sync::watch::Receiver<u64>>,
) -> Result<ProbeResult, String> {
    let mut prepared = proxy::prepare_bounded_model_probe(client, provider, key, model, output_limit)?;
    prepared.observation.verification_capture=capture;
    prepared.observation.verification_active=true;
    let started = Instant::now();
    let created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "系统时间无效")?
        .as_millis()
        .min(i64::MAX as u128) as i64;
    let mut collector = Collector::new(prepared.format, prepared.observation);
    let mut http_status = None;
    let mut streaming = false;
    let mut received_bytes = 0usize;
    let mut error_message = None;
    let sent=tokio::select! { biased; _=cancelled(&mut cancellation)=>None, result=prepared.request.send()=>Some(result) };
    match sent {
        None => error_message=Some("验证设置已更改，复核已取消；已发送请求仍可能计费".into()),
        Some(Err(error)) => error_message = Some(network_message(&error)),
        Some(Ok(response)) => {
            collector.capture_headers(response.headers());
            let status = response.status();
            http_status = Some(status.as_u16());
            streaming = status.is_success()
                && response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .is_some_and(|value| value.to_ascii_lowercase().contains("text/event-stream"));
            let mut stream = Box::pin(response.bytes_stream());
            let mut buffer = Vec::new();
            let mut clean_eof = true;
            loop {
                let chunk=tokio::select! { biased; _=cancelled(&mut cancellation)=>{
                    error_message=Some("验证设置已更改，复核已取消；已发送请求仍可能计费".into());clean_eof=false;None
                }, chunk=stream.next()=>chunk };
                let Some(chunk)=chunk else {break;};
                match chunk {
                    Err(error) => {
                        error_message = Some(network_message(&error));
                        clean_eof = false;
                        break;
                    }
                    Ok(bytes) => {
                        received_bytes = received_bytes.saturating_add(bytes.len());
                        if received_bytes > MAX_RESPONSE_BYTES {
                            error_message = Some(
                                "本次响应超过 2 MiB 探测上限，已停止读取并保留部分证据".into(),
                            );
                            clean_eof = false;
                            break;
                        }
                        if streaming {
                            collector.push_sse(
                                &mut buffer,
                                &bytes,
                                started.elapsed().as_millis().min(u64::MAX as u128) as u64,
                            );
                        } else {
                            buffer.extend_from_slice(&bytes);
                        }
                    }
                }
            }
            if !status.is_success() {
                error_message = Some(http_message(status.as_u16(), &buffer));
            } else if streaming {
                collector.finish_stream(clean_eof && buffer.is_empty());
            } else if clean_eof {
                match serde_json::from_slice(&buffer) {
                    Ok(body) => collector = collector.capture_non_stream(&body),
                    Err(_) => {
                        error_message =
                            Some("上游响应不是有效的 JSON / SSE，本次无法判断模型".into())
                    }
                }
            }
        }
    }
    let usage = collector.request_usage(&prepared.model);
    let mut audit = usage.audit.clone().ok_or("本次探测证据未生成")?;
    if error_message.is_none() && !audit.observation.complete {
        error_message = Some(
            "本次响应未完整结束（可能达到输出上限或缺少终止帧）；仅保留已收到的模型线索".into(),
        );
    }
    let success = error_message.is_none();
    let latency_ms = started.elapsed().as_millis().min(i64::MAX as u128) as i64;
    let record = db.record_request(&RequestEvent {
        provider_id: provider.id.clone(),
        route: format!("model_probe/{}", prepared.protocol),
        status_code: http_status,
        success,
        affects_health: false,
        latency_ms,
        retry_count: 0,
        error_kind: error_message.clone(),
        model: usage.model.clone(),
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        cost: usage.cost,
        price_source: usage.price_source.clone(),
        price_version: usage.price_version.clone(),
        created_at,
    })?;
    db.update_request_usage(record.event_id, &usage)?;
    if let Some(verified)=db.verification.process(record.event_id,audit.observation.clone(),"active")? {audit=verified;}
    Ok(ProbeResult {
        request_id: record.event_id.to_string(),
        provider_id: provider.id.clone(),
        protocol: prepared.protocol.into(),
        created_at,
        http_status,
        latency_ms,
        success,
        streaming,
        received_bytes,
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_creation_tokens: usage.cache_creation_tokens,
        total_cost_usd: usage.cost,
        audit,
        error_message,
    })
}

#[cfg(test)]
mod tests {
    include!("model_verification/probe_tests.rs");
    use super::*;
    use axum::{
        Json, Router,
        body::Body,
        extract::OriginalUri,
        http::{HeaderMap, StatusCode},
        response::Response,
    };
    use serde_json::{Value, json};
    use std::{
        collections::VecDeque,
        path::Path,
        sync::{Arc, Mutex},
    };

    fn provider(endpoint: &str, format: &str) -> Provider {
        serde_json::from_value(
            json!({"id":"probe-test", "name":"Probe", "providerType":"custom", "endpoint":endpoint,
            "model":"expected", "enabled":true, "advanced":{"upstreamFormat":format}}),
        )
        .unwrap()
    }
    fn database(p: &Provider) -> Database {
        let db = Database::open(Path::new(":memory:")).unwrap();
        db.save_provider(p).unwrap();
        db.verification.set_settings(crate::model_verification::VerificationSettings { enabled:true, ..Default::default() }).unwrap();
        db
    }
    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }
    fn chat(model: Option<&str>) -> String {
        let mut body = json!({"choices":[{"message":{"role":"assistant","content":"OK"},"finish_reason":"stop"}],
            "usage":{"prompt_tokens":4,"completion_tokens":1,"total_tokens":5}});
        if let Some(model) = model {
            body["model"] = json!(model);
        }
        body.to_string()
    }
    async fn server(
        replies: Vec<(u16, &'static str, String)>,
    ) -> (String, Arc<Mutex<Vec<Value>>>, tokio::task::JoinHandle<()>) {
        let replies = Arc::new(Mutex::new(VecDeque::from(replies)));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let captured = seen.clone();
        let app = Router::new().fallback(
            move |uri: OriginalUri, headers: HeaderMap, Json(body): Json<Value>| {
                let replies = replies.clone();
                let captured = captured.clone();
                async move {
                    captured
                        .lock()
                        .unwrap()
                        .push(json!({"path":uri.0.path(),"body":body,
                    "authorization":headers.get("authorization").and_then(|v|v.to_str().ok()),
                    "apiKey":headers.get("x-api-key").and_then(|v|v.to_str().ok()),
                    "version":headers.get("anthropic-version").and_then(|v|v.to_str().ok())}));
                    let (status, kind, body) = replies
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("unexpected retry");
                    Response::builder()
                        .status(StatusCode::from_u16(status).unwrap())
                        .header("content-type", kind)
                        .header("location", "http://127.0.0.1:1/must-not-follow")
                        .body(Body::from(body))
                        .unwrap()
                }
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}"), seen, task)
    }

    #[tokio::test]
    async fn probes_use_native_protocols_and_each_request_has_its_own_evidence() {
        for (format, route, body) in [
            ("openai_responses", "/v1/responses", json!({"model":"expected","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"OK"}]}],"usage":{"input_tokens":4,"output_tokens":1,"total_tokens":5}}).to_string()),
            ("openai_chat", "/v1/chat/completions", chat(Some("expected"))),
            ("anthropic", "/v1/messages", json!({"model":"expected","content":[{"type":"text","text":"OK"}],"stop_reason":"end_turn","usage":{"input_tokens":4,"output_tokens":1}}).to_string()),
        ] {
            let (url, seen, server) = server(vec![(200,"application/json",body)]).await;
            let p = provider(&url, format); let db = database(&p);
            let before_current = db.current_provider().unwrap().map(|p|p.id);
            let before_health = serde_json::to_value(db.list_provider_health().unwrap()).unwrap();
            let result = run_probe(&db, &client(), &p, "probe-secret", "expected").await.unwrap();
            assert!(result.success, "{result:?}");
            assert_eq!(result.audit.model_status, "matched");
            assert_eq!(result.audit.observation.upstream_models, ["expected"]);
            assert_eq!(result.input_tokens, Some(4));
            assert_eq!(result.output_tokens, Some(1));
            assert_eq!(db.current_provider().unwrap().map(|p|p.id), before_current);
            assert_eq!(serde_json::to_value(db.list_provider_health().unwrap()).unwrap(), before_health);
            assert_eq!(serde_json::to_value(db.get_provider(&p.id).unwrap()).unwrap(), serde_json::to_value(Some(p)).unwrap());
            let records = seen.lock().unwrap();
            assert_eq!(records.len(), 1);
            assert_eq!(records[0]["path"], route);
            assert_eq!(records[0]["body"]["stream"], true);
            assert_eq!(records[0]["body"][match format {"openai_responses"=>"max_output_tokens","openai_chat"=>"max_completion_tokens",_=>"max_tokens"}], 8);
            let sent=records[0]["body"].to_string();
            assert!(sent.contains("Reply OK."));
            for forbidden in ["tools","system","instructions","reasoning_effort"] {assert!(records[0]["body"].get(forbidden).is_none());}
            if format == "anthropic" {
                assert_eq!(records[0]["apiKey"], "probe-secret");
                assert_eq!(records[0]["version"], "2023-06-01");
                assert!(records[0]["authorization"].is_null());
            } else { assert_eq!(records[0]["authorization"], "Bearer probe-secret"); }
            assert!(!serde_json::to_string(&result).unwrap().contains("probe-secret"));
            server.abort();
        }
    }

    #[tokio::test]
    async fn same_provider_can_match_mismatch_omit_and_fail_without_reusing_a_verdict() {
        let (url, seen, server) = server(vec![(200,"application/json",chat(Some("expected"))),
            (200,"application/json",chat(Some("other"))), (200,"application/json",chat(None)),
            (503,"application/json",json!({"model":"fake-verified-model","error":{"code":"model_not_found","message":"probe-secret"}}).to_string())]).await;
        let p = provider(&url, "openai_chat");
        let db = database(&p);
        let mut ids = Vec::new();
        for (expected, declared) in [
            ("matched", Some("expected")),
            ("mismatched", Some("other")),
            ("unknown", None),
            ("unknown", None),
        ] {
            let result = run_probe(&db, &client(), &p, "probe-secret", "expected")
                .await
                .unwrap();
            assert_eq!(result.audit.model_status, expected);
            assert_eq!(result.audit.observation.upstream_model.as_deref(), declared);
            assert!(
                !serde_json::to_string(&result)
                    .unwrap()
                    .contains("probe-secret")
            );
            ids.push(result.request_id);
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 4);
        assert_eq!(seen.lock().unwrap().len(), 4);
        let conn = db.conn();
        let conn = conn.lock().unwrap();
        let audits: Vec<String> = conn.prepare("SELECT audit_json FROM request_events WHERE route LIKE 'model_probe/%' ORDER BY id").unwrap()
            .query_map([], |row|row.get(0)).unwrap().collect::<Result<_,_>>().unwrap();
        assert_eq!(audits.len(), 4);
        assert!(audits[1].contains("other"));
        assert!(!audits[2].contains("other"));
        assert!(!audits[3].contains("fake-verified-model"));
        server.abort();
    }

    #[tokio::test]
    async fn streaming_model_changes_are_retained_and_an_incomplete_stream_is_not_success() {
        let complete = "data: {\"model\":\"expected\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"O\"},\"finish_reason\":null}]}\n\ndata: {\"model\":\"other\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"K\"},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";
        let partial = "data: {\"model\":\"third\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\"O\"},\"finish_reason\":null}]}\n\n";
        let (url, _, server) = server(vec![
            (200, "text/event-stream", complete.into()),
            (200, "text/event-stream", partial.into()),
        ])
        .await;
        let p = provider(&url, "openai_chat");
        let db = database(&p);
        let first = run_probe(&db, &client(), &p, "probe-secret", "expected")
            .await
            .unwrap();
        assert!(first.success);
        assert_eq!(first.audit.model_status, "conflicting");
        assert_eq!(
            first.audit.observation.upstream_models,
            ["expected", "other"]
        );
        let second = run_probe(&db, &client(), &p, "probe-secret", "expected")
            .await
            .unwrap();
        assert!(!second.success);
        assert!(!second.audit.observation.complete);
        assert_eq!(second.audit.observation.upstream_models, ["third"]);
        server.abort();
    }

    #[test]
    fn probe_limits_cannot_be_overridden_and_mapping_uses_the_final_dispatched_name() {
        let mut p = provider(
            "https://relay.example/custom/responses?version=1",
            "openai_responses",
        );
        p.advanced.is_full_url = true;
        p.advanced
            .model_mappings
            .push(crate::database::ProviderModelMapping {
                model: "mapped".into(),
                display_name: "alias".into(),
                context_window: None,
            });
        p.advanced.body_overrides=serde_json::from_value(json!({"model":"override","max_output_tokens":100000,"stream":false,"tools":[{"type":"web_search"}],"input":"private history","n":500})).unwrap();
        p.advanced
            .header_overrides
            .insert("Authorization".into(), "Bearer wrong".into());
        let prepared =
            proxy::prepare_response_model_probe(&client(), &p, "probe-secret", "alias").unwrap();
        assert_eq!(
            prepared.observation.requested_model.as_deref(),
            Some("alias")
        );
        assert_eq!(
            prepared.observation.forwarded_model.as_deref(),
            Some("override")
        );
        let request = prepared.request.build().unwrap();
        assert_eq!(request.url().as_str(), p.endpoint);
        assert_eq!(request.headers()["authorization"], "Bearer probe-secret");
        let body: Value =
            serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["max_output_tokens"], 8);
        assert_eq!(body["stream"], true);
        assert_eq!(body["input"], "Reply OK.");
        assert!(body.get("tools").is_none());
        assert!(body.get("n").is_none());
    }

    #[tokio::test]
    async fn invalid_oversized_and_redirected_responses_do_not_invent_models_or_retry() {
        let (url, seen, server) = server(vec![
            (200, "application/json", "not json".into()),
            (200, "application/json", "x".repeat(MAX_RESPONSE_BYTES + 1)),
            (302, "application/json", chat(Some("forged"))),
        ])
        .await;
        let p = provider(&url, "openai_chat");
        let db = database(&p);
        for _ in 0..3 {
            let result = run_probe(&db, &client(), &p, "probe-secret", "expected")
                .await
                .unwrap();
            assert!(!result.success);
            assert_eq!(result.audit.model_status, "unknown");
            assert!(result.audit.observation.upstream_model.is_none());
        }
        assert_eq!(seen.lock().unwrap().len(), 3);
        server.abort();
    }

    #[tokio::test]
    async fn connection_failure_records_no_http_and_no_upstream_identity() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let p = provider(&url, "openai_responses");
        let db = database(&p);
        let result = run_probe(&db, &client(), &p, "probe-secret", "expected")
            .await
            .unwrap();
        assert!(result.http_status.is_none());
        assert!(!result.success);
        assert_eq!(result.audit.model_status, "unknown");
        assert!(result.audit.observation.upstream_models.is_empty());
    }

    #[test]
    fn explicit_probe_model_does_not_fall_back_to_the_saved_default() {
        let mut p = provider("https://relay.example", "openai_chat");
        p.advanced
            .model_mappings
            .push(crate::database::ProviderModelMapping {
                model: "mapped".into(),
                display_name: "alias".into(),
                context_window: None,
            });
        for (input, forwarded) in [("another-model", "another-model"), ("alias", "mapped")] {
            let prepared =
                proxy::prepare_response_model_probe(&client(), &p, "probe-secret", input).unwrap();
            assert_eq!(prepared.observation.requested_model.as_deref(), Some(input));
            assert_eq!(
                prepared.observation.forwarded_model.as_deref(),
                Some(forwarded)
            );
            let request = prepared.request.build().unwrap();
            let body: Value =
                serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
            assert_eq!(body["model"], forwarded);
        }
        assert_eq!(p.model, "expected");
    }

    #[tokio::test]
    async fn error_objects_are_not_model_evidence_and_do_not_erase_prior_valid_frames() {
        let error =
            json!({"model":"forged-error-model", "error":{"code":"test_error"}}).to_string();
        let stream = format!(
            "data: {{\"model\":\"expected\",\"choices\":[{{\"index\":0,\"delta\":{{\"content\":\"O\"}},\"finish_reason\":null}}]}}\n\ndata: {error}\n\n"
        );
        let (url, _, server) = server(vec![
            (200, "application/json", error),
            (200, "text/event-stream", stream),
        ])
        .await;
        let p = provider(&url, "openai_chat");
        let db = database(&p);
        let first = run_probe(&db, &client(), &p, "probe-secret", "expected")
            .await
            .unwrap();
        assert!(!first.success);
        assert!(first.audit.observation.upstream_models.is_empty());
        let second = run_probe(&db, &client(), &p, "probe-secret", "expected")
            .await
            .unwrap();
        assert!(!second.success);
        assert_eq!(second.audit.observation.upstream_models, ["expected"]);
        assert!(
            !serde_json::to_string(&second)
                .unwrap()
                .contains("forged-error-model")
        );
        server.abort();
    }
}
