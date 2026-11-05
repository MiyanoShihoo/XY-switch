use super::*;
use reqwest::StatusCode;

#[tokio::test]
async fn discovery_validates_each_request_metadata_without_creating_sessions() {
    let server = Server::new().await;
    let (connection, token) = server.fixture.connection("claude-desktop");
    let rpc = modern(1, "server/discover", json!({}));
    let response = server.post(&connection, &token, &rpc).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key("mcp-session-id"));
    let result: Value = response.json().await.unwrap();
    assert_eq!(result["result"]["resultType"], "complete");
    assert_eq!(result["result"]["supportedVersions"], json!(SUPPORTED_VERSIONS));
    assert!(server.fixture.manager().state.lock().unwrap().sessions.is_empty());
    for field in [http::VERSION_META, http::CAPABILITIES_META] {
        let mut invalid = rpc.clone();
        invalid["params"]["_meta"].as_object_mut().unwrap().remove(field);
        let mut request = server.post(&connection, &token, &invalid);
        if field == http::VERSION_META { request = request.header("mcp-protocol-version", MODERN_VERSION).header("mcp-method", "server/discover"); }
        let response = request.send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let result: Value = response.json().await.unwrap();
        assert_eq!(result["error"]["code"], -32602);
    }
    let mut invalid = rpc.clone();
    invalid["params"]["_meta"]["io.modelcontextprotocol/clientInfo"] = json!({"name":"Fixture"});
    let result: Value = server.post(&connection, &token, &invalid).send().await.unwrap().json().await.unwrap();
    assert_eq!(result["error"]["code"], -32602);
    server.shutdown().await;
}

#[tokio::test]
async fn mirrored_headers_versions_and_methods_use_their_protocol_errors() {
    let server = Server::new().await;
    let (connection, token) = server.fixture.connection("claude-desktop");
    let rpc = modern(2, "ping", json!({}));
    for (name, value) in [("mcp-method", "tools/list"), ("mcp-protocol-version", "2025-11-25")] {
        let response = server.post(&connection, &token, &rpc).header(name, value).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let result: Value = response.json().await.unwrap();
        assert_eq!(result["error"]["code"], -32020);
    }
    let mut unsupported = rpc.clone();
    unsupported["params"]["_meta"][http::VERSION_META] = json!("2099-01-01");
    let result: Value = server.post(&connection, &token, &unsupported).send().await.unwrap().json().await.unwrap();
    assert_eq!(result["error"]["code"], -32022);
    assert_eq!(result["error"]["data"]["supported"], json!(SUPPORTED_VERSIONS));
    let unknown = modern(3, "unimplemented/method", json!({}));
    let response = server.post(&connection, &token, &unknown).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"], -32601);
    let named = modern(4, "tools/call", json!({"name":"test.shared-echo.echo","arguments":{"message":"safe"}}));
    let response = server.post(&connection, &token, &named).header("mcp-name", "wrong.name").send().await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert_eq!(response.json::<Value>().await.unwrap()["error"]["code"], -32020);
    server.shutdown().await;
}

#[tokio::test]
async fn invalid_json_duplicate_fields_and_private_ids_are_not_echoed() {
    let server = Server::new().await;
    let (connection, token) = server.fixture.connection("claude-desktop");
    let private_id = legacy(1, "ping", json!({})).to_string().replace("\"id\":1", &format!("\"id\":\"{token}\""));
    let personal_id = r#"{"jsonrpc":"2.0","id":"person@example.test","method":"ping"}"#;
    for (text, expected) in [("{".to_owned(), -32700),
        (r#"{"jsonrpc":"2.0","id":1,"id":2,"method":"ping"}"#.into(), -32600), (private_id, -32600), (personal_id.into(), -32600)] {
        let response = server.client.post(server.endpoint(&connection)).bearer_auth(&token).header("accept", "application/json, text/event-stream")
            .header("content-type", "application/json").body(text).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let result: Value = response.json().await.unwrap();
        assert_eq!(result["error"]["code"], expected);
        assert_eq!(result["id"], Value::Null);
        assert!(!result.to_string().contains(&token));
        assert!(!result.to_string().contains("person@example.test"));
    }
    let response = server.client.post(server.endpoint(&connection)).bearer_auth(&token).header("content-type", "application/json")
        .header("accept", "application/json, text/event-stream").body("x".repeat(MAX_MESSAGE_BYTES + 1)).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    server.shutdown().await;
}

#[tokio::test]
async fn connections_origins_and_hosts_are_isolated() {
    let server = Server::new().await;
    let (first, first_token) = server.fixture.connection("claude-desktop");
    let (_, other_token) = server.fixture.connection("claude");
    let rpc = modern(5, "ping", json!({}));
    assert_eq!(server.post(&first, &other_token, &rpc).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
    assert_eq!(server.post(&first, &first_token, &rpc).header("origin", "https://example.test").send().await.unwrap().status(), StatusCode::FORBIDDEN);
    assert_eq!(server.post(&first, &first_token, &rpc).header("host", "example.test").send().await.unwrap().status(), StatusCode::FORBIDDEN);
    assert_eq!(server.post(&first, &first_token, &rpc).header("authorization", format!("Bearer {first_token}")).send().await.unwrap().status(), StatusCode::UNAUTHORIZED);
    server.shutdown().await;
}

#[tokio::test]
async fn all_legacy_versions_require_handshake_and_keep_sessions_with_the_owner() {
    let server = Server::new().await;
    let (connection, token) = server.fixture.connection("claude-desktop");
    let (other, other_token) = server.fixture.connection("claude");
    for version in SUPPORTED_VERSIONS.iter().copied().filter(|version| *version != MODERN_VERSION) {
        let initialize = legacy(1, "initialize", json!({"protocolVersion":version,"capabilities":{},"clientInfo":{"name":"Fixture","version":"1.0"}}));
        let response = server.post(&connection, &token, &initialize).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let session = response.headers()["mcp-session-id"].to_str().unwrap().to_owned();
        assert_eq!(response.json::<Value>().await.unwrap()["result"]["protocolVersion"], version);
        let tools = legacy(2, "tools/list", json!({}));
        assert_eq!(server.post(&connection, &token, &tools).header("mcp-protocol-version", version).header("mcp-session-id", &session).send().await.unwrap().status(), StatusCode::NOT_FOUND);
        let ready = json!({"jsonrpc":"2.0","method":"notifications/initialized","params":{}});
        assert_eq!(server.post(&connection, &token, &ready).header("mcp-protocol-version", version).header("mcp-session-id", &session).send().await.unwrap().status(), StatusCode::ACCEPTED);
        let response = server.post(&connection, &token, &tools).header("mcp-protocol-version", version).header("mcp-session-id", &session).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(response.json::<Value>().await.unwrap()["result"].get("resultType").is_none());
        assert_eq!(server.post(&other, &other_token, &tools).header("mcp-protocol-version", version).header("mcp-session-id", &session).send().await.unwrap().status(), StatusCode::NOT_FOUND);
        assert_eq!(server.post(&connection, &token, &initialize).header("mcp-session-id", &session).send().await.unwrap().status(), StatusCode::BAD_REQUEST);
        let response = server.client.delete(server.endpoint(&connection)).bearer_auth(&token).header("mcp-protocol-version", version).header("mcp-session-id", &session).send().await.unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert_eq!(server.post(&connection, &token, &tools).header("mcp-protocol-version", version).header("mcp-session-id", &session).send().await.unwrap().status(), StatusCode::NOT_FOUND);
    }
    server.shutdown().await;
}

#[tokio::test]
async fn closing_an_sse_body_removes_the_review_and_cancels_the_call() {
    let server = Server::new().await;
    server.fixture.install_echo(&["claude-desktop", "claude-desktop-3p"]);
    let (connection, token) = server.fixture.connection("claude-desktop");
    let rpc = modern(10, "tools/call", json!({"name":"test.shared-echo.echo","arguments":{"message":"safe"}}));
    let response = server.post(&connection, &token, &rpc).send().await.unwrap();
    assert_eq!(response.headers()["content-type"], "text/event-stream");
    let review = server.fixture.pending().await;
    drop(response);
    server.fixture.cleared().await;
    assert_eq!(server.fixture.manager().decide(&review.id, true, true).unwrap_err(), "shared_mcp_expired");
    server.shutdown().await;
}

#[tokio::test]
async fn approved_sse_results_only_include_redacted_arguments_and_reply_content() {
    let server = Server::new().await;
    server.fixture.install_echo(&["claude-desktop", "claude-desktop-3p"]);
    let (connection, token) = server.fixture.connection("claude-desktop");
    let rpc = modern(11, "tools/call", json!({"name":"test.shared-echo.echo","arguments":{"message":format!("Hello person@example.test {token}")}}));
    let response = server.post(&connection, &token, &rpc).send().await.unwrap();
    let review = server.fixture.pending().await;
    assert!(!review.arguments.to_string().contains(&token));
    assert!(!review.arguments.to_string().contains("person@example.test"));
    assert!(review.privacy.secret_redactions > 0);
    assert!(review.privacy.personal_redactions > 0);
    server.fixture.manager().decide(&review.id, true, true).unwrap();
    let body = response.text().await.unwrap();
    assert!(!body.contains(&token));
    assert!(!body.contains("person@example.test"));
    let data = body.lines().find_map(|line| line.strip_prefix("data:")).unwrap();
    let result: Value = serde_json::from_str(data.trim()).unwrap();
    assert_eq!(result["result"]["resultType"], "complete");
    assert_eq!(result["result"]["isError"], false);
    server.shutdown().await;
}
