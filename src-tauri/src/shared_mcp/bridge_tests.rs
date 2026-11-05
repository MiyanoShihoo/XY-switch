use super::*;
use std::{pin::Pin, sync::atomic::AtomicUsize, task::{Context, Poll}};
use tokio::{io::{AsyncBufRead, AsyncRead, AsyncWriteExt, BufReader, DuplexStream, ReadBuf}, sync::Notify};

struct TrackedRead { inner: BufReader<DuplexStream>, consumed: Arc<AtomicUsize>, changed: Arc<Notify> }
impl AsyncRead for TrackedRead {
    fn poll_read(self: Pin<&mut Self>, context: &mut Context<'_>, buffer: &mut ReadBuf<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(context, buffer)
    }
}
impl AsyncBufRead for TrackedRead {
    fn poll_fill_buf(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<std::io::Result<&[u8]>> { Pin::new(&mut self.get_mut().inner).poll_fill_buf(context) }
    fn consume(self: Pin<&mut Self>, count: usize) {
        let reader = self.get_mut();
        Pin::new(&mut reader.inner).consume(count);
        reader.consumed.fetch_add(count, Ordering::SeqCst);
        reader.changed.notify_one();
    }
}

async fn write_rpc(writer: &mut DuplexStream, rpc: &Value) {
    writer.write_all(format!("{rpc}\n").as_bytes()).await.unwrap();
}
async fn read_rpc(reader: &mut BufReader<DuplexStream>) -> Value {
    use tokio::io::AsyncBufReadExt;
    let mut line = String::new();
    tokio::time::timeout(Duration::from_secs(3), reader.read_line(&mut line)).await.unwrap().unwrap();
    serde_json::from_str(line.trim()).expect("one JSON-RPC response")
}

#[tokio::test]
async fn a_reply_between_input_fragments_preserves_the_consumed_prefix() {
    let server = Server::new().await;
    server.fixture.install_echo(&["claude-desktop", "claude-desktop-3p"]);
    let (connection, token) = server.fixture.connection("claude-desktop");
    let (mut input, input_reader) = tokio::io::duplex(8192);
    let (output, output_reader) = tokio::io::duplex(8192);
    let consumed = Arc::new(AtomicUsize::new(0));
    let changed = Arc::new(Notify::new());
    let reader = TrackedRead { inner: BufReader::new(input_reader), consumed: consumed.clone(), changed: changed.clone() };
    let endpoint = reqwest::Url::parse(&server.endpoint(&connection)).unwrap();
    let bridge = tokio::spawn(super::super::bridge::run_test_io(reader, output, endpoint, token));
    let first = modern(1, "tools/call", json!({"name":"test.shared-echo.echo","arguments":{"message":"safe"}}));
    write_rpc(&mut input, &first).await;
    let review = server.fixture.pending().await;
    let second = format!("{}\n", modern(2, "ping", json!({})));
    let midpoint = second.len() / 2;
    input.write_all(&second.as_bytes()[..midpoint]).await.unwrap();
    let expected = first.to_string().len() + 1 + midpoint;
    tokio::time::timeout(Duration::from_secs(3), async {
        loop { let wake = changed.notified(); if consumed.load(Ordering::SeqCst) >= expected { break; } wake.await; }
    }).await.expect("partial input was consumed before the reply");
    server.fixture.manager().decide(&review.id, true, true).unwrap();
    let mut output = BufReader::new(output_reader);
    assert_eq!(read_rpc(&mut output).await["id"], 1);
    input.write_all(&second.as_bytes()[midpoint..]).await.unwrap();
    let result = read_rpc(&mut output).await;
    assert_eq!(result["id"], 2);
    assert_eq!(result["result"]["resultType"], "complete");
    input.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), bridge).await.unwrap().unwrap().unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn stdio_cancellation_drops_the_http_body_and_allows_request_id_reuse() {
    let server = Server::new().await;
    server.fixture.install_echo(&["claude-desktop", "claude-desktop-3p"]);
    let (connection, token) = server.fixture.connection("claude-desktop");
    let (mut input, input_reader) = tokio::io::duplex(8192);
    let (output, output_reader) = tokio::io::duplex(8192);
    let endpoint = reqwest::Url::parse(&server.endpoint(&connection)).unwrap();
    let bridge = tokio::spawn(super::super::bridge::run_test_io(BufReader::new(input_reader), output, endpoint, token));
    write_rpc(&mut input, &modern(20, "tools/call", json!({"name":"test.shared-echo.echo","arguments":{"message":"safe"}}))).await;
    let review = server.fixture.pending().await;
    write_rpc(&mut input, &json!({"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":20}})).await;
    server.fixture.cleared().await;
    assert_eq!(server.fixture.manager().decide(&review.id, true, true).unwrap_err(), "shared_mcp_expired");
    write_rpc(&mut input, &modern(20, "ping", json!({}))).await;
    let result = read_rpc(&mut BufReader::new(output_reader)).await;
    assert_eq!(result["id"], 20);
    assert_eq!(result["result"], json!({"resultType":"complete"}));
    input.shutdown().await.unwrap();
    bridge.await.unwrap().unwrap();
    server.shutdown().await;
}

#[tokio::test]
async fn stdio_eof_cancels_work_and_deletes_its_legacy_session() {
    let server = Server::new().await;
    server.fixture.install_echo(&["claude-desktop", "claude-desktop-3p"]);
    let (connection, token) = server.fixture.connection("claude-desktop");
    let (mut input, input_reader) = tokio::io::duplex(8192);
    let (output, output_reader) = tokio::io::duplex(8192);
    let endpoint = reqwest::Url::parse(&server.endpoint(&connection)).unwrap();
    let bridge = tokio::spawn(super::super::bridge::run_test_io(BufReader::new(input_reader), output, endpoint, token));
    let mut output = BufReader::new(output_reader);
    write_rpc(&mut input, &legacy(1, "initialize", json!({"protocolVersion":"2025-11-25","capabilities":{},"clientInfo":{"name":"Fixture","version":"1.0"}}))).await;
    assert_eq!(read_rpc(&mut output).await["result"]["protocolVersion"], "2025-11-25");
    write_rpc(&mut input, &json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}})).await;
    write_rpc(&mut input, &legacy(2, "tools/call", json!({"name":"test.shared-echo.echo","arguments":{"message":"safe"}}))).await;
    let review = server.fixture.pending().await;
    input.shutdown().await.unwrap();
    tokio::time::timeout(Duration::from_secs(3), bridge).await.unwrap().unwrap().unwrap();
    server.fixture.cleared().await;
    assert!(server.fixture.manager().state.lock().unwrap().sessions.is_empty());
    assert_eq!(server.fixture.manager().decide(&review.id, true, true).unwrap_err(), "shared_mcp_expired");
    server.shutdown().await;
}

#[test]
fn header_name_encoding_round_trips_unicode_whitespace_and_reserved_prefixes() {
    for name in ["plain.name", "工具/名称", " leading", "trailing ", "=?base64?literal"] {
        let encoded = http::encode_name(name).unwrap();
        assert_eq!(http::decode_name(&encoded).unwrap(), name);
        if name != "plain.name" { assert!(encoded.starts_with("=?base64?")); }
    }
    for malformed in ["=?base64?abc?=", "=?base64?AAAA====?=", "bad\nname", "=?base64?YQ=="] {
        assert!(http::decode_name(malformed).is_err());
    }
}

#[test]
fn bridge_credentials_accept_only_bounded_loopback_endpoints_and_hex_tokens() {
    let id = uuid::Uuid::new_v4().to_string();
    let valid = json!({"endpoint":format!("http://127.0.0.1:15722/mcp/{id}"),"token":"a".repeat(64)});
    assert!(super::super::manager::bridge_credential(&valid.to_string()).is_ok());
    for endpoint in [format!("https://127.0.0.1:15722/mcp/{id}"),format!("http://example.test:15722/mcp/{id}"),
        format!("http://user@127.0.0.1:15722/mcp/{id}"),format!("http://127.0.0.1:15722/mcp/{id}?token=value")] {
        let invalid = json!({"endpoint":endpoint,"token":"a".repeat(64)});
        assert_eq!(super::super::manager::bridge_credential(&invalid.to_string()).unwrap_err(), "shared_mcp_credentials");
    }
}
