//! User-triggered analysis. No network access and no work on the forwarding path.
use crate::{
    database::Database,
    model_audit,
    usage::{UsageFilter, build_filter_clause},
};
use serde::Serialize;
use std::collections::BTreeMap;
use tauri::State;

const SAMPLE_LIMIT: usize = 10_000;
const MIN_COMPARABLE: u64 = 20;
const DAY_MS: i64 = 86_400_000;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditAnalysis {
    sample_limit: usize,
    sample_count: usize,
    truncated: bool,
    audited_requests: usize,
    evidence_requests: usize,
    model_mismatch_requests: usize,
    gap_requests: usize,
    untested_requests: usize,
    groups: Vec<AuditGroup>,
    comparisons: Vec<AuditComparison>,
    notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditGroup {
    provider_id: String,
    requested_model: String,
    window_start: i64,
    composition: String,
    samples: u64,
    comparable_samples: u64,
    billing_samples: u64,
    mean_tokens_per_byte: Option<f64>,
    mean_bytes_per_frame: Option<f64>,
    mean_delta_chars: Option<f64>,
    mean_ttfb_ms: Option<f64>,
    mean_digit_run: Option<f64>,
    mean_han_run: Option<f64>,
    mean_space_run: Option<f64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditComparison {
    kind: String,
    left: String,
    right: String,
    requested_model: String,
    composition: String,
    status: String,
    samples_left: u64,
    samples_right: u64,
    billing_samples_left: u64,
    billing_samples_right: u64,
    billing_status: String,
    token_byte_ratio: Option<f64>,
    frame_byte_ratio: Option<f64>,
    delta_ratio: Option<f64>,
    ttfb_ratio: Option<f64>,
}

#[derive(Default)]
struct Sums {
    samples: u64,
    comparable: u64,
    billing: u64,
    tokens_per_byte: f64,
    bytes_per_frame: f64,
    delta: f64,
    ttfb: f64,
    ttfb_n: u64,
    digit: f64,
    han: f64,
    space: f64,
}

fn composition(stream: &model_audit::StreamSummary) -> String {
    let total = stream.visible_output_chars.max(1);
    let bucket = |value: u64| ((value as u128 * 4) / total as u128).min(4);
    let length = match total {
        0..=63 => "<64",
        64..=255 => "64-255",
        256..=1023 => "256-1023",
        _ => ">=1024",
    };
    format!(
        "digit:{}/4 han:{}/4 space:{}/4 chars:{length}",
        bucket(stream.digit_chars),
        bucket(stream.han_chars),
        bucket(stream.space_chars)
    )
}

fn relative_ratio(left: Option<f64>, right: Option<f64>) -> Option<f64> {
    match (left, right) {
        (Some(a), Some(b)) if a.is_finite() && b.is_finite() && a > 0.0 && b > 0.0 => {
            Some(a.max(b) / a.min(b))
        }
        _ => None,
    }
}

fn compare(kind: &str, left: &AuditGroup, right: &AuditGroup) -> AuditComparison {
    let enough =
        left.comparable_samples >= MIN_COMPARABLE && right.comparable_samples >= MIN_COMPARABLE;
    let delta_ratio = enough
        .then(|| relative_ratio(left.mean_delta_chars, right.mean_delta_chars))
        .flatten();
    let ttfb_ratio = enough
        .then(|| relative_ratio(left.mean_ttfb_ms, right.mean_ttfb_ms))
        .flatten();
    let status = if !enough || delta_ratio.is_none() {
        "insufficient"
    } else if delta_ratio.is_some_and(|r| r >= 2.0) || ttfb_ratio.is_some_and(|r| r >= 2.0) {
        "changed"
    } else {
        "similar"
    };
    let enough_billing =
        left.billing_samples >= MIN_COMPARABLE && right.billing_samples >= MIN_COMPARABLE;
    let token_byte_ratio = enough_billing
        .then(|| relative_ratio(left.mean_tokens_per_byte, right.mean_tokens_per_byte))
        .flatten();
    let frame_byte_ratio = enough_billing
        .then(|| relative_ratio(left.mean_bytes_per_frame, right.mean_bytes_per_frame))
        .flatten();
    let billing_status = if token_byte_ratio.is_none() || frame_byte_ratio.is_none() {
        "insufficient"
    } else if token_byte_ratio.is_some_and(|ratio| ratio >= 2.0)
        || frame_byte_ratio.is_some_and(|ratio| ratio >= 2.0)
    {
        "changed"
    } else {
        "similar"
    };
    AuditComparison {
        kind: kind.into(),
        left: format!("{} @ {}", left.provider_id, left.window_start),
        right: format!("{} @ {}", right.provider_id, right.window_start),
        requested_model: left.requested_model.clone(),
        composition: left.composition.clone(),
        status: status.into(),
        samples_left: left.comparable_samples,
        samples_right: right.comparable_samples,
        billing_samples_left: left.billing_samples,
        billing_samples_right: right.billing_samples,
        billing_status: billing_status.into(),
        token_byte_ratio,
        frame_byte_ratio,
        delta_ratio,
        ttfb_ratio,
    }
}

pub(crate) fn analyze_database(
    db: &Database,
    filter: &UsageFilter,
) -> Result<AuditAnalysis, String> {
    let (where_clause, mut parameters) = build_filter_clause(filter);
    let conjunction = if where_clause.is_empty() {
        "WHERE"
    } else {
        "AND"
    };
    let sql = format!(
        "SELECT provider_id,created_at,audit_json FROM request_events {where_clause} {conjunction} route <> 'health_check' ORDER BY created_at DESC,id DESC LIMIT ?"
    );
    parameters.push(rusqlite::types::Value::Integer((SAMPLE_LIMIT + 1) as i64));
    let records = {
        let arc = db.conn();
        let connection = arc.lock().map_err(|e| e.to_string())?;
        let mut statement = connection.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = statement
            .query_map(rusqlite::params_from_iter(parameters), |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    }; // Release SQLite before statistical work.
    let mut result = AuditAnalysis {
        sample_limit: SAMPLE_LIMIT, sample_count: records.len().min(SAMPLE_LIMIT), truncated: records.len() > SAMPLE_LIMIT,
        audited_requests: 0, evidence_requests: 0, model_mismatch_requests: 0, gap_requests: 0, untested_requests: 0,
        groups: vec![], comparisons: vec![], notes: vec![
            "仅分析筛选范围内最近 10000 条业务记录；健康检查不计入。旧记录没有审计数据，不能回推。".into(),
            "按 UTC 日、请求模型、字符分布四分位及输出长度分组；每组至少 20 条完整且未检测到批处理的流式样本才比较。".into(),
            "token/UTF-8 字节与字节/帧使用逐请求比率的均值；仅计完整纯文本、已知推理用量、无工具且可比的记录，两侧各至少 20 份计费样本才比较。优先同渠道时间自比，跨渠道必须同日且字符组成分桶相同；旧记录缺字节指标时保持未测到。".into(),
            "2 倍变化阈值是探索性提示；未排除网络、负载、提示词、采样参数和网关重切帧影响，不能据此确认更换模型或两个渠道共用上游。相近不代表同源或可信。".into(),
            "缺口计数与异常计数可重叠；未测到不等于没问题。未保存请求、响应正文或工具参数。".into(),
        ],
    };
    let mut groups: BTreeMap<(String, String, i64, String), Sums> = BTreeMap::new();
    for (provider, created, raw) in records.into_iter().take(SAMPLE_LIMIT) {
        let audit =
            raw.and_then(|json| serde_json::from_str::<model_audit::AuditRecord>(&json).ok());
        let Some(audit) = audit else {
            result.untested_requests += 1;
            continue;
        };
        result.audited_requests += 1;
        if audit.flags.iter().any(|flag| {
            matches!(
                flag.as_str(),
                "inv.byte_bound" | "inv.usage_selfcontradictory" | "inv.max_tokens_violated"
            )
        }) {
            result.evidence_requests += 1;
        }
        if audit.model_status == "mismatched" {
            result.model_mismatch_requests += 1;
        }
        if !audit.gaps.is_empty() {
            result.gap_requests += 1;
        }
        let observation = &audit.observation;
        let Some(stream) = &observation.stream else {
            continue;
        };
        let Some(requested) = observation
            .requested_model
            .as_ref()
            .filter(|model| !model.is_empty())
        else {
            continue;
        };
        let key = (
            provider,
            requested.clone(),
            created.div_euclid(DAY_MS) * DAY_MS,
            composition(stream),
        );
        let sums = groups.entry(key).or_default();
        sums.samples += 1;
        if !observation.complete
            || !observation.text_only
            || stream.batched
            || !stream.minimum_samples_met
            || stream.samples_truncated
            || stream.visible_output_chars == 0
        {
            continue;
        }
        sums.comparable += 1;
        sums.delta += stream.median_delta_chars;
        sums.digit += stream.max_digit_run as f64;
        sums.han += stream.max_han_run as f64;
        sums.space += stream.max_space_run as f64;
        if let Some(ttfb) = stream.ttfb_ms {
            sums.ttfb += ttfb as f64;
            sums.ttfb_n += 1;
        }
        if observation.tool_payload_bytes != Some(0) {
            continue;
        }
        let Some(visible_tokens) = observation
            .output_tokens
            .zip(observation.reasoning_tokens)
            .and_then(|(output, reasoning)| output.checked_sub(reasoning))
            .filter(|tokens| *tokens > 0)
        else {
            continue;
        };
        let Some(visible_bytes) = observation.visible_output_bytes.filter(|bytes| *bytes > 0)
        else {
            continue;
        };
        let Some(billing) = audit.stream_billing.as_ref().filter(|billing| {
            billing.status != model_audit::AxisResult::NotComparable
                && billing.visible_tokens == Some(visible_tokens)
                && billing.visible_bytes == Some(visible_bytes)
                && billing.text_frames == stream.text_delta_count
        }) else {
            continue;
        };
        if let (Some(tokens_per_byte), Some(bytes_per_frame)) = (
            billing
                .tokens_per_byte
                .filter(|value| value.is_finite() && *value > 0.0),
            billing
                .bytes_per_frame
                .filter(|value| value.is_finite() && *value > 0.0),
        ) {
            sums.billing += 1;
            sums.tokens_per_byte += tokens_per_byte;
            sums.bytes_per_frame += bytes_per_frame;
        }
    }
    for ((provider_id, requested_model, window_start, composition), sums) in groups {
        let mean = |total: f64| (sums.comparable > 0).then(|| total / sums.comparable as f64);
        result.groups.push(AuditGroup {
            provider_id,
            requested_model,
            window_start,
            composition,
            samples: sums.samples,
            comparable_samples: sums.comparable,
            billing_samples: sums.billing,
            mean_tokens_per_byte: (sums.billing > 0)
                .then(|| sums.tokens_per_byte / sums.billing as f64),
            mean_bytes_per_frame: (sums.billing > 0)
                .then(|| sums.bytes_per_frame / sums.billing as f64),
            mean_delta_chars: mean(sums.delta),
            mean_ttfb_ms: (sums.ttfb_n >= MIN_COMPARABLE).then(|| sums.ttfb / sums.ttfb_n as f64),
            mean_digit_run: mean(sums.digit),
            mean_han_run: mean(sums.han),
            mean_space_run: mean(sums.space),
        });
    }
    let mut histories: BTreeMap<(String, String, String), Vec<&AuditGroup>> = BTreeMap::new();
    for group in &result.groups {
        histories
            .entry((
                group.provider_id.clone(),
                group.requested_model.clone(),
                group.composition.clone(),
            ))
            .or_default()
            .push(group);
    }
    for groups in histories.values_mut() {
        groups.sort_by_key(|group| group.window_start);
        if groups.len() >= 2 && result.comparisons.len() < 100 {
            result.comparisons.push(compare(
                "history",
                groups[groups.len() - 2],
                groups[groups.len() - 1],
            ));
        }
    }
    let latest: Vec<_> = histories
        .values()
        .filter_map(|groups| groups.last().copied())
        .collect();
    'peers: for (index, left) in latest.iter().enumerate() {
        for right in latest.iter().skip(index + 1) {
            if result.comparisons.len() >= 200 {
                break 'peers;
            }
            if left.provider_id != right.provider_id
                && left.requested_model == right.requested_model
                && left.composition == right.composition
                && left.window_start == right.window_start
            {
                result.comparisons.push(compare("peer", left, right));
            }
        }
    }
    result
        .groups
        .sort_by_key(|group| std::cmp::Reverse(group.window_start));
    if result.groups.len() > 200 {
        result.groups.truncate(200);
        result
            .notes
            .push("界面分组展示上限 200，比较展示上限 200；请求计数仍覆盖全部已选样本。".into());
    }
    Ok(result)
}

#[tauri::command]
pub async fn analyze_model_audit(
    filter: UsageFilter,
    db: State<'_, Database>,
) -> Result<AuditAnalysis, String> {
    let db = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || analyze_database(&db, &filter))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn generate_model_reference(
    request: model_audit::ReferenceRequest,
) -> Result<model_audit::ReferenceFingerprint, String> {
    tauri::async_runtime::spawn_blocking(move || model_audit::generate_reference(request))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn load_model_reference(
    path: String,
) -> Result<model_audit::ReferenceFingerprint, String> {
    tauri::async_runtime::spawn_blocking(move || model_audit::load_reference(&path))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn analyze_model_reference(
    request: model_audit::RankingRequest,
) -> Result<model_audit::RankingResult, String> {
    tauri::async_runtime::spawn_blocking(move || model_audit::rank_references(request))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{Provider, RequestUsage};
    use crate::model_audit::{AuditRecord, Observation, StreamSummary};
    use serde_json::json;

    fn database_fixture() -> (Database, std::path::PathBuf) {
        let path =
            std::env::temp_dir().join(format!("xy-model-audit-sql-{}.db", uuid::Uuid::new_v4()));
        let db = Database::open(&path).unwrap();
        for id in ["a", "b"] {
            let provider: Provider =
                serde_json::from_value(json!({"id":id,"name":id,"model":"native",
                "endpoint":"http://127.0.0.1:1","appGroup":"Codex","enabled":true}))
                .unwrap();
            db.save_provider(&provider).unwrap();
        }
        (db, path)
    }

    // Seed a pre-upgrade record for legacy SQL/report compatibility tests only.
    // Normal production updates must not create audit metadata with opt-in disabled.
    fn seed_legacy_audit(db: &Database, id: i64, record: &AuditRecord) {
        let o=&record.observation;
        let count=|n:Option<u64>|n.and_then(|n|i64::try_from(n).ok());
        db.conn().lock().unwrap().execute(
            "UPDATE request_events SET model='native',requested_model=?2,upstream_model=?3,total_tokens=?4,reasoning_tokens=?5,visible_output_bytes=?6,tool_payload_bytes=?7,max_tokens=?8,text_delta_count=?9,audit_flags=?10,audit_json=?11 WHERE id=?1",
            rusqlite::params![id,o.requested_model,o.upstream_model,count(o.total_tokens),count(o.reasoning_tokens),count(o.visible_output_bytes),count(o.tool_payload_bytes),count(o.max_tokens),count(o.stream.as_ref().map(|s|s.text_delta_count)),record.flags.join(","),serde_json::to_string(record).unwrap()]
        ).unwrap();
    }

    fn add_record(
        db: &Database,
        provider: &str,
        route: &str,
        created: i64,
        audit: Option<&AuditRecord>,
    ) -> i64 {
        let id = {
            let arc = db.conn();
            let connection = arc.lock().unwrap();
            connection.execute("INSERT INTO request_events(provider_id,route,status_code,success,created_at) VALUES(?1,?2,200,1,?3)",
                rusqlite::params![provider,route,created]).unwrap();
            connection.last_insert_rowid()
        };
        if let Some(audit) = audit {
            seed_legacy_audit(db,id,audit);
        }
        id
    }

    fn sampled(median: f64, ttfb: Option<u64>) -> AuditRecord {
        model_audit::audit(Observation {
            requested_model: Some("client-alias".into()),
            forwarded_model: Some("native".into()),
            upstream_model: Some("native".into()),
            upstream_models: Vec::new(),
            model_evidence_truncated: false,
            input_tokens: Some(3),
            output_tokens: Some(4),
            total_tokens: Some(7),
            reasoning_tokens: Some(0),
            max_tokens: Some(64),
            visible_output_bytes: Some(32),
            tool_payload_bytes: Some(0),
            complete: true,
            text_only: true,
            stream: Some(StreamSummary {
                text_delta_count: 16,
                visible_output_chars: 32,
                median_delta_chars: median,
                ttfb_ms: ttfb,
                minimum_samples_met: true,
                ..Default::default()
            }),
            ..Default::default()
        })
    }

    #[test]
    fn model_audit_sql_legacy_migration_is_idempotent_and_preserves_structural_audit() {
        let path =
            std::env::temp_dir().join(format!("xy-model-audit-legacy-{}.db", uuid::Uuid::new_v4()));
        let connection = rusqlite::Connection::open(&path).unwrap();
        connection.execute_batch("CREATE TABLE request_events(id INTEGER PRIMARY KEY AUTOINCREMENT,provider_id TEXT NOT NULL,route TEXT NOT NULL,status_code INTEGER,success INTEGER NOT NULL DEFAULT 0,latency_ms INTEGER NOT NULL DEFAULT 0,retry_count INTEGER NOT NULL DEFAULT 0,error_kind TEXT,created_at INTEGER NOT NULL);
            INSERT INTO request_events(provider_id,route,status_code,success,created_at) VALUES('legacy','/v1/responses',200,1,1234);").unwrap();
        drop(connection);
        for _ in 0..2 {
            drop(Database::open(&path).unwrap());
        }
        let db = Database::open(&path).unwrap();
        let mut record = sampled(2.0, Some(15));
        record.observation.total_tokens = Some(u64::MAX);
        db.update_request_usage(
            1,
            &RequestUsage {
                model: Some("native".into()),
                input_tokens: Some(3),
                output_tokens: Some(4),
                audit: Some(record.clone()),
                ..Default::default()
            },
        )
        .unwrap();
        seed_legacy_audit(&db,1,&record);
        assert!(
            db.update_request_usage(9999, &RequestUsage::default())
                .is_err()
        );
        {
            let arc = db.conn();
            let connection = arc.lock().unwrap();
            let row: (String, i64, String, Option<i64>, String, String) = connection.query_row(
                "SELECT provider_id,created_at,requested_model,total_tokens,upstream_model,audit_json FROM request_events WHERE id=1", [],
                |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?,row.get(3)?,row.get(4)?,row.get(5)?))).unwrap();
            assert_eq!(
                (&row.0, row.1, &row.2, row.3, &row.4),
                (
                    &"legacy".to_owned(),
                    1234,
                    &"client-alias".to_owned(),
                    None,
                    &"native".to_owned()
                )
            );
            let saved: AuditRecord = serde_json::from_str(&row.5).unwrap();
            assert_eq!(saved.observation.total_tokens, Some(u64::MAX));
            assert_eq!(saved.observation.visible_output_bytes, Some(32));
            let value: serde_json::Value = serde_json::from_str(&row.5).unwrap();
            for forbidden in [
                "prompt",
                "messages",
                "content",
                "arguments",
                "headers",
                "apiKey",
            ] {
                assert!(value["observation"].get(forbidden).is_none());
            }
            let columns: Vec<String> = connection
                .prepare("PRAGMA table_info(request_events)")
                .unwrap()
                .query_map([], |row| row.get(1))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            for required in [
                "audit_json",
                "audit_flags",
                "requested_model",
                "upstream_model",
                "total_tokens",
                "reasoning_tokens",
                "visible_output_bytes",
                "tool_payload_bytes",
                "max_tokens",
                "text_delta_count",
            ] {
                assert_eq!(
                    columns.iter().filter(|column| *column == required).count(),
                    1
                );
            }
        }
        drop(db);
        drop(Database::open(&path).unwrap());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn model_audit_sql_counts_exclude_health_and_keep_missing_and_mismatch_distinct() {
        let (db, path) = database_fixture();
        let honest = sampled(2.0, Some(15));
        let gap = model_audit::audit(Observation::default());
        let mismatch = model_audit::audit(Observation {
            requested_model: Some("requested".into()),
            upstream_model: Some("other".into()),
            ..Default::default()
        });
        let mut fault_observation = honest.observation.clone();
        fault_observation.output_tokens = Some(100);
        fault_observation.total_tokens = Some(103);
        let fault = model_audit::audit(fault_observation);
        add_record(&db, "a", "/v1/responses", 1, None);
        let corrupt = add_record(&db, "a", "/v1/responses", 2, None);
        {
            let arc = db.conn();
            let connection = arc.lock().unwrap();
            connection
                .execute(
                    "UPDATE request_events SET audit_json='invalid-json' WHERE id=?1",
                    [corrupt],
                )
                .unwrap();
        }
        add_record(&db, "a", "/v1/responses", 3, Some(&gap));
        add_record(&db, "a", "/v1/responses", 4, Some(&mismatch));
        add_record(&db, "a", "/v1/responses", 5, Some(&fault));
        add_record(&db, "a", "/v1/responses", 6, Some(&honest));
        add_record(&db, "a", "health_check", 7, Some(&fault));
        add_record(&db, "b", "/v1/responses", 8, Some(&fault));
        let result = analyze_database(
            &db,
            &UsageFilter {
                provider_id: Some("a".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(result.sample_count, 6);
        assert_eq!(result.untested_requests, 2);
        assert_eq!(result.audited_requests, 4);
        assert_eq!(result.evidence_requests, 1);
        assert_eq!(result.model_mismatch_requests, 1);
        assert_eq!(result.gap_requests, 4);
        assert!(result.comparisons.is_empty());
        let filtered = analyze_database(
            &db,
            &UsageFilter {
                provider_id: Some("a".into()),
                start_time_ms: Some(6),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(filtered.sample_count, 1);
        drop(db);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn model_audit_sql_comparisons_require_clean_samples_and_sufficient_timing() {
        let (db, path) = database_fixture();
        for (provider, day, median) in [
            ("a", DAY_MS, 1.0),
            ("a", 2 * DAY_MS, 3.0),
            ("b", 2 * DAY_MS, 3.0),
        ] {
            for n in 0..20 {
                let ttfb = if provider == "b" {
                    (n == 0).then_some(1)
                } else {
                    Some(100)
                };
                add_record(
                    &db,
                    provider,
                    "/v1/responses",
                    day + n,
                    Some(&sampled(median, ttfb)),
                );
            }
        }
        for mode in 0..5 {
            let mut excluded = sampled(100.0, Some(999_999));
            match mode {
                0 => excluded.observation.complete = false,
                1 => excluded.observation.text_only = false,
                2 => excluded.observation.stream.as_mut().unwrap().batched = true,
                3 => {
                    excluded
                        .observation
                        .stream
                        .as_mut()
                        .unwrap()
                        .minimum_samples_met = false
                }
                _ => {
                    excluded
                        .observation
                        .stream
                        .as_mut()
                        .unwrap()
                        .samples_truncated = true
                }
            }
            add_record(
                &db,
                "a",
                "/v1/responses",
                2 * DAY_MS + 30 + mode,
                Some(&excluded),
            );
        }
        let result = analyze_database(&db, &UsageFilter::default()).unwrap();
        let current = result
            .groups
            .iter()
            .find(|group| group.provider_id == "a" && group.window_start == 2 * DAY_MS)
            .unwrap();
        assert_eq!(current.samples, 25);
        assert_eq!(current.comparable_samples, 20);
        assert_eq!(current.mean_delta_chars, Some(3.0));
        let history = result
            .comparisons
            .iter()
            .find(|comparison| comparison.kind == "history")
            .unwrap();
        assert_eq!(history.status, "changed");
        assert_eq!(history.delta_ratio, Some(3.0));
        let peer = result
            .comparisons
            .iter()
            .find(|comparison| comparison.kind == "peer")
            .unwrap();
        assert_eq!(peer.status, "similar");
        assert_eq!(peer.ttfb_ratio, None);
        drop(db);
        let _ = std::fs::remove_file(path);
    }
    fn chinese_billing(output_tokens: u64) -> AuditRecord {
        let mut stream = model_audit::StreamAccumulator::default();
        for index in 0..8 {
            stream.observe("汉字", 10 + index * 10);
        }
        let mut observation = sampled(2.0, Some(10)).observation;
        observation.output_tokens = Some(output_tokens);
        observation.total_tokens = Some(3 + output_tokens);
        observation.visible_output_bytes = Some(("汉字".len() * 8) as u64);
        observation.stream = Some(stream.finish());
        model_audit::audit(observation)
    }
    #[test]
    fn model_audit_sql_billing_uses_utf8_bytes_and_compares_history_before_language_matched_peers()
    {
        let (db, path) = database_fixture();
        for (day, tokens) in [(DAY_MS, 8), (2 * DAY_MS, 16)] {
            for n in 0..20 {
                add_record(
                    &db,
                    "a",
                    "/v1/responses",
                    day + n,
                    Some(&chinese_billing(tokens)),
                );
            }
        }
        // Same model/day/sample count, different language composition: no peer comparison.
        for n in 0..20 {
            add_record(
                &db,
                "b",
                "/v1/responses",
                2 * DAY_MS + n,
                Some(&sampled(2.0, Some(10))),
            );
        }
        let result = analyze_database(&db, &UsageFilter::default()).unwrap();
        let first = result
            .groups
            .iter()
            .find(|group| group.provider_id == "a" && group.window_start == DAY_MS)
            .unwrap();
        assert_eq!(first.billing_samples, 20);
        assert!((first.mean_tokens_per_byte.unwrap() - 1.0 / 6.0).abs() < 1e-12);
        assert_eq!(first.mean_bytes_per_frame, Some(6.0));
        assert_eq!(result.comparisons.len(), 1);
        let history = &result.comparisons[0];
        assert_eq!(history.kind, "history");
        assert_eq!(history.status, "similar"); // Existing frame/latency metrics did not change.
        assert_eq!(history.billing_status, "changed");
        assert_eq!(
            (history.billing_samples_left, history.billing_samples_right),
            (20, 20)
        );
        assert_eq!(history.token_byte_ratio, Some(2.0));
        assert_eq!(history.frame_byte_ratio, Some(1.0));
        drop(db);
        let _ = std::fs::remove_file(path);
    }
    #[test]
    fn model_audit_sql_billing_missing_fields_do_not_fill_the_twenty_sample_gate() {
        let (db, path) = database_fixture();
        for (day, samples) in [(DAY_MS, 19), (2 * DAY_MS, 20)] {
            for n in 0..samples {
                add_record(
                    &db,
                    "a",
                    "/v1/responses",
                    day + n,
                    Some(&chinese_billing(8)),
                );
            }
        }
        for mode in 0..6 {
            let mut excluded = chinese_billing(8);
            match mode {
                0 => excluded.observation.reasoning_tokens = None,
                1 => excluded.observation.tool_payload_bytes = Some(1),
                2 => excluded.observation.output_tokens = None,
                3 => excluded.stream_billing = None,
                4 => excluded.stream_billing.as_mut().unwrap().visible_bytes = None,
                _ => {
                    excluded.stream_billing.as_mut().unwrap().status =
                        model_audit::AxisResult::NotComparable
                }
            }
            add_record(
                &db,
                "a",
                "/v1/responses",
                DAY_MS + 30 + mode,
                Some(&excluded),
            );
        }
        let result = analyze_database(&db, &UsageFilter::default()).unwrap();
        let comparison = &result.comparisons[0];
        assert_eq!(comparison.samples_left, 25);
        assert_eq!(comparison.billing_samples_left, 19);
        assert_eq!(comparison.billing_samples_right, 20);
        assert_eq!(comparison.status, "similar");
        assert_eq!(comparison.billing_status, "insufficient");
        assert_eq!(comparison.token_byte_ratio, None);
        assert_eq!(comparison.frame_byte_ratio, None);
        drop(db);
        let _ = std::fs::remove_file(path);
    }
    fn group(n: u64, value: f64) -> AuditGroup {
        AuditGroup {
            provider_id: "fixture".into(),
            requested_model: "model".into(),
            window_start: 0,
            composition: "same".into(),
            samples: n,
            comparable_samples: n,
            billing_samples: 0,
            mean_tokens_per_byte: None,
            mean_bytes_per_frame: None,
            mean_delta_chars: Some(value),
            mean_ttfb_ms: Some(10.0),
            mean_digit_run: Some(0.0),
            mean_han_run: Some(0.0),
            mean_space_run: Some(0.0),
        }
    }
    #[test]
    fn model_audit_comparison_requires_samples_and_keeps_unknown_separate() {
        assert_eq!(
            compare("peer", &group(19, 1.0), &group(100, 3.0)).status,
            "insufficient"
        );
        assert_eq!(
            compare("peer", &group(20, 1.0), &group(20, 1.0)).status,
            "similar"
        );
        assert_eq!(
            compare("history", &group(20, 1.0), &group(20, 3.0)).status,
            "changed"
        );
        assert_eq!(
            compare("peer", &group(20, 0.0), &group(20, 3.0)).status,
            "insufficient"
        );
    }
    #[test]
    fn model_audit_billing_comparison_keeps_missing_metrics_separate() {
        let mut left = group(20, 2.0);
        let mut right = group(20, 2.0);
        left.billing_samples = 20;
        right.billing_samples = 20;
        left.mean_tokens_per_byte = Some(0.25);
        right.mean_tokens_per_byte = Some(0.25);
        left.mean_bytes_per_frame = Some(4.0);
        right.mean_bytes_per_frame = Some(8.0);
        assert_eq!(compare("history", &left, &right).billing_status, "changed");
        right.mean_bytes_per_frame = None;
        let missing = compare("history", &left, &right);
        assert_eq!(missing.billing_status, "insufficient");
        assert_eq!(missing.frame_byte_ratio, None);
        assert_eq!(missing.status, "similar");
    }
}
