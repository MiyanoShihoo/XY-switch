use crate::{database, model_pricing::{self, ModelPricing, PricingInput}};
use serde::{Deserialize, Serialize};
use tauri::State;

/// 本地当天 00:00 对应的真实 Unix 毫秒时间戳。
///
/// 末尾那个 `'utc'` 修饰符是**必须**的，不是冗余。SQLite 的修饰符从左往右依次作用：
/// `'now'` 取 UTC 当前时刻 → `'localtime'` 把它平移成本地挂钟时间 → `'start of day'`
/// 截断到当天零点。此时得到的是一个「本地挂钟」的日期时间值，而 `strftime('%s', …)`
/// **总是按 UTC 解释**传给它的日期时间值，所以不加 `'utc'` 收尾的话，算出来的是
/// 「本地日期的 UTC 零点」，比真正的本地零点整整晚了一个时区偏移量
/// （东八区晚 8 小时）。后果是「今天」会把本地 00:00–08:00 的用量全部漏掉，
/// 「昨天」的窗口也跟着平移 8 小时。`'utc'` 负责把挂钟时间换算回真实 UTC 时刻。
///
/// 配套回归测试见本文件末尾 `local_day_start_lands_on_real_local_midnight`。
const LOCAL_DAY_START_MS: &str =
    "CAST(strftime('%s', 'now', 'localtime', 'start of day', 'utc') AS INTEGER) * 1000";

#[derive(Debug, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsageFilter {
    pub date_range: Option<String>,
    pub app_type: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub status: Option<String>,
    pub start_time_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageSummary {
    pub total_requests: i64,
    pub unknown_cost_requests: i64,
    pub success_count: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_cache_read_tokens: i64,
    pub total_cache_creation_tokens: i64,
    pub total_cost_usd: String,
    pub avg_latency_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageLogEntry {
    pub audit: Option<crate::model_audit::AuditRecord>,
    pub request_id: String,
    pub source: String,
    pub total_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub provider_id: String,
    pub app_type: String,
    pub model: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub total_cost_usd: String,
    pub input_cost_usd: String,
    pub output_cost_usd: String,
    pub cache_read_cost_usd: String,
    pub cache_creation_cost_usd: String,
    pub cost_multiplier: String,
    pub is_streaming: bool,
    pub latency_ms: i64,
    pub status_code: i64,
    pub success: bool,
    pub error_message: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageTrendPoint {
    pub timestamp: i64,
    pub date: String,
    pub label: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_creation_tokens: i64,
    pub cost: Option<f64>,
    pub requests: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageStat {
    pub provider_id: String,
    pub total_requests: i64,
    pub success_rate: f64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_cost_usd: String,
    pub avg_latency_ms: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsageStat {
    pub model: String,
    pub provider_id: String,
    pub total_requests: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_cost_usd: String,
}

pub(crate) fn build_filter_clause(filter: &UsageFilter) -> (String, Vec<rusqlite::types::Value>) {
    let mut conditions = Vec::new();
    let mut params = Vec::new();

    let mut date_condition: Option<String> = None;
    let min_time = if let Some(start_ms) = filter.start_time_ms {
        if start_ms > 0 { Some(start_ms) } else { None }
    } else if let Some(ref range) = filter.date_range {
        const DAY_MS: i64 = 86_400_000;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64;
        let local_today_start = LOCAL_DAY_START_MS;
        match range.as_str() {
            "today" => {
                date_condition = Some(format!("created_at >= {local_today_start}"));
                None
            }
            "yesterday" => {
                date_condition = Some(format!(
                    "created_at >= ({local_today_start} - {DAY_MS}) AND created_at < {local_today_start}"
                ));
                None
            }
            "1d" => Some(now_ms - DAY_MS),
            "7d" => Some(now_ms - 7 * DAY_MS),
            "14d" => Some(now_ms - 14 * DAY_MS),
            "30d" => Some(now_ms - 30 * DAY_MS),
            _ => None,
        }
    } else {
        None
    };

    if let Some(condition) = date_condition {
        conditions.push(condition);
    }
    if let Some(min_time) = min_time {
        conditions.push("created_at >= ?".to_string());
        params.push(rusqlite::types::Value::Integer(min_time));
    }

    if let Some(ref app) = filter.app_type {
        if app != "all" && !app.is_empty() {
            let lower = app.to_ascii_lowercase();
            if lower != "model_probe" {
                conditions.push("route NOT LIKE 'model_probe/%'".to_string());
            }
            if lower == "model_probe" {
                conditions.push("route LIKE 'model_probe/%'".to_string());
            } else if lower == "deepseek_harness" {
                conditions.push("route = 'session/deepseek_harness'".to_string());
            } else if lower == "codex" {
                conditions.push("(route LIKE '%codex%' OR route LIKE '%responses%' OR provider_id LIKE '%codex%')".to_string());
            } else if lower == "claude" {
                conditions.push("(route LIKE '%claude%' OR route LIKE '%messages%' OR provider_id LIKE '%claude%' OR provider_id LIKE '%anthropic%')".to_string());
            } else if lower == "gemini" {
                conditions.push("(route LIKE '%gemini%' OR route LIKE '%models%' OR provider_id LIKE '%gemini%')".to_string());
            } else {
                conditions.push("(route LIKE ? OR provider_id LIKE ?)".to_string());
                params.push(rusqlite::types::Value::Text(format!("%{app}%")));
                params.push(rusqlite::types::Value::Text(format!("%{app}%")));
            }
        }
    }

    if let Some(ref provider) = filter.provider_id {
        if provider != "all" && !provider.is_empty() {
            conditions.push("provider_id = ?".to_string());
            params.push(rusqlite::types::Value::Text(provider.clone()));
        }
    }

    if let Some(ref model) = filter.model {
        if model != "all" && !model.is_empty() {
            conditions.push("model = ?".to_string());
            params.push(rusqlite::types::Value::Text(model.clone()));
        }
    }

    if let Some(ref status) = filter.status {
        if status == "success" {
            conditions.push("success = 1".to_string());
        } else if status == "error" || status == "failed" {
            conditions.push("success = 0".to_string());
        }
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", conditions.join(" AND "))
    };

    (where_clause, params)
}

#[derive(Debug, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LocalUsageSync {
    pub codex_imported: usize,
    pub harness_imported: usize,
    pub warnings: Vec<String>,
}

#[tauri::command]
pub async fn sync_local_usage(
    app: tauri::AppHandle,
    db: State<'_, database::Database>,
) -> Result<LocalUsageSync, String> {
    let paths = crate::app_paths(app)?;
    let database = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut result = LocalUsageSync::default();
        match crate::session_usage_codex::sync_codex_usage(&database) {
            Ok(value) => {
                result.codex_imported = value.events_imported;
                if value.invalid_events > 0 { result.warnings.push(format!("本次跳过 {} 条缺失、无效或无法计算增量的 Codex 用量记录；未将其计为免费请求。", value.invalid_events)); }
            },
            Err(error) => result.warnings.push(format!("Codex 用量读取失败：{error}")),
        }
        let home = std::path::Path::new(&paths.home_dir);
        let harness = crate::skills_local::agent_home(home, "DSH_HOME", ".dsh").and_then(|path| {
            crate::session_usage_harness::sync_root(&database, &path.join("sessions"))
        });
        match harness {
            Ok(value) => {
                result.harness_imported = value.imported;
                result.warnings.extend(value.warnings);
            }
            Err(error) => result
                .warnings
                .push(format!("DeepSeek Harness 用量读取失败：{error}")),
        }
        result
    })
    .await
    .map_err(|error| format!("本地用量同步任务失败：{error}"))
}

fn format_cost(cost: Option<f64>) -> String {
    cost.filter(|value| value.is_finite())
        .map(|value| format!("{value:.4}"))
        .unwrap_or_default()
}

#[tauri::command]
pub async fn get_usage_summary(
    filter: UsageFilter,
    db: State<'_, database::Database>,
) -> Result<UsageSummary, String> {
    let arc_conn = db.conn();
    let conn = arc_conn
        .lock()
        .map_err(|error| format!("数据库连接锁已损坏：{error}"))?;

    let (where_clause, params) = build_filter_clause(&filter);
    let sql = format!(
        "SELECT 
            COUNT(*), 
            COALESCE(SUM(success), 0), 
            COALESCE(AVG(CASE WHEN price_source IN ('codex_session','harness_session') THEN NULL ELSE latency_ms END), 0.0),
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            COALESCE(SUM(cache_read_tokens), 0),
            COALESCE(SUM(cache_creation_tokens), 0),
            SUM(cost),
            COUNT(*) - COUNT(cost)
         FROM request_events {where_clause}"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_ref: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
    let mut rows = stmt
        .query(params_ref.as_slice())
        .map_err(|e| e.to_string())?;

    let mut total = 0;
    let mut success = 0;
    let mut avg = 0.0;
    let mut input = 0;
    let mut output = 0;
    let mut cache_read = 0;
    let mut cache_creation = 0;
    let mut cost = None;
    let mut unknown_cost_requests = 0;

    if let Some(row) = rows.next().map_err(|e| e.to_string())? {
        total = row.get::<_, i64>(0).unwrap_or(0);
        success = row.get::<_, i64>(1).unwrap_or(0);
        avg = row.get::<_, f64>(2).unwrap_or(0.0);
        input = row.get::<_, i64>(3).unwrap_or(0);
        output = row.get::<_, i64>(4).unwrap_or(0);
        cache_read = row.get::<_, i64>(5).unwrap_or(0);
        cache_creation = row.get::<_, i64>(6).unwrap_or(0);
        cost = row.get::<_, Option<f64>>(7).unwrap_or(None);
        unknown_cost_requests = row.get::<_, i64>(8).unwrap_or(0);
    }

    Ok(UsageSummary {
        total_requests: total,
        unknown_cost_requests,
        success_count: success,
        total_input_tokens: input,
        total_output_tokens: output,
        total_cache_read_tokens: cache_read,
        total_cache_creation_tokens: cache_creation,
        total_cost_usd: format_cost(cost),
        avg_latency_ms: avg as i64,
    })
}

#[tauri::command]
pub async fn get_usage_logs(
    filter: UsageFilter,
    limit: u32,
    offset: u32,
    db: State<'_, database::Database>,
) -> Result<Vec<UsageLogEntry>, String> {
    let arc_conn = db.conn();
    let conn = arc_conn
        .lock()
        .map_err(|error| format!("数据库连接锁已损坏：{error}"))?;

    let (where_clause, mut params) = build_filter_clause(&filter);
    let sql = format!(
        "SELECT 
            id, 
            provider_id, 
            COALESCE(model, 'unknown'),
            COALESCE(input_tokens, 0),
            COALESCE(output_tokens, 0),
            COALESCE(cache_read_tokens, 0),
            COALESCE(cache_creation_tokens, 0),
            cost,
            latency_ms,
            COALESCE(status_code, 0),
            error_kind,
            created_at,
            route,
            audit_json,
            price_source,
            total_tokens,
            reasoning_tokens,
            success,
            price.input_cost,
            price.output_cost,
            price.cache_read_cost,
            price.cache_creation_cost
         FROM request_events LEFT JOIN request_price_snapshots AS price ON price.event_id=request_events.id {where_clause} 
         ORDER BY created_at DESC, id DESC LIMIT ? OFFSET ?"
    );

    params.push(rusqlite::types::Value::Integer(limit as i64));
    params.push(rusqlite::types::Value::Integer(offset as i64));

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_ref: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            let id: i64 = row.get(0)?;
            let provider_id: String = row.get(1)?;
            let model: String = row.get(2)?;
            let input_tokens: i64 = row.get(3)?;
            let output_tokens: i64 = row.get(4)?;
            let cache_read_tokens: i64 = row.get(5)?;
            let cache_creation_tokens: i64 = row.get(6)?;
            let cost: Option<f64> = row.get(7)?;
            let latency_ms: i64 = row.get(8).unwrap_or(0);
            let status_code: i64 = row.get(9).unwrap_or(200);
            let error_message: Option<String> = row.get(10)?;
            let created_at: i64 = row.get(11)?;
            let route: String = row.get(12).unwrap_or_default();
            let audit: Option<crate::model_audit::AuditRecord> = row
                .get::<_, Option<String>>(13)?
                .and_then(|json| serde_json::from_str(&json).ok());
            let mut source = row
                .get::<_, Option<String>>(14)?
                .unwrap_or_else(|| "proxy".into());
            let total_tokens: Option<i64> = row.get(15)?;
            let reasoning_tokens: Option<i64> = row.get(16)?;
            let is_streaming = audit
                .as_ref()
                .is_some_and(|value| value.observation.stream.is_some());

            let app_type = if route.starts_with("model_probe/") {
                source = "model_probe".into();
                "model_probe"
            } else if route == "session/deepseek_harness" {
                "deepseek_harness"
            } else if route.contains("claude") || route.contains("messages") {
                "claude"
            } else if route.contains("codex") || route.contains("responses") {
                "codex"
            } else if route.contains("gemini") {
                "gemini"
            } else {
                "proxy"
            }
            .to_string();

            Ok(UsageLogEntry {
                audit,
                request_id: id.to_string(),
                success: row.get::<_, i64>(17)? != 0,
                source,
                total_tokens,
                reasoning_tokens,
                provider_id,
                app_type,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                total_cost_usd: format_cost(cost),
                input_cost_usd: format_cost(row.get::<_, Option<f64>>(18)?),
                output_cost_usd: format_cost(row.get::<_, Option<f64>>(19)?),
                cache_read_cost_usd: format_cost(row.get::<_, Option<f64>>(20)?),
                cache_creation_cost_usd: format_cost(row.get::<_, Option<f64>>(21)?),
                model,
                cost_multiplier: if cost.is_some() {
                    "1.0".into()
                } else {
                    String::new()
                },
                is_streaming,
                latency_ms,
                status_code,
                error_message,
                created_at,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }

    Ok(result)
}

#[tauri::command]
pub async fn get_usage_trends(
    filter: UsageFilter,
    db: State<'_, database::Database>,
) -> Result<Vec<UsageTrendPoint>, String> {
    let arc_conn = db.conn();
    let conn = arc_conn
        .lock()
        .map_err(|error| format!("数据库连接锁已损坏：{error}"))?;

    query_usage_trends(&conn, &filter)
}

fn query_usage_trends(
    conn: &rusqlite::Connection,
    filter: &UsageFilter,
) -> Result<Vec<UsageTrendPoint>, String> {
    let (where_clause, params) = build_filter_clause(filter);

    let (bucket_ms, label_format) = match filter.date_range.as_deref() {
        Some("today") | Some("1d") | Some("yesterday") => (3_600_000_i64, "%m/%d %H:00"),
        Some("7d") => (4 * 3_600_000_i64, "%m/%d %H:00"),
        _ => (86_400_000_i64, "%m/%d"),
    };

    // SQLite's unix timestamps are UTC. Shift each event by the local offset before
    // flooring it into a bucket, then shift the bucket back so the API still returns
    // the original UTC millisecond timestamp. Computing the offset per event keeps
    // daylight-saving transitions correct on systems that use DST.
    let sql = format!(
        "WITH local_events AS (
            SELECT
                created_at,
                COALESCE(input_tokens, 0) AS input_tokens,
                COALESCE(output_tokens, 0) AS output_tokens,
                COALESCE(cache_read_tokens, 0) AS cache_read_tokens,
                COALESCE(cache_creation_tokens, 0) AS cache_creation_tokens,
                cost,
                (
                    CAST(strftime(
                        '%s',
                        datetime(created_at / 1000, 'unixepoch', 'localtime')
                    ) AS INTEGER) - (created_at / 1000)
                ) * 1000 AS local_offset
            FROM request_events {where_clause}
        ),
        bucketed AS (
            SELECT
                (((created_at + local_offset) / {bucket_ms}) * {bucket_ms} - local_offset) AS bucket,
                input_tokens,
                output_tokens,
                cache_read_tokens,
                cache_creation_tokens,
                cost
            FROM local_events
        )
        SELECT
            bucket,
            COALESCE(strftime('{label_format}', bucket / 1000, 'unixepoch', 'localtime'), '') AS label,
            SUM(input_tokens),
            SUM(output_tokens),
            SUM(cache_read_tokens),
            SUM(cache_creation_tokens),
            SUM(cost),
            COUNT(*)
        FROM bucketed
        GROUP BY bucket
        ORDER BY bucket ASC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_ref: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            let bucket: i64 = row.get(0)?;
            let label: String = row.get(1)?;
            let input: i64 = row.get(2)?;
            let output: i64 = row.get(3)?;
            let cache_read: i64 = row.get(4)?;
            let cache_creation: i64 = row.get(5)?;
            let cost: Option<f64> = row.get(6)?;
            let requests: i64 = row.get(7)?;

            Ok(UsageTrendPoint {
                timestamp: bucket,
                date: label.clone(),
                label,
                input_tokens: input,
                output_tokens: output,
                cache_read_tokens: cache_read,
                cache_creation_tokens: cache_creation,
                cost,
                requests,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_provider_stats(
    filter: UsageFilter,
    db: State<'_, database::Database>,
) -> Result<Vec<ProviderUsageStat>, String> {
    let arc_conn = db.conn();
    let conn = arc_conn
        .lock()
        .map_err(|error| format!("数据库连接锁已损坏：{error}"))?;

    let (where_clause, params) = build_filter_clause(&filter);
    let sql = format!(
        "SELECT 
            provider_id,
            COUNT(*),
            COALESCE(SUM(success), 0),
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            SUM(cost),
            COALESCE(AVG(CASE WHEN price_source IN ('codex_session','harness_session') THEN NULL ELSE latency_ms END), 0.0)
         FROM request_events {where_clause}
         GROUP BY provider_id
         ORDER BY COUNT(*) DESC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_ref: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            let provider_id: String = row.get(0)?;
            let total: i64 = row.get(1)?;
            let success: i64 = row.get(2)?;
            let input: i64 = row.get(3)?;
            let output: i64 = row.get(4)?;
            let cost: Option<f64> = row.get(5)?;
            let avg_latency: f64 = row.get(6)?;

            let success_rate = if total > 0 {
                (success as f64 / total as f64) * 100.0
            } else {
                100.0
            };

            Ok(ProviderUsageStat {
                provider_id,
                total_requests: total,
                success_rate,
                input_tokens: input,
                output_tokens: output,
                total_cost_usd: format_cost(cost),
                avg_latency_ms: avg_latency as i64,
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

#[tauri::command]
pub async fn get_model_stats(
    filter: UsageFilter,
    db: State<'_, database::Database>,
) -> Result<Vec<ModelUsageStat>, String> {
    let arc_conn = db.conn();
    let conn = arc_conn
        .lock()
        .map_err(|error| format!("数据库连接锁已损坏：{error}"))?;

    let (where_clause, params) = build_filter_clause(&filter);
    let sql = format!(
        "SELECT 
            COALESCE(model, 'unknown'),
            provider_id,
            COUNT(*),
            COALESCE(SUM(input_tokens), 0),
            COALESCE(SUM(output_tokens), 0),
            SUM(cost)
         FROM request_events {where_clause}
         GROUP BY COALESCE(model, 'unknown'), provider_id
         ORDER BY COUNT(*) DESC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let params_ref: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();

    let rows = stmt
        .query_map(params_ref.as_slice(), |row| {
            let model: String = row.get(0)?;
            let provider_id: String = row.get(1)?;
            let total: i64 = row.get(2)?;
            let input: i64 = row.get(3)?;
            let output: i64 = row.get(4)?;
            let cost: Option<f64> = row.get(5)?;

            Ok(ModelUsageStat {
                model,
                provider_id,
                total_requests: total,
                input_tokens: input,
                output_tokens: output,
                total_cost_usd: format_cost(cost),
            })
        })
        .map_err(|e| e.to_string())?;

    let mut result = Vec::new();
    for row in rows {
        result.push(row.map_err(|e| e.to_string())?);
    }
    Ok(result)
}

#[tauri::command]
pub async fn list_model_pricing(db: State<'_, database::Database>) -> Result<Vec<ModelPricing>, String> {
    crate::platform::require_desktop("模型定价管理")?;
    let database = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || model_pricing::list(&database)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn save_model_pricing(pricing: PricingInput, db: State<'_, database::Database>) -> Result<(), String> {
    crate::platform::require_desktop("模型定价管理")?;
    let database = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || model_pricing::save(&database, &pricing)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn delete_model_pricing(id: String, db: State<'_, database::Database>) -> Result<(), String> {
    crate::platform::require_desktop("模型定价管理")?;
    let database = db.inner().clone();
    tauri::async_runtime::spawn_blocking(move || model_pricing::remove_custom(&database, &id)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn sync_models_dev_pricing(db: State<'_, database::Database>) -> Result<i32, String> {
    crate::platform::require_desktop("模型定价管理")?;
    model_pricing::sync(db.inner().clone()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 返回本机当前的 UTC 偏移秒数（东八区为 28800）。
    fn local_offset_seconds(conn: &rusqlite::Connection) -> i64 {
        conn.query_row(
            "SELECT CAST(strftime('%s', 'now', 'localtime') AS INTEGER) \
             - CAST(strftime('%s', 'now') AS INTEGER)",
            [],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// 回归：`LOCAL_DAY_START_MS` 必须落在**真正的**本地午夜上。
    ///
    /// 这条断言与时区无关（任何时区下本地午夜的挂钟时间都是 00:00:00），但只有在
    /// 非 UTC 时区下才真正有鉴别力——在 UTC 机器上正确写法和漏了 `'utc'` 的错误
    /// 写法算出来的值完全相同。用户本机是 Asia/Shanghai，所以在那里这条测试有牙。
    #[test]
    fn local_day_start_lands_on_real_local_midnight() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let wall_clock: String = conn
            .query_row(
                &format!(
                    "SELECT strftime('%H:%M:%S', ({LOCAL_DAY_START_MS}) / 1000, \
                     'unixepoch', 'localtime')"
                ),
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            wall_clock, "00:00:00",
            "本地当天起点换算回本地挂钟时间必须正好是午夜，实际是 {wall_clock}"
        );
    }

    /// 回归：漏掉 `'utc'` 修饰符时，算出的起点会比真实本地午夜晚一个时区偏移量。
    /// 这条测试把那个陷阱本身钉住——在 UTC 机器上两种写法等价，所以偏移为 0 时跳过。
    #[test]
    fn dropping_the_utc_modifier_shifts_the_day_start_by_the_timezone_offset() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let offset = local_offset_seconds(&conn);
        if offset == 0 {
            return; // UTC 机器上无法区分，跳过而不是假装通过
        }

        const BUGGY: &str =
            "CAST(strftime('%s', 'now', 'localtime', 'start of day') AS INTEGER) * 1000";
        let (correct, buggy): (i64, i64) = conn
            .query_row(
                &format!("SELECT ({LOCAL_DAY_START_MS}), ({BUGGY})"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(
            buggy - correct,
            offset * 1000,
            "漏掉 'utc' 的写法应当正好晚一个时区偏移量"
        );
    }

    /// 回归：「昨天」必须是紧贴「今天」之前的、整整一个本地日的窗口。
    #[test]
    fn yesterday_window_is_exactly_one_local_day_before_today() {
        const DAY_MS: i64 = 86_400_000;
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        let (lower, upper): (i64, i64) = conn
            .query_row(
                &format!("SELECT (({LOCAL_DAY_START_MS}) - {DAY_MS}), ({LOCAL_DAY_START_MS})"),
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();

        assert_eq!(upper - lower, DAY_MS, "「昨天」窗口必须正好 24 小时");

        let lower_clock: String = conn
            .query_row(
                "SELECT strftime('%H:%M:%S', ?1 / 1000, 'unixepoch', 'localtime')",
                [lower],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(lower_clock, "00:00:00", "「昨天」下界也必须落在本地午夜");
    }

    #[test]
    fn trend_query_preserves_unknown_cost_without_hiding_real_zero() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE request_events (
                created_at INTEGER, input_tokens INTEGER, output_tokens INTEGER,
                cache_read_tokens INTEGER, cache_creation_tokens INTEGER, cost REAL
            );
            INSERT INTO request_events VALUES (1700000000000, 2984, 25, 2816, 0, NULL);
            INSERT INTO request_events VALUES (1700086400000, 12, 3, 0, 0, 0.0);
            INSERT INTO request_events VALUES (1700172800000, 24, 6, 0, 0, 0.25);",
        )
        .unwrap();
        let points = query_usage_trends(&conn, &UsageFilter::default()).unwrap();
        assert_eq!(points.len(), 3);
        assert_eq!(points[0].cost, None);
        assert_eq!(points[0].requests, 1);
        assert_eq!(points[0].input_tokens, 2984);
        assert_eq!(points[0].cache_read_tokens, 2816);
        assert!(serde_json::to_value(&points[0]).unwrap()["cost"].is_null());
        assert_eq!(points[1].cost, Some(0.0));
        assert_eq!(points[2].cost, Some(0.25));
    }

    #[test]
    fn harness_filter_is_exact_and_unknown_cost_is_not_zero() {
        let (sql, params) = build_filter_clause(&UsageFilter {
            app_type: Some("deepseek_harness".into()),
            ..Default::default()
        });
        assert_eq!(
            sql,
            "WHERE route NOT LIKE 'model_probe/%' AND route = 'session/deepseek_harness'"
        );
        assert!(params.is_empty());
        assert_eq!(format_cost(None), "");
        assert_eq!(format_cost(Some(0.0)), "0.0000");
        assert_eq!(format_cost(Some(f64::NAN)), "");
    }

}
