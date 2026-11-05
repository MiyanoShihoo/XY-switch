use super::{MarketplaceError, MAX_NOTE_BYTES, MAX_UPLOAD_BYTES};
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

const MAX_DEPTH: usize = 48;
const MAX_NODES: usize = 100_000;

pub fn inspect_display_name(name: &str) -> Result<(), MarketplaceError> {
    if name.trim() != name || name.is_empty() || name.chars().count() > 80
        || name.chars().any(char::is_control) {
        return Err(MarketplaceError::InvalidRequest);
    }
    inspect_text(name)
}

pub fn inspect_note(note: &str) -> Result<(), MarketplaceError> {
    if note.len() > MAX_NOTE_BYTES { return Err(MarketplaceError::InvalidRequest); }
    inspect_text(note)
}

pub fn inspect_note_with_secrets(note: &str, secrets: &[&str]) -> Result<(), MarketplaceError> {
    if note.len() > MAX_NOTE_BYTES { return Err(MarketplaceError::InvalidRequest); }
    inspect_value(&Value::String(note.to_owned()), secrets)
}

pub fn inspect_text(text: &str) -> Result<(), MarketplaceError> {
    inspect_value(&Value::String(text.to_owned()), &[])
}

pub fn inspect_package(value: &Value) -> Result<(), MarketplaceError> {
    inspect_package_with_secrets(value, &[])
}

pub fn inspect_package_with_secrets(value: &Value, secrets: &[&str]) -> Result<(), MarketplaceError> {
    let files = value.get("files").and_then(Value::as_object).ok_or(MarketplaceError::InvalidPackage)?;
    if files.is_empty() || files.len() > 512 { return Err(MarketplaceError::InvalidPackage); }
    for (path, content) in files {
        let text = content.as_str().ok_or(MarketplaceError::InvalidPackage)?;
        if text.len() > 1024 * 1024 { return Err(MarketplaceError::InvalidPackage); }
        let lowered = path.replace('\\', "/").to_ascii_lowercase();
        let filename = lowered.rsplit('/').next().unwrap_or("");
        if matches!(filename, ".env" | "auth.json" | "credentials" | "credentials.json"
            | ".npmrc" | ".netrc" | "id_rsa" | "id_ed25519" | "sessions.json" | "history.jsonl")
            || [".pem", ".key", ".p12", ".pfx", ".png", ".jpg", ".jpeg", ".gif", ".webp",
                ".pdf", ".zip", ".gz", ".zst", ".exe", ".dll", ".so", ".wasm"].iter()
                .any(|extension| lowered.ends_with(extension)) {
            return Err(MarketplaceError::PrivacyRejected);
        }
    }
    inspect_value(value, secrets)
}

fn inspect_value(value: &Value, secrets: &[&str]) -> Result<(), MarketplaceError> {
    let encoded = serde_json::to_vec(value).map_err(|_| MarketplaceError::PrivacyRejected)?;
    if encoded.len() > MAX_UPLOAD_BYTES { return Err(MarketplaceError::PrivacyRejected); }
    let mut inspection_value = value.clone();
    if let Some(dependencies) = inspection_value.get_mut("manifest")
        .and_then(|manifest| manifest.get_mut("dependencies")).and_then(Value::as_array_mut) {
        for dependency in dependencies {
            if let Some(text) = dependency.as_str() {
                if secrets.iter().any(|secret| !secret.is_empty() && text.contains(secret)) {
                    return Err(MarketplaceError::PrivacyRejected);
                }
                if let Ok((id, version)) = crate::plugins::validation::parse_dependency(text) {
                    *dependency = Value::Array(vec![Value::String(id.to_owned()), Value::String(version.to_owned())]);
                }
            }
        }
    }
    let inspection = crate::privacy::inspect(inspection_value.clone(), &crate::privacy::PrivacySettings::default(), secrets)
        .map_err(|_| MarketplaceError::PrivacyRejected)?;
    if inspection.blocked_code.is_some() || inspection.redacted != inspection_value {
        return Err(MarketplaceError::PrivacyRejected);
    }
    let mut visitor = Visitor { nodes: 0, joined: String::new(), secrets, encoding_depth: 0 };
    visitor.visit(&inspection_value, 0)?;
    scan_text_with_encoding_policy(&visitor.joined, secrets, 0, false)
}

struct Visitor<'a> {
    nodes: usize,
    joined: String,
    secrets: &'a [&'a str],
    encoding_depth: usize,
}

impl Visitor<'_> {
    fn visit(&mut self, value: &Value, depth: usize) -> Result<(), MarketplaceError> {
        self.nodes += 1;
        if depth > MAX_DEPTH || self.nodes > MAX_NODES { return Err(MarketplaceError::PrivacyRejected); }
        match value {
            Value::String(text) => self.text(text)?,
            Value::Array(items) => {
                for item in items { self.visit(item, depth + 1)?; }
            }
            Value::Object(fields) => {
                for (key, child) in fields {
                    self.text(key)?;
                    if sensitive_key(key) && contains_concrete_secret(child) {
                        return Err(MarketplaceError::PrivacyRejected);
                    }
                    self.visit(child, depth + 1)?;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn text(&mut self, text: &str) -> Result<(), MarketplaceError> {
        scan_text(text, self.secrets, self.encoding_depth)?;
        if self.joined.len().saturating_add(text.len()) > MAX_UPLOAD_BYTES {
            return Err(MarketplaceError::PrivacyRejected);
        }
        self.joined.push_str(text);
        Ok(())
    }
}

fn sensitive_key(key: &str) -> bool {
    let normalized: String = key.chars().filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase).collect();
    matches!(normalized.as_str(), "apikey" | "password" | "passwd" | "clientsecret" | "secret"
        | "accesstoken" | "refreshtoken" | "idtoken" | "authorization" | "cookie" | "setcookie"
        | "privatekey" | "sessiontoken" | "bearertoken")
}

fn contains_concrete_secret(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !placeholder(text),
        Value::Array(items) => !items.is_empty(),
        Value::Object(fields) => !fields.is_empty() && !fields.keys().all(|key| matches!(key.as_str(),
            "type" | "title" | "description" | "$ref" | "$id" | "$schema" | "pattern" | "format"
            | "minLength" | "maxLength" | "nullable" | "readOnly" | "writeOnly" | "deprecated")),
        _ => true,
    }
}

fn placeholder(text: &str) -> bool {
    let text = text.trim().trim_matches(['\'', '"']);
    text.is_empty() || matches!(text, "null" | "None" | "undefined")
        || text.starts_with("${") && text.ends_with('}')
        || text.starts_with("{{") && text.ends_with("}}")
        || text.starts_with('<') && text.ends_with('>')
        || text.starts_with("process.env.") || text.starts_with("os.environ[")
        || text.starts_with("std::env::var(") || text.starts_with("$env:")
}

fn rules() -> Result<&'static Vec<Regex>, MarketplaceError> {
    static RULES: OnceLock<Result<Vec<Regex>, ()>> = OnceLock::new();
    RULES.get_or_init(|| [
        r"(?i)-----BEGIN [A-Z ]*PRIVATE KEY-----",
        r"\b(?:sk-(?:proj-|ant-)?[A-Za-z0-9_-]{12,}|gh[pousr]_[A-Za-z0-9]{20,}|github_pat_[A-Za-z0-9_]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16}|xox[baprs]-[A-Za-z0-9-]{10,})\b",
        r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\b",
        r"(?i)\bbearer\s+[A-Za-z0-9._~+/-]{8,}",
        r"(?i)\b[A-Z0-9.!#$%&'*+/=?^_`{|}~-]+@[A-Z0-9](?:[A-Z0-9-]*[A-Z0-9])?(?:\.[A-Z0-9](?:[A-Z0-9-]*[A-Z0-9])?)+\b",
        r#"(?i)(?:[A-Z]:[\\/]Users[\\/][^\\/\s"']+|/(?:Users|home)/[^/\s"']+/)"#,
        r"\b1[3-9][0-9]{9}\b",
        r"\+[1-9][0-9](?:[ .()-]?[0-9]){7,13}\b",
        r"\b[1-9][0-9]{16}[0-9Xx]\b",
        r"(?i)(?:data:[^\s,;]+;base64,|-----BEGIN (?:CERTIFICATE|OPENSSH PRIVATE KEY)-----)",
    ].into_iter().map(|pattern| Regex::new(pattern).map_err(|_| ())).collect())
        .as_ref().map_err(|_| MarketplaceError::PrivacyRejected)
}

fn assignments() -> Result<&'static Regex, MarketplaceError> {
    static RULE: OnceLock<Result<Regex, ()>> = OnceLock::new();
    RULE.get_or_init(|| Regex::new(
        r#"(?i)\b(?:api[_ -]?key|access[_ -]?token|refresh[_ -]?token|client[_ -]?secret|password|passwd|secret|authorization|cookie|session[_ -]?token)["']?\s*[:=]\s*(?:["']([^"'\r\n]{1,256})["']|([^\s,;})\]]{1,256}))"#,
    ).map_err(|_| ())).as_ref().map_err(|_| MarketplaceError::PrivacyRejected)
}

fn encoded_chunks() -> Result<&'static Regex, MarketplaceError> {
    static RULE: OnceLock<Result<Regex, ()>> = OnceLock::new();
    RULE.get_or_init(|| Regex::new(r"[A-Za-z0-9+/_-]{24,}={0,2}").map_err(|_| ()))
        .as_ref().map_err(|_| MarketplaceError::PrivacyRejected)
}

fn scan_text(text: &str, secrets: &[&str], depth: usize) -> Result<(), MarketplaceError> {
    scan_text_with_encoding_policy(text, secrets, depth, true)
}

fn scan_text_with_encoding_policy(text: &str, secrets: &[&str], depth: usize, reject_opaque_encoding: bool) -> Result<(), MarketplaceError> {
    if depth > 4 || text.chars().any(|character| character.is_control() && !matches!(character, '\n' | '\r' | '\t'))
        || secrets.iter().any(|secret| !secret.is_empty() && text.contains(secret))
        || rules()?.iter().any(|rule| rule.is_match(text)) {
        return Err(MarketplaceError::PrivacyRejected);
    }
    for captures in assignments()?.captures_iter(text) {
        let content = captures.get(1).or_else(|| captures.get(2))
            .ok_or(MarketplaceError::PrivacyRejected)?.as_str();
        if !placeholder(content) { return Err(MarketplaceError::PrivacyRejected); }
    }
    let unescaped = unescape(text)?;
    if unescaped != text { scan_text_with_encoding_policy(&unescaped, secrets, depth + 1, reject_opaque_encoding)?; }
    if (text.starts_with('{') && text.ends_with('}')) || (text.starts_with('[') && text.ends_with(']')) {
        if let Ok(value) = serde_json::from_str::<Value>(text) {
            if depth >= 4 { return Err(MarketplaceError::PrivacyRejected); }
            let mut visitor = Visitor { nodes: 0, joined: String::new(), secrets, encoding_depth: depth+1 };
            visitor.visit(&value, 0)?;
            scan_text_with_encoding_policy(&visitor.joined, secrets, depth+1, false)?;
        }
    }
    for chunk in encoded_chunks()?.find_iter(text) {
        let encoded = chunk.as_str();
        if encoded.len() % 2 == 0 && encoded.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            let decoded = super::auth::decode_hex(encoded,encoded.len()/2).map_err(|_| MarketplaceError::PrivacyRejected)?;
            if let Ok(decoded) = std::str::from_utf8(&decoded) {
                if !decoded.chars().any(char::is_control) { scan_text_with_encoding_policy(decoded,secrets,depth+1,reject_opaque_encoding)?; }
            }
        }
        if let Some(decoded) = decode_base64(encoded) {
            if let Ok(decoded_text) = std::str::from_utf8(&decoded) {
                if decoded_text != encoded && decoded_text.chars().all(|character|
                    !character.is_control() || matches!(character, '\n' | '\r' | '\t')) {
                    scan_text_with_encoding_policy(decoded_text, secrets, depth + 1, reject_opaque_encoding)?;
                } else if reject_opaque_encoding && encoded.len() >= 96 {
                    return Err(MarketplaceError::PrivacyRejected);
                }
            } else if reject_opaque_encoding && encoded.len() >= 96 {
                return Err(MarketplaceError::PrivacyRejected);
            }
        } else if reject_opaque_encoding && encoded.len() >= 256 {
            return Err(MarketplaceError::PrivacyRejected);
        }
    }
    Ok(())
}

fn unescape(text: &str) -> Result<String, MarketplaceError> {
    let bytes = text.as_bytes();
    let mut output = Vec::with_capacity(bytes.len());
    let mut position = 0;
    while position < bytes.len() {
        let remaining = &bytes[position..];
        let (digits, prefix) = if remaining.starts_with(b"\\u") && remaining.len() >= 6 {
            (4, 2)
        } else if remaining.starts_with(b"\\x") && remaining.len() >= 4 {
            (2, 2)
        } else if remaining.starts_with(b"%") && remaining.len() >= 3 {
            (2, 1)
        } else { (0, 0) };
        if digits > 0 && remaining[prefix..prefix + digits].iter().all(u8::is_ascii_hexdigit) {
            let number = std::str::from_utf8(&remaining[prefix..prefix + digits])
                .ok().and_then(|encoded| u32::from_str_radix(encoded, 16).ok())
                .ok_or(MarketplaceError::PrivacyRejected)?;
            if digits == 4 {
                let character = char::from_u32(number).ok_or(MarketplaceError::PrivacyRejected)?;
                let mut encoded = [0_u8; 4];
                output.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
            } else {
                output.push(number as u8);
            }
            position += prefix + digits;
        } else {
            output.push(bytes[position]);
            position += 1;
        }
    }
    String::from_utf8(output).map_err(|_| MarketplaceError::PrivacyRejected)
}

fn decode_base64(encoded: &str) -> Option<Vec<u8>> {
    if encoded.len() > MAX_UPLOAD_BYTES || encoded.len() % 4 == 1 { return None; }
    let mut output = Vec::with_capacity(encoded.len() * 3 / 4);
    let mut pending = 0_u32;
    let mut bits = 0;
    let mut padding = false;
    for byte in encoded.bytes() {
        if byte == b'=' { padding = true; continue; }
        if padding { return None; }
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        pending = (pending << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            output.push((pending >> bits) as u8);
            pending &= (1 << bits) - 1;
        }
    }
    if pending != 0 { return None; }
    Some(output)
}
