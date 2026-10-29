use crate::database::Database;
use rusqlite::params;
use serde::Serialize;
use std::{
    fs::{self, File},
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

#[derive(Debug, Default, Clone, Serialize)]
pub struct SyncResult {
    pub files_scanned: usize,
    pub events_imported: usize,
    pub events_skipped: usize,
    pub invalid_events: usize,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_read_tokens: i64,
}

/// Computes days since Unix epoch (1970-01-01) for a Gregorian calendar date.
/// Howard Hinnant's algorithm.
fn days_from_civil(mut y: i64, m: i64, d: i64) -> i64 {
    y -= if m <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let m_adj = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * m_adj + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// Parses an RFC3339 / ISO 8601 timestamp string (e.g. "2026-09-22T12:42:31.846Z")
/// into Unix timestamp in milliseconds.
pub fn parse_iso_timestamp_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 19 || bytes.get(4) != Some(&b'-') || bytes.get(7) != Some(&b'-')
        || !matches!(bytes.get(10), Some(b'T' | b't' | b' '))
        || bytes.get(13) != Some(&b':') || bytes.get(16) != Some(&b':') {
        return None;
    }
    let number = |start, end| -> Option<i64> {
        let part = s.get(start..end)?;
        if !part.bytes().all(|b| b.is_ascii_digit()) { return None; }
        part.parse().ok()
    };
    let year = number(0, 4)?;
    let month = number(5, 7)?;
    let day = number(8, 10)?;
    let hour = number(11, 13)?;
    let minute = number(14, 16)?;
    let second = number(17, 19)?;
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let month_days = match month { 2 if leap => 29, 2 => 28, 4 | 6 | 9 | 11 => 30, 1 | 3 | 5 | 7 | 8 | 10 | 12 => 31, _ => return None };
    if day < 1 || day > month_days || hour > 23 || minute > 59 || second > 59 { return None; }

    let mut rest = s.get(19..)?;
    let mut millis = 0;
    if let Some(fraction) = rest.strip_prefix('.') {
        let end = fraction.find(|c: char| !c.is_ascii_digit()).unwrap_or(fraction.len());
        if end == 0 { return None; }
        for index in 0..3 {
            millis = millis * 10 + fraction.as_bytes().get(index).filter(|_| index < end).map_or(0, |b| i64::from(b - b'0'));
        }
        rest = fraction.get(end..)?;
    }
    // Preserve naive ISO timestamps as UTC, but never accept a loose, unbounded
    // or non-ASCII timezone prefix from a client-controlled session log.
    let offset = if matches!(rest, "" | "Z" | "z") {
        0
    } else {
        let sign = match rest.as_bytes().first() { Some(b'+') => 1, Some(b'-') => -1, _ => return None };
        let zone = rest.get(1..)?;
        let (hours, minutes) = match zone.len() {
            2 => (zone, "00"),
            4 => (zone.get(..2)?, zone.get(2..)?),
            5 if zone.as_bytes().get(2) == Some(&b':') => (zone.get(..2)?, zone.get(3..)?),
            _ => return None,
        };
        if !hours.bytes().chain(minutes.bytes()).all(|b| b.is_ascii_digit()) { return None; }
        let hours: i64 = hours.parse().ok()?;
        let minutes: i64 = minutes.parse().ok()?;
        if hours > 23 || minutes > 59 { return None; }
        sign * (hours * 3600 + minutes * 60)
    };
    let days = days_from_civil(year, month, day);
    Some((days * 86400 + hour * 3600 + minute * 60 + second - offset) * 1000 + millis)
}

/// Legacy normalization used only to validate historical model-name handling.
#[cfg(test)]
pub fn normalize_codex_model(raw: &str) -> String {
    let mut name = raw.to_lowercase();
    if let Some(pos) = name.rfind('/') {
        name = name[pos + 1..].to_string();
    }
    // Remove date suffix like -2026-03-05. Keep this structural rather than
    // merely counting digits: the suffix contains three hyphens (the leading
    // separator plus the two separators inside the date).
    if name.len() >= 11 && name.is_char_boundary(name.len() - 11) {
        let suffix = &name[name.len() - 11..];
        let bytes = suffix.as_bytes();
        let is_date_suffix = bytes.len() == 11
            && bytes[0] == b'-'
            && bytes[5] == b'-'
            && bytes[8] == b'-'
            && bytes[1..5].iter().all(u8::is_ascii_digit)
            && bytes[6..8].iter().all(u8::is_ascii_digit)
            && bytes[9..11].iter().all(u8::is_ascii_digit);
        if is_date_suffix {
            name.truncate(name.len() - 11);
        }
    }
    name
}

/// Per-million-token prices used for usage accounting.
///
/// Cache creation pricing is intentionally zero: the upstream price table used by
/// this importer does not publish a separate cache-write rate, so we must not
/// invent one. Unknown models return None instead of inheriting a nearby model's
/// price.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModelPricingRates {
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_read_per_million: f64,
    pub cache_creation_per_million: f64,
}

pub fn model_pricing(model: &str) -> Option<ModelPricingRates> {
    let lower = model.to_ascii_lowercase();
    let (input_per_million, output_per_million, cache_read_per_million) =
        if lower.contains("gpt-5") || lower.contains("o1") || lower.contains("o3") {
            (2.50, 10.00, 1.25)
        } else if lower.contains("gpt-4o-mini") {
            (0.15, 0.60, 0.075)
        } else if lower.contains("gpt-4o") || lower.contains("gpt-4") {
            (2.50, 10.00, 1.25)
        } else if lower.contains("claude-3-5-sonnet") || lower.contains("claude-sonnet-4") {
            (3.00, 15.00, 0.30)
        } else if lower.contains("claude-3-5-haiku") {
            (0.80, 4.00, 0.08)
        } else if lower.contains("claude-3-opus") {
            (15.00, 75.00, 1.50)
        } else if lower.contains("deepseek") {
            // DeepSeek 按家族分档，不能一口价：
            // - V4 Pro 走高峰档 $1.32 / $3.96 / 缓存读 $0.044（人民币 9 / 27 / 0.3 元）。
            //   厂商 2026-09-12 已撤回「V4 Pro 路由到 V4.1 Flash」的公告，V4 Pro 照旧按原价计费，
            //   用 Flash 档算会把费用严重低估。
            // - deepseek-flash / deepseek-v4-flash 以及其余 V4 家族按 V4.1 Flash 档
            //   $0.30 / $1.20 / 缓存读 $0.006。
            // - V3 及更早维持原价。
            if lower.contains("v4-pro") || lower.contains("v4.1-pro") {
                (1.32, 3.96, 0.044)
            } else if lower.contains("flash") || lower.contains("v4") {
                (0.30, 1.20, 0.006)
            } else {
                (0.14, 0.28, 0.014)
            }
        } else if lower.contains("gemini-2.5-pro") || lower.contains("gemini-1.5-pro") {
            (1.25, 5.00, 0.3125)
        } else if lower.contains("gemini-2.5-flash") || lower.contains("gemini-1.5-flash") {
            (0.075, 0.30, 0.01875)
        } else if lower.contains("glm-4") {
            (1.00, 1.00, 0.10)
        } else {
            return None;
        };

    Some(ModelPricingRates {
        input_per_million,
        output_per_million,
        cache_read_per_million,
        cache_creation_per_million: 0.0,
    })
}

/// Estimates cost in USD for a model and token usage.
pub fn estimate_model_cost(model: &str, input: i64, output: i64, cache_read: i64) -> f64 {
    let Some(rates) = model_pricing(model) else {
        return 0.0;
    };

    let billable_input = input.saturating_sub(cache_read).max(0);
    let cost = (billable_input as f64 * rates.input_per_million
        + output.max(0) as f64 * rates.output_per_million
        + cache_read.max(0) as f64 * rates.cache_read_per_million)
        / 1_000_000.0;
    (cost * 10000.0).round() / 10000.0
}

pub use estimate_model_cost as estimate_codex_cost;

/// Debug desktop runs must never fall back to the real Codex directory, even
/// before their isolated .codex directory exists. Keep this helper pure so tests
/// do not mutate process-wide environment variables used by concurrent tests.
#[cfg(debug_assertions)]
fn debug_codex_dir(home: Option<std::ffi::OsString>) -> Option<PathBuf> {
    home.filter(|value| !value.is_empty())
        .map(|value| PathBuf::from(value).join(".codex"))
}

/// Resolves Codex home directory path.
pub fn get_codex_dir() -> PathBuf {
    #[cfg(debug_assertions)]
    if let Some(directory) = debug_codex_dir(std::env::var_os("XY_SWITCH_TEST_HOME_DIR")) {
        return directory;
    }
    if let Some(codex_home) = std::env::var_os("CODEX_HOME").filter(|v| !v.is_empty()) {
        let p = PathBuf::from(codex_home);
        if p.is_absolute() && p.exists() {
            return p;
        }
    }
    if let Some(userprofile) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
    {
        let p = PathBuf::from(userprofile).join(".codex");
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(".codex")
}

/// Recursively gathers all .jsonl session files in `sessions` and `archived_sessions`.
fn collect_session_files(codex_dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let sessions_dir = codex_dir.join("sessions");
    if sessions_dir.is_dir() {
        collect_jsonl_recursive(&sessions_dir, &mut files, 0, 4);
    }
    let archived_dir = codex_dir.join("archived_sessions");
    if archived_dir.is_dir() {
        collect_jsonl_recursive(&archived_dir, &mut files, 0, 2);
    }
    files.sort();
    files
}

fn collect_jsonl_recursive(dir: &Path, files: &mut Vec<PathBuf>, depth: u32, max_depth: u32) {
    if depth > max_depth {
        return;
    }
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_recursive(&path, files, depth + 1, max_depth);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
}

// Counters are [input, output, cache read, cache write]. Missing required or
// malformed counters are not free usage. Optional absent cache counters mean no
// cache usage was reported; explicit invalid values are rejected.
fn reported_token_counts(value: &serde_json::Value) -> Result<[i64; 4], ()> {
    let required = |key| value.get(key).and_then(serde_json::Value::as_i64).filter(|n| *n >= 0).ok_or(());
    let optional = |value: Option<&serde_json::Value>| match value {
        None => Ok(0),
        Some(value) => value.as_i64().filter(|n| *n >= 0).ok_or(()),
    };
    Ok([required("input_tokens")?, required("output_tokens")?,
        optional(value.get("cached_input_tokens").or_else(|| value.get("cache_read_input_tokens")))?,
        optional(value.get("cache_write_input_tokens"))?])
}

fn token_event_counts(info: &serde_json::Value, previous: &mut Option<[i64; 4]>) -> Result<[i64; 4], ()> {
    let prior = *previous;
    let last = info.get("last_token_usage").filter(|value| !value.is_null());
    let total = info.get("total_token_usage").filter(|value| !value.is_null());
    // Keep the cumulative baseline current even when this event supplies a last
    // usage record. Otherwise switching to total-only events double-counts it.
    if let Some(total) = total { *previous = reported_token_counts(total).ok(); }
    if let Some(last) = last {
        let counts = match reported_token_counts(last) {
            Ok(counts) => counts,
            Err(()) => { if total.is_none() { *previous = None; } return Err(()); }
        };
        if total.is_none() {
            *previous = prior.and_then(|prior| {
                Some([prior[0].checked_add(counts[0])?, prior[1].checked_add(counts[1])?, prior[2].checked_add(counts[2])?, prior[3].checked_add(counts[3])?])
            });
        }
        return Ok(counts);
    }
    let current = if total.is_some() { previous.ok_or(())? } else { return Err(()); };
    let prior = prior.ok_or(())?;
    let delta = |index: usize| current[index].checked_sub(prior[index]).filter(|n| *n >= 0).ok_or(());
    Ok([delta(0)?, delta(1)?, delta(2)?, delta(3)?])
}

/// Synchronizes Codex session logs into `request_events` table.
pub fn sync_codex_usage(db: &Database) -> Result<SyncResult, String> {
    sync_root(db, &get_codex_dir())
}

fn sync_root(db: &Database, codex_dir: &Path) -> Result<SyncResult, String> {
    if !codex_dir.exists() {
        return Ok(SyncResult::default());
    }

    let files = collect_session_files(&codex_dir);
    if files.is_empty() {
        return Ok(SyncResult::default());
    }

    // Local session metadata cannot establish which currently selected upstream served it.
    let provider_id = "local:codex";

    let conn_arc = db.conn();
    let mut conn = conn_arc
        .lock()
        .map_err(|e| format!("database lock error: {e}"))?;

    crate::session_usage_harness::ensure_source(&conn, provider_id, "Codex · 本地用量")?;

    // Ensure session_log_sync table exists
    conn.execute(
        "CREATE TABLE IF NOT EXISTS session_log_sync (
            file_path TEXT PRIMARY KEY,
            last_modified INTEGER NOT NULL,
            last_line_offset INTEGER NOT NULL,
            last_byte_offset INTEGER NOT NULL
        )",
        [],
    )
    .map_err(|e| format!("failed to initialize session_log_sync table: {e}"))?;

    let mut result = SyncResult {
        files_scanned: files.len(),
        ..Default::default()
    };

    let tx = conn
        .transaction()
        .map_err(|e| format!("failed to start transaction: {e}"))?;

    for file_path in &files {
        let metadata = match fs::metadata(file_path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        let file_len = metadata.len() as i64;
        let modified_time_ms = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);

        let file_path_str = file_path.to_string_lossy().to_string();

        // Check if file is already up to date
        let sync_state: Option<(i64, i64, i64)> = tx
            .query_row(
                "SELECT last_modified, last_line_offset, last_byte_offset FROM session_log_sync WHERE file_path = ?1",
                [&file_path_str],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .ok();

        if let Some((sync_mod, _line_off, sync_len)) = sync_state {
            if sync_mod == modified_time_ms && sync_len == file_len {
                continue; // File completely unchanged
            }
        }

        let file = match File::open(file_path) {
            Ok(f) => f,
            Err(_) => continue,
        };
        let reader = BufReader::new(file);

        let file_stem = file_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown");

        let mut current_model = String::new();
        let mut line_number = 0i64;
        let mut insert_error: Option<String> = None;
        let mut previous_totals = Some([0i64; 4]);

        for line_res in reader.lines() {
            let line = line_res.map_err(|error| format!("failed to read Codex usage log: {error}"))?;
            line_number += 1;
            if line.trim().is_empty() {
                continue;
            }

            // Quick string scan
            let has_turn_context = line.contains("\"turn_context\"");
            let has_token_count = line.contains("\"token_count\"");
            if !has_turn_context && !has_token_count {
                continue;
            }

            let json: serde_json::Value = match serde_json::from_str(&line) {
                Ok(v) => v,
                Err(_) => continue,
            };

            let ev_type = json.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if ev_type == "turn_context" {
                if let Some(payload) = json.get("payload") {
                    if let Some(model) = payload.get("model").and_then(|v| v.as_str()) {
                        current_model = model.trim().to_string();
                    }
                }
            } else if ev_type == "event_msg" {
                let payload = match json.get("payload") {
                    Some(p) => p,
                    None => continue,
                };
                if payload.get("type").and_then(|v| v.as_str()) != Some("token_count") {
                    continue;
                }
                let info = match payload.get("info") {
                    Some(i) => i,
                    None => continue,
                };

                if let Some(m) = info.get("model").and_then(|v| v.as_str()) {
                    current_model = m.trim().to_string();
                }

                if info.is_null() { continue; }
                let [input_tokens, output_tokens, cache_read, cache_write] = match token_event_counts(info, &mut previous_totals) {
                    Ok(counts) => counts,
                    Err(()) => { result.invalid_events += 1; continue; }
                };
                if input_tokens == 0 && output_tokens == 0 && cache_read == 0 && cache_write == 0 { continue; }

                let timestamp_ms = json
                    .get("timestamp")
                    .and_then(|v| v.as_str())
                    .and_then(parse_iso_timestamp_ms)
                    .unwrap_or(modified_time_ms);

                let price_version = format!("session:codex:{file_stem}:{line_number}");

                // 1. Check if already recorded from session import
                let already_imported: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM request_events WHERE price_version = ?1)",
                        [&price_version],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;

                if already_imported {
                    result.events_skipped += 1;
                    continue;
                }

                // 2. Check if already recorded from proxy within a 15-second window
                let proxy_duplicate: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM request_events WHERE (price_source IS NULL OR price_source NOT IN ('codex_session','harness_session')) AND ABS(created_at - ?1) <= 15000 AND input_tokens = ?2 AND output_tokens = ?3)",
                        params![timestamp_ms, input_tokens, output_tokens],
                        |row| row.get(0),
                    )
                    .map_err(|error| error.to_string())?;

                if proxy_duplicate {
                    result.events_skipped += 1;
                    continue;
                }

                let price = crate::model_pricing::estimate(&tx, &crate::database::RequestUsage {
                    model: Some(current_model.clone()), input_tokens: Some(input_tokens), output_tokens: Some(output_tokens),
                    cache_read_tokens: Some(cache_read), cache_creation_tokens: Some(cache_write), ..Default::default()
                })?;
                let cost = price.as_ref().and_then(|price| price.total);

                let insert_res = tx.execute(
                    "INSERT INTO request_events (
                        provider_id, route, status_code, success, latency_ms, retry_count,
                        error_kind, model, input_tokens, output_tokens, cache_creation_tokens,
                        cache_read_tokens, cost, price_source, price_version, created_at
                    ) VALUES (?1, ?2, 200, 1, 0, 0, NULL, ?3, ?4, ?5, ?6, ?7, ?8, 'codex_session', ?9, ?10)",
                    params![
                        provider_id,
                        "/v1/responses",
                        current_model,
                        input_tokens,
                        output_tokens,
                        cache_write,
                        cache_read,
                        cost,
                        price_version,
                        timestamp_ms
                    ],
                );

                if insert_res.is_ok() {
                    crate::model_pricing::save_snapshot(&tx, tx.last_insert_rowid(), price.as_ref())?;
                    result.events_imported += 1;
                    result.total_input_tokens = result.total_input_tokens.saturating_add(input_tokens);
                    result.total_output_tokens = result.total_output_tokens.saturating_add(output_tokens);
                    result.total_cache_read_tokens = result.total_cache_read_tokens.saturating_add(cache_read);
                } else if let Err(error) = insert_res {
                    // 写入失败时不能悄悄跳过：记下第一条错误，
                    // 并在本文件上放弃推进同步水位，下次还能重试。
                    insert_error.get_or_insert_with(|| error.to_string());
                }
            }
        }

        if let Some(error) = insert_error {
            return Err(format!("failed to import Codex usage; changes rolled back: {error}"));
        }

        // Update sync state
        tx.execute(
            "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_byte_offset)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(file_path) DO UPDATE SET
                last_modified = excluded.last_modified,
                last_line_offset = excluded.last_line_offset,
                last_byte_offset = excluded.last_byte_offset",
            params![file_path_str, modified_time_ms, line_number, file_len],
        ).map_err(|error| format!("failed to save Codex usage sync state: {error}"))?;
    }

    tx.commit()
        .map_err(|e| format!("failed to commit session sync transaction: {e}"))?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_codex_home_uses_the_isolated_home() {
        let home = std::env::temp_dir().join(format!("xy-switch-home-{}", uuid::Uuid::new_v4()));
        assert_eq!(
            debug_codex_dir(Some(home.clone().into_os_string())),
            Some(home.join(".codex"))
        );
    }

    #[test]
    fn missing_debug_codex_home_does_not_request_production_fallback() {
        let home =
            std::env::temp_dir().join(format!("xy-switch-missing-home-{}", uuid::Uuid::new_v4()));
        let expected = home.join(".codex");
        assert!(!expected.exists());
        assert_eq!(debug_codex_dir(Some(home.into_os_string())), Some(expected));
    }

    #[test]
    fn absent_or_empty_debug_home_keeps_normal_path_resolution() {
        assert_eq!(debug_codex_dir(None), None);
        assert_eq!(debug_codex_dir(Some(std::ffi::OsString::new())), None);
    }

    #[test]
    fn test_parse_iso_timestamp() {
        let ts = "2026-09-22T12:42:31.846Z";
        let ms = parse_iso_timestamp_ms(ts).expect("should parse");
        // 2026-09-22 12:42:31 UTC = 1790080951 seconds = 1790080951846 ms
        assert_eq!(ms, 1790080951846);
    }

    #[test]
    fn test_normalize_model() {
        assert_eq!(normalize_codex_model("openai/gpt-5.6-sol"), "gpt-5.6-sol");
        assert_eq!(
            normalize_codex_model("gpt-5.6-sol-2026-03-05"),
            "gpt-5.6-sol"
        );
        assert_eq!(normalize_codex_model("GPT-5.6-SOL"), "gpt-5.6-sol");
    }

    #[test]
    fn test_cost_estimation() {
        // gpt-5.6-sol: 1000 input (200 cached), 500 output
        // billable input: 800 * $2.50/M = $0.002
        // output: 500 * $10.00/M = $0.005
        // cached: 200 * $1.25/M = $0.00025
        // total = 0.00725 -> 0.0073
        let cost = estimate_model_cost("gpt-5.6-sol", 1000, 500, 200);
        assert!(cost > 0.007 && cost < 0.008);
    }

    #[test]
    fn deepseek_pricing_is_split_by_family() {
        // V4 Pro：高峰档 1.32 / 3.96 / 0.044
        // 1_000_000 input（无缓存）→ 1.32；1_000_000 output → 3.96
        let pro = estimate_model_cost("deepseek-v4-pro", 1_000_000, 1_000_000, 0);
        assert!((pro - (1.32 + 3.96)).abs() < 0.01, "pro cost = {pro}");

        // V4.1 Flash 档：0.30 / 1.20 / 0.006
        let flash = estimate_model_cost("deepseek-v4-flash", 1_000_000, 1_000_000, 0);
        assert!((flash - (0.30 + 1.20)).abs() < 0.01, "flash cost = {flash}");
        let flash_alias = estimate_model_cost("deepseek-flash", 1_000_000, 1_000_000, 0);
        assert!((flash_alias - flash).abs() < 0.001);

        // V3 及更早维持原价 0.14 / 0.28
        let v3 = estimate_model_cost("deepseek-chat", 1_000_000, 1_000_000, 0);
        assert!((v3 - (0.14 + 0.28)).abs() < 0.01, "v3 cost = {v3}");

        // 高峰档必须明显贵于 Flash 档，避免以后有人把两条分支写反。
        assert!(pro > flash * 2.0);
    }
    #[test]
    fn unknown_models_do_not_receive_invented_pricing() {
        assert_eq!(model_pricing("vendor-future-model"), None);
        assert_eq!(
            estimate_model_cost("vendor-future-model", 1_000_000, 1_000_000, 0),
            0.0
        );
    }

    #[test]
    fn pricing_rates_include_cache_creation_as_explicit_zero() {
        let rates = model_pricing("deepseek-v4-pro").expect("known model");
        assert_eq!(rates.input_per_million, 1.32);
        assert_eq!(rates.output_per_million, 3.96);
        assert_eq!(rates.cache_read_per_million, 0.044);
        assert_eq!(rates.cache_creation_per_million, 0.0);
    }
    #[test]
    fn mixed_last_and_total_events_bill_only_all_four_counter_deltas() {
        let mut previous = Some([0; 4]);
        let counts = serde_json::json!({"input_tokens":100,"output_tokens":10,"cached_input_tokens":20,"cache_write_input_tokens":5});
        assert_eq!(token_event_counts(&serde_json::json!({"last_token_usage":counts,"total_token_usage":counts}), &mut previous), Ok([100,10,20,5]));
        let later = serde_json::json!({"total_token_usage":{"input_tokens":150,"output_tokens":15,"cached_input_tokens":30,"cache_write_input_tokens":8}});
        assert_eq!(token_event_counts(&later, &mut previous), Ok([50,5,10,3]));
        assert_eq!(token_event_counts(&later, &mut previous), Ok([0,0,0,0]));
    }

    #[test]
    fn last_only_events_and_null_last_keep_total_baselines_consistent() {
        let mut previous = Some([0; 4]);
        assert_eq!(token_event_counts(&serde_json::json!({"last_token_usage":{"input_tokens":100,"output_tokens":10}}), &mut previous), Ok([100,10,0,0]));
        assert_eq!(token_event_counts(&serde_json::json!({"last_token_usage":null,"total_token_usage":{"input_tokens":150,"output_tokens":15}}), &mut previous), Ok([50,5,0,0]));
    }

    #[test]
    fn invalid_missing_overflowing_and_reset_counters_are_not_free_requests() {
        for counts in [serde_json::json!({"input_tokens":100}), serde_json::json!({"input_tokens":-1,"output_tokens":1}), serde_json::json!({"input_tokens":1.5,"output_tokens":1}), serde_json::json!({"input_tokens":100,"output_tokens":1,"cached_input_tokens":"20"}), serde_json::json!({"input_tokens":1e30,"output_tokens":1})] {
            assert!(token_event_counts(&serde_json::json!({"last_token_usage":counts}), &mut Some([0;4])).is_err());
        }
        let mut previous = Some([100,10,0,0]);
        assert!(token_event_counts(&serde_json::json!({"total_token_usage":{"input_tokens":10,"output_tokens":1}}), &mut previous).is_err());
        assert_eq!(previous, Some([10,1,0,0]));
        assert_eq!(token_event_counts(&serde_json::json!({"total_token_usage":{"input_tokens":20,"output_tokens":2}}), &mut previous), Ok([10,1,0,0]));
        let mut previous = Some([i64::MAX,0,0,0]);
        assert_eq!(token_event_counts(&serde_json::json!({"last_token_usage":{"input_tokens":1,"output_tokens":1}}), &mut previous), Ok([1,1,0,0]));
        assert!(previous.is_none());
    }

    struct UsageFixture { root: PathBuf, file: PathBuf, db: Database }
    impl UsageFixture {
        fn new(counts: &[serde_json::Value]) -> Self {
            let temp = std::env::temp_dir().canonicalize().unwrap();
            let root = temp.join(format!("xy-codex-usage-{}", uuid::Uuid::new_v4()));
            assert!(root.starts_with(&temp));
            fs::create_dir(&root).unwrap(); fs::create_dir(root.join("sessions")).unwrap();
            let file = root.join("sessions/rollout-qa-usage.jsonl");
            let mut rows = vec![serde_json::json!({"type":"turn_context","payload":{"model":"Vendor/Exact-Model"}}).to_string()];
            rows.extend(counts.iter().map(|counts| serde_json::json!({"type":"event_msg","timestamp":"2026-10-27T00:00:00Z","payload":{"type":"token_count","info":{"last_token_usage":counts}}}).to_string()));
            fs::write(&file, rows.join("\n") + "\n").unwrap();
            let db = Database::open(Path::new(":memory:")).unwrap();
            crate::model_pricing::save(&db, &crate::model_pricing::PricingInput { id:"Vendor/Exact-Model".into(), input_per_million:2.0, output_per_million:4.0, cache_read_per_million:Some(1.0), cache_creation_per_million:Some(3.0) }).unwrap();
            Self { root, file, db }
        }
        fn event_count(&self) -> i64 { self.db.conn().lock().unwrap().query_row("SELECT COUNT(*) FROM request_events", [], |row| row.get(0)).unwrap() }
    }
    impl Drop for UsageFixture {
        fn drop(&mut self) {
            // Exact files created by this fixture; never recursively delete a computed home.
            let _ = fs::remove_file(&self.file);
            let _ = fs::remove_dir(self.root.join("sessions"));
            let _ = fs::remove_dir(&self.root);
        }
    }

    #[test]
    fn imported_usage_keeps_exact_model_and_immutable_custom_price_snapshot() {
        let fixture = UsageFixture::new(&[serde_json::json!({"input_tokens":100,"output_tokens":50,"cached_input_tokens":20})]);
        assert_eq!(sync_root(&fixture.db, &fixture.root).unwrap().events_imported, 1);
        let (model,cost): (String,f64) = fixture.db.conn().lock().unwrap().query_row("SELECT model,cost FROM request_events", [], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(model, "Vendor/Exact-Model"); assert!((cost-0.00038).abs()<1e-12);
        fixture.db.conn().lock().unwrap().execute("UPDATE model_pricing SET input_per_million=100", []).unwrap();
        assert_eq!(sync_root(&fixture.db, &fixture.root).unwrap().events_imported, 0);
        let (saved,source): (f64,String) = fixture.db.conn().lock().unwrap().query_row("SELECT e.cost,p.source FROM request_events e JOIN request_price_snapshots p ON e.id=p.event_id", [], |row| Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(saved,cost); assert_eq!(source,"custom");
    }

    #[test]
    fn malformed_required_counters_are_reported_without_zero_cost_rows() {
        let fixture = UsageFixture::new(&[serde_json::json!({"input_tokens":100}),serde_json::json!({"input_tokens":100,"output_tokens":-1})]);
        let result = sync_root(&fixture.db, &fixture.root).unwrap();
        assert_eq!(result.invalid_events,2); assert_eq!(result.events_imported,0); assert_eq!(fixture.event_count(),0);
    }

    #[test]
    fn usage_or_watermark_write_failure_rolls_back_and_remains_retryable() {
        for table in ["request_events","session_log_sync"] {
            let fixture = UsageFixture::new(&[serde_json::json!({"input_tokens":100,"output_tokens":10})]);
            // Initialization also covers the legacy migration before injecting the failure.
            fixture.db.conn().lock().unwrap().execute_batch("CREATE TABLE IF NOT EXISTS session_log_sync(file_path TEXT PRIMARY KEY,last_modified INTEGER NOT NULL,last_line_offset INTEGER NOT NULL,last_byte_offset INTEGER NOT NULL);").unwrap();
            fixture.db.conn().lock().unwrap().execute_batch(&format!("CREATE TRIGGER reject_usage_sync BEFORE INSERT ON {table} BEGIN SELECT RAISE(ABORT, 'synthetic usage write failure'); END;")).unwrap();
            assert!(sync_root(&fixture.db, &fixture.root).unwrap_err().contains("synthetic usage write failure"));
            assert_eq!(fixture.event_count(),0);
            assert_eq!(fixture.db.conn().lock().unwrap().query_row("SELECT COUNT(*) FROM request_price_snapshots", [], |row| row.get::<_,i64>(0)).unwrap(),0);
            assert_eq!(fixture.db.conn().lock().unwrap().query_row("SELECT COUNT(*) FROM session_log_sync", [], |row| row.get::<_,i64>(0)).unwrap(),0);
            fixture.db.conn().lock().unwrap().execute_batch("DROP TRIGGER reject_usage_sync").unwrap();
            assert_eq!(sync_root(&fixture.db, &fixture.root).unwrap().events_imported,1);
        }
    }

}
