use super::*;
use tokio::{io::{AsyncReadExt, AsyncWriteExt}, net::{TcpListener, TcpStream}, task::JoinHandle};

fn options(address: &str) -> TransportOptions {
    TransportOptions { origin: address.into(), allow_development_http: true, timeout_ms: 5_000 }
}

fn get(path: &str) -> TransportRequest {
    TransportRequest { method: "GET".into(), path: path.into(), body: None, bearer: None }
}

fn response(status: &str, headers: &str, body: &[u8]) -> Vec<u8> {
    let mut bytes = format!("HTTP/1.1 {status}\r\nConnection: close\r\nContent-Length: {}\r\n{headers}\r\n", body.len()).into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

async fn read_request(stream: &mut TcpStream) -> Vec<u8> {
    let mut received = Vec::new();
    loop {
        let mut buffer = [0_u8; 4096];
        let count = stream.read(&mut buffer).await.unwrap();
        assert!(count > 0, "request ended before its body");
        received.extend_from_slice(&buffer[..count]);
        assert!(received.len() <= 32 * 1024, "fixture request exceeded its bound");
        if let Some(position) = received.windows(4).position(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&received[..position]);
            let length = headers.lines().filter_map(|line| line.split_once(':'))
                .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
                .map(|(_, value)| value.trim().parse::<usize>().unwrap()).unwrap_or(0);
            if received.len() >= position + 4 + length { return received; }
        }
    }
}

async fn server(replies: Vec<Vec<u8>>) -> (String, JoinHandle<Vec<Vec<u8>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        let mut requests = Vec::new();
        for reply in replies {
            let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
            requests.push(tokio::time::timeout(Duration::from_secs(5), read_request(&mut stream)).await.unwrap());
            let _ = stream.write_all(&reply).await;
            let _ = stream.shutdown().await;
        }
        requests
    });
    (address, task)
}

#[test]
fn origins_require_https_or_explicit_exact_development_loopback() {
    for address in ["https://market.example.test", "https://market.example.test/", "https://market.example.test:8443"] {
        assert!(origin(address, false).is_ok());
    }
    for host in ["localhost", "127.0.0.1", "[::1]"] {
        for suffix in ["", ":47831", ":47831/"] {
            let address = format!("http://{host}{suffix}");
            assert!(origin(&address, true).is_ok(), "{address}");
            assert_eq!(origin(&address, false).unwrap_err(), "invalid_origin");
        }
    }
    for address in ["http://market.example.test", "http://127.1", "http://2130706433", "http://0x7f000001",
        "http://127.0.0.2", "http://localhost.example.test", "http://LOCALHOST:47831", "http://[::ffff:127.0.0.1]",
        "https://user:PRIVATE_PASSWORD@market.example.test", "https://market.example.test/v1/", "https://market.example.test//",
        "https://market.example.test?token=PRIVATE_TOKEN", "https://market.example.test#fragment", "https://market.example.test\\evil",
        "https://market.example.test:0", "http://127.0.0.1:0", "https://market.example.test:65536", " https://market.example.test",
        "https://market.example.test\n", "file:///v1/catalog"] {
        assert_eq!(origin(address, true).unwrap_err(), "invalid_origin", "{address}");
    }
}

#[test]
fn registration_is_bounded_single_use_and_released_on_exit() {
    let transport = MarketplaceTransport::new().unwrap();
    for timeout_ms in [0, 99, 60_001, u64::MAX] {
        let mut invalid = options("https://market.example.test");
        invalid.timeout_ms = timeout_ms;
        assert_eq!(transport.begin(invalid).unwrap_err(), "invalid_request");
    }
    let ids: Vec<_> = (0..MAX_REQUESTS).map(|_| transport.begin(options("https://market.example.test")).unwrap()).collect();
    assert_eq!(transport.begin(options("https://market.example.test")).unwrap_err(), "rate_limited");
    let lease = transport.claim(&ids[0]).unwrap();
    assert!(matches!(transport.claim(&ids[0]), Err(error) if error == "invalid_request"));
    let cancellation = lease.pending.cancellation.clone();
    drop(lease);
    assert!(cancellation.is_cancelled());
    assert!(matches!(transport.claim(&ids[0]), Err(error) if error == "session_changed"));
    let replacement = transport.begin(options("https://market.example.test")).unwrap();
    assert!(!ids.contains(&replacement));
    transport.cancel_all();
    assert!(transport.requests.lock().unwrap().is_empty());
    assert!(matches!(transport.claim(&replacement), Err(error) if error == "session_changed"));
    assert!(matches!(transport.claim("PRIVATE_INVALID_ID"), Err(error) if error == "invalid_request"));
}

#[tokio::test]
async fn expired_registrations_are_reclaimed_before_the_limit_is_checked() {
    let transport = MarketplaceTransport::new().unwrap();
    let mut short = options("https://market.example.test");
    short.timeout_ms = 100;
    let expired = transport.begin(short).unwrap();
    for _ in 1..MAX_REQUESTS { transport.begin(options("https://market.example.test")).unwrap(); }
    tokio::time::sleep(Duration::from_millis(120)).await;
    transport.begin(options("https://market.example.test")).unwrap();
    assert!(matches!(transport.claim(&expired), Err(error) if error == "session_changed"));
    assert_eq!(transport.requests.lock().unwrap().len(), MAX_REQUESTS);
}

#[tokio::test]
async fn native_json_transport_sends_only_selected_headers_and_never_reuses_cookies() {
    let (address, task) = server(vec![
        response("200 OK", "Content-Type: application/json\r\nSet-Cookie: PRIVATE_COOKIE=secret; Path=/\r\n", br#"{"ok":true}"#),
        response("204 No Content", "", b""),
    ]).await;
    let transport = MarketplaceTransport::new().unwrap();
    let id = transport.begin(options(&address)).unwrap();
    let result = transport.request(&id, get("/v1/catalog?limit=20")).await.unwrap();
    assert_eq!(result.status, 200);
    assert_eq!(serde_json::from_str::<serde_json::Value>(&result.body).unwrap(), serde_json::json!({"ok":true}));
    assert_eq!(transport.request(&id, get("/v1/catalog")).await.unwrap_err(), "session_changed");
    let id = transport.begin(options(&address)).unwrap();
    let request = TransportRequest { method: "POST".into(), path: "/v1/auth/logout".into(),
        body: Some("{}".into()), bearer: Some("synthetic_token-123".into()) };
    let result = transport.request(&id, request).await.unwrap();
    assert_eq!((result.status, result.content_type, result.body), (204, None, String::new()));
    let captured = task.await.unwrap();
    let first = String::from_utf8(captured[0].clone()).unwrap().to_ascii_lowercase();
    let second = String::from_utf8(captured[1].clone()).unwrap().to_ascii_lowercase();
    assert!(first.starts_with("get /v1/catalog?limit=20 http/1.1\r\n"));
    assert!(!first.contains("authorization:"));
    assert!(second.starts_with("post /v1/auth/logout http/1.1\r\n"));
    assert!(second.contains("authorization: bearer synthetic_token-123\r\n"));
    assert!(second.contains("content-type: application/json\r\n"));
    assert!(second.ends_with("\r\n\r\n{}"));
    for headers in [first, second] {
        assert!(headers.contains("accept: application/json\r\n"));
        assert!(headers.contains("cache-control: no-store\r\n"));
        for forbidden in ["cookie:", "referer:", "origin:", "private_cookie"] { assert!(!headers.contains(forbidden)); }
    }
    assert!(transport.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn invalid_requests_cannot_open_a_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let transport = MarketplaceTransport::new().unwrap();
    let mut invalid = Vec::new();
    for path in ["/health", "//other.example.test/v1/catalog", "/v1/../../private", "/v1/%2e%2e/private",
        "/v1/catalog#PRIVATE_FRAGMENT", "/v1/\\other", "/v1/catalog\n", "/v1/\u{4e2d}"] {
        invalid.push(get(path));
    }
    for (method, body) in [("DELETE", None), ("GET", Some("{}")), ("POST", None), ("POST", Some("PRIVATE_INVALID_JSON"))] {
        invalid.push(TransportRequest { method: method.into(), path: "/v1/submissions".into(), body: body.map(str::to_owned), bearer: None });
    }
    invalid.push(TransportRequest { body: Some(format!("\"{}\"", "x".repeat(MAX_ENVELOPE_BYTES))), method: "POST".into(), ..get("/v1/submissions") });
    for bearer in [String::new(), "PRIVATE\r\nInjected: secret".into(), "x".repeat(4097)] {
        invalid.push(TransportRequest { bearer: Some(bearer), ..get("/v1/submissions") });
    }
    for request in invalid {
        let id = transport.begin(options(&address)).unwrap();
        assert_eq!(transport.request(&id, request).await.unwrap_err(), "invalid_request");
    }
    assert!(tokio::time::timeout(Duration::from_millis(100), listener.accept()).await.is_err());
    assert!(transport.requests.lock().unwrap().is_empty());
}

#[tokio::test]
async fn redirects_never_forward_bearer_or_login_body_to_another_server() {
    let destination = TcpListener::bind("127.0.0.1:0").await.unwrap();
    for request in [TransportRequest { bearer: Some("PRIVATE_BEARER".into()), ..get("/v1/submissions") },
        TransportRequest { method: "POST".into(), body: Some(r#"{"password":"PRIVATE_PASSWORD"}"#.into()), ..get("/v1/auth/login") }] {
        let headers = format!("Location: http://{}/v1/collect\r\nContent-Type: application/json\r\n", destination.local_addr().unwrap());
        let (address, task) = server(vec![response("307 Temporary Redirect", &headers, b"{}")]).await;
        let transport = MarketplaceTransport::new().unwrap();
        let id = transport.begin(options(&address)).unwrap();
        assert_eq!(transport.request(&id, request).await.unwrap_err(), "invalid_response");
        assert_eq!(task.await.unwrap().len(), 1);
    }
    assert!(tokio::time::timeout(Duration::from_millis(100), destination.accept()).await.is_err());
}

#[tokio::test]
async fn untrusted_response_errors_never_echo_private_server_details() {
    let replies = vec![
        response("200 OK", "Content-Type: text/html\r\n", b"PRIVATE_SERVER_HTML"),
        response("200 OK", "", b"{}"),
        response("200 OK", "Content-Type: application/json\r\n", b"PRIVATE_INVALID_JSON"),
        response("200 OK", "Content-Type: application/json\r\n", &[b'"', 0xff, b'"']),
        response("204 No Content", "", b"PRIVATE_BODY"),
        response("304 Not Modified", "Content-Type: application/json\r\n", b"{}"),
        format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", MAX_ENVELOPE_BYTES + 1).into_bytes(),
    ];
    for reply in replies {
        let (address, task) = server(vec![reply]).await;
        let transport = MarketplaceTransport::new().unwrap();
        let id = transport.begin(options(&address)).unwrap();
        assert_eq!(transport.request(&id, get("/v1/catalog")).await.unwrap_err(), "invalid_response");
        task.await.unwrap();
    }
}

#[tokio::test]
async fn response_limit_checks_actual_chunked_bytes_and_accepts_the_exact_boundary() {
    let body = format!("\"{}\"", "x".repeat(MAX_ENVELOPE_BYTES - 2));
    let (address, task) = server(vec![response("200 OK", "Content-Type: Application/JSON; charset=utf-8\r\n", body.as_bytes())]).await;
    let transport = MarketplaceTransport::new().unwrap();
    let id = transport.begin(options(&address)).unwrap();
    assert_eq!(transport.request(&id, get("/v1/catalog")).await.unwrap().body.len(), MAX_ENVELOPE_BYTES);
    task.await.unwrap();
    let oversized = format!("\"{}\"", "x".repeat(MAX_ENVELOPE_BYTES));
    let reply = format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n{:x}\r\n{oversized}\r\n0\r\n\r\n", oversized.len()).into_bytes();
    let (address, task) = server(vec![reply]).await;
    let id = transport.begin(options(&address)).unwrap();
    assert_eq!(transport.request(&id, get("/v1/catalog")).await.unwrap_err(), "invalid_response");
    task.await.unwrap();
}

#[tokio::test]
async fn json_error_status_is_preserved_for_the_service_decoder() {
    let body = br#"{"error":{"code":"authentication_required","message":"PRIVATE_SERVER_DETAIL"}}"#;
    let (address, task) = server(vec![response("401 Unauthorized", "Content-Type: application/json\r\n", body)]).await;
    let transport = MarketplaceTransport::new().unwrap();
    let id = transport.begin(options(&address)).unwrap();
    assert_eq!(transport.request(&id, get("/v1/submissions")).await.unwrap().status, 401);
    task.await.unwrap();
}

#[tokio::test]
async fn cancellation_before_registration_claim_or_first_poll_prevents_dispatch() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let transport = MarketplaceTransport::new().unwrap();
    let id = transport.begin(options(&address)).unwrap();
    transport.cancel(&id).unwrap();
    transport.cancel(&id).unwrap();
    assert_eq!(transport.request(&id, get("/v1/catalog")).await.unwrap_err(), "session_changed");
    let id = transport.begin(options(&address)).unwrap();
    let pending = Box::pin(transport.request(&id, get("/v1/catalog")));
    transport.cancel(&id).unwrap();
    assert_eq!(pending.await.unwrap_err(), "session_changed");
    assert!(tokio::time::timeout(Duration::from_millis(100), listener.accept()).await.is_err());
}

#[tokio::test]
async fn cancellation_and_exit_stop_an_in_flight_request_and_release_the_registration() {
    for exiting in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        let transport = MarketplaceTransport::new().unwrap();
        let id = transport.begin(options(&address)).unwrap();
        let active_transport = transport.clone();
        let active_id = id.clone();
        let task = tokio::spawn(async move { active_transport.request(&active_id, get("/v1/catalog")).await });
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(5), read_request(&mut stream)).await.unwrap();
        if exiting { transport.cancel_all(); } else { transport.cancel(&id).unwrap(); }
        assert_eq!(tokio::time::timeout(Duration::from_secs(1), task).await.unwrap().unwrap().unwrap_err(), "session_changed");
        assert!(transport.requests.lock().unwrap().is_empty());
        assert!(matches!(transport.claim(&id), Err(error) if error == "session_changed"));
    }
}

#[tokio::test]
async fn native_deadline_cancels_a_stalled_server_and_network_errors_are_stable() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = format!("http://{}", listener.local_addr().unwrap());
    let transport = MarketplaceTransport::new().unwrap();
    let mut short = options(&address);
    short.timeout_ms = 100;
    let id = transport.begin(short).unwrap();
    let active_transport = transport.clone();
    let task = tokio::spawn(async move { active_transport.request(&id, get("/v1/catalog")).await });
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept()).await.unwrap().unwrap();
    tokio::time::timeout(Duration::from_secs(5), read_request(&mut stream)).await.unwrap();
    assert_eq!(tokio::time::timeout(Duration::from_secs(1), task).await.unwrap().unwrap().unwrap_err(), "network");
    assert!(transport.requests.lock().unwrap().is_empty());
    drop(stream);
    drop(listener);
    let id = transport.begin(options(&address)).unwrap();
    assert_eq!(transport.request(&id, get("/v1/catalog")).await.unwrap_err(), "network");
}
