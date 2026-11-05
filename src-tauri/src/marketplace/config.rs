use super::auth::{decode_hex, normalize_username, PasswordHash};
use super::{MarketplaceError, privacy};
use serde::{Deserialize, Serialize};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::collections::{BTreeMap, BTreeSet};
use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReviewerConfig {
    pub username: String,
    pub display_name: String,
    pub password_hash: PasswordHash,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct VerificationKeyConfig {
    pub id: String,
    pub public_key_hex: String,
}

pub struct MarketplaceConfig {
    pub database_path: PathBuf,
    pub listen: SocketAddr,
    pub public_origin: String,
    pub allowed_origins: Vec<String>,
    pub reviewers: Vec<ReviewerConfig>,
    pub signing_key_id: String,
    pub signing_private_key: Vec<u8>,
    pub verification_keys: BTreeMap<String, Vec<u8>>,
    pub session_ttl_seconds: u64,
    pub allow_development_http: bool,
    pub allow_non_loopback: bool,
}

impl MarketplaceConfig {
    pub fn from_env() -> Result<Self, MarketplaceError> {
        let database_path = PathBuf::from(required("XY_MARKETPLACE_DATABASE")?);
        let listen = optional("XY_MARKETPLACE_LISTEN")?.unwrap_or_else(|| "127.0.0.1:47831".to_owned())
            .parse().map_err(|_| MarketplaceError::InvalidConfiguration)?;
        let public_origin = required("XY_MARKETPLACE_PUBLIC_ORIGIN")?;
        let mut allowed_origins: Vec<String> = match optional("XY_MARKETPLACE_ALLOWED_ORIGINS")? {
            Some(encoded) => serde_json::from_str(&encoded).map_err(|_| MarketplaceError::InvalidConfiguration)?,
            None => vec![public_origin.clone()],
        };
        if !allowed_origins.contains(&public_origin) { allowed_origins.push(public_origin.clone()); }
        let reviewers = serde_json::from_slice(&read_config_file(Path::new(&required("XY_MARKETPLACE_REVIEWERS_FILE")?), 128 * 1024)?)
            .map_err(|_| MarketplaceError::InvalidConfiguration)?;
        let signing_key_id = required("XY_MARKETPLACE_SIGNING_KEY_ID")?;
        let signing_private_key = read_config_file(Path::new(&required("XY_MARKETPLACE_SIGNING_KEY_FILE")?), 16 * 1024)?;
        let mut verification_keys = BTreeMap::new();
        if let Some(path) = optional("XY_MARKETPLACE_VERIFICATION_KEYS_FILE")? {
            let configured: Vec<VerificationKeyConfig> = serde_json::from_slice(&read_config_file(Path::new(&path), 128 * 1024)?)
                .map_err(|_| MarketplaceError::InvalidConfiguration)?;
            for key in configured {
                if !valid_key_id(&key.id) || verification_keys.insert(key.id, decode_hex(&key.public_key_hex, 32)?).is_some() {
                    return Err(MarketplaceError::InvalidConfiguration);
                }
            }
        }
        let session_ttl_seconds = optional("XY_MARKETPLACE_SESSION_TTL_SECONDS")?
            .unwrap_or_else(|| "86400".to_owned()).parse().map_err(|_| MarketplaceError::InvalidConfiguration)?;
        let config = Self {
            database_path, listen, public_origin, allowed_origins, reviewers,
            signing_key_id, signing_private_key, verification_keys, session_ttl_seconds,
            allow_development_http: boolean("XY_MARKETPLACE_ALLOW_DEVELOPMENT_HTTP")?,
            allow_non_loopback: boolean("XY_MARKETPLACE_ALLOW_NON_LOOPBACK")?,
        };
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<(), MarketplaceError> {
        if !self.database_path.is_absolute() || self.database_path.file_name().is_none()
            || self.database_path.as_os_str().is_empty()
            || self.reviewers.is_empty() || self.reviewers.len() > 64
            || !valid_key_id(&self.signing_key_id)
            || self.signing_private_key.is_empty() || self.signing_private_key.len() > 16 * 1024
            || !(900..=604_800).contains(&self.session_ttl_seconds)
            || self.allowed_origins.is_empty() || self.allowed_origins.len() > 32
            || self.verification_keys.len() > 64
            || !self.listen.ip().is_loopback() && !self.allow_non_loopback {
            return Err(MarketplaceError::InvalidConfiguration);
        }
        validate_origin(&self.public_origin, self.allow_development_http)?;
        for origin in &self.allowed_origins {
            if !matches!(origin.as_str(), "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost") {
                validate_origin(origin, self.allow_development_http)?;
            }
        }
        let mut usernames = BTreeSet::new();
        for reviewer in &self.reviewers {
            let username = normalize_username(&reviewer.username).map_err(|_| MarketplaceError::InvalidConfiguration)?;
            if username != reviewer.username || !usernames.insert(username) {
                return Err(MarketplaceError::InvalidConfiguration);
            }
            privacy::inspect_display_name(&reviewer.display_name).map_err(|_| MarketplaceError::InvalidConfiguration)?;
            reviewer.password_hash.validate()?;
        }
        for (key_id, public_key) in &self.verification_keys {
            if !valid_key_id(key_id) || public_key.len() != 32 { return Err(MarketplaceError::InvalidConfiguration); }
        }
        let signer = Ed25519KeyPair::from_pkcs8(&self.signing_private_key)
            .map_err(|_| MarketplaceError::InvalidConfiguration)?;
        if self.verification_keys.get(&self.signing_key_id)
            .is_some_and(|configured| configured.as_slice() != signer.public_key().as_ref()) {
            return Err(MarketplaceError::InvalidConfiguration);
        }
        Ok(())
    }
}

fn validate_origin(origin: &str, allow_http: bool) -> Result<(), MarketplaceError> {
    let parsed = reqwest::Url::parse(origin).map_err(|_| MarketplaceError::InvalidConfiguration)?;
    let local = parsed.host_str().is_some_and(|host|
        host == "localhost" || host == "127.0.0.1" || host == "[::1]" || host == "::1");
    if origin.len() > 256 || parsed.host_str().is_none() || !parsed.username().is_empty()
        || parsed.password().is_some() || parsed.query().is_some() || parsed.fragment().is_some()
        || parsed.path() != "/" || origin.ends_with('/')
        || !(parsed.scheme() == "https" || allow_http && local && parsed.scheme() == "http") {
        return Err(MarketplaceError::InvalidConfiguration);
    }
    Ok(())
}

fn valid_key_id(value: &str) -> bool {
    value.len() <= 64 && crate::plugins::validation::validate_id(value).is_ok()
}

fn required(name: &str) -> Result<String, MarketplaceError> {
    optional(name)?.filter(|value| !value.is_empty()).ok_or(MarketplaceError::InvalidConfiguration)
}

fn optional(name: &str) -> Result<Option<String>, MarketplaceError> {
    match std::env::var(name) {
        Ok(value) if value.len() <= 16 * 1024 => Ok(Some(value)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        _ => Err(MarketplaceError::InvalidConfiguration),
    }
}

fn boolean(name: &str) -> Result<bool, MarketplaceError> {
    match optional(name)?.as_deref() {
        None | Some("false") => Ok(false),
        Some("true") => Ok(true),
        _ => Err(MarketplaceError::InvalidConfiguration),
    }
}

fn read_config_file(path: &Path, maximum: u64) -> Result<Vec<u8>, MarketplaceError> {
    let metadata = std::fs::metadata(path).map_err(|_| MarketplaceError::InvalidConfiguration)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > maximum {
        return Err(MarketplaceError::InvalidConfiguration);
    }
    std::fs::read(path).map_err(|_| MarketplaceError::InvalidConfiguration)
}
