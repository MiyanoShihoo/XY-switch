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
    if s.len() < 19 {
        return None;
    }
    let year: i64 = s.get(0..4)?.parse().ok()?;
    let month: i64 = s.get(5..7)?.parse().ok()?;
    let day: i64 = s.get(8..10)?.parse().ok()?;
    let hour: i64 = s.get(11..13)?.parse().ok()?;
    let min: i64 = s.get(14..16)?.parse().ok()?;
    let sec: i64 = s.get(17..19)?.parse().ok()?;

    let mut millis: i64 = 0;
    let mut rest = s.get(19..)?;
    if rest.starts_with('.') {
        rest = &rest[1..];
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        let frac_str = &rest[..end];
        rest = &rest[end..];
        // 只取小数部分的前 3 位当毫秒，不整体 parse：
        // 超过 18 位会 i64 溢出，而 10^(len-3) 在 len==0 或 len>=22 时会直接 panic。
        let digits = frac_str.as_bytes();
        for index in 0..3 {
            let digit = digits.get(index).map_or(0, |byte| i64::from(byte - b'0'));
            millis = millis * 10 + digit;
        }
    }

    let mut offset_seconds: i64 = 0;
    let rest = rest.trim();
    if rest == "Z" || rest.is_empty() {
        offset_seconds = 0;
    } else if rest.starts_with('+') || rest.starts_with('-') {
        let sign = if rest.starts_with('-') { -1 } else { 1 };
        let tz = &rest[1..];
        let (oh, om) = if let Some((h, m)) = tz.split_once(':') {
            (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?)
        } else if tz.len() >= 2 {
            (tz[..2].parse::<i64>().ok()?, 0)
        } else {
            (0, 0)
        };
        offset_seconds = sign * (oh * 3600 + om * 60);
    }

    let days = days_from_civil(year, month, day);
    let total_seconds = days * 86400 + hour * 3600 + min * 60 + sec - offset_seconds;
    Some(total_seconds * 1000 + millis)
}

/// Normalizes model name by removing vendor prefix and date suffixes.
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

/// Synchronizes Codex session logs into `request_events` table.
pub fn sync_codex_usage(db: &Database) -> Result<SyncResult, String> {
    let codex_dir = get_codex_dir();
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

        let mut current_model = "gpt-5.6-sol".to_string();
        let mut line_number = 0i64;
        let mut insert_error: Option<String> = None;
        let mut prev_total_in = 0i64;
        let mut prev_total_out = 0i64;
        let mut prev_total_cached = 0i64;

        for line_res in reader.lines() {
            let line = match line_res {
                Ok(l) => l,
                Err(_) => break,
            };
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
                        current_model = normalize_codex_model(model);
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
                    current_model = normalize_codex_model(m);
                }

                let last = info.get("last_token_usage");
                let total = info.get("total_token_usage");

                let (input_tokens, output_tokens, cache_read, cache_write) =
                    if let Some(last) = last {
                        let inp = last
                            .get("input_tokens")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let outp = last
                            .get("output_tokens")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let cached = last
                            .get("cached_input_tokens")
                            .or_else(|| last.get("cache_read_input_tokens"))
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let cwrite = last
                            .get("cache_write_input_tokens")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        (inp, outp, cached, cwrite)
                    } else if let Some(total) = total {
                        let cur_in = total
                            .get("input_tokens")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let cur_out = total
                            .get("output_tokens")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let cur_cached = total
                            .get("cached_input_tokens")
                            .or_else(|| total.get("cache_read_input_tokens"))
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);
                        let cwrite = total
                            .get("cache_write_input_tokens")
                            .and_then(|v| v.as_i64())
                            .unwrap_or(0);

                        let delta_in = cur_in.saturating_sub(prev_total_in);
                        let delta_out = cur_out.saturating_sub(prev_total_out);
                        let delta_cached = cur_cached.saturating_sub(prev_total_cached);

                        prev_total_in = cur_in;
                        prev_total_out = cur_out;
                        prev_total_cached = cur_cached;

                        (delta_in, delta_out, delta_cached, cwrite)
                    } else {
                        continue;
                    };

                if input_tokens == 0 && output_tokens == 0 && cache_read == 0 {
                    continue;
                }

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
                    .unwrap_or(false);

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
                    .unwrap_or(false);

                if proxy_duplicate {
                    result.events_skipped += 1;
                    continue;
                }

                let cost =
                    estimate_model_cost(&current_model, input_tokens, output_tokens, cache_read);

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
                    result.events_imported += 1;
                    result.total_input_tokens += input_tokens;
                    result.total_output_tokens += output_tokens;
                    result.total_cache_read_tokens += cache_read;
                } else if let Err(error) = insert_res {
                    // 写入失败时不能悄悄跳过：记下第一条错误，
                    // 并在本文件上放弃推进同步水位，下次还能重试。
                    insert_error.get_or_insert_with(|| error.to_string());
                }
            }
        }

        if let Some(error) = insert_error {
            tracing::warn!(
                file = %file_path_str,
                %error,
                "codex session usage import failed; sync watermark kept for retry"
            );
            continue;
        }

        // Update sync state
        let _ = tx.execute(
            "INSERT INTO session_log_sync (file_path, last_modified, last_line_offset, last_byte_offset)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(file_path) DO UPDATE SET
                last_modified = excluded.last_modified,
                last_line_offset = excluded.last_line_offset,
                last_byte_offset = excluded.last_byte_offset",
            params![file_path_str, modified_time_ms, line_number, file_len],
        );
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
}
