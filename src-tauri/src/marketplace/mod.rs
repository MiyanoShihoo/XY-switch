mod api;
mod auth;
mod config;
mod privacy;
mod requests;
mod schema;
mod store;

#[cfg(test)]
mod tests;

pub use api::router;
pub use auth::{PasswordHash, hash_password_for_deployment};
pub use config::{MarketplaceConfig, ReviewerConfig, VerificationKeyConfig};
pub use requests::{DecisionRequest, ReviewDecision, SubmitRequest, VersionRequest, WithdrawRequest};
pub use crate::plugins::{PublishedPlugin, PluginWithdrawal, WithdrawalReason};

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use crate::plugins::{PluginPackage, package_digest, validate_package};
use ring::signature::{Ed25519KeyPair, KeyPair, UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::Arc;
use tokio::sync::Semaphore;
use zeroize::Zeroizing;

pub const MAX_UPLOAD_BYTES: usize = crate::plugins::MAX_PACKAGE_BYTES + 16 * 1024;
pub const MAX_NOTE_BYTES: usize = 4 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketplaceError {
    InvalidRequest,
    InvalidPackage,
    PrivacyRejected,
    AuthenticationRequired,
    InvalidCredentials,
    Forbidden,
    NotFound,
    Conflict,
    RateLimited,
    Unavailable,
    InvalidConfiguration,
}

impl MarketplaceError {
    pub fn code(self) -> &'static str {
        match self {
            Self::InvalidRequest => "invalid_request",
            Self::InvalidPackage => "invalid_package",
            Self::PrivacyRejected => "privacy_rejected",
            Self::AuthenticationRequired => "authentication_required",
            Self::InvalidCredentials => "invalid_credentials",
            Self::Forbidden => "forbidden",
            Self::NotFound => "not_found",
            Self::Conflict => "conflict",
            Self::RateLimited => "rate_limited",
            Self::Unavailable => "service_unavailable",
            Self::InvalidConfiguration => "invalid_configuration",
        }
    }

    fn status(self) -> StatusCode {
        match self {
            Self::InvalidRequest | Self::InvalidPackage => StatusCode::BAD_REQUEST,
            Self::PrivacyRejected => StatusCode::UNPROCESSABLE_ENTITY,
            Self::AuthenticationRequired | Self::InvalidCredentials => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            Self::Unavailable | Self::InvalidConfiguration => StatusCode::SERVICE_UNAVAILABLE,
        }
    }

    fn message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "The request does not match the API contract.",
            Self::InvalidPackage => "The package does not satisfy the host package contract.",
            Self::PrivacyRejected => "Remove private information or uninspectable content before submitting.",
            Self::AuthenticationRequired => "A valid marketplace bearer session is required.",
            Self::InvalidCredentials => "The marketplace credentials are invalid.",
            Self::Forbidden => "This marketplace account cannot perform this action.",
            Self::NotFound => "The requested resource is not available.",
            Self::Conflict => "The version, digest, ownership, or workflow state conflicts with this action.",
            Self::RateLimited => "Wait before retrying this operation.",
            Self::Unavailable => "The marketplace cannot safely complete this operation.",
            Self::InvalidConfiguration => "The marketplace deployment configuration is invalid.",
        }
    }
}

impl std::fmt::Display for MarketplaceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.code())
    }
}

impl std::error::Error for MarketplaceError {}

impl IntoResponse for MarketplaceError {
    fn into_response(self) -> Response {
        let mut response = (self.status(), Json(serde_json::json!({
            "error": { "code": self.code(), "message": self.message() }
        }))).into_response();
        if self == Self::RateLimited {
            response.headers_mut().insert("retry-after", axum::http::HeaderValue::from_static("60"));
        }
        response
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubmissionStatus {
    Submitted,
    ChangesRequested,
    Rejected,
    Approved,
    Published,
    Withdrawn,
}

impl SubmissionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Submitted => "submitted",
            Self::ChangesRequested => "changes_requested",
            Self::Rejected => "rejected",
            Self::Approved => "approved",
            Self::Published => "published",
            Self::Withdrawn => "withdrawn",
        }
    }

    fn parse(value: &str) -> Result<Self, MarketplaceError> {
        match value {
            "submitted" => Ok(Self::Submitted),
            "changes_requested" => Ok(Self::ChangesRequested),
            "rejected" => Ok(Self::Rejected),
            "approved" => Ok(Self::Approved),
            "published" => Ok(Self::Published),
            "withdrawn" => Ok(Self::Withdrawn),
            _ => Err(MarketplaceError::Unavailable),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Submission {
    pub submission_id: String,
    pub status: SubmissionStatus,
    pub package: PluginPackage,
    pub digest: String,
    pub author_display_name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub review_note: Option<String>,
    pub previous_published_version: Option<String>,
    pub permission_changes: crate::plugins::PluginPermissions,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionUser {
    pub user_id: String,
    pub username: String,
    pub display_name: String,
    pub reviewer: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmissionSummary {
    pub submission_id: String,
    pub plugin_id: String,
    pub name: String,
    pub version: String,
    pub digest: String,
    pub status: SubmissionStatus,
    pub author_display_name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub review_note: Option<String>,
    pub previous_published_version: Option<String>,
    pub permission_changes: crate::plugins::PluginPermissions,
    pub permissions: crate::plugins::PluginPermissions,
    pub clients: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthSession {
    pub token: String,
    pub token_type: &'static str,
    pub expires_at: i64,
    pub user: SessionUser,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub description: String,
    pub author_display_name: String,
    pub version: String,
    pub kind: serde_json::Value,
    pub clients: Vec<String>,
    pub digest: String,
    pub signature: String,
    pub signing_key_id: String,
    pub published_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Page<T> {
    pub items: Vec<T>,
    pub limit: u32,
    pub offset: u32,
    pub has_more: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEvent {
    pub sequence: i64,
    pub submission_id: Option<String>,
    pub actor_display_name: String,
    pub action: String,
    pub from_status: Option<String>,
    pub to_status: Option<String>,
    pub version: Option<String>,
    pub digest: Option<String>,
    pub note: Option<String>,
    pub created_at: i64,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginReport {
    pub report_id: String,
    pub plugin_id: String,
    pub version: String,
    pub reporter_display_name: String,
    pub reason: String,
    pub note: String,
    pub status: String,
    pub resolution_note: Option<String>,
    pub created_at: i64,
    pub resolved_at: Option<i64>,
}

#[derive(Clone)]
pub struct Marketplace {
    inner: Arc<MarketplaceInner>,
}

struct MarketplaceInner {
    store: store::MarketStore,
    signer: Ed25519KeyPair,
    signing_key_id: String,
    verification_keys: BTreeMap<String, Vec<u8>>,
    allowed_origins: Vec<String>,
    session_ttl_seconds: i64,
    secure_origin: bool,
    workers: Arc<Semaphore>,
    requests: Arc<Semaphore>,
}

impl Marketplace {
    pub fn open(config: MarketplaceConfig) -> Result<Self, MarketplaceError> {
        Self::configured(config, false)
    }

    pub fn verify_database(config: MarketplaceConfig) -> Result<(), MarketplaceError> {
        Self::configured(config, true)?.verify_state()
    }

    fn configured(config: MarketplaceConfig, verification_only: bool) -> Result<Self, MarketplaceError> {
        config.validate()?;
        let private_key = Zeroizing::new(config.signing_private_key);
        let signer = Ed25519KeyPair::from_pkcs8(&private_key)
            .map_err(|_| MarketplaceError::InvalidConfiguration)?;
        let mut verification_keys = config.verification_keys;
        let active_public_key = signer.public_key().as_ref().to_vec();
        if verification_keys.get(&config.signing_key_id)
            .is_some_and(|configured| configured != &active_public_key) {
            return Err(MarketplaceError::InvalidConfiguration);
        }
        verification_keys.insert(config.signing_key_id.clone(), active_public_key);
        let store = if verification_only {
            store::MarketStore::existing(&config.database_path)?
        } else {
            store::MarketStore::open(&config.database_path, &config.reviewers, &verification_keys)?
        };
        Ok(Self { inner: Arc::new(MarketplaceInner {
            store,
            signer,
            signing_key_id: config.signing_key_id,
            verification_keys,
            allowed_origins: config.allowed_origins,
            session_ttl_seconds: config.session_ttl_seconds as i64,
            secure_origin: config.public_origin.starts_with("https://"),
            workers: Arc::new(Semaphore::new(8)),
            requests: Arc::new(Semaphore::new(32)),
        }) })
    }

    pub fn verify_state(&self) -> Result<(), MarketplaceError> {
        self.inner.store.verify_state(&self.inner)
    }

    async fn blocking<T, F>(&self, operation: F) -> Result<T, MarketplaceError>
    where
        T: Send + 'static,
        F: FnOnce(&MarketplaceInner) -> Result<T, MarketplaceError> + Send + 'static,
    {
        let permit = self.inner.workers.clone().try_acquire_owned()
            .map_err(|_| MarketplaceError::RateLimited)?;
        let inner = self.inner.clone();
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            operation(&inner)
        }).await.map_err(|_| MarketplaceError::Unavailable)?
    }
}

pub fn signature_payload(id: &str, version: &str, digest: &str) -> Vec<u8> {
    format!("xy-switch-plugin-v1\n{id}\n{version}\n{digest}").into_bytes()
}

fn checked_package(package: &PluginPackage) -> Result<(String, String), MarketplaceError> {
    let value = serde_json::to_value(package).map_err(|_| MarketplaceError::InvalidPackage)?;
    privacy::inspect_package(&value)?;
    validate_package(package).map_err(|_| MarketplaceError::InvalidPackage)?;
    let bytes = serde_json::to_vec(package).map_err(|_| MarketplaceError::InvalidPackage)?;
    if bytes.len() > MAX_UPLOAD_BYTES { return Err(MarketplaceError::InvalidPackage); }
    let digest = package_digest(package).map_err(|_| MarketplaceError::InvalidPackage)?;
    if digest != auth::encode_hex(&Sha256::digest(&bytes)) {
        return Err(MarketplaceError::InvalidPackage);
    }
    let json = String::from_utf8(bytes).map_err(|_| MarketplaceError::InvalidPackage)?;
    Ok((json, digest))
}

fn verify_published(inner: &MarketplaceInner, plugin: &PublishedPlugin) -> Result<(), MarketplaceError> {
    let (_, digest) = checked_package(&plugin.package).map_err(|_| MarketplaceError::Unavailable)?;
    if digest != plugin.digest { return Err(MarketplaceError::Unavailable); }
    let public_key = inner.verification_keys.get(&plugin.signing_key_id)
        .ok_or(MarketplaceError::Unavailable)?;
    let signature = auth::decode_hex(&plugin.signature, 64)
        .map_err(|_| MarketplaceError::Unavailable)?;
    UnparsedPublicKey::new(&ED25519, public_key).verify(
        &signature_payload(&plugin.package.manifest.id, &plugin.package.manifest.version, &digest),
        &signature,
    ).map_err(|_| MarketplaceError::Unavailable)
}
