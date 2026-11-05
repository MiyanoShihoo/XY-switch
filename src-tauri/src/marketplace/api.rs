use super::requests::{CatalogQuery, ListQuery, ReportRequest, ResolveReportRequest};
use super::{AuditEvent, AuthSession, CatalogEntry, DecisionRequest, Marketplace, MarketplaceError,
    Page, PluginReport, PluginWithdrawal, PublishedPlugin, SessionUser, Submission, SubmissionSummary,
    SubmitRequest, VersionRequest, WithdrawRequest, MAX_UPLOAD_BYTES};
use axum::{Json, Router, body::Bytes, extract::{DefaultBodyLimit, Path, Query, Request, State,
    rejection::{BytesRejection, PathRejection, QueryRejection}}, http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next}, response::{IntoResponse, Response}, routing::{get, post}};
use serde::{Deserialize, de::DeserializeOwned};
use std::time::Duration;
use zeroize::Zeroizing;

type IdPath = Result<Path<String>, PathRejection>;
type VersionPath = Result<Path<(String, String)>, PathRejection>;
type RequestBody = Result<Bytes, BytesRejection>;
type ListParams = Result<Query<ListQuery>, QueryRejection>;
type CatalogParams = Result<Query<CatalogQuery>, QueryRejection>;

pub fn router(marketplace: Marketplace) -> Router {
    let private = Router::new()
        .route("/v1/auth/me",get(me))
        .route("/v1/auth/logout",post(logout))
        .route("/v1/submissions",post(submit).get(submissions))
        .route("/v1/submissions/{submissionId}",get(submission))
        .route("/v1/submissions/{submissionId}/publish",post(publish).layer(DefaultBodyLimit::max(16*1024)))
        .route("/v1/submissions/{submissionId}/withdraw",post(withdraw).layer(DefaultBodyLimit::max(16*1024)))
        .route("/v1/submissions/{submissionId}/audit",get(submission_audit))
        .route("/v1/review/submissions",get(review_queue))
        .route("/v1/review/submissions/{submissionId}/decision",post(decide).layer(DefaultBodyLimit::max(16*1024)))
        .route("/v1/review/audit",get(review_audit))
        .route("/v1/review/reports",get(reports))
        .route("/v1/review/reports/{reportId}/resolve",post(resolve_report).layer(DefaultBodyLimit::max(16*1024)))
        .route("/v1/plugins/{id}/versions/{version}/reports",post(report).layer(DefaultBodyLimit::max(16*1024)))
        .route_layer(middleware::from_fn_with_state(marketplace.clone(),authenticate));
    Router::new()
        .merge(private)
        .route("/v1/auth/register",post(register).layer(DefaultBodyLimit::max(16*1024)))
        .route("/v1/auth/login",post(login).layer(DefaultBodyLimit::max(16*1024)))
        .route("/v1/plugins",get(catalog))
        .route("/v1/plugins/{id}/versions",get(versions))
        .route("/v1/plugins/{id}/versions/{version}",get(download))
        .route("/v1/plugins/{id}/versions/{version}/withdrawal",get(withdrawal))
        .route("/healthz",get(health))
        .route("/readyz",get(ready))
        .fallback(missing)
        .method_not_allowed_fallback(unsupported_method)
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .layer(middleware::from_fn_with_state(marketplace.clone(),transport))
        .with_state(marketplace)
}

fn bearer(headers: &HeaderMap) -> Result<Zeroizing<String>, MarketplaceError> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let header = values.next().ok_or(MarketplaceError::AuthenticationRequired)?;
    if values.next().is_some() { return Err(MarketplaceError::AuthenticationRequired); }
    let value = header.to_str().map_err(|_| MarketplaceError::AuthenticationRequired)?;
    let (scheme, token) = value.split_once(' ').ok_or(MarketplaceError::AuthenticationRequired)?;
    if !scheme.eq_ignore_ascii_case("bearer") { return Err(MarketplaceError::AuthenticationRequired); }
    super::auth::token_hash(token)?;
    Ok(Zeroizing::new(token.to_owned()))
}

fn json_body<T: DeserializeOwned>(headers: &HeaderMap, body: RequestBody) -> Result<T, MarketplaceError> {
    let content_type = headers.get(header::CONTENT_TYPE).and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next()).map(str::trim);
    if !content_type.is_some_and(|content_type| content_type.eq_ignore_ascii_case("application/json")) { return Err(MarketplaceError::InvalidRequest); }
    let body = body.map_err(|_| MarketplaceError::InvalidRequest)?;
    let text = std::str::from_utf8(&body).map_err(|_| MarketplaceError::InvalidRequest)?;
    let value = crate::plugins::validation::strict_json(text,MAX_UPLOAD_BYTES).map_err(|_| MarketplaceError::InvalidRequest)?;
    serde_json::from_value(value).map_err(|_| MarketplaceError::InvalidRequest)
}

async fn authenticate(State(marketplace): State<Marketplace>, request: Request, next: Next) -> Response {
    let token = match bearer(request.headers()) { Ok(token) => token,Err(error) => return error.into_response() };
    match marketplace.blocking(move |inner| inner.store.me(&token)).await {
        Ok(_) => next.run(request).await,
        Err(error) => error.into_response(),
    }
}

async fn transport(State(marketplace): State<Marketplace>, request: Request, next: Next) -> Response {
    let mut origins = request.headers().get_all(header::ORIGIN).iter();
    let origin = origins.next().cloned();
    let valid_origin = origins.next().is_none() && origin.as_ref().is_none_or(|value|
        value.to_str().ok().is_some_and(|value| marketplace.inner.allowed_origins.iter().any(|allowed| allowed==value)));
    let encoding_valid = request.headers().get(header::CONTENT_ENCODING).is_none_or(|value| value=="identity");
    let permit = marketplace.inner.requests.clone().try_acquire_owned();
    let mut response = if !valid_origin {
        MarketplaceError::Forbidden.into_response()
    } else if !encoding_valid || request.uri().to_string().len()>2048 {
        MarketplaceError::InvalidRequest.into_response()
    } else if permit.is_err() {
        MarketplaceError::RateLimited.into_response()
    } else if request.method() == Method::OPTIONS {
        let method_ok = request.headers().get(header::ACCESS_CONTROL_REQUEST_METHOD).is_none_or(|value| value=="GET" || value=="POST");
        let headers_ok = request.headers().get(header::ACCESS_CONTROL_REQUEST_HEADERS).is_none_or(|value|
            value.to_str().ok().is_some_and(|value| value.split(',').all(|name|
                matches!(name.trim().to_ascii_lowercase().as_str(),"authorization" | "content-type"))));
        if method_ok && headers_ok { StatusCode::NO_CONTENT.into_response() } else { MarketplaceError::Forbidden.into_response() }
    } else {
        match tokio::time::timeout(Duration::from_secs(20),next.run(request)).await {
            Ok(response) => response,Err(_) => MarketplaceError::Unavailable.into_response(),
        }
    };
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL,HeaderValue::from_static("no-store"));
    headers.insert(header::PRAGMA,HeaderValue::from_static("no-cache"));
    headers.insert("x-content-type-options",HeaderValue::from_static("nosniff"));
    headers.insert("referrer-policy",HeaderValue::from_static("no-referrer"));
    headers.insert("content-security-policy",HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"));
    headers.insert(header::VARY,HeaderValue::from_static("Origin"));
    if marketplace.inner.secure_origin { headers.insert("strict-transport-security",HeaderValue::from_static("max-age=31536000")); }
    if valid_origin && let Some(origin) = origin {
        headers.insert(header::ACCESS_CONTROL_ALLOW_ORIGIN,origin);
        headers.insert(header::ACCESS_CONTROL_ALLOW_METHODS,HeaderValue::from_static("GET, POST, OPTIONS"));
        headers.insert(header::ACCESS_CONTROL_ALLOW_HEADERS,HeaderValue::from_static("Authorization, Content-Type"));
    }
    response
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Registration { username: String, display_name: String, password: String }

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Login { username: String, password: String }

async fn register(State(marketplace): State<Marketplace>, headers: HeaderMap, body: RequestBody) -> Result<(StatusCode,Json<AuthSession>),MarketplaceError> {
    let request: Registration = json_body(&headers,body)?;
    let password = Zeroizing::new(request.password);
    let session = marketplace.blocking(move |inner| inner.store.register(&request.username,&request.display_name,&password,inner.session_ttl_seconds)).await?;
    Ok((StatusCode::CREATED,Json(session)))
}

async fn login(State(marketplace): State<Marketplace>, headers: HeaderMap, body: RequestBody) -> Result<Json<AuthSession>,MarketplaceError> {
    let request: Login = json_body(&headers,body)?;
    let password = Zeroizing::new(request.password);
    Ok(Json(marketplace.blocking(move |inner| inner.store.login(&request.username,&password,inner.session_ttl_seconds)).await?))
}

async fn me(State(marketplace): State<Marketplace>, headers: HeaderMap) -> Result<Json<SessionUser>,MarketplaceError> {
    let token = bearer(&headers)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.me(&token)).await?))
}

async fn logout(State(marketplace): State<Marketplace>, headers: HeaderMap) -> Result<StatusCode,MarketplaceError> {
    let token = bearer(&headers)?;
    marketplace.blocking(move |inner| inner.store.logout(&token)).await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn submit(State(marketplace): State<Marketplace>, headers: HeaderMap, body: RequestBody) -> Result<(StatusCode,Json<Submission>),MarketplaceError> {
    let token = bearer(&headers)?;
    let request: SubmitRequest = json_body(&headers,body)?;
    let submission = marketplace.blocking(move |inner| inner.store.submit(inner,&token,request)).await?;
    Ok((StatusCode::CREATED,Json(submission)))
}

async fn submissions(State(marketplace): State<Marketplace>, headers: HeaderMap, query: ListParams) -> Result<Json<Page<SubmissionSummary>>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Query(query) = query.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.submissions(inner,&token,&query,false)).await?))
}

async fn review_queue(State(marketplace): State<Marketplace>, headers: HeaderMap, query: ListParams) -> Result<Json<Page<SubmissionSummary>>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Query(query) = query.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.submissions(inner,&token,&query,true)).await?))
}

async fn submission(State(marketplace): State<Marketplace>, headers: HeaderMap, path: IdPath) -> Result<Json<Submission>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Path(id) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.submission(inner,&token,&id)).await?))
}

async fn decide(State(marketplace): State<Marketplace>, headers: HeaderMap, path: IdPath, body: RequestBody) -> Result<Json<Submission>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Path(id) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    let request: DecisionRequest = json_body(&headers,body)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.decide(inner,&token,&id,&request)).await?))
}

async fn publish(State(marketplace): State<Marketplace>, headers: HeaderMap, path: IdPath, body: RequestBody) -> Result<Json<PublishedPlugin>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Path(id) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    let expected: VersionRequest = json_body(&headers,body)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.publish(inner,&token,&id,&expected)).await?))
}

async fn withdraw(State(marketplace): State<Marketplace>, headers: HeaderMap, path: IdPath, body: RequestBody) -> Result<Json<Submission>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Path(id) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    let request: WithdrawRequest = json_body(&headers,body)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.withdraw(inner,&token,&id,&request)).await?))
}

async fn submission_audit(State(marketplace): State<Marketplace>, headers: HeaderMap, path: IdPath, query: ListParams) -> Result<Json<Page<AuditEvent>>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Path(id) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    let Query(query) = query.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.audit(&token,Some(&id),&query)).await?))
}

async fn review_audit(State(marketplace): State<Marketplace>, headers: HeaderMap, query: ListParams) -> Result<Json<Page<AuditEvent>>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Query(query) = query.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.audit(&token,None,&query)).await?))
}

async fn catalog(State(marketplace): State<Marketplace>, query: CatalogParams) -> Result<Json<Page<CatalogEntry>>,MarketplaceError> {
    let Query(query) = query.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.catalog(inner,&query,None)).await?))
}

async fn versions(State(marketplace): State<Marketplace>, path: IdPath, query: CatalogParams) -> Result<Json<Page<CatalogEntry>>,MarketplaceError> {
    let Path(id) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    let Query(query) = query.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.catalog(inner,&query,Some(&id))).await?))
}

async fn download(State(marketplace): State<Marketplace>, path: VersionPath) -> Result<Json<PublishedPlugin>,MarketplaceError> {
    let Path((id,version)) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.download(inner,&id,&version)).await?))
}

async fn withdrawal(State(marketplace): State<Marketplace>, path: VersionPath) -> Result<Json<PluginWithdrawal>,MarketplaceError> {
    let Path((id,version)) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.withdrawal(inner,&id,&version)).await?))
}

async fn report(State(marketplace): State<Marketplace>, headers: HeaderMap, path: VersionPath, body: RequestBody) -> Result<(StatusCode,Json<PluginReport>),MarketplaceError> {
    let token = bearer(&headers)?;
    let Path((id,version)) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    let request: ReportRequest = json_body(&headers,body)?;
    let report = marketplace.blocking(move |inner| inner.store.report(&token,&id,&version,&request)).await?;
    Ok((StatusCode::CREATED,Json(report)))
}

async fn reports(State(marketplace): State<Marketplace>, headers: HeaderMap, query: ListParams) -> Result<Json<Page<PluginReport>>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Query(query) = query.map_err(|_| MarketplaceError::InvalidRequest)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.reports(&token,&query)).await?))
}

async fn resolve_report(State(marketplace): State<Marketplace>, headers: HeaderMap, path: IdPath, body: RequestBody) -> Result<Json<PluginReport>,MarketplaceError> {
    let token = bearer(&headers)?;
    let Path(id) = path.map_err(|_| MarketplaceError::InvalidRequest)?;
    let request: ResolveReportRequest = json_body(&headers,body)?;
    Ok(Json(marketplace.blocking(move |inner| inner.store.resolve_report(&token,&id,&request)).await?))
}

async fn health() -> Json<serde_json::Value> { Json(serde_json::json!({"status":"ok"})) }

async fn ready(State(marketplace): State<Marketplace>) -> Result<Json<serde_json::Value>,MarketplaceError> {
    marketplace.blocking(|inner| inner.store.ready()).await?;
    Ok(Json(serde_json::json!({"status":"ok"})))
}

async fn missing() -> MarketplaceError { MarketplaceError::NotFound }

async fn unsupported_method() -> Response {
    let mut response = MarketplaceError::InvalidRequest.into_response();
    *response.status_mut() = StatusCode::METHOD_NOT_ALLOWED;
    response
}
