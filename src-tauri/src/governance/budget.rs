use super::*;
use crate::database::Provider;
use rusqlite::OptionalExtension;
use serde_json::{Value, json};
pub const KEY: &str = "hard_budget_v1";
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BudgetPolicy {
    pub enabled: bool,
    pub daily_tokens: i64,
    pub daily_microusd: i64,
    pub input_reservation: i64,
    pub max_output_tokens: i64,
}
impl Default for BudgetPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            daily_tokens: 100000,
            daily_microusd: 1000000,
            input_reservation: 4096,
            max_output_tokens: 512,
        }
    }
}
impl BudgetPolicy {
    pub fn validate(&self) -> Result<(), String> {
        if !(0..=1000000000).contains(&self.daily_tokens)
            || !(0..=1000000000000).contains(&self.daily_microusd)
            || !(256..=1000000).contains(&self.input_reservation)
            || !(1..=128000).contains(&self.max_output_tokens)
        {
            return Err("Invalid hard budget limits".into());
        }
        Ok(())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BudgetStatus {
    pub policy: BudgetPolicy,
    pub day_utc: i64,
    pub reserved_tokens: i64,
    pub reserved_microusd: i64,
    pub attempts: i64,
}
fn strict_text(value: &Value) -> bool {
    match value {
        Value::Object(m) => m.iter().all(|(k, v)| {
            !matches!(
                k.as_str(),
                "previous_response_id"
                    | "conversation"
                    | "file_id"
                    | "image_url"
                    | "input_audio"
                    | "audio"
                    | "file_data"
                    | "file_url"
                    | "cached_content"
                    | "cachedContent"
                    | "container"
                    | "mcp_servers"
                    | "context_management"
                    | "encrypted_content"
                    | "web_search_options"
                    | "search_parameters"
                    | "extra_body"
                    | "extra_headers"
                    | "inlineData"
                    | "fileData"
                    | "inline_data"
                    | "thinkingConfig"
                    | "prediction"
            ) && !(k == "type"
                && v.as_str().is_some_and(|s| {
                    s.contains("image")
                        || s.contains("audio")
                        || s.contains("file")
                        || s.contains("video")
                }))
                && strict_text(v)
        }),
        Value::Array(a) => a.iter().all(strict_text),
        _ => true,
    }
}
fn bound_payload(payload: &mut Value, format: &str, policy: &BudgetPolicy) -> Result<i64, String> {
    if !strict_text(payload) {
        return Err(
            "budget_unbounded_input: stateful, remote-tool and multimodal requests are blocked"
                .into(),
        );
    }
    let object = payload.as_object_mut().ok_or("budget_invalid_request")?;
    let allowed: &[&str] = match format {
        "chat" => &[
            "model",
            "messages",
            "max_tokens",
            "max_completion_tokens",
            "stream",
            "stream_options",
            "n",
            "temperature",
            "top_p",
            "stop",
            "presence_penalty",
            "frequency_penalty",
            "logit_bias",
            "logprobs",
            "top_logprobs",
            "seed",
            "user",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "response_format",
            "reasoning_effort",
            "verbosity",
            "metadata",
            "store",
            "service_tier",
        ],
        "responses" => &[
            "model",
            "input",
            "instructions",
            "max_output_tokens",
            "stream",
            "temperature",
            "top_p",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "text",
            "reasoning",
            "metadata",
            "store",
            "user",
            "truncation",
            "include",
            "service_tier",
            "safety_identifier",
            "prompt_cache_key",
        ],
        "anthropic" => &[
            "model",
            "messages",
            "system",
            "max_tokens",
            "stream",
            "temperature",
            "top_p",
            "top_k",
            "stop_sequences",
            "tools",
            "tool_choice",
            "metadata",
            "thinking",
            "output_config",
        ],
        _ => return Err("budget_unsupported_protocol".into()),
    };
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("budget_unbounded_parameter".into());
    }
    if object.get("service_tier").is_some_and(|v| v != "default") {
        return Err("budget_unknown_service_tier_price".into());
    }
    if object.get("n").is_some_and(|v| v.as_i64() != Some(1)) {
        return Err("budget_multiple_choices_blocked".into());
    }
    if let Some(tools) = object.get("tools") {
        let tools = tools.as_array().ok_or("budget_invalid_tools")?;
        for tool in tools {
            let local_function = match format {
                "anthropic" => {
                    tool.get("type").is_none()
                        && tool.get("name").and_then(Value::as_str).is_some()
                        && tool.get("input_schema").is_some()
                }
                _ => tool.get("type").and_then(Value::as_str) == Some("function"),
            };
            if !local_function {
                return Err("budget_server_tools_blocked".into());
            }
        }
    }
    let requested = ["max_tokens", "max_completion_tokens", "max_output_tokens"]
        .iter()
        .filter_map(|k| object.get(*k))
        .try_fold(policy.max_output_tokens, |cap, v| {
            v.as_i64()
                .filter(|v| *v > 0)
                .map(|v| cap.min(v))
                .ok_or("budget_invalid_output_limit")
        })?;
    if object
        .get("thinking")
        .and_then(|v| v.get("budget_tokens"))
        .is_some_and(|v| v.as_i64().is_none_or(|n| n < 0 || n >= requested))
    {
        return Err("budget_thinking_exceeds_output_cap".into());
    }
    for key in ["max_tokens", "max_completion_tokens", "max_output_tokens"] {
        object.remove(key);
    }
    let cap_key = match format {
        "chat" => "max_completion_tokens",
        "responses" => "max_output_tokens",
        _ => "max_tokens",
    };
    object.insert(cap_key.into(), json!(requested));
    if format == "chat" {
        object.insert("n".into(), json!(1));
    }
    // This is a conservative size admission check, NOT a tokenizer or a promise
    // that arbitrary upstream models cannot add their own hidden context.
    let bytes = serde_json::to_vec(payload)
        .map_err(|_| "budget_invalid_request")?
        .len() as i64;
    if bytes + 256 > policy.input_reservation {
        return Err("budget_input_reservation_exceeded".into());
    }
    Ok(requested)
}

/// Reserves BEFORE EVERY attempt. No automatic refunds, including timeout,
/// cancellation, failure and missing usage. The ledger survives app restart.
/// This is a local admission quota, not a guarantee of a provider's final invoice.
pub fn reserve(
    db: &Database,
    provider: &Provider,
    payload: &mut Value,
    format: &str,
    source: &str,
) -> Result<(), String> {
    let result = reserve_inner(db, provider, payload, format, source);
    if let Err(reason) = &result {
        // Only a static reason code is persisted, never a request or response body.
        let _ = event(
            db,
            "budget_rejected",
            reason.split(':').next().unwrap_or("budget_rejected"),
        );
    }
    result
}
fn reserve_inner(
    db: &Database,
    provider: &Provider,
    payload: &mut Value,
    format: &str,
    source: &str,
) -> Result<(), String> {
    let c = db.conn();
    let mut c = c.lock().map_err(|_| "budget_database_unavailable")?;
    let tx = c.transaction().map_err(|_| "budget_database_unavailable")?;
    let raw: Option<String> = tx
        .query_row("SELECT value FROM app_settings WHERE key=?1", [KEY], |r| {
            r.get(0)
        })
        .optional()
        .map_err(|_| "budget_database_unavailable")?;
    let p: BudgetPolicy = raw
        .map(|s| serde_json::from_str(&s).map_err(|_| "budget_policy_invalid"))
        .transpose()?
        .unwrap_or_default();
    p.validate()?;
    if !p.enabled {
        return if source == "synthetic" { Err("synthetic_budget_required".into()) } else { Ok(()) };
    }
    let requested = bound_payload(payload, format, &p)?;
    let model = payload
        .get("model")
        .and_then(Value::as_str)
        .ok_or("budget_model_missing")?;
    // Only an exact, explicitly saved price can authorize spend. No fuzzy/reference fallback.
    let rate:Option<(f64,f64,Option<f64>,Option<f64>)>=tx.query_row("SELECT input_per_million,output_per_million,cache_read_per_million,cache_creation_per_million FROM model_pricing WHERE id=?1 AND source='custom'",[model],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional().map_err(|_|"budget_pricing_unavailable")?;
    let (input, output, read, write) = rate.ok_or("budget_exact_custom_price_required")?;
    let rates = [input, output, read.unwrap_or(input), write.unwrap_or(input)];
    if rates.iter().any(|r| !r.is_finite() || *r < 0.0) {
        return Err("budget_invalid_price".into());
    }
    let input_rate = input.max(read.unwrap_or(input)).max(write.unwrap_or(input));
    let cost = (p.input_reservation as f64 * input_rate + requested as f64 * output).ceil();
    if !cost.is_finite() || cost > i64::MAX as f64 {
        return Err("budget_cost_overflow".into());
    }
    let cost = cost as i64;
    let tokens = p
        .input_reservation
        .checked_add(requested)
        .ok_or("budget_overflow")?;
    let day = now() / 86400;
    let (used_tokens,used_cost):(i64,i64)=tx.query_row("SELECT COALESCE(SUM(tokens),0),COALESCE(SUM(cost_microusd),0) FROM budget_ledger WHERE day=?1",[day],|r|Ok((r.get(0)?,r.get(1)?))).map_err(|_|"budget_ledger_unavailable")?;
    if used_tokens
        .checked_add(tokens)
        .is_none_or(|v| v > p.daily_tokens)
    {
        return Err("hard_token_budget_exhausted".into());
    }
    if used_cost
        .checked_add(cost)
        .is_none_or(|v| v > p.daily_microusd)
    {
        return Err("hard_cost_budget_exhausted".into());
    }
    tx.execute("INSERT INTO budget_ledger(id,day,provider_id,model,tokens,cost_microusd,source,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![uuid::Uuid::new_v4().to_string(),day,provider.id,model,tokens,cost,source,now()]).map_err(|_|"budget_ledger_unavailable")?;
    tx.commit().map_err(|_| "budget_ledger_unavailable".into())
}
pub fn status(db: &Database) -> Result<BudgetStatus, String> {
    let policy: BudgetPolicy = load(db, KEY)?;
    let day = now() / 86400;
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    let(t,m,a)=c.query_row("SELECT COALESCE(SUM(tokens),0),COALESCE(SUM(cost_microusd),0),COUNT(*) FROM budget_ledger WHERE day=?1",[day],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).map_err(|e|e.to_string())?;
    Ok(BudgetStatus {
        policy,
        day_utc: day,
        reserved_tokens: t,
        reserved_microusd: m,
        attempts: a,
    })
}
#[tauri::command]
pub fn get_hard_budget(db: tauri::State<'_, Database>) -> Result<BudgetStatus, String> {
    status(&db)
}
#[tauri::command]
pub fn save_hard_budget(
    policy: BudgetPolicy,
    confirmed: bool,
    db: tauri::State<'_, Database>,
) -> Result<(), String> {
    crate::platform::require_desktop("Governance changes")?;
    if !confirmed {
        return Err("Explicit confirmation required".into());
    }
    policy.validate()?;
    save(&db, KEY, &policy)?;
    event(
        &db,
        "budget_policy_changed",
        if policy.enabled {
            "enabled_without_reset"
        } else {
            "disabled_without_reset"
        },
    )
}

#[cfg(test)]
#[path = "budget_tests.rs"]
mod tests;
