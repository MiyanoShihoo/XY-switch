use super::MarketplaceError;
use ring::pbkdf2;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::num::NonZeroU32;
use std::time::{SystemTime, UNIX_EPOCH};

pub const PASSWORD_ITERATIONS: u32 = 600_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PasswordHash {
    pub algorithm: String,
    pub iterations: u32,
    pub salt_hex: String,
    pub hash_hex: String,
}

impl PasswordHash {
    pub fn validate(&self) -> Result<(), MarketplaceError> {
        if self.algorithm != "pbkdf2-sha256"
            || !(PASSWORD_ITERATIONS..=2_000_000).contains(&self.iterations) {
            return Err(MarketplaceError::InvalidConfiguration);
        }
        decode_hex(&self.salt_hex, 32)?;
        decode_hex(&self.hash_hex, 32)?;
        Ok(())
    }

    pub fn verify(&self, password: &str) -> Result<bool, MarketplaceError> {
        self.validate().map_err(|_| MarketplaceError::Unavailable)?;
        let salt = decode_hex(&self.salt_hex, 32).map_err(|_| MarketplaceError::Unavailable)?;
        let expected = decode_hex(&self.hash_hex, 32).map_err(|_| MarketplaceError::Unavailable)?;
        let iterations = NonZeroU32::new(self.iterations).ok_or(MarketplaceError::Unavailable)?;
        Ok(pbkdf2::verify(pbkdf2::PBKDF2_HMAC_SHA256, iterations, &salt,
            password.as_bytes(), &expected).is_ok())
    }
}

pub fn hash_password_for_deployment(password: &str) -> Result<PasswordHash, MarketplaceError> {
    validate_password(password)?;
    let salt = random_bytes::<32>()?;
    let mut derived = [0_u8; 32];
    let iterations = NonZeroU32::new(PASSWORD_ITERATIONS).ok_or(MarketplaceError::Unavailable)?;
    pbkdf2::derive(pbkdf2::PBKDF2_HMAC_SHA256, iterations, &salt, password.as_bytes(), &mut derived);
    Ok(PasswordHash {
        algorithm: "pbkdf2-sha256".to_owned(),
        iterations: PASSWORD_ITERATIONS,
        salt_hex: encode_hex(&salt),
        hash_hex: encode_hex(&derived),
    })
}

pub fn validate_password(password: &str) -> Result<(), MarketplaceError> {
    if !(12..=128).contains(&password.len()) || password.chars().any(char::is_control) {
        return Err(MarketplaceError::InvalidRequest);
    }
    Ok(())
}

pub fn normalize_username(username: &str) -> Result<String, MarketplaceError> {
    if !(3..=32).contains(&username.len())
        || !username.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-') {
        return Err(MarketplaceError::InvalidRequest);
    }
    Ok(username.to_ascii_lowercase())
}

pub fn random_bytes<const LENGTH: usize>() -> Result<[u8; LENGTH], MarketplaceError> {
    let mut bytes = [0_u8; LENGTH];
    SystemRandom::new().fill(&mut bytes).map_err(|_| MarketplaceError::Unavailable)?;
    Ok(bytes)
}

pub fn token_hash(token: &str) -> Result<Vec<u8>, MarketplaceError> {
    if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()) {
        return Err(MarketplaceError::AuthenticationRequired);
    }
    Ok(Sha256::digest(token.as_bytes()).to_vec())
}

pub fn encode_hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 15) as usize] as char);
    }
    encoded
}

pub fn decode_hex(encoded: &str, expected_bytes: usize) -> Result<Vec<u8>, MarketplaceError> {
    if encoded.len() != expected_bytes * 2 { return Err(MarketplaceError::InvalidConfiguration); }
    encoded.as_bytes().chunks_exact(2).map(|pair| {
        let high = (pair[0] as char).to_digit(16).ok_or(MarketplaceError::InvalidConfiguration)?;
        let low = (pair[1] as char).to_digit(16).ok_or(MarketplaceError::InvalidConfiguration)?;
        Ok(((high << 4) | low) as u8)
    }).collect()
}

pub fn now() -> Result<i64, MarketplaceError> {
    let seconds = SystemTime::now().duration_since(UNIX_EPOCH)
        .map_err(|_| MarketplaceError::Unavailable)?.as_secs();
    i64::try_from(seconds).map_err(|_| MarketplaceError::Unavailable)
}
