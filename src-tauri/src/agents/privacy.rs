use super::models::{AgentEvent, AgentUsageRecord};
use crate::database::{Database, RequestUsage};
use regex::Regex;
use rusqlite::OptionalExtension;
use std::sync::OnceLock;

pub fn text(value: &str, limit: usize) -> String {
    static LABELS: OnceLock<Regex> = OnceLock::new();
    static URL: OnceLock<Regex> = OnceLock::new();
    let labels = LABELS.get_or_init(|| Regex::new(
        r#"(?i)(?:[a-z0-9_]*(?:api[_-]?key|access[_-]?token|refresh[_-]?token|client[_-]?secret|password|passwd|credential|authorization|cookie|private[_-]?key|secret|token)|\b(?:user(?:\s*id|name)?|uid|pwd))[\"']?\s*[:=]\s*(?:\"[^\"]*\"|'[^']*'|[^\s,;<>]+)"#
    ).expect("constant credential expression"));
    let urls = URL.get_or_init(|| Regex::new(r#"(?i)[a-z][a-z0-9+.-]*://[^\s<>\"']+"#).expect("constant URL expression"));
    let bounded: String = value.chars().take(16_384).collect();
    let redacted = urls.replace_all(&bounded, |capture: &regex::Captures<'_>| {
        if let Ok(mut parsed) = reqwest::Url::parse(&capture[0]) {
            let _ = parsed.set_username("");
            let _ = parsed.set_password(None);
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.to_string()
        } else { "[链接已隐藏]".into() }
    });
    let redacted = crate::privacy::redact_diagnostic(&redacted);
    let redacted = labels.replace_all(&redacted, "[凭据已隐藏]");
    let clean: String = redacted.chars().filter(|character| !character.is_control() || character.is_whitespace()).collect();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut result: String = clean.chars().take(limit).collect();
    if clean.chars().count() > limit { result.push('…'); }
    result
}

pub fn identifier(value: &str) -> Option<String> {
    let valid = !value.is_empty() && value.len() <= 240
        && value.bytes().all(|character| character.is_ascii_alphanumeric() || b"-_:./".contains(&character))
        && !value.starts_with("sk-") && !value.starts_with("eyJ");
    valid.then(|| value.to_owned())
}

pub fn scrub_event(event: &mut AgentEvent) {
    event.title = text(&event.title, 160);
    for value in [&mut event.summary, &mut event.name, &mut event.role, &mut event.model, &mut event.workspace].into_iter().flatten() {
        *value = text(value, 512);
    }
    if let Some(context) = &mut event.context { context.basis = text(&context.basis, 120); }
    if let Some(usage) = &mut event.usage {
        usage.basis = text(&usage.basis, 120);
        if let Some(source) = &mut usage.price_source { *source = text(source, 80); }
        if let Some(version) = &mut usage.price_version { *version = text(version, 160); }
    }
}

pub fn valid_usage(usage: &AgentUsageRecord) -> bool {
    const MAX_EXACT: i64 = 9_007_199_254_740_991;
    [usage.input_tokens, usage.output_tokens, usage.cache_read_tokens, usage.cache_write_tokens,
        usage.reasoning_tokens, usage.total_tokens].into_iter().all(|count| (0..=MAX_EXACT).contains(&count))
        && usage.cache_read_tokens.saturating_add(usage.cache_write_tokens) <= usage.input_tokens
        && usage.reasoning_tokens <= usage.output_tokens
        && usage.input_tokens.checked_add(usage.output_tokens) == Some(usage.total_tokens)
}

pub fn valid_timestamp(value: i64) -> bool {
    (-8_640_000_000_000_000..=8_640_000_000_000_000).contains(&value)
}

pub fn price_event(database: &Database, event: &mut AgentEvent) -> Result<(), String> {
    let Some(usage) = event.usage.as_mut() else { return Ok(()); };
    if !valid_usage(usage) { return Err("用量计数无效，未存储该记录".into()); }
    if usage.actual_cost_usd.is_some() { return Ok(()); }
    let Some(model) = event.model.as_ref() else { return Ok(()); };
    let arc = database.conn();
    let connection = arc.lock().map_err(|_| "database lock poisoned")?;
    let exact: Option<String> = connection.query_row("SELECT id FROM model_pricing WHERE id=?1", [model], |row| row.get(0))
        .optional().map_err(|error| error.to_string())?;
    if exact.is_none() { return Ok(()); }
    let request = RequestUsage {
        model: Some(model.clone()), input_tokens: Some(usage.input_tokens), output_tokens: Some(usage.output_tokens),
        cache_read_tokens: Some(usage.cache_read_tokens), cache_creation_tokens: Some(usage.cache_write_tokens),
        ..RequestUsage::default()
    };
    if let Some(price) = crate::model_pricing::estimate(&connection, &request)? {
        usage.estimated_cost_usd = price.total;
        usage.price_source = Some(price.source);
        usage.price_version = Some(price.version);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrenderable_timestamps_cannot_poison_persisted_history() {
        use crate::agents::{models::AgentContext, store::{EventStore, FileCursor}};
        let store = EventStore::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
        let cursor = FileCursor { source_id: "date-fixture".into(), root_key: "d".repeat(64), path: "rollout-dates.jsonl".into(), ..Default::default() };
        let valid = AgentEvent { id: "valid-date".into(), source_id: cursor.source_id.clone(), source_path: cursor.path.clone(),
            client_id: "codex".into(), agent_id: "codex:date-fixture".into(), timestamp: 1_794_307_716_000,
            observed_at: 1_794_307_716_001, kind: "agent_created".into(), ..Default::default() };
        for field in 0..4 {
            let mut invalid = valid.clone();
            match field {
                0 => invalid.timestamp = 9_007_199_254_740_991,
                1 => invalid.observed_at = i64::MAX,
                2 => invalid.lifecycle_at = Some(i64::MAX),
                _ => invalid.context = Some(AgentContext { used_tokens: 1, window_tokens: Some(2), measured_at: 8_640_000_000_000_001, basis: "fixture".into() }),
            }
            assert!(store.append(&[invalid], &cursor).is_err());
            assert_eq!(store.history().unwrap().event_count, 0);
            assert!(store.cursors().unwrap().is_empty());
        }
        assert_eq!(store.append(&[valid], &cursor).unwrap().len(), 1);
        assert_eq!(store.history().unwrap().latest_at, Some(1_794_307_716_000));
        for boundary in [-8_640_000_000_000_000, 0, 8_640_000_000_000_000] { assert!(valid_timestamp(boundary)); }
    }

    #[test]
    fn summaries_remove_credentials_before_truncation_and_do_not_keep_url_secrets() {
        let secret = "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789";
        let result = text(&format!("OPENAI_API_KEY={secret}; Bearer {secret}; https://user:pass@host.test/x?token=hidden#secret; password=private"), 512);
        for value in [secret, "user:pass", "token=hidden", "#secret", "password=private"] { assert!(!result.contains(value), "{result}"); }
        assert!(result.contains("host.test/x"));
    }

    #[test]
    fn cache_and_reasoning_are_subsets_not_extra_totals() {
        let mut usage = AgentUsageRecord { input_tokens: 100, output_tokens: 20, cache_read_tokens: 60, cache_write_tokens: 10, reasoning_tokens: 8, total_tokens: 120, ..Default::default() };
        assert!(valid_usage(&usage));
        usage.cache_read_tokens = 99;
        assert!(!valid_usage(&usage));
    }
}
