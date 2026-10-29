//! xAI 原生 Responses 端点的请求清洗。
//!
//! 对应 DEBUG.txt 第 35 条：Grok 经 xAI 的 `/v1/responses` 直连时有四个独立故障。
//! xAI 的 Responses 实现比 OpenAI 严格得多，以下四类请求体会被它直接 400，
//! 而客户端（Codex / Claude Code）自己不会改：
//!
//! 1. 函数工具的 `parameters` 根级带 `oneOf` / `anyOf` —— xAI 只认单个 object schema。
//! 2. 历史 `function_call` 的 `arguments` 里整数被写成了 `1.0` 这种浮点 ——
//!    严格 schema 校验会判类型不符。
//! 3. `agent_message` 这类 OpenAI 私有输入项、以及 `encrypted_content` 加密推理块 ——
//!    xAI 不认识这些 type / 字段。
//! 4. 客户端发来的模型名不在供应商映射表里时被无条件改写成供应商配置的模型 ——
//!    `grok-*` 系列应当原样放行；另外 `reasoning.effort` 的档位在原生直通这条路上
//!    从来没被夹过（`clamp_effort_for_model` 只在 `openai_chat_to_responses` 里调），
//!    实测 xAI 对 `max` 报 400。
//!
//! **为什么单独开一个模块**：这些全是 xAI 一家的怪癖，混进 `adapters.rs` 会污染
//! 通用协议转换层。收在一个「原生 Responses 闸门后」的独立文件里，将来上游
//! （CC Switch）自己覆盖了同样的场景，可以整文件 rebase 或 cherry-pick 掉。
//!
//! **闸门**：调用方必须先确认 `EffectiveUpstream::OpenAiResponses`，再用
//! [`is_xai_target`] 判断是不是 xAI/Grok。DeepSeek 官方 Codex 同样走原生 Responses，
//! 它不需要也不应该被这里改写。

use crate::adapters::{self, EffortClamp};
use serde_json::{Map, Value};

/// f64 能精确表示的整数上界（2^53）。超过这个值的整数经过 JSON 浮点往返
/// 就已经不保真了，但这里只用它做诊断信息，不用来拒绝——精度是上游丢的，
/// 我们能做的最好结果就是把当前这个 f64 所代表的那个确切整数写回去。
const F64_EXACT_INT_LIMIT: f64 = 9_007_199_254_740_992.0;
/// 2^64。`f as u64` 在这里开始饱和：2^64 会被截成 u64::MAX = 2^64-1，
/// 也就是 DEBUG.txt 说的「静默差一」。所以到这个值就必须报错而不是转换。
const U64_UPPER_EXCLUSIVE: f64 = 18_446_744_073_709_551_616.0;
/// -2^63，i64 的下界，本身可精确表示，所以是闭区间端点。
const I64_LOWER_INCLUSIVE: f64 = -9_223_372_036_854_775_808.0;

/// 这个供应商是不是应该走 xAI 清洗。
///
/// 两个信号取或：端点主机是 xAI 官方域，或者最终发给上游的模型名是 `grok-*`。
/// 后者是为了照顾中转站——很多人用第三方地址转发 Grok，主机名完全看不出来。
/// 反过来，DeepSeek 官方 Codex 这类原生 Responses 供应商两个信号都不命中，
/// 不会被误伤。
pub(crate) fn is_xai_target(endpoint: &str, model: &str) -> bool {
    is_xai_host(endpoint) || adapters::bare_model_name(model).starts_with("grok")
}

/// 只看主机名判断是不是 xAI 官方域。
///
/// 刻意不用「包含 xai 子串」这种宽松写法：`maxai.com` 之类的域名会被误判。
fn is_xai_host(endpoint: &str) -> bool {
    let lower = endpoint.trim().to_ascii_lowercase();
    let without_scheme = lower.split_once("//").map_or(lower.as_str(), |(_, rest)| rest);
    let authority = without_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    // 去掉 userinfo 和端口。
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host)
        .split(':')
        .next()
        .unwrap_or_default();
    host == "x.ai" || host.ends_with(".x.ai") || host == "xai.com" || host.ends_with(".xai.com")
}

/// 第 4 条的前半段：客户端发来的模型名在映射表里找不到时，要不要原样放行。
///
/// 原本 `prepare_payload` 的行为是「没命中映射就一律改写成供应商配置的模型」，
/// 于是在 xAI 卡上请求 `grok-4.6-fast` 会被悄悄换成卡里配的 `grok-4.5`——
/// 用户以为在用新模型，账单和能力却是旧的。
///
/// 放行条件收得很紧，必须同时满足：
/// - 请求的模型（去掉 `vendor/` 前缀后）以 `grok` 开头；
/// - 这张卡确实是 xAI 目标（官方域，或者卡里配的就是 grok 模型）。
///
/// 第二个条件是防止「在一张 Claude 卡上发 grok-x 就能绕过模型锁定」。
pub(crate) fn passthrough_model(
    endpoint: &str,
    configured_model: &str,
    requested: Option<&str>,
) -> Option<String> {
    let requested = requested?.trim();
    if requested.is_empty() {
        return None;
    }
    if !adapters::bare_model_name(requested).starts_with("grok") {
        return None;
    }
    if !is_xai_target(endpoint, configured_model) {
        return None;
    }
    Some(requested.to_string())
}

/// 就地清洗一个**已经是 Responses 形状**的请求体。
///
/// 调用点必须在模型映射之后（DEBUG.txt 明确要求「映射先于清洗」：`reasoning.effort`
/// 要按最终模型夹，不是按客户端报的模型），在 `body_overrides` 之前
/// （用户显式写的覆盖值优先级最高，不该被我们改回去）。
///
/// 返回 `Err` 只有一种情况：`function_call` 参数里出现了无法无损转换的整数，
/// 详见 [`rewrite_integral_floats`]。其余一律尽力而为，不阻断请求。
pub(crate) fn sanitize_request(payload: &mut Value, model: &str) -> Result<(), String> {
    let Some(object) = payload.as_object_mut() else {
        return Ok(());
    };
    clamp_reasoning_effort(object, model);
    fold_tool_schemas(object);
    sanitize_input_items(object)?;
    Ok(())
}

// ---------------------------------------------------------------------------
// 第 4 条后半段：reasoning.effort 夹档
// ---------------------------------------------------------------------------

/// 原生 Responses 直通时 `reasoning.effort` 是客户端原样发过来的，
/// 中间没有任何一层调过 `clamp_effort_for_model`（它只在
/// `openai_chat_to_responses` 里被调用）。而 Codex 不会把目录外的档位夹回范围，
/// 所以 `/effort max` 到了 grok-4.5 上就是一个 HTTP 400。
fn clamp_reasoning_effort(object: &mut Map<String, Value>, model: &str) {
    let Some(reasoning) = object.get_mut("reasoning").and_then(Value::as_object_mut) else {
        return;
    };
    // 先取成 owned：下面要在同一个 map 上做 insert/remove，
    // 留着一个指向它内部的 &str 只会给借用检查器添堵。
    let Some(current) = reasoning
        .get("effort")
        .and_then(Value::as_str)
        .map(str::to_string)
    else {
        return;
    };
    match adapters::clamp_effort_str(&current, model) {
        EffortClamp::Keep(clamped) => {
            if clamped != current {
                reasoning.insert("effort".to_string(), Value::String(clamped.to_string()));
            }
        }
        EffortClamp::Drop => {
            reasoning.remove("effort");
        }
        // 不认识的档位原样交给上游，不猜。
        EffortClamp::Unknown => {}
    }
    // `reasoning: {}` 有的网关也会挑刺，空了就整个拿掉。
    if reasoning.is_empty() {
        object.remove("reasoning");
    }
}

// ---------------------------------------------------------------------------
// 第 1 条：根级 oneOf / anyOf 折叠
// ---------------------------------------------------------------------------

/// 遍历 `tools`，把每个函数工具 `parameters` 根级的 `oneOf` / `anyOf` 折叠掉。
///
/// 同时兼容两种工具形状：Responses 的扁平形状 `{type,name,parameters}`
/// 和 Chat 的嵌套形状 `{type,function:{name,parameters}}`。虽然走到这里的
/// 理论上都是前者，但客户端直通时什么都可能发过来。
fn fold_tool_schemas(object: &mut Map<String, Value>) {
    let Some(tools) = object.get_mut("tools").and_then(Value::as_array_mut) else {
        return;
    };
    for tool in tools.iter_mut() {
        let Some(tool_object) = tool.as_object_mut() else {
            continue;
        };
        if let Some(parameters) = tool_object.get_mut("parameters") {
            fold_root_union(parameters);
        }
        if let Some(function) = tool_object.get_mut("function").and_then(Value::as_object_mut)
            && let Some(parameters) = function.get_mut("parameters")
        {
            fold_root_union(parameters);
        }
    }
}

/// 把 `{"oneOf":[A,B]}` 折成一个 object schema。
///
/// **`required` 取交集不是并集**，这是整条实现里唯一容易写错、写错了还很难发现的地方：
/// `oneOf:[{required:[a]},{required:[b]}]` 的语义是「要么有 a，要么有 b」，
/// 取并集会变成「a 和 b 都必须有」，直接把一批合法调用判死；取交集则退化为
/// 「都不强制」，最坏情况是校验放松，模型少填一个参数会被上游自己的业务逻辑挡下来，
/// 不会凭空 400。放松优于收紧。
///
/// 只处理**根级**。嵌套在某个 property 里的 `oneOf` 保持原样：那属于 schema 的正常
/// 表达能力，xAI 报错的场景是根级，没有实据的地方不动手。
fn fold_root_union(parameters: &mut Value) {
    let Some(schema) = parameters.as_object_mut() else {
        return;
    };
    // 只折 oneOf / anyOf。**故意不碰 allOf**：allOf 的语义是「同时满足所有分支」，
    // 它的 required 该取并集，和这里的交集逻辑正好相反，顺手加进来就是写反。
    // DEBUG.txt 报的也只有 oneOf / anyOf 这两个，没有实据的不动。
    let raw = match schema.remove("oneOf") {
        Some(value) => Some(value),
        None => schema.remove("anyOf"),
    };
    let branches = match raw {
        Some(Value::Array(branches)) => branches,
        // 不是数组（畸形）就当没有过：字段已经被 remove 掉，反而更干净。
        Some(_) => {
            ensure_object_schema(schema);
            return;
        }
        None => return,
    };
    // 两个键同时出现时上面只吃掉了一个，剩下的也要清掉，否则白折腾。
    schema.remove("oneOf");
    schema.remove("anyOf");

    let mut merged_properties = schema
        .get("properties")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    // 根级自带的 required 是所有分支共有的前提，作为交集的起点。
    let mut required: Option<Vec<String>> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        });
    let mut all_branches_closed = !branches.is_empty();

    for branch in &branches {
        let Some(branch_object) = branch.as_object() else {
            continue;
        };
        if let Some(properties) = branch_object.get("properties").and_then(Value::as_object) {
            for (key, value) in properties {
                // 先出现的分支优先：根级已声明的属性定义最权威，不被分支覆盖。
                merged_properties
                    .entry(key.clone())
                    .or_insert_with(|| value.clone());
            }
        }
        // 分支没写 required 就等于「这个分支什么都不强制」，交集必然为空。
        let branch_required: Vec<String> = branch_object
            .get("required")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        required = Some(match required {
            Some(current) => current
                .into_iter()
                .filter(|key| branch_required.contains(key))
                .collect(),
            None => branch_required,
        });
        if branch_object.get("additionalProperties") != Some(&Value::Bool(false)) {
            all_branches_closed = false;
        }
    }

    if merged_properties.is_empty() {
        schema.remove("properties");
    } else {
        schema.insert("properties".to_string(), Value::Object(merged_properties));
    }
    match required {
        Some(keys) if !keys.is_empty() => {
            schema.insert(
                "required".to_string(),
                Value::Array(keys.into_iter().map(Value::String).collect()),
            );
        }
        // 交集为空就必须把 required 删掉，留一个空数组有的校验器也不认。
        _ => {
            schema.remove("required");
        }
    }
    // 只有在所有分支都显式封闭、且根级自己没表态时才补 false，不凭空收紧。
    if all_branches_closed && !schema.contains_key("additionalProperties") {
        schema.insert("additionalProperties".to_string(), Value::Bool(false));
    }
    ensure_object_schema(schema);
}

fn ensure_object_schema(schema: &mut Map<String, Value>) {
    schema
        .entry("type".to_string())
        .or_insert_with(|| Value::String("object".to_string()));
}

// ---------------------------------------------------------------------------
// 第 2、3 条：input 项清洗
// ---------------------------------------------------------------------------

fn sanitize_input_items(object: &mut Map<String, Value>) -> Result<(), String> {
    let Some(items) = object.get_mut("input").and_then(Value::as_array_mut) else {
        return Ok(());
    };
    let mut rewritten: Vec<Value> = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        match sanitize_input_item(item)? {
            Some(item) => rewritten.push(item),
            None => continue,
        }
    }
    *items = rewritten;
    Ok(())
}

/// 返回 `None` 表示这一项应当整个丢弃。
fn sanitize_input_item(mut item: Value) -> Result<Option<Value>, String> {
    let Some(object) = item.as_object_mut() else {
        return Ok(Some(item));
    };
    let item_type = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // ---- 第 3 条前半段：agent_message ----
    if item_type == "agent_message" {
        return Ok(rewrite_agent_message(object));
    }

    // ---- 第 3 条后半段：encrypted_content ----
    // 这是 OpenAI 私有的加密推理块，对 xAI 就是一坨它解不开的密文。
    // 删字段是无疑义的正确动作；有 summary 文本就把 summary 留下当作可读上下文。
    if object.contains_key("encrypted_content") {
        object.remove("encrypted_content");
        if item_type == "reasoning" && !reasoning_has_text(object) {
            // 一个既没有密文也没有摘要的 reasoning 项不携带任何信息，
            // 而部分网关会对空 reasoning 项报错，直接丢掉。
            return Ok(None);
        }
    }

    // ---- 第 2 条：整数浮点改写 ----
    if item_type == "function_call" {
        let completed = object
            .get("status")
            .and_then(Value::as_str)
            .is_none_or(|status| status == "completed");
        if completed
            && let Some(arguments) = object.get("arguments").and_then(Value::as_str)
        {
            // 解析不了就原样留着：流式中断产生的半截 JSON 不该在这里被当成错误。
            if let Ok(mut parsed) = serde_json::from_str::<Value>(arguments) {
                let changed = rewrite_integral_floats(&mut parsed)?;
                if changed {
                    let encoded = serde_json::to_string(&parsed)
                        .map_err(|error| format!("Failed to re-encode tool arguments: {error}"))?;
                    object.insert("arguments".to_string(), Value::String(encoded));
                }
            }
        }
    }

    Ok(Some(item))
}

fn reasoning_has_text(object: &Map<String, Value>) -> bool {
    ["summary", "content"].iter().any(|key| {
        object
            .get(*key)
            .map(|value| match value {
                Value::Array(items) => !items.is_empty(),
                Value::String(text) => !text.trim().is_empty(),
                Value::Null => false,
                _ => true,
            })
            .unwrap_or(false)
    })
}

/// `agent_message` 是 OpenAI 侧的私有输入项，xAI 不认这个 type，整条请求 400。
/// 改写成一条普通 user 消息，把能提取到的文本原样带过去。
///
/// 提不出文本就把整项丢掉，而不是塞一条空消息，也不是把原始 JSON 序列化进正文——
/// 前者会让上游看到一条没有内容的 user 轮次，后者等于往模型上下文里灌结构化垃圾。
fn rewrite_agent_message(object: &Map<String, Value>) -> Option<Value> {
    let text = extract_text(object.get("content"))
        .or_else(|| extract_text(object.get("text")))
        .or_else(|| extract_text(object.get("message")))?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    let role = object
        .get("role")
        .and_then(Value::as_str)
        .filter(|role| matches!(*role, "user" | "assistant" | "system" | "developer"))
        .unwrap_or("user");
    // assistant 轮次的文本部件叫 output_text，其余叫 input_text，弄反了同样会 400。
    let part_type = if role == "assistant" {
        "output_text"
    } else {
        "input_text"
    };
    Some(serde_json::json!({
        "type": "message",
        "role": role,
        "content": [{"type": part_type, "text": trimmed}],
    }))
}

/// 从字符串、字符串数组、或 `{text}` / `{content}` 部件数组里尽力抠出纯文本。
fn extract_text(value: Option<&Value>) -> Option<String> {
    match value? {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => {
            let collected: Vec<String> = parts
                .iter()
                .filter_map(|part| match part {
                    Value::String(text) => Some(text.clone()),
                    Value::Object(object) => object
                        .get("text")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .or_else(|| {
                            object
                                .get("content")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                        }),
                    _ => None,
                })
                .filter(|text| !text.is_empty())
                .collect();
            if collected.is_empty() {
                None
            } else {
                Some(collected.join("\n"))
            }
        }
        Value::Object(object) => object
            .get("text")
            .and_then(Value::as_str)
            .map(str::to_string),
        _ => None,
    }
}

/// 把 JSON 里「值是浮点但小数部分为 0」的数字改回整数，返回是否真的改过。
///
/// 起因：工具参数在客户端那边被 JSON 解析成 f64 再重新序列化，`1` 就变成了 `1.0`。
/// OpenAI 不在乎，xAI 的严格 schema 校验会判 `integer` 类型不符。
///
/// **越界必须报错，不能静默转换**：`f as u64` 在 Rust 里是饱和转换，
/// 恰好 2^64 会被截成 `u64::MAX`（2^64−1），也就是凭空差一。工具参数里差一
/// 可能就是删错一条记录、转错一笔钱，宁可让这次请求失败并把值打到错误信息里。
fn rewrite_integral_floats(value: &mut Value) -> Result<bool, String> {
    match value {
        Value::Object(object) => {
            let mut changed = false;
            for (_, child) in object.iter_mut() {
                changed |= rewrite_integral_floats(child)?;
            }
            Ok(changed)
        }
        Value::Array(items) => {
            let mut changed = false;
            for child in items.iter_mut() {
                changed |= rewrite_integral_floats(child)?;
            }
            Ok(changed)
        }
        Value::Number(number) => {
            // 本来就是整数字面量（is_i64 / is_u64）的不碰，只处理浮点。
            if !number.is_f64() {
                return Ok(false);
            }
            let Some(raw) = number.as_f64() else {
                return Ok(false);
            };
            if !raw.is_finite() || raw.fract() != 0.0 {
                return Ok(false);
            }
            if raw >= 0.0 && raw < U64_UPPER_EXCLUSIVE {
                *value = Value::Number((raw as u64).into());
                Ok(true)
            } else if raw < 0.0 && raw >= I64_LOWER_INCLUSIVE {
                *value = Value::Number((raw as i64).into());
                Ok(true)
            } else {
                Err(format!(
                    "Tool call argument {raw} is an integral value outside the range this proxy \
                     can forward without silently changing it (must be within -2^63 .. 2^64-1; \
                     values at or beyond 2^64 saturate). Values above {F64_EXACT_INT_LIMIT} were \
                     already imprecise before reaching the proxy."
                ))
            }
        }
        _ => Ok(false),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ---- 闸门 ----

    #[test]
    fn detects_xai_targets_without_matching_lookalike_hosts() {
        assert!(is_xai_target("https://api.x.ai/v1", "grok-4.5"));
        assert!(is_xai_target("https://API.X.AI/v1/", "whatever"));
        assert!(is_xai_target("https://api.xai.com/v1", "whatever"));
        // 中转站：主机名看不出来，靠模型名认出来。
        assert!(is_xai_target("https://relay.example.com/v1", "grok-4.6-fast"));
        assert!(is_xai_target("https://relay.example.com/v1", "xai/grok-4.5"));
        // 不能误伤：DeepSeek 官方 Codex 也是原生 Responses。
        assert!(!is_xai_target("https://api.deepseek.com/v1", "deepseek-v4-pro"));
        // 「包含 xai 子串」的写法会在这里翻车，所以我们按主机名精确判断。
        assert!(!is_xai_target("https://maxai.com/v1", "gpt-5.6"));
        assert!(!is_xai_target("https://api.openai.com/v1", "gpt-5.6"));
    }

    #[test]
    fn passes_through_grok_models_only_on_xai_cards() {
        assert_eq!(
            passthrough_model("https://api.x.ai/v1", "grok-4.5", Some("grok-4.6-fast")),
            Some("grok-4.6-fast".to_string())
        );
        // 中转站 + 卡里配的就是 grok，也放行。
        assert_eq!(
            passthrough_model("https://relay.example.com/v1", "grok-4.5", Some("grok-build-1")),
            Some("grok-build-1".to_string())
        );
        // 非 grok 模型不放行，仍然落到供应商配置的模型。
        assert_eq!(
            passthrough_model("https://api.x.ai/v1", "grok-4.5", Some("gpt-5.6")),
            None
        );
        // 在一张非 xAI 的卡上发 grok-*，不能借此绕过模型锁定。
        assert_eq!(
            passthrough_model("https://api.deepseek.com/v1", "deepseek-v4-pro", Some("grok-4.5")),
            None
        );
        assert_eq!(passthrough_model("https://api.x.ai/v1", "grok-4.5", None), None);
        assert_eq!(
            passthrough_model("https://api.x.ai/v1", "grok-4.5", Some("   ")),
            None
        );
    }

    // ---- 第 4 条后半段：effort 夹档 ----

    #[test]
    fn clamps_max_effort_down_for_grok() {
        let mut payload = json!({"model":"grok-4.5","reasoning":{"effort":"max","summary":"auto"}});
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        assert_eq!(payload["reasoning"]["effort"], json!("xhigh"));
        // 同层的其他字段不能被顺手清掉。
        assert_eq!(payload["reasoning"]["summary"], json!("auto"));
    }

    #[test]
    fn keeps_supported_efforts_and_drops_unsupported_models() {
        let mut payload = json!({"reasoning":{"effort":"xhigh"}});
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        assert_eq!(payload["reasoning"]["effort"], json!("xhigh"));

        // gpt-5.6 明确支持 max，不该被夹。
        let mut payload = json!({"reasoning":{"effort":"max"}});
        sanitize_request(&mut payload, "gpt-5.6").unwrap();
        assert_eq!(payload["reasoning"]["effort"], json!("max"));

        // 不支持推理强度的模型：整个 reasoning 空了就删掉。
        let mut payload = json!({"reasoning":{"effort":"high"}});
        sanitize_request(&mut payload, "grok-2-vision").unwrap();
        assert!(payload.get("reasoning").is_none());

        // 不认识的档位原样保留，不猜。
        let mut payload = json!({"reasoning":{"effort":"ultra"}});
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        assert_eq!(payload["reasoning"]["effort"], json!("ultra"));
    }

    // ---- 第 1 条：oneOf / anyOf 折叠 ----

    #[test]
    fn folds_root_one_of_and_intersects_required() {
        let mut payload = json!({
            "tools": [{
                "type": "function",
                "name": "search",
                "parameters": {
                    "oneOf": [
                        {"type":"object","properties":{"query":{"type":"string"},"limit":{"type":"integer"}},"required":["query","limit"]},
                        {"type":"object","properties":{"url":{"type":"string"},"query":{"type":"string"}},"required":["query","url"]}
                    ]
                }
            }]
        });
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        let parameters = &payload["tools"][0]["parameters"];
        assert!(parameters.get("oneOf").is_none(), "oneOf 必须被折掉");
        assert_eq!(parameters["type"], json!("object"));
        // 属性取并集：三个都在。
        for key in ["query", "limit", "url"] {
            assert!(
                parameters["properties"].get(key).is_some(),
                "属性 {key} 应当合并进来"
            );
        }
        // required 取交集：只有 query 是两个分支都要的。取并集就会变成三个都必填，
        // 那才是真正让合法调用被判死的写法。
        assert_eq!(parameters["required"], json!(["query"]));
    }

    #[test]
    fn empty_required_intersection_removes_the_key() {
        let mut payload = json!({
            "tools": [{
                "type":"function",
                "name":"pick",
                "parameters":{"anyOf":[
                    {"type":"object","properties":{"a":{"type":"string"}},"required":["a"]},
                    {"type":"object","properties":{"b":{"type":"string"}}}
                ]}
            }]
        });
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        let parameters = &payload["tools"][0]["parameters"];
        assert!(parameters.get("anyOf").is_none());
        // 第二个分支没写 required = 它什么都不强制，交集必须为空，
        // 而不是把第一个分支的 ["a"] 顺下来。
        assert!(
            parameters.get("required").is_none(),
            "交集为空时不该留下 required（也不该留空数组）"
        );
        assert!(parameters["properties"].get("a").is_some());
        assert!(parameters["properties"].get("b").is_some());
    }

    #[test]
    fn folds_nested_chat_shaped_tools_and_leaves_plain_schemas_alone() {
        let mut payload = json!({
            "tools": [
                {"type":"function","function":{"name":"nested","parameters":{"anyOf":[
                    {"type":"object","properties":{"x":{"type":"number"}},"required":["x"],"additionalProperties":false},
                    {"type":"object","properties":{"y":{"type":"number"}},"required":["x"],"additionalProperties":false}
                ]}}},
                {"type":"function","name":"plain","parameters":{"type":"object","properties":{"z":{"type":"string"}},"required":["z"]}}
            ]
        });
        let untouched = payload["tools"][1]["parameters"].clone();
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        let nested = &payload["tools"][0]["function"]["parameters"];
        assert!(nested.get("anyOf").is_none());
        assert_eq!(nested["required"], json!(["x"]));
        // 所有分支都显式封闭、根级没表态 → 补 false。
        assert_eq!(nested["additionalProperties"], json!(false));
        // 没有 union 的 schema 一个字节都不该动。
        assert_eq!(payload["tools"][1]["parameters"], untouched);
    }

    // ---- 第 2 条：整数浮点改写 ----

    #[test]
    fn rewrites_integral_floats_in_completed_function_call_arguments() {
        let mut payload = json!({
            "input": [{
                "type":"function_call",
                "call_id":"c1",
                "name":"page",
                "status":"completed",
                "arguments":"{\"limit\":10.0,\"ratio\":1.5,\"deep\":{\"offset\":-3.0,\"list\":[2.0,\"7.0\"]},\"already\":4}"
            }]
        });
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        let arguments = payload["input"][0]["arguments"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(arguments).unwrap();
        assert_eq!(parsed["limit"], json!(10));
        assert!(parsed["limit"].is_u64(), "10.0 必须变成整数字面量 10");
        // 真的有小数部分的不能动。
        assert_eq!(parsed["ratio"], json!(1.5));
        assert_eq!(parsed["deep"]["offset"], json!(-3));
        assert!(parsed["deep"]["offset"].is_i64());
        assert_eq!(parsed["deep"]["list"][0], json!(2));
        // 字符串里的 "7.0" 是文本，不是数字，不能碰。
        assert_eq!(parsed["deep"]["list"][1], json!("7.0"));
        assert_eq!(parsed["already"], json!(4));
    }

    #[test]
    fn rejects_out_of_range_integral_floats_instead_of_silently_saturating() {
        // 恰好 2^64：`as u64` 会饱和成 2^64-1，静默差一。必须报错。
        let mut payload = json!({
            "input":[{"type":"function_call","arguments":"{\"id\":18446744073709551616.0}"}]
        });
        let error = sanitize_request(&mut payload, "grok-4.5").unwrap_err();
        assert!(error.contains("outside the range"), "错误信息应说明越界：{error}");

        // 2^64 - 4096 是小于 2^64 的、f64 能精确表示的最大整数
        //（这一段的浮点间隔正好是 4096），必须仍然通过。
        let mut payload = json!({
            "input":[{"type":"function_call","arguments":"{\"id\":18446744073709547520.0}"}]
        });
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        let parsed: Value =
            serde_json::from_str(payload["input"][0]["arguments"].as_str().unwrap()).unwrap();
        assert_eq!(parsed["id"], json!(18_446_744_073_709_547_520u64));

        // 负方向越界同样拒绝。
        let mut payload = json!({
            "input":[{"type":"function_call","arguments":"{\"id\":-1e30}"}]
        });
        assert!(sanitize_request(&mut payload, "grok-4.5").is_err());
    }

    #[test]
    fn leaves_unparsable_or_in_progress_tool_arguments_alone() {
        // 半截 JSON：不报错也不改写。
        let mut payload = json!({
            "input":[{"type":"function_call","status":"completed","arguments":"{\"a\":1.0"}]
        });
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        assert_eq!(payload["input"][0]["arguments"], json!("{\"a\":1.0"));

        // 未完成的调用参数还在增量拼，不动。
        let mut payload = json!({
            "input":[{"type":"function_call","status":"in_progress","arguments":"{\"a\":1.0}"}]
        });
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        assert_eq!(payload["input"][0]["arguments"], json!("{\"a\":1.0}"));
    }

    // ---- 第 3 条：agent_message / encrypted_content ----

    #[test]
    fn rewrites_agent_message_into_a_plain_user_message() {
        let mut payload = json!({
            "input":[
                {"type":"agent_message","content":[{"type":"text","text":"来自邮箱的指令"}]},
                {"type":"message","role":"user","content":[{"type":"input_text","text":"保持原样"}]}
            ]
        });
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        assert_eq!(payload["input"][0]["type"], json!("message"));
        assert_eq!(payload["input"][0]["role"], json!("user"));
        assert_eq!(payload["input"][0]["content"][0]["type"], json!("input_text"));
        assert_eq!(payload["input"][0]["content"][0]["text"], json!("来自邮箱的指令"));
        // 普通消息不受影响。
        assert_eq!(payload["input"][1]["content"][0]["text"], json!("保持原样"));
    }

    #[test]
    fn drops_agent_messages_with_no_extractable_text() {
        let mut payload = json!({"input":[{"type":"agent_message","metadata":{"from":"a@b.c"}}]});
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        // 宁可丢掉，也不塞一条空 user 消息或者把原始 JSON 灌进上下文。
        assert_eq!(payload["input"].as_array().unwrap().len(), 0);
    }

    #[test]
    fn strips_encrypted_reasoning_but_keeps_summaries() {
        let mut payload = json!({
            "input":[
                {"type":"reasoning","encrypted_content":"gAAAAA","summary":[{"type":"summary_text","text":"想了想"}]},
                {"type":"reasoning","encrypted_content":"gAAAAA","summary":[]}
            ]
        });
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        let items = payload["input"].as_array().unwrap();
        // 带摘要的保留下来，但密文字段必须没了。
        assert_eq!(items.len(), 1);
        assert!(items[0].get("encrypted_content").is_none());
        assert_eq!(items[0]["summary"][0]["text"], json!("想了想"));
    }

    #[test]
    fn sanitizing_a_clean_payload_changes_nothing() {
        let original = json!({
            "model":"grok-4.5",
            "stream":true,
            "input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}],
            "tools":[{"type":"function","name":"noop","parameters":{"type":"object","properties":{}}}],
            "reasoning":{"effort":"high"}
        });
        let mut payload = original.clone();
        sanitize_request(&mut payload, "grok-4.5").unwrap();
        assert_eq!(payload, original, "干净的请求体不应被改动");
    }
}
