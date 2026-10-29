use axum::{
    Json,
    body::Body,
    extract::State,
    http::{HeaderMap, Method, Request, Uri, header, uri::Authority},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::net::{IpAddr, SocketAddr};

fn allowed_authority(raw: &str, address: Option<SocketAddr>) -> bool {
    if raw.contains('@') {
        return false;
    }
    let Ok(authority) = raw.parse::<Authority>() else {
        return false;
    };
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let local = host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("localhost.")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    let valid_port =
        address.is_none_or(|address| authority.port_u16().unwrap_or(80) == address.port());
    local && valid_port
}

fn validate(
    headers: &HeaderMap,
    uri: &Uri,
    method: &Method,
    address: Option<SocketAddr>,
) -> Result<(), &'static str> {
    let mut hosts = headers.get_all(header::HOST).iter();
    let host = hosts.next();
    if hosts.next().is_some() {
        return Err("forbidden_host");
    }
    if let Some(host) = host {
        if !host
            .to_str()
            .is_ok_and(|host| allowed_authority(host, address))
        {
            return Err("forbidden_host");
        }
    } else if uri.authority().is_none() {
        return Err("forbidden_host");
    }
    if uri
        .authority()
        .is_some_and(|authority| !allowed_authority(authority.as_str(), address))
    {
        return Err("forbidden_host");
    }
    // CORS alone does not stop a text/plain, no-cors browser POST from spending
    // the user's upstream quota. CLI/native clients do not need browser access.
    // An address-bar health check is the only intentionally public browser read.
    let browser = headers.contains_key(header::ORIGIN) || headers.contains_key("sec-fetch-site");
    if browser && !(*method == Method::GET && uri.path() == "/health") {
        return Err("forbidden_origin");
    }
    Ok(())
}

pub(crate) async fn protect(
    State(address): State<Option<SocketAddr>>,
    request: Request<Body>,
    next: Next,
) -> Response {
    if let Err(code) = validate(request.headers(), request.uri(), request.method(), address) {
        return (axum::http::StatusCode::FORBIDDEN, Json(serde_json::json!({"error":{
            "type":code,"message":"The local proxy only accepts loopback CLI/native requests, not browser-origin requests."
        }}))).into_response();
    }
    next.run(request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, routing::post};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    #[test]
    fn host_validation_blocks_rebinding_userinfo_and_wrong_ports() {
        let address = Some("127.0.0.1:14550".parse().unwrap());
        for good in [
            "127.0.0.1:14550",
            "127.0.0.2:14550",
            "localhost:14550",
            "LOCALHOST.:14550",
            "[::1]:14550",
        ] {
            assert!(allowed_authority(good, address), "{good}");
        }
        for bad in [
            "attacker.invalid:14550",
            "localhost.attacker.invalid:14550",
            "localhost@attacker.invalid:14550",
            "127.0.0.1:14551",
            "127.0.0.1",
            "[::]:14550",
            "0.0.0.0:14550",
            "localhost:invalid",
        ] {
            assert!(!allowed_authority(bad, address), "{bad}");
        }
    }

    #[test]
    fn browser_and_null_origins_cannot_reach_billable_routes() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "localhost:14550".parse().unwrap());
        let uri = "/v1/responses".parse().unwrap();
        assert!(validate(&headers, &uri, &Method::POST, None).is_ok());
        for origin in ["https://attacker.invalid", "null", "http://localhost:1234"] {
            headers.insert(header::ORIGIN, origin.parse().unwrap());
            assert_eq!(
                validate(&headers, &uri, &Method::POST, None),
                Err("forbidden_origin")
            );
        }
        headers.remove(header::ORIGIN);
        headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
        assert!(validate(&headers, &uri, &Method::POST, None).is_err());
        assert!(validate(&headers, &"/health".parse().unwrap(), &Method::GET, None).is_ok());
        headers.remove(header::HOST);
        assert!(validate(&headers, &uri, &Method::POST, None).is_err());
    }

    #[tokio::test]
    async fn real_http_simple_post_is_blocked_before_the_handler_runs() {
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let router = Router::new()
            .route(
                "/v1/responses",
                post(move || {
                    let called = called.clone();
                    async move {
                        called.fetch_add(1, Ordering::SeqCst);
                        "accepted"
                    }
                }),
            )
            .layer(axum::middleware::from_fn_with_state(Some(address), protect));
        let task = tokio::spawn(async move { axum::serve(listener, router).await });
        let client = crate::network::test_client();
        let url = format!("http://{address}/v1/responses");
        let response = client
            .post(&url)
            .header(header::ORIGIN, "https://attacker.invalid")
            .header(header::CONTENT_TYPE, "text/plain")
            .body("{\"model\":\"billable\"}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403);
        let response = client
            .post(&url)
            .header(header::HOST, format!("attacker.invalid:{}", address.port()))
            .body("{}")
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 403);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(
            client.post(&url).body("{}").send().await.unwrap().status(),
            200
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        task.abort();
        let _ = task.await;
    }

    #[test]
    fn release_webview_has_a_restrictive_content_security_policy() {
        let config: serde_json::Value =
            serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        let policy = config["app"]["security"]["csp"]
            .as_str()
            .expect("production CSP");
        assert!(policy.contains("object-src 'none'"));
        assert!(policy.contains("script-src 'self'"));
        assert!(!policy.contains("'unsafe-eval'"));
        assert!(
            !policy
                .split(';')
                .find(|p| p.trim().starts_with("script-src"))
                .unwrap()
                .contains("'unsafe-inline'")
        );
    }
}
