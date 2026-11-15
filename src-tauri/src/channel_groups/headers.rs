use std::{collections::BTreeMap, time::UNIX_EPOCH};

#[derive(Clone, Debug, Default)]
pub struct HeaderEvidence {
    pub retry_at: Option<i64>,
    pub requests: Option<u64>,
    pub tokens: Option<u64>,
    pub requests_reset: Option<i64>,
    pub tokens_reset: Option<i64>,
}

fn deadline(now: i64, delay: u64) -> Option<i64> {
    now.checked_add(i64::try_from(delay).ok()?)
}

pub fn retry_after(value: &str, now: i64) -> Option<i64> {
    let value = value.trim();
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return deadline(now, value.parse::<u64>().ok()?.checked_mul(1000)?);
    }
    let date = httpdate::parse_http_date(value).ok()?.duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(date.as_millis()).ok().map(|timestamp| timestamp.max(now))
}

pub fn reset_duration(value: &str, now: i64) -> Option<i64> {
    let mut remaining = value.trim();
    let mut total = 0f64;
    if remaining.is_empty() { return None; }
    while !remaining.is_empty() {
        let count = remaining.bytes().take_while(|byte| byte.is_ascii_digit() || *byte == b'.').count();
        if count == 0 { return None; }
        let number = remaining[..count].parse::<f64>().ok()?;
        if !number.is_finite() || number < 0.0 { return None; }
        remaining = &remaining[count..];
        let (unit, length) = if remaining.starts_with("ms") { (1f64, 2) }
            else if remaining.starts_with('s') { (1000f64, 1) }
            else if remaining.starts_with('m') { (60_000f64, 1) }
            else if remaining.starts_with('h') { (3_600_000f64, 1) }
            else if remaining.starts_with('d') { (86_400_000f64, 1) }
            else { return None; };
        total += number * unit;
        if !total.is_finite() || total >= i64::MAX as f64 { return None; }
        remaining = &remaining[length..];
    }
    deadline(now, total.ceil() as u64)
}

pub fn parse(headers: &BTreeMap<String, String>, now: i64) -> Result<HeaderEvidence, String> {
    if headers.len() > 32 || headers.iter().any(|(key, value)| key.len() > 128 || value.len() > 512) {
        return Err("response_headers_too_large".into());
    }
    let mut normalized = BTreeMap::new();
    for (key, value) in headers {
        if normalized.insert(key.to_ascii_lowercase(), value.trim()).is_some() {
            return Err("duplicate_response_header".into());
        }
    }
    let get = |key: &str| normalized.get(key).copied();
    let number = |key: &str| get(key).filter(|value| !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()))
        .and_then(|value| value.parse::<u64>().ok());
    Ok(HeaderEvidence {
        retry_at: get("retry-after").and_then(|value| retry_after(value, now)),
        requests: number("x-ratelimit-remaining-requests"),
        tokens: number("x-ratelimit-remaining-tokens"),
        requests_reset: get("x-ratelimit-reset-requests").and_then(|value| reset_duration(value, now)),
        tokens_reset: get("x-ratelimit-reset-tokens").and_then(|value| reset_duration(value, now)),
    })
}
