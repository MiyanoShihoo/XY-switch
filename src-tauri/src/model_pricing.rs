use crate::{
    database::{Database, RequestUsage},
    session_usage_codex,
};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

const MODELS_DEV_URL: &str = "https://models.dev/api.json";
const MAX_DOWNLOAD_BYTES: usize = 32 * 1024 * 1024;
pub(crate) const MAX_PRICES: usize = 30_000;
const MAX_RATE: f64 = 1_000_000.0;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PricingInput {
    pub id: String,
    pub input_per_million: f64,
    pub output_per_million: f64,
    pub cache_read_per_million: Option<f64>,
    pub cache_creation_per_million: Option<f64>,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ModelPricing {
    #[serde(flatten)]
    pub pricing: PricingInput,
    pub source: String,
    pub updated_at: i64,
}

impl PricingInput {
    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty()
            || self.id.len() > 256
            || self.id.chars().any(|c| c.is_control() || c.is_whitespace())
        {
            return Err("模型定价 ID 必须为 1–256 字节，且不能含空白或控制字符".into());
        }
        for rate in [
            Some(self.input_per_million),
            Some(self.output_per_million),
            self.cache_read_per_million,
            self.cache_creation_per_million,
        ]
        .into_iter()
        .flatten()
        {
            if !rate.is_finite() || !(0.0..=MAX_RATE).contains(&rate) {
                return Err("模型价格必须是 0–1000000 之间的有限数值（USD / 百万 token）".into());
            }
        }
        Ok(())
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}

fn from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelPricing> {
    Ok(ModelPricing {
        pricing: PricingInput {
            id: row.get(0)?,
            input_per_million: row.get(1)?,
            output_per_million: row.get(2)?,
            cache_read_per_million: row.get(3)?,
            cache_creation_per_million: row.get(4)?,
        },
        source: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

pub fn list(database: &Database) -> Result<Vec<ModelPricing>, String> {
    let arc = database.conn();
    let conn = arc.lock().map_err(|_| "database lock poisoned")?;
    let mut stmt = conn.prepare("SELECT id,input_per_million,output_per_million,cache_read_per_million,cache_creation_per_million,source,updated_at FROM model_pricing ORDER BY (source='custom') DESC,id").map_err(|e| e.to_string())?;
    stmt.query_map([], from_row)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}

pub fn save(database: &Database, pricing: &PricingInput) -> Result<(), String> {
    let arc = database.conn();
    let conn = arc.lock().map_err(|_| "database lock poisoned")?;
    save_on(&conn, pricing)
}

pub(crate) fn save_on(conn: &Connection, pricing: &PricingInput) -> Result<(), String> {
    pricing.validate()?;
    let count: usize = conn
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE id<>?1",
            [&pricing.id],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if count >= MAX_PRICES {
        return Err("模型定价记录已达上限，请先移除不需要的自定义记录".into());
    }
    conn.execute("INSERT INTO model_pricing(id,input_per_million,output_per_million,cache_read_per_million,cache_creation_per_million,source,updated_at) VALUES(?1,?2,?3,?4,?5,'custom',?6) ON CONFLICT(id) DO UPDATE SET input_per_million=excluded.input_per_million,output_per_million=excluded.output_per_million,cache_read_per_million=excluded.cache_read_per_million,cache_creation_per_million=excluded.cache_creation_per_million,source='custom',updated_at=excluded.updated_at",
        params![pricing.id,pricing.input_per_million,pricing.output_per_million,pricing.cache_read_per_million,pricing.cache_creation_per_million,now_ms()]).map_err(|e| e.to_string())?;
    Ok(())
}

pub fn remove_custom(database: &Database, id: &str) -> Result<(), String> {
    PricingInput {
        id: id.into(),
        input_per_million: 0.0,
        output_per_million: 0.0,
        cache_read_per_million: None,
        cache_creation_per_million: None,
    }
    .validate()?;
    let arc = database.conn();
    let conn = arc.lock().map_err(|_| "database lock poisoned")?;
    let removed = conn
        .execute(
            "DELETE FROM model_pricing WHERE id=?1 AND source='custom'",
            [id],
        )
        .map_err(|e| e.to_string())?;
    if removed != 1 {
        return Err("未找到可移除的自定义定价；同步参考价不能直接删除".into());
    }
    Ok(())
}

fn parse_catalog(bytes: &[u8]) -> Result<Vec<PricingInput>, String> {
    if bytes.len() > MAX_DOWNLOAD_BYTES {
        return Err("models.dev 数据超过 32 MiB 上限".into());
    }
    let data: Value =
        serde_json::from_slice(bytes).map_err(|_| "models.dev 返回的价格目录不是有效 JSON")?;
    let providers = data
        .as_object()
        .filter(|p| p.len() <= 4096)
        .ok_or("models.dev 价格目录结构无效或超过供应商上限")?;
    let mut prices = Vec::new();
    let mut ids = BTreeSet::new();
    let mut scanned = 0;
    for (provider, value) in providers {
        let Some(models) = value.get("models").and_then(Value::as_object) else {
            continue;
        };
        for (key, model) in models {
            scanned += 1;
            if scanned > MAX_PRICES {
                return Err("models.dev 模型数量超过安全上限".into());
            }
            let Some(cost) = model.get("cost").and_then(Value::as_object) else {
                continue;
            };
            // This editor handles token prices, not media, per-request or tier-dependent rates.
            if cost.keys().any(|key| {
                !matches!(
                    key.as_str(),
                    "input" | "output" | "cache_read" | "cache_write"
                )
            }) {
                continue;
            }
            let (Some(input), Some(output)) = (cost.get("input"), cost.get("output")) else {
                continue;
            };
            let rate = |value: &Value| {
                value
                    .as_f64()
                    .ok_or_else(|| "models.dev 价格包含非数值字段，未更新本地目录".to_string())
            };
            let optional = |name| {
                cost.get(name)
                    .filter(|v| !v.is_null())
                    .map(rate)
                    .transpose()
            };
            let id = model.get("id").and_then(Value::as_str).unwrap_or(key);
            // Provider qualification avoids applying a reseller's price to another provider's bare model ID.
            let pricing = PricingInput {
                id: format!("{provider}/{id}"),
                input_per_million: rate(input)?,
                output_per_million: rate(output)?,
                cache_read_per_million: optional("cache_read")?,
                cache_creation_per_million: optional("cache_write")?,
            };
            pricing.validate()?;
            if !ids.insert(pricing.id.clone()) {
                return Err("models.dev 目录包含重复模型 ID，未更新本地目录".into());
            }
            prices.push(pricing);
        }
    }
    if prices.is_empty() {
        return Err("models.dev 未提供可用的基础 token 价格，已保留原有定价".into());
    }
    Ok(prices)
}

async fn download_catalog(
    client: &reqwest::Client,
    url: &str,
) -> Result<Vec<PricingInput>, String> {
    let mut response = client
        .get(url)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(crate::network::describe_error)?;
    if response.status() != reqwest::StatusCode::OK {
        return Err(format!(
            "models.dev 返回 HTTP {}，未更新本地定价",
            response.status().as_u16()
        ));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_DOWNLOAD_BYTES as u64)
    {
        return Err("models.dev 数据超过 32 MiB 上限".into());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(crate::network::describe_error)?
    {
        if bytes.len().saturating_add(chunk.len()) > MAX_DOWNLOAD_BYTES {
            return Err("models.dev 数据超过 32 MiB 上限".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    parse_catalog(&bytes)
}

fn replace_synced(database: &Database, prices: &[PricingInput]) -> Result<i32, String> {
    if prices.is_empty() || prices.len() > MAX_PRICES {
        return Err("同步价格数量无效，未更新本地目录".into());
    }
    let mut ids = BTreeSet::new();
    for price in prices {
        price.validate()?;
        if !ids.insert(&price.id) {
            return Err("同步价格包含重复 ID".into());
        }
    }
    let arc = database.conn();
    let mut conn = arc.lock().map_err(|_| "database lock poisoned")?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    let custom_count: usize = tx
        .query_row(
            "SELECT COUNT(*) FROM model_pricing WHERE source='custom'",
            [],
            |row| row.get(0),
        )
        .map_err(|e| e.to_string())?;
    if custom_count.saturating_add(prices.len()) > MAX_PRICES {
        return Err("同步后价格数量超过上限，已保留原有目录".into());
    }
    tx.execute("DELETE FROM model_pricing WHERE source='models.dev'", [])
        .map_err(|e| e.to_string())?;
    let updated = now_ms();
    let mut count = 0;
    for price in prices {
        count += tx.execute("INSERT INTO model_pricing(id,input_per_million,output_per_million,cache_read_per_million,cache_creation_per_million,source,updated_at) VALUES(?1,?2,?3,?4,?5,'models.dev',?6) ON CONFLICT(id) DO NOTHING",
            params![price.id,price.input_per_million,price.output_per_million,price.cache_read_per_million,price.cache_creation_per_million,updated]).map_err(|e| e.to_string())?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    Ok(count as i32)
}

pub async fn sync(database: Database) -> Result<i32, String> {
    let client = crate::network::client_builder()
        .timeout(Duration::from_secs(25))
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(crate::network::describe_error)?;
    let prices = download_catalog(&client, MODELS_DEV_URL).await?;
    tauri::async_runtime::spawn_blocking(move || replace_synced(&database, &prices))
        .await
        .map_err(|e| e.to_string())?
}

#[derive(Debug, Clone, PartialEq)]
pub struct PriceEstimate {
    pub input: Option<f64>,
    pub output: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_creation: Option<f64>,
    pub total: Option<f64>,
    pub source: String,
    pub version: String,
}

pub fn estimate(conn: &Connection, usage: &RequestUsage) -> Result<Option<PriceEstimate>, String> {
    let Some(model) = usage.model.as_deref().filter(|id| !id.is_empty()) else {
        return Ok(None);
    };
    let stored = conn.query_row("SELECT id,input_per_million,output_per_million,cache_read_per_million,cache_creation_per_million,source,updated_at FROM model_pricing WHERE id=?1", [model], from_row).optional().map_err(|e| e.to_string())?;
    let (rates, source, version) = if let Some(row) = stored {
        row.pricing.validate()?;
        let version = format!("{}:{}:{}", row.source, row.pricing.id, row.updated_at);
        (row.pricing, row.source, version)
    } else if let Some(rates) = session_usage_codex::model_pricing(model) {
        (
            PricingInput {
                id: model.into(),
                input_per_million: rates.input_per_million,
                output_per_million: rates.output_per_million,
                cache_read_per_million: Some(rates.cache_read_per_million),
                cache_creation_per_million: None,
            },
            "builtin-reference".into(),
            "builtin-reference-v1".into(),
        )
    } else {
        return Ok(None);
    };
    let read = usage.cache_read_tokens.unwrap_or(0);
    let write = usage.cache_creation_tokens.unwrap_or(0);
    let input = usage.input_tokens.filter(|n| *n >= 0);
    let output = usage.output_tokens.filter(|n| *n >= 0);
    let anthropic = usage
        .audit
        .as_ref()
        .and_then(|a| a.observation.protocol.as_deref())
        == Some("anthropic");
    let billable_input = if anthropic {
        input
    } else {
        input
            .and_then(|input| {
                read.checked_add(write)
                    .and_then(|cached| input.checked_sub(cached))
            })
            .filter(|n| *n >= 0)
    };
    let component = |tokens: Option<i64>, rate: Option<f64>| {
        tokens.filter(|n| *n >= 0).and_then(|n| {
            if n == 0 {
                Some(0.0)
            } else {
                rate.map(|r| n as f64 * r / 1_000_000.0)
                    .filter(|v| v.is_finite())
            }
        })
    };
    let mut estimate = PriceEstimate {
        input: component(billable_input, Some(rates.input_per_million)),
        output: component(output, Some(rates.output_per_million)),
        cache_read: component(Some(read), rates.cache_read_per_million),
        cache_creation: component(Some(write), rates.cache_creation_per_million),
        total: None,
        source,
        version,
    };
    estimate.total = estimate
        .input
        .zip(estimate.output)
        .zip(estimate.cache_read)
        .zip(estimate.cache_creation)
        .map(|(((input, output), read), write)| input + output + read + write)
        .filter(|n| n.is_finite());
    // Missing final usage or a broken stream cannot be reported as a complete bill.
    if usage.audit.as_ref().is_some_and(|a| {
        a.observation.parse_invalid || (a.observation.streaming && !a.observation.usage_final)
    }) {
        estimate.total = None;
    }
    Ok(Some(estimate))
}

pub fn save_snapshot(
    conn: &Connection,
    event_id: i64,
    estimate: Option<&PriceEstimate>,
) -> Result<(), String> {
    if let Some(price) = estimate {
        conn.execute("INSERT INTO request_price_snapshots(event_id,input_cost,output_cost,cache_read_cost,cache_creation_cost,source,version) VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(event_id) DO UPDATE SET input_cost=excluded.input_cost,output_cost=excluded.output_cost,cache_read_cost=excluded.cache_read_cost,cache_creation_cost=excluded.cache_creation_cost,source=excluded.source,version=excluded.version", params![event_id,price.input,price.output,price.cache_read,price.cache_creation,price.source,price.version]).map_err(|e| e.to_string())?;
    } else {
        conn.execute(
            "DELETE FROM request_price_snapshots WHERE event_id=?1",
            [event_id],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
