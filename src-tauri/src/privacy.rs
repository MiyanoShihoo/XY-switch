use crate::database::Database;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::OnceLock;
use tauri::State;

#[path = "privacy_stream.rs"]
pub(crate) mod response_stream;

const SETTINGS_KEY: &str = "privacy_settings";
const MAX_SCAN_BYTES: usize = 10 * 1024 * 1024;
pub(crate) const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024;
const MAX_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_MATCHES: usize = 4096;
const MAX_DEPTH: usize = 96;

fn default_true() -> bool { true }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PrivacySettings {
    #[serde(default = "default_true")]
    pub redact_personal_information: bool,
    #[serde(default = "default_true")]
    pub block_uninspected_attachments: bool,
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self { redact_personal_information: true, block_uninspected_attachments: true }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyReport {
    pub secret_redactions: u32,
    pub personal_redactions: u32,
    pub protected_fields: u32,
    pub uninspected_attachments: u32,
}

impl PrivacyReport {
    pub fn merge(&mut self, other: &Self) {
        self.secret_redactions = self.secret_redactions.saturating_add(other.secret_redactions);
        self.personal_redactions = self.personal_redactions.saturating_add(other.personal_redactions);
        self.protected_fields = self.protected_fields.saturating_add(other.protected_fields);
        self.uninspected_attachments = self.uninspected_attachments.max(other.uninspected_attachments);
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrivacyInspection {
    pub redacted: Value,
    pub report: PrivacyReport,
    pub blocked_code: Option<&'static str>,
}

#[derive(Clone)]
pub struct ResponsePrivacy {
    settings: PrivacySettings,
    credentials: Vec<zeroize::Zeroizing<String>>,
}

impl ResponsePrivacy {
    pub fn new(settings: PrivacySettings, credentials: &[&str]) -> Self {
        Self { settings, credentials: credentials.iter().map(|secret| zeroize::Zeroizing::new((*secret).to_owned())).collect() }
    }

    pub fn inspect(&self, value: Value) -> Result<PrivacyInspection, &'static str> {
        let credentials: Vec<&str> = self.credentials.iter().map(|secret| secret.as_str()).collect();
        inspect(value, &self.settings, &credentials)
    }

    pub fn safe_header(&self, value: &axum::http::HeaderValue) -> Option<axum::http::HeaderValue> {
        let original = value.to_str().ok()?;
        let checked = self.inspect(Value::String(original.to_owned())).ok()?;
        (checked.redacted.as_str() == Some(original) && original.len() <= 256).then(|| value.clone())
    }

    pub(crate) fn text_boundary_ready(&self, text: &str) -> bool {
        let Some(last) = text.chars().last() else { return true; };
        if !last.is_whitespace() && !"，。！？；".contains(last) { return false; }
        for credential in &self.credentials {
            for (end, _) in credential.char_indices().skip(1) {
                if text.ends_with(&credential[..end]) { return false; }
            }
        }
        let Ok(pending) = stream_pending_rules() else { return false; };
        if pending.iter().any(|rule| rule.is_match(text)) { return false; }
        for (start, _) in text.match_indices("-----BEGIN ") {
            let header = &text[start + "-----BEGIN ".len()..];
            let Some(end) = header.find("-----") else { return false; };
            let label = &header[..end];
            if label.ends_with("PRIVATE KEY") {
                let marker = format!("-----END {label}-----");
                if !header[end + 5..].contains(&marker) { return false; }
            }
        }
        true
    }
}

fn stream_pending_rules() -> Result<&'static Vec<Regex>, ()> {
    static PENDING: OnceLock<Result<Vec<Regex>, ()>> = OnceLock::new();
    PENDING.get_or_init(|| [
        r"(?i)\b(?:bearer|api(?:[_ -]+key)?|access(?:[_ -]+token)?|refresh(?:[_ -]+token)?|client(?:[_ -]+secret)?|password|passwd|authorization|cookie)[ _-]*\z",
        r#"(?i)\b(?:api[_ -]?key|access[_ -]?token|refresh[_ -]?token|client[_ -]?secret|password|passwd|authorization|cookie)["']?\s*[:=]\s*["']?[^\r\n"'<>;]*\z"#,
        r"[+0-9()][0-9() .-]*\z",
        r"-----BEGIN [A-Z ]*\z",
    ].into_iter().map(|pattern| Regex::new(pattern).map_err(|_| ())).collect()).as_ref().map_err(|_| ())
}

pub(crate) fn response_code(code: &'static str) -> &'static str {
    match code {
        "privacy_sensitive_structure" => "privacy_response_sensitive_structure",
        "privacy_attachment_uninspected" => "privacy_response_attachment_uninspected",
        "privacy_scan_limit" => "privacy_response_scan_limit",
        _ => "privacy_response_scan_failed",
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Category { Secret, Email, Phone }

impl Category {
    fn placeholder(self) -> &'static str {
        match self {
            Self::Secret => "[REDACTED:SECRET]",
            Self::Email => "[REDACTED:EMAIL]",
            Self::Phone => "[REDACTED:PHONE]",
        }
    }
}

struct Rule { expression: Regex, category: Category, capture: usize, boundary: bool }
struct Span { start: usize, end: usize, category: Category }

fn is_placeholder(text: &str) -> bool {
    matches!(text, "[REDACTED:SECRET]" | "[REDACTED:EMAIL]" | "[REDACTED:PHONE]")
}

fn rules() -> Result<&'static Vec<Rule>, &'static str> {
    static RULES: OnceLock<Result<Vec<Rule>, ()>> = OnceLock::new();
    RULES.get_or_init(|| {
        [
            (r"(?i)\b(?:sk-(?:proj-|ant-[a-z0-9]+-)?[a-z0-9_-]{16,}|gh[pousr]_[a-z0-9_]{20,}|github_pat_[a-z0-9_]{20,}|(?:AKIA|ASIA)[A-Z0-9]{16})\b", Category::Secret, 0, false),
            (r"\beyJ[a-zA-Z0-9_-]{8,}\.[a-zA-Z0-9_-]{8,}\.[a-zA-Z0-9_-]{8,}\b", Category::Secret, 0, false),
            (r"(?i)\bbearer\s+([a-z0-9._~+/=-]{8,})", Category::Secret, 1, false),
            (r#"(?i)\b(?:api[_ -]?key|access[_ -]?token|refresh[_ -]?token|client[_ -]?secret|password|passwd|authorization|cookie)["']?\s*[:=]\s*["']?([^\r\n"'<>;]+)"#, Category::Secret, 1, false),
            (r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?(?:-----END [A-Z ]*PRIVATE KEY-----|\z)", Category::Secret, 0, false),
            (r"(?i)[a-z0-9.!#$%&'*+/=?^_`{|}~-]+@[a-z0-9](?:[a-z0-9-]*[a-z0-9])?(?:\.[a-z0-9](?:[a-z0-9-]*[a-z0-9])?)+", Category::Email, 0, true),
            (r"(?:\+?86[ -]?)?1[3-9][0-9]{9}", Category::Phone, 0, true),
            (r"\+[1-9][0-9]{7,14}", Category::Phone, 0, true),
            (r"(?:\+1[ .-]?)?(?:\([2-9][0-9]{2}\)|[2-9][0-9]{2})[ .-][2-9][0-9]{2}[ .-][0-9]{4}", Category::Phone, 0, true),
        ].into_iter().map(|(pattern, category, capture, boundary)| {
            Regex::new(pattern).map(|expression| Rule { expression, category, capture, boundary }).map_err(|_| ())
        }).collect()
    }).as_ref().map_err(|_| "privacy_scan_failed")
}

fn bounded(text: &str, start: usize, end: usize) -> bool {
    let word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    !start.checked_sub(1).and_then(|offset| text.as_bytes().get(offset)).is_some_and(|byte| word(*byte))
        && !text.as_bytes().get(end).is_some_and(|byte| word(*byte))
}

struct Scanner<'a> {
    settings: &'a PrivacySettings,
    known_secrets: &'a [&'a str],
    report: PrivacyReport,
    scanned_bytes: usize,
    matches: usize,
}

impl Scanner<'_> {
    fn scan_text(&mut self, text: &str) -> Result<String, &'static str> {
        self.scanned_bytes = self.scanned_bytes.saturating_add(text.len());
        if self.scanned_bytes > MAX_SCAN_BYTES { return Err("privacy_scan_limit"); }
        let mut spans = Vec::new();
        for secret in self.known_secrets.iter().copied().filter(|secret| !secret.is_empty()) {
            for (start, _) in text.match_indices(secret) {
                let end = start + secret.len();
                if secret.len() < 8 && !bounded(text, start, end) { continue; }
                self.matches += 1;
                if self.matches > MAX_MATCHES { return Err("privacy_scan_limit"); }
                spans.push(Span { start, end, category: Category::Secret });
            }
        }
        for rule in rules()? {
            if rule.category != Category::Secret && !self.settings.redact_personal_information { continue; }
            for captures in rule.expression.captures_iter(text) {
                let Some(found) = captures.get(rule.capture) else { continue; };
                if is_placeholder(found.as_str().trim()) { continue; }
                if rule.boundary && !bounded(text, found.start(), found.end()) { continue; }
                self.matches += 1;
                if self.matches > MAX_MATCHES { return Err("privacy_scan_limit"); }
                spans.push(Span { start: found.start(), end: found.end(), category: rule.category });
            }
        }
        if spans.is_empty() { return Ok(text.to_owned()); }
        spans.sort_by_key(|span| (span.start, std::cmp::Reverse(span.end)));
        let mut merged: Vec<Span> = Vec::new();
        for span in spans {
            if let Some(previous) = merged.last_mut() && span.start < previous.end {
                previous.end = previous.end.max(span.end);
                if span.category == Category::Secret { previous.category = Category::Secret; }
            } else { merged.push(span); }
        }
        let mut output = String::with_capacity(text.len());
        let mut offset = 0;
        for span in merged {
            output.push_str(&text[offset..span.start]);
            output.push_str(span.category.placeholder());
            offset = span.end;
            match span.category {
                Category::Secret => self.report.secret_redactions += 1,
                Category::Email | Category::Phone => self.report.personal_redactions += 1,
            }
        }
        output.push_str(&text[offset..]);
        Ok(output)
    }

    fn visit(&mut self, value: &mut Value, protected: bool, schema: bool, depth: usize) -> Result<(), &'static str> {
        if depth > MAX_DEPTH { return Err("privacy_scan_limit"); }
        match value {
            Value::String(text) => {
                if protected && matches!(text.trim_start().as_bytes().first(), Some(b'{' | b'['))
                    && let Ok(mut decoded) = serde_json::from_str::<Value>(text)
                {
                    let before = self.report.clone();
                    self.visit(&mut decoded, true, false, depth + 1)?;
                    if before != self.report {
                        *text = serde_json::to_string(&decoded).map_err(|_| "privacy_scan_failed")?;
                        return Ok(());
                    }
                }
                let redacted = self.scan_text(text)?;
                if redacted != *text && protected { self.report.protected_fields += 1; }
                *text = redacted;
            }
            Value::Array(items) => {
                for item in items { self.visit(item, protected, schema, depth + 1)?; }
            }
            Value::Object(object) => {
                let item_type = object.get("type").and_then(Value::as_str).unwrap_or("");
                let attachment = !schema && (matches!(item_type, "image" | "image_url" | "input_image" | "input_file" | "file" | "document" | "audio" | "input_audio" | "video")
                    || ["inlineData", "fileData", "image_url", "file_data", "input_audio"].iter().any(|key| object.contains_key(*key)));
                if attachment {
                    self.report.uninspected_attachments += 1;
                    if self.settings.block_uninspected_attachments {
                        *value = Value::String("[WITHHELD:UNINSPECTED_ATTACHMENT]".to_owned());
                        return Ok(());
                    }
                }
                let tool_call = matches!(item_type, "tool_use" | "function_call" | "custom_tool_call")
                    || object.contains_key("functionCall");
                let original = std::mem::take(object);
                for (index, (key, mut child)) in original.into_iter().enumerate() {
                    let clean_key = self.scan_text(&key)?;
                    let renamed = clean_key != key;
                    if renamed { self.report.protected_fields += 1; }
                    let sensitive_value = sensitive_key(&key) && !(schema && safe_credential_schema(&child)) && match &child {
                        Value::Null => false,
                        Value::String(text) => !text.is_empty() && !is_placeholder(text),
                        Value::Array(items) => !items.is_empty(),
                        Value::Object(fields) => !fields.is_empty(),
                        _ => true,
                    };
                    if sensitive_value {
                        self.report.secret_redactions += 1;
                        self.report.protected_fields += 1;
                        child = Value::String(Category::Secret.placeholder().to_owned());
                    } else {
                        let protected_child = protected || tool_call || protected_key(&key)
                            || child.is_string() && !text_key(&key);
                        let child_schema = schema || matches!(key.as_str(), "parameters" | "input_schema" | "schema" | "json_schema");
                        self.visit(&mut child, protected_child, child_schema, depth + 1)?;
                    }
                    object.insert(if renamed { format!("__withheld_key_{index}") } else { key }, child);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn safe_credential_schema(value: &Value) -> bool {
    let Some(fields) = value.as_object() else { return false; };
    !fields.is_empty() && fields.keys().all(|key| matches!(key.as_str(),
        "type" | "title" | "description" | "$ref" | "$id" | "$schema" | "pattern" | "format"
        | "minLength" | "maxLength" | "nullable" | "readOnly" | "writeOnly" | "deprecated"))
}

fn sensitive_key(key: &str) -> bool {
    let normalized: String = key.chars().filter(|character| character.is_ascii_alphanumeric()).flat_map(char::to_lowercase).collect();
    matches!(normalized.as_str(), "apikey" | "password" | "passwd" | "secret" | "clientsecret" | "accesstoken" | "refreshtoken" | "idtoken" | "authorization" | "cookie" | "setcookie" | "privatekey" | "sessiontoken")
}

fn text_key(key: &str) -> bool {
    matches!(key, "text" | "content" | "input" | "output" | "prompt" | "instructions" | "system" | "response" | "description" | "thinking" | "reasoning" | "reasoning_content" | "refusal" | "message")
}

fn protected_key(key: &str) -> bool {
    matches!(key, "tools" | "functions" | "tool_choice" | "tool_calls" | "function" | "functionCall" | "arguments" | "parameters" | "input_schema" | "schema" | "response_format" | "format" | "metadata" | "source" | "inlineData" | "fileData")
}

pub fn inspect(mut payload: Value, settings: &PrivacySettings, known_secrets: &[&str]) -> Result<PrivacyInspection, &'static str> {
    let mut scanner = Scanner { settings, known_secrets, report: PrivacyReport::default(), scanned_bytes: 0, matches: 0 };
    scanner.visit(&mut payload, false, false, 0)?;
    let blocked_code = if scanner.report.protected_fields > 0 {
        Some("privacy_sensitive_structure")
    } else if settings.block_uninspected_attachments && scanner.report.uninspected_attachments > 0 {
        Some("privacy_attachment_uninspected")
    } else { None };
    Ok(PrivacyInspection { redacted: payload, report: scanner.report, blocked_code })
}

pub fn inspect_transport(
    url: &reqwest::Url,
    headers: &std::collections::BTreeMap<String, String>,
    user_agent: &str,
    settings: &PrivacySettings,
    known_secrets: &[&str],
) -> Result<PrivacyInspection, &'static str> {
    let headers: serde_json::Map<String, Value> = headers.iter().filter(|(name, _)|
        !["authorization", "x-api-key", "x-goog-api-key"].iter().any(|reserved| name.eq_ignore_ascii_case(reserved)))
        .map(|(name, value)| (name.clone(), Value::String(value.clone()))).collect();
    let metadata = serde_json::json!({"metadata":{"endpoint":url.as_str(), "headers":headers, "user_agent":user_agent}});
    let mut checked = inspect(metadata, settings, known_secrets)?;
    if !url.username().is_empty() || url.password().is_some()
        || url.query_pairs().any(|(key, _)| sensitive_key(&key) || matches!(key.to_ascii_lowercase().as_str(), "key" | "token" | "sig" | "signature"))
    {
        checked.report.secret_redactions = checked.report.secret_redactions.saturating_add(1);
        checked.report.protected_fields = checked.report.protected_fields.saturating_add(1);
        checked.blocked_code = Some("privacy_sensitive_structure");
        checked.redacted = Value::Null;
    }
    Ok(checked)
}

pub fn load(database: &Database) -> Result<PrivacySettings, String> {
    let stored = database.get_setting(SETTINGS_KEY).map_err(|_| "Privacy settings cannot be read; sending is blocked.".to_owned())?;
    match stored {
        Some(value) => serde_json::from_str(&value).map_err(|_| "Privacy settings are invalid; sending is blocked.".to_owned()),
        None => Ok(PrivacySettings::default()),
    }
}

pub(crate) fn load_on(connection: &rusqlite::Connection) -> Result<PrivacySettings, String> {
    use rusqlite::OptionalExtension;
    let stored: Option<String> = connection.query_row(
        "SELECT value FROM app_settings WHERE key=?1", [SETTINGS_KEY], |row| row.get(0),
    ).optional().map_err(|_| "Privacy settings cannot be read; sending is blocked.".to_owned())?;
    match stored {
        Some(value) => serde_json::from_str(&value).map_err(|_| "Privacy settings are invalid; sending is blocked.".to_owned()),
        None => Ok(PrivacySettings::default()),
    }
}

pub fn save(database: &Database, settings: &PrivacySettings) -> Result<(), String> {
    let serialized = serde_json::to_string(settings).map_err(|_| "Privacy settings cannot be serialized.".to_owned())?;
    database.set_setting(SETTINGS_KEY, &serialized)
}

pub fn redact_diagnostic(message: &str) -> String {
    let settings = PrivacySettings::default();
    let mut scanner = Scanner { settings: &settings, known_secrets: &[], report: PrivacyReport::default(), scanned_bytes: 0, matches: 0 };
    scanner.scan_text(message).unwrap_or_else(|_| "Details withheld because privacy checking failed.".to_owned())
}

#[tauri::command]
pub fn get_privacy_settings(db: State<'_, Database>) -> Result<PrivacySettings, String> { load(&db) }

#[tauri::command]
pub fn save_privacy_settings(settings: PrivacySettings, db: State<'_, Database>) -> Result<PrivacySettings, String> {
    save(&db, &settings)?;
    load(&db)
}

#[tauri::command]
pub fn preview_privacy(text: String, settings: Option<PrivacySettings>, db: State<'_, Database>) -> Result<PrivacyInspection, String> {
    if text.len() > MAX_PREVIEW_BYTES { return Err("Privacy preview is limited to 256 KiB.".to_owned()); }
    let settings = match settings { Some(settings) => settings, None => load(&db)? };
    let payload = serde_json::from_str(&text).unwrap_or(Value::String(text));
    inspect(payload, &settings, &[]).map_err(|_| "Privacy preview could not safely inspect this input.".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn text_redacts_credentials_and_personal_information_with_unicode() {
        let token = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
        let checked = inspect(json!({"model":"example","input":format!("你好 user@example.com 电话13800138000 key={token}")}), &PrivacySettings::default(), &[]).unwrap();
        let sent = checked.redacted.to_string();
        assert!(!sent.contains(token));
        assert!(!sent.contains("user@example.com"));
        assert!(!sent.contains("13800138000"));
        assert!(sent.contains("你好"));
        assert_eq!(checked.report.secret_redactions, 1);
        assert_eq!(checked.report.personal_redactions, 2);
        assert_eq!(checked.blocked_code, None);
    }

    #[test]
    fn provider_secret_is_redacted_even_when_it_has_no_recognizable_prefix() {
        let checked = inspect(json!({"input":"Use opaque-value-for-provider here"}), &PrivacySettings::default(), &["opaque-value-for-provider"]).unwrap();
        assert!(!checked.redacted.to_string().contains("opaque-value-for-provider"));
        assert_eq!(checked.report.secret_redactions, 1);
    }

    #[test]
    fn malformed_placeholders_and_structured_credentials_cannot_bypass_scanning() {
        for value in [json!("[REDACTED:SECRET] still-private"), json!(["opaque-private"]), json!({"value":"opaque-private"}), json!(123456)] {
            let checked = inspect(json!({"password": value}), &PrivacySettings::default(), &[]).unwrap();
            assert_eq!(checked.blocked_code, Some("privacy_sensitive_structure"));
            assert_eq!(checked.redacted["password"], "[REDACTED:SECRET]");
        }
        let schema = json!({"tools":[{"type":"function", "name":"inspect", "parameters":{"type":"object", "properties":{"image_url":{"type":"string"}, "password":{"type":"string"}}}}]});
        assert_eq!(inspect(schema.clone(), &PrivacySettings::default(), &[]).unwrap().redacted, schema);
    }

    #[test]
    fn safe_tool_names_ids_schemas_and_arguments_are_unchanged() {
        let request = json!({"model":"m","input":[{"type":"function_call","id":"item-1","call_id":"call-1","name":"read_file","arguments":"{\"path\":\"C:/project/main.rs\"}"},{"type":"function_call_output","call_id":"call-1","output":"ordinary result"}],"tools":[{"type":"function","name":"read_file","parameters":{"type":"object","properties":{"password":{"type":"string"}}}}]});
        let checked = inspect(request.clone(), &PrivacySettings::default(), &[]).unwrap();
        assert_eq!(checked.redacted, request);
        assert_eq!(checked.blocked_code, None);
    }

    #[test]
    fn serialized_tool_parameters_and_schema_defaults_do_not_bypass_credentials() {
        for arguments in [r#"{"password":"opaque-private"}"#, r#"{"pass\u0077ord":"opaque-private"}"#] {
            let request = json!({"input":[{"type":"function_call","name":"inspect","call_id":"call-1","arguments":arguments}]});
            let checked = inspect(request, &PrivacySettings::default(), &[]).unwrap();
            assert_eq!(checked.blocked_code, Some("privacy_sensitive_structure"));
            assert!(!checked.redacted.to_string().contains("opaque-private"));
        }
        for value in [json!({"parameters":{"password":"opaque-private"}}), json!({"parameters":{"properties":{"password":{"type":"string","default":"opaque-private"}}}})] {
            let checked = inspect(value, &PrivacySettings::default(), &[]).unwrap();
            assert_eq!(checked.blocked_code, Some("privacy_sensitive_structure"));
            assert!(!checked.redacted.to_string().contains("opaque-private"));
        }
        let checked = inspect(json!({"input":r#"{"password":"opaque-private"}"#}), &PrivacySettings::default(), &[]).unwrap();
        assert!(!checked.redacted.to_string().contains("opaque-private"));
    }

    #[test]
    fn sensitive_tool_arguments_are_blocked_and_preview_withholds_the_original() {
        let request = json!({"input":[{"type":"tool_use","id":"call-1","name":"send_mail","input":{"to":"person@example.com"}}]});
        let checked = inspect(request, &PrivacySettings::default(), &[]).unwrap();
        assert_eq!(checked.blocked_code, Some("privacy_sensitive_structure"));
        assert!(!checked.redacted.to_string().contains("person@example.com"));
        assert_eq!(checked.redacted["input"][0]["id"], "call-1");
    }

    #[test]
    fn attachments_require_a_deliberate_policy_choice() {
        let request = json!({"input":[{"type":"input_image","image_url":"data:image/png;base64,private-image-data"}]});
        let blocked = inspect(request.clone(), &PrivacySettings::default(), &[]).unwrap();
        assert_eq!(blocked.blocked_code, Some("privacy_attachment_uninspected"));
        assert!(!blocked.redacted.to_string().contains("private-image-data"));
        let settings = PrivacySettings { block_uninspected_attachments: false, ..Default::default() };
        let allowed = inspect(request.clone(), &settings, &[]).unwrap();
        assert_eq!(allowed.redacted, request);
        assert_eq!(allowed.blocked_code, None);
        assert_eq!(allowed.report.uninspected_attachments, 1);
    }

    #[test]
    fn personal_redaction_choice_cannot_disable_secret_protection() {
        let settings = PrivacySettings { redact_personal_information: false, ..Default::default() };
        let checked = inspect(json!({"input":"user@example.com password: actual secret"}), &settings, &[]).unwrap();
        let sent = checked.redacted.to_string();
        assert!(sent.contains("user@example.com"));
        assert!(!sent.contains("actual secret"));
        assert_eq!(checked.report.personal_redactions, 0);
        assert_eq!(checked.report.secret_redactions, 1);
    }

    #[test]
    fn redaction_is_idempotent_and_limits_fail_closed() {
        let first = inspect(json!({"input":"user@example.com password: hidden"}), &PrivacySettings::default(), &[]).unwrap();
        let again = inspect(first.redacted.clone(), &PrivacySettings::default(), &[]).unwrap();
        assert_eq!(first.redacted, again.redacted);
        assert_eq!(again.report, PrivacyReport::default());
        assert_eq!(inspect(Value::String("x".repeat(MAX_SCAN_BYTES + 1)), &PrivacySettings::default(), &[]).unwrap_err(), "privacy_scan_limit");
        assert_eq!(inspect(Value::String("a@b.example ".repeat(MAX_MATCHES + 1)), &PrivacySettings::default(), &[]).unwrap_err(), "privacy_scan_limit");
    }

    #[test]
    fn transport_credentials_and_personal_metadata_are_blocked_without_rewriting_routes() {
        for url in ["https://user:opaque-secret@example.test/v1", "https://example.test/v1?key=opaque-secret", "https://example.test/v1?email=user@example.test"] {
            let checked = inspect_transport(&reqwest::Url::parse(url).unwrap(), &Default::default(), "", &PrivacySettings::default(), &[]).unwrap();
            assert_eq!(checked.blocked_code, Some("privacy_sensitive_structure"));
        }
        let headers = [("x-session".into(), "opaque-provider-key".into())].into_iter().collect();
        let checked = inspect_transport(&reqwest::Url::parse("https://example.test/v1?api-version=2024-01-01").unwrap(), &headers, "", &PrivacySettings::default(), &["opaque-provider-key"]).unwrap();
        assert_eq!(checked.blocked_code, Some("privacy_sensitive_structure"));
        let context = ResponsePrivacy::new(PrivacySettings::default(), &["opaque-provider-key"]);
        for header in ["opaque-provider-key", "user@example.test", "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789"] {
            assert!(context.safe_header(&header.parse().unwrap()).is_none());
        }
        assert_eq!(context.safe_header(&"req-safe-1".parse().unwrap()).unwrap(), "req-safe-1");
    }

    #[test]
    fn saved_settings_survive_reopen_and_corruption_never_disables_protection() {
        let path = std::env::temp_dir().join(format!("xy-privacy-{}.db", uuid::Uuid::new_v4()));
        let database = Database::open(&path).unwrap();
        assert_eq!(load(&database).unwrap(), PrivacySettings::default());
        let settings = PrivacySettings { redact_personal_information: false, ..Default::default() };
        save(&database, &settings).unwrap();
        drop(database);
        let database = Database::open(&path).unwrap();
        assert_eq!(load(&database).unwrap(), settings);
        database.set_setting(SETTINGS_KEY, "{not-json}").unwrap();
        assert!(load(&database).is_err());
        drop(database);
        let _ = std::fs::remove_file(path);
    }
}
