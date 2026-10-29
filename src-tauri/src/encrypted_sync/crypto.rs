use ring::{
    aead, pbkdf2,
    rand::{SecureRandom, SystemRandom},
};
use std::num::NonZeroU32;
use zeroize::Zeroizing;

pub const MAX_PLAINTEXT: usize = 8 * 1024 * 1024;
const MAGIC: &[u8; 8] = b"XYSYNC01";
const HEADER: usize = 8 + 32 + 12;
pub const MAX_FILE: usize = MAX_PLAINTEXT + HEADER + 16;
const ITERATIONS: u32 = 600_000;

pub fn validate_passphrase(passphrase: &str) -> Result<(), String> {
    if passphrase.chars().count() < 12 || passphrase.len() > 1024 {
        return Err(
            "Use a passphrase of at least 12 characters and at most 1024 UTF-8 bytes".into(),
        );
    }
    Ok(())
}
fn key(passphrase: &str, salt: &[u8]) -> Result<aead::LessSafeKey, String> {
    validate_passphrase(passphrase)?;
    let mut derived = Zeroizing::new([0u8; 32]);
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        NonZeroU32::new(ITERATIONS).unwrap(),
        salt,
        passphrase.as_bytes(),
        derived.as_mut(),
    );
    aead::UnboundKey::new(&aead::AES_256_GCM, derived.as_ref())
        .map(aead::LessSafeKey::new)
        .map_err(|_| "Encryption key initialization failed".into())
}
pub fn encrypt(plaintext: &[u8], passphrase: &str) -> Result<Vec<u8>, String> {
    if plaintext.len() > MAX_PLAINTEXT {
        return Err("Sync payload exceeds 8 MiB".into());
    }
    validate_passphrase(passphrase)?;
    let mut header = [0u8; HEADER];
    header[..8].copy_from_slice(MAGIC);
    SystemRandom::new()
        .fill(&mut header[8..])
        .map_err(|_| "Secure randomness unavailable")?;
    let nonce: [u8; 12] = header[40..].try_into().unwrap();
    let key = key(passphrase, &header[8..40])?;
    let mut content = Zeroizing::new(plaintext.to_vec());
    key.seal_in_place_append_tag(
        aead::Nonce::assume_unique_for_key(nonce),
        aead::Aad::from(header.as_slice()),
        &mut *content,
    )
    .map_err(|_| "Encryption failed")?;
    let mut file = header.to_vec();
    file.extend_from_slice(&content);
    Ok(file)
}
pub fn decrypt(file: &[u8], passphrase: &str) -> Result<Zeroizing<Vec<u8>>, String> {
    if file.len() < HEADER + 16 || file.len() > MAX_FILE || &file[..8] != MAGIC {
        return Err("Unsupported or oversized encrypted sync file".into());
    }
    let key = key(passphrase, &file[8..40])?;
    let nonce: [u8; 12] = file[40..HEADER].try_into().unwrap();
    let mut plaintext = Zeroizing::new(file[HEADER..].to_vec());
    let size = key
        .open_in_place(
            aead::Nonce::assume_unique_for_key(nonce),
            aead::Aad::from(&file[..HEADER]),
            &mut plaintext,
        )
        .map_err(|_| "Wrong passphrase or damaged encrypted file")?
        .len();
    plaintext.truncate(size);
    Ok(plaintext)
}
