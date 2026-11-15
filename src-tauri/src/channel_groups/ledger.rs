use super::types::*;
use rusqlite::{Connection, params};
use std::collections::{BTreeMap, BTreeSet};

pub fn microusd(cost: Option<f64>) -> Option<u64> {
    let scaled = cost? * 1_000_000.0;
    if !scaled.is_finite() || scaled < 0.0 || scaled > 9_007_199_254_740_991.0 { return None; }
    let nearest = scaled.round();
    Some(if (scaled - nearest).abs() <= f64::EPSILON * scaled.abs().max(1.0) * 4.0 { nearest } else { scaled.ceil() } as u64)
}

fn unsigned(value: Option<i64>) -> Option<u64> { value.and_then(|value| u64::try_from(value).ok()) }

pub fn read(connection: &Connection, providers: &BTreeSet<String>) -> Result<BTreeMap<String, LedgerBaseline>, String> {
    let mut result = BTreeMap::new();
    for provider in providers {
        let mut baseline = LedgerBaseline::default();
        let mut events = connection.prepare("SELECT e.total_tokens,e.input_tokens,e.output_tokens,e.cost,e.created_at,
            EXISTS(SELECT 1 FROM proxy_attempts a WHERE a.event_id=e.id)
            FROM request_events e WHERE e.provider_id=?1").map_err(|error| error.to_string())?;
        let mut rows = events.query([provider]).map_err(|error| error.to_string())?;
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            let total = unsigned(row.get(0).map_err(|error| error.to_string())?);
            let input = unsigned(row.get(1).map_err(|error| error.to_string())?);
            let output = unsigned(row.get(2).map_err(|error| error.to_string())?);
            let tokens = total.or_else(|| input.zip(output).and_then(|(input, output)| input.checked_add(output)));
            let cost = microusd(row.get(3).map_err(|error| error.to_string())?);
            let at: i64 = row.get(4).map_err(|error| error.to_string())?;
            let linked: bool = row.get(5).map_err(|error| error.to_string())?;
            baseline.usage.requests += 1;
            if let Some(tokens) = tokens {
                baseline.usage.known_tokens = baseline.usage.known_tokens.saturating_add(tokens);
                baseline.usage.token_known_requests += 1;
            }
            if let Some(cost) = cost {
                baseline.usage.known_microusd = baseline.usage.known_microusd.saturating_add(cost);
                baseline.usage.cost_known_requests += 1;
            }
            if !linked {
                let day = baseline.days.entry(at / DAY_MS).or_default();
                day.committed_microusd = day.committed_microusd.saturating_add(cost.unwrap_or(0));
                day.unpriced_requests += u64::from(cost.is_none());
            }
        }
        let mut attempts = connection.prepare("SELECT a.started_at,a.actual_microusd,a.reserved_microusd,b.cost_microusd,e.cost
            FROM proxy_attempts a LEFT JOIN budget_ledger b ON b.id=a.id LEFT JOIN request_events e ON e.id=a.event_id
            WHERE a.provider_id=?1").map_err(|error| error.to_string())?;
        let mut rows = attempts.query([provider]).map_err(|error| error.to_string())?;
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            let at: i64 = row.get(0).map_err(|error| error.to_string())?;
            let amounts = [unsigned(row.get(1).map_err(|error| error.to_string())?),
                unsigned(row.get(2).map_err(|error| error.to_string())?), unsigned(row.get(3).map_err(|error| error.to_string())?),
                microusd(row.get(4).map_err(|error| error.to_string())?)];
            let committed = amounts.into_iter().flatten().max();
            let day = baseline.days.entry(at / DAY_MS).or_default();
            day.committed_microusd = day.committed_microusd.saturating_add(committed.unwrap_or(0));
            day.unpriced_requests += u64::from(committed.is_none());
        }
        let mut legacy = connection.prepare("SELECT b.day,b.cost_microusd FROM budget_ledger b
            WHERE b.provider_id=?1 AND NOT EXISTS(SELECT 1 FROM proxy_attempts a WHERE a.id=b.id)").map_err(|error| error.to_string())?;
        let mut rows = legacy.query([provider]).map_err(|error| error.to_string())?;
        while let Some(row) = rows.next().map_err(|error| error.to_string())? {
            let date = row.get::<_, i64>(0).map_err(|error| error.to_string())?;
            let cost = unsigned(row.get(1).map_err(|error| error.to_string())?);
            let day = baseline.days.entry(date).or_default();
            day.committed_microusd = day.committed_microusd.saturating_add(cost.unwrap_or(0));
            day.unpriced_requests += u64::from(cost.is_none());
        }
        result.insert(provider.clone(), baseline);
    }
    Ok(result)
}

pub fn events(connection: &Connection, providers: &BTreeSet<String>, limit: usize) -> Result<Vec<RecordedEvent>, String> {
    let mut result = Vec::new();
    for provider in providers {
        let mut query = connection.prepare("SELECT id,provider_id,created_at,status_code,latency_ms,total_tokens,input_tokens,output_tokens,cost,error_kind
            FROM request_events WHERE provider_id=?1 ORDER BY created_at DESC,id DESC LIMIT ?2").map_err(|error| error.to_string())?;
        let rows = query.query_map(params![provider, limit], event_row).map_err(|error| error.to_string())?;
        result.extend(rows.collect::<Result<Vec<_>, _>>().map_err(|error| error.to_string())?);
    }
    result.sort_by_key(|event| std::cmp::Reverse((event.at_ms, event.id)));
    result.truncate(limit);
    Ok(result)
}

pub fn event_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<RecordedEvent> {
    let total = unsigned(row.get(5)?);
    let input = unsigned(row.get(6)?);
    let output = unsigned(row.get(7)?);
    Ok(RecordedEvent {
        id: row.get(0)?, provider_id: row.get(1)?, at_ms: row.get(2)?, status: row.get::<_, Option<i64>>(3)?.and_then(|value| u16::try_from(value).ok()),
        latency_ms: unsigned(row.get(4)?), tokens: total.or_else(|| input.zip(output).and_then(|(input, output)| input.checked_add(output))),
        cost_microusd: microusd(row.get(8)?), error_kind: row.get(9)?,
    })
}
