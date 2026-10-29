use super::*;
use axum::{
    Json,
    body::Body,
    extract::State,
    http::{Request, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use futures_util::StreamExt;
use rusqlite::OptionalExtension;
use std::sync::{Arc, Mutex};
use tokio::sync::Semaphore;

pub const KEY: &str = "proxy_access_v1";
const MANAGED_CREDENTIAL: &str = "proxy-access-v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccessPolicy {
    pub require_token: bool,
    pub requests_per_minute: u32,
    pub max_in_flight: u32,
}
impl Default for AccessPolicy {
    fn default() -> Self {
        Self {
            require_token: false,
            requests_per_minute: 120,
            max_in_flight: 8,
        }
    }
}
impl AccessPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if !(1..=10000).contains(&self.requests_per_minute)
            || !(1..=128).contains(&self.max_in_flight)
        {
            Err("Invalid rate/concurrency limits".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    pub id: String,
    pub label: String,
    pub scope: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub revoked: bool,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IssuedToken {
    pub token: String,
    pub info: TokenInfo,
}
pub fn issue(db: &Database, label: &str, scope: &str, days: u32) -> Result<IssuedToken, String> {
    if label.trim().is_empty()
        || label.len() > 160
        || label.chars().any(char::is_control)
        || !matches!(scope, "proxy" | "monitor")
        || !(1..=365).contains(&days)
    {
        return Err("Invalid token label, scope or lifetime".into());
    }
    let token = format!(
        "xy_{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    );
    let info = TokenInfo {
        id: uuid::Uuid::new_v4().to_string(),
        label: label.trim().into(),
        scope: scope.into(),
        created_at: now(),
        expires_at: now() + i64::from(days) * 86400,
        revoked: false,
    };
    insert(db, &token, &info)?;
    Ok(IssuedToken { token, info })
}
fn insert(db: &Database, token: &str, i: &TokenInfo) -> Result<(), String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    let count: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM access_tokens WHERE revoked=0 AND expires_at>?1",
            [now()],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count >= 100 {
        return Err("Revoke an existing token before creating another (limit 100)".into());
    }
    c.execute("INSERT INTO access_tokens(id,label,digest,scope,created_at,expires_at,revoked) VALUES(?1,?2,?3,?4,?5,?6,0)",params![i.id,i.label,digest(token.as_bytes()),i.scope,i.created_at,i.expires_at]).map_err(|e|e.to_string())?;
    Ok(())
}
pub fn managed_token(db: &Database) -> Result<String, String> {
    static MANAGED_LOCK: Mutex<()> = Mutex::new(());
    let _guard = MANAGED_LOCK.lock().map_err(|_| "managed token lock")?;
    if !load::<AccessPolicy>(db, KEY)?.require_token {
        return Ok("XY_SWITCH_MANAGED".into());
    }
    if let Some(token) = crate::credential_store::get_secret("internal", MANAGED_CREDENTIAL)? {
        if token_scope(db, &token)?.as_deref() == Some("proxy") {
            return Ok(token);
        }
    }
    let issued = issue(db, "XY Switch managed clients", "proxy", 365)?;
    if let Err(error) =
        crate::credential_store::set_secret("internal", MANAGED_CREDENTIAL, &issued.token)
    {
        let connection = db.conn();
        if let Ok(connection) = connection.lock() {
            let _ = connection.execute(
                "UPDATE access_tokens SET revoked=1 WHERE id=?1",
                [&issued.info.id],
            );
        }
        return Err(error);
    }
    Ok(issued.token)
}
pub fn replace_managed(db: &Database, files: Vec<Vec<u8>>) -> Result<Vec<Vec<u8>>, String> {
    if !load::<AccessPolicy>(db, KEY)?.require_token {
        return Ok(files);
    }
    let token = managed_token(db)?;
    files
        .into_iter()
        .map(|b| {
            String::from_utf8(b)
                .map(|s| s.replace("XY_SWITCH_MANAGED", &token).into_bytes())
                .map_err(|_| "Managed client configuration must be UTF-8".into())
        })
        .collect()
}
fn token_scope(db: &Database, token: &str) -> Result<Option<String>, String> {
    if token.len() > 256 {
        return Ok(None);
    }
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    c.query_row(
        "SELECT scope FROM access_tokens WHERE digest=?1 AND revoked=0 AND expires_at>?2 AND scope IN ('proxy','monitor')",
        params![digest(token.as_bytes()), now()],
        |r| r.get(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}
#[derive(Clone)]
pub struct AccessState {
    db: Database,
    slots: Arc<Semaphore>,
    rate: Arc<Mutex<(i64, u32)>>,
}
impl AccessState {
    pub fn new(db: Database) -> Self {
        // Invalid persisted policy fails closed in the middleware.
        let max = load::<AccessPolicy>(&db, KEY)
            .map(|p| p.max_in_flight)
            .unwrap_or(1)
            .clamp(1, 128);
        Self {
            db,
            slots: Arc::new(Semaphore::new(max as usize)),
            rate: Arc::new(Mutex::new((0, 0))),
        }
    }
}
fn denial(status: StatusCode, code: &str) -> Response {
    (
        status,
        Json(serde_json::json!({"error":{"type":code,"message":code}})),
    )
        .into_response()
}
pub async fn protect(State(s): State<AccessState>, request: Request<Body>, next: Next) -> Response {
    if request.method() == axum::http::Method::GET && request.uri().path() == "/health" {
        return next.run(request).await;
    }
    let policy = match load::<AccessPolicy>(&s.db, KEY).and_then(|p| {
        p.validate()?;
        Ok(p)
    }) {
        Ok(p) => p,
        Err(_) => return denial(StatusCode::SERVICE_UNAVAILABLE, "access_policy_invalid"),
    };
    // Count rejected credentials too; otherwise token guessing bypasses the
    // shared request limit. The minimal liveness endpoint stays exempt.
    {
        let Ok(mut rate) = s.rate.lock() else {
            return denial(StatusCode::SERVICE_UNAVAILABLE, "rate_state_unavailable");
        };
        let minute = now() / 60;
        if rate.0 != minute {
            *rate = (minute, 0)
        }
        if rate.1 >= policy.requests_per_minute {
            return denial(StatusCode::TOO_MANY_REQUESTS, "local_rate_limited");
        }
        rate.1 += 1;
    }
    let readonly =
        request.method() == axum::http::Method::GET && request.uri().path() == "/xy/status";
    let mut tokens = Vec::new();
    for name in [
        header::AUTHORIZATION.as_str(),
        "x-api-key",
        "x-goog-api-key",
    ] {
        let values = request.headers().get_all(name);
        if values.iter().count() > 1 {
            return denial(StatusCode::UNAUTHORIZED, "ambiguous_credentials");
        }
        if let Some(v) = values.iter().next() {
            let Ok(value) = v.to_str() else {
                return denial(StatusCode::UNAUTHORIZED, "invalid_credentials");
            };
            let value = if name == "authorization" {
                match value.split_once(' ') {
                    Some((scheme, v)) if scheme.eq_ignore_ascii_case("bearer") => v,
                    _ => return denial(StatusCode::UNAUTHORIZED, "invalid_credentials"),
                }
            } else {
                value
            };
            tokens.push(value);
        }
    }
    if tokens.windows(2).any(|v| v[0] != v[1]) {
        return denial(StatusCode::UNAUTHORIZED, "ambiguous_credentials");
    }
    let scope = match tokens.first().map(|v| token_scope(&s.db, v)).transpose() {
        Ok(value) => value.flatten(),
        Err(_) => return denial(StatusCode::SERVICE_UNAVAILABLE, "access_store_unavailable"),
    };
    // A read-only token never grants model access, even while legacy tokenless
    // loopback access is enabled. Unknown xy_ tokens are not legacy credentials.
    if scope.as_deref() == Some("monitor") && !readonly {
        return denial(StatusCode::FORBIDDEN, "token_scope_denied");
    }
    if (policy.require_token || readonly || tokens.iter().any(|t| t.starts_with("xy_")))
        && scope.is_none()
    {
        return denial(StatusCode::UNAUTHORIZED, "local_token_required");
    }
    let Ok(permit) = s.slots.clone().try_acquire_owned() else {
        return denial(StatusCode::TOO_MANY_REQUESTS, "local_concurrency_limited");
    };
    let response = next.run(request).await;
    let (parts, body) = response.into_parts();
    let stream = body.into_data_stream().map(move |chunk| {
        let _ = &permit;
        chunk
    });
    Response::from_parts(parts, Body::from_stream(stream))
}
#[tauri::command]
pub fn get_access_policy(db: tauri::State<'_, Database>) -> Result<AccessPolicy, String> {
    load(&db, KEY)
}
#[tauri::command]
pub async fn save_access_policy(
    policy: AccessPolicy,
    confirmed: bool,
    db: tauri::State<'_, Database>,
    proxy: tauri::State<'_, crate::proxy_manager::ProxyManager>,
    takeover: tauri::State<'_, crate::takeover::AppTakeoverManager>,
) -> Result<(), String> {
    crate::platform::require_desktop("Governance changes")?;
    if !confirmed {
        return Err("Explicit confirmation required".into());
    }
    policy.validate()?;
    let _proxy_guard = proxy.configuration_guard().await;
    if proxy.status().await?.running {
        return Err(
            "Stop the proxy and restore active takeovers before changing access policy".into(),
        );
    }
    let _takeover_guard = takeover.inactive_guard()?;
    save(&db, KEY, &policy)?;
    event(
        &db,
        "access_policy_changed",
        if policy.require_token {
            "authentication_required"
        } else {
            "legacy_loopback_access"
        },
    )
}
#[tauri::command]
pub fn create_access_token(
    label: String,
    scope: String,
    days: u32,
    db: tauri::State<'_, Database>,
) -> Result<IssuedToken, String> {
    crate::platform::require_desktop("Governance changes")?;
    let token = issue(&db, &label, &scope, days)?;
    event(&db, "token_created", &token.info.id)?;
    Ok(token)
}
#[tauri::command]
pub fn list_access_tokens(db: tauri::State<'_, Database>) -> Result<Vec<TokenInfo>, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    let mut s=c.prepare("SELECT id,label,scope,created_at,expires_at,revoked FROM access_tokens ORDER BY (revoked=0 AND expires_at>strftime('%s','now')) DESC,created_at DESC,rowid DESC LIMIT 200").map_err(|e|e.to_string())?;
    s.query_map([], |r| {
        Ok(TokenInfo {
            id: r.get(0)?,
            label: r.get(1)?,
            scope: r.get(2)?,
            created_at: r.get(3)?,
            expires_at: r.get(4)?,
            revoked: r.get(5)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn revoke_access_token(
    id: String,
    confirmed: bool,
    db: tauri::State<'_, Database>,
) -> Result<(), String> {
    crate::platform::require_desktop("Governance changes")?;
    if !confirmed {
        return Err("Explicit confirmation required".into());
    }
    {
        let c = db.conn();
        let c = c.lock().map_err(|_| "database lock")?;
        c.execute("UPDATE access_tokens SET revoked=1 WHERE id=?1", [&id])
            .map_err(|e| e.to_string())?;
    }
    event(&db, "token_revoked", &id)
}
