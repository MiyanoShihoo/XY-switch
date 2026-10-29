use serde_json::{Map, Value, json};

fn string_field<'a>(value: &'a Value, field: &str) -> Option<&'a str> {
    value.get(field).and_then(Value::as_str)
}

/// 取工具的 description，缺失或显式为 null 时返回 None，让调用方**整字段省略**。
///
/// 之前统一 `.unwrap_or("")`，会给没有描述的工具写出 `"description": ""`。
/// Anthropic 的托管工具（如 web_search）按定义就没有描述，没写描述的自定义工具和
/// MCP 工具也一样；一旦把这个字段写成 null 或空串，严格的 OpenAI 兼容上游会直接
/// 400 拒掉**整轮请求**（不只是那一个工具）。省略字段是唯一安全的做法。
///
/// 注意：本项目的 serde_json 没有开 preserve_order，Map 是 BTreeMap、按键名字母序
/// 序列化，所以"有描述的工具序列化结果逐字节不变"这一点与插入顺序无关，
/// prompt cache 不受影响。
fn tool_description<'a>(source: &'a Value) -> Option<&'a str> {
    source
        .get("description")
        .filter(|value| !value.is_null())
        .and_then(Value::as_str)
}

/// 推理强度档位。
///
/// Claude Code 的 `/effort` 发的是 `output_config.effort`，老一些的客户端只发
/// `thinking.budget_tokens`。转成 OpenAI 格式时要落到 Chat 的 `reasoning_effort`
/// 或 Responses 的 `reasoning.effort`——之前这两个字段在本项目里完全没有实现，
/// 所有推理强度在转换时被整个丢掉。
#[derive(Clone, Copy, PartialEq, Eq)]
enum ReasoningEffort {
    Low,
    Medium,
    High,
    XHigh,
    Max,
}

impl ReasoningEffort {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "low" => Some(Self::Low),
            "medium" | "med" => Some(Self::Medium),
            "high" => Some(Self::High),
            // `/effort xhigh` 发的就是这个值，之前不认识它，
            // 而显式的 effort 又会屏蔽 thinking 回退，结果一个档位都没发出去。
            "xhigh" | "x-high" | "extra_high" => Some(Self::XHigh),
            "max" | "maximum" => Some(Self::Max),
            _ => None,
        }
    }
    fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
            Self::Max => "max",
        }
    }
}

/// 去掉 `vendor/` 前缀后的模型名，便于做前缀与全名匹配。
pub(crate) fn bare_model_name(model: &str) -> String {
    let lower = model.to_lowercase();
    match lower.rsplit_once('/') {
        Some((_, name)) => name.to_string(),
        None => lower,
    }
}

/// 只有确认接受推理强度的模型才带这个字段——给不支持的模型硬塞会被上游直接 400，
/// 所以这里用白名单而不是黑名单。
fn model_supports_reasoning_effort(model: &str) -> bool {
    let name = bare_model_name(model);
    name.starts_with("o1")
        || name.starts_with("o3")
        || name.starts_with("o4")
        || name.starts_with("gpt-5")
        || name.starts_with("gpt-6")
        // xAI：grok-4.5 系列、grok-4.6 系列（含映射过来的 grok-4.6-*），以及 grok-build-*。
        || name.starts_with("grok-4.5")
        || name.starts_with("grok-4.6")
        || name.starts_with("grok-build")
}

/// 能原样接受 `max` 档的模型。其余支持推理的模型收到 max 会 400
/// （xAI 实测 grok-4.5 只接受 low/medium/high/xhigh），要降到 xhigh。
fn model_supports_max_effort(name: &str) -> bool {
    matches!(
        name,
        "gpt-5.6" | "gpt-5.6-sol" | "gpt-5.6-terra" | "gpt-5.6-luna" | "gpt-6-astra"
    )
}

/// 把档位夹到目标模型真正能接受的范围；模型不支持推理强度时返回 None（整字段不发）。
fn clamp_effort_for_model(effort: ReasoningEffort, model: &str) -> Option<&'static str> {
    if !model_supports_reasoning_effort(model) {
        return None;
    }
    if effort == ReasoningEffort::Max && !model_supports_max_effort(&bare_model_name(model)) {
        return Some(ReasoningEffort::XHigh.as_str());
    }
    Some(effort.as_str())
}

/// `clamp_effort_for_model` 的字符串版结果。给 `xai_responses` 这类
/// 「请求体里已经是 Responses 形状、只需要就地夹一下 `reasoning.effort`」的调用方用。
///
/// 之所以要分出 `Unknown`：原生 Responses 直通那条路上，请求体是客户端原样发来的，
/// 里面可能出现本项目还不认识的档位名。认不出来就原样留着交给上游判断，
/// 绝不自作主张删掉或改写——猜错会让用户明明设了档位却静默失效。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EffortClamp {
    /// 用这个值替换（可能与原值相同）。
    Keep(&'static str),
    /// 目标模型根本不接受推理强度，整个字段删掉，否则上游 400。
    Drop,
    /// 不认识的档位，原样保留。
    Unknown,
}

/// 按目标模型夹住一个字符串档位。逻辑完全复用 `clamp_effort_for_model`，
/// 不要在别处另写一份模型名单，否则两边迟早不一致。
pub(crate) fn clamp_effort_str(value: &str, model: &str) -> EffortClamp {
    match ReasoningEffort::parse(value) {
        Some(effort) => match clamp_effort_for_model(effort, model) {
            Some(clamped) => EffortClamp::Keep(clamped),
            None => EffortClamp::Drop,
        },
        None => EffortClamp::Unknown,
    }
}

/// 从 Anthropic 请求里解析推理强度：显式 `output_config.effort` 优先，
/// 解析不出来时回退到 `thinking.budget_tokens`。
fn anthropic_reasoning_effort(source: &Map<String, Value>) -> Option<ReasoningEffort> {
    let explicit = source
        .get("output_config")
        .and_then(|config| config.get("effort"))
        .and_then(Value::as_str)
        .and_then(ReasoningEffort::parse);
    if explicit.is_some() {
        return explicit;
    }
    // thinking 预算只映射到 low/medium/high —— xhigh 与 max 是显式档位才有的语义，
    // 不能凭预算大小猜出来。
    let thinking = source.get("thinking")?;
    if thinking.get("type").and_then(Value::as_str) == Some("disabled") {
        return None;
    }
    let budget = thinking.get("budget_tokens").and_then(Value::as_u64)?;
    match budget {
        0 => None,
        1..=4_096 => Some(ReasoningEffort::Low),
        4_097..=16_384 => Some(ReasoningEffort::Medium),
        _ => Some(ReasoningEffort::High),
    }
}

fn parse_arguments(arguments: &Value) -> Value {
    match arguments {
        Value::String(value) => serde_json::from_str(value).unwrap_or_else(|_| json!({})),
        Value::Object(_) => arguments.clone(),
        _ => json!({}),
    }
}

fn content_text(content: &Value) -> String {
    match content {
        Value::String(value) => value.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                string_field(block, "text").or_else(|| string_field(block, "content"))
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn openai_content_to_anthropic(content: &Value) -> Vec<Value> {
    match content {
        Value::String(text) => vec![json!({"type":"text", "text":text})],
        Value::Array(blocks) => blocks
            .iter()
            .filter_map(|block| {
                let kind = string_field(block, "type").unwrap_or("text");
                match kind {
                    "text" | "input_text" | "output_text" => {
                        string_field(block, "text").map(|text| json!({"type":"text", "text":text}))
                    }
                    _ => None,
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

pub fn openai_chat_to_anthropic(mut request: Value, model: &str) -> Result<Value, String> {
    let object = request
        .as_object_mut()
        .ok_or_else(|| "OpenAI request must be an object".to_string())?;
    let messages = object
        .remove("messages")
        .and_then(|value| value.as_array().cloned())
        .ok_or_else(|| "OpenAI request messages must be an array".to_string())?;

    let mut system = Vec::new();
    let mut anthropic_messages = Vec::new();
    for message in messages {
        let role = string_field(&message, "role").unwrap_or("");
        let content = message.get("content").cloned().unwrap_or(Value::Null);
        match role {
            "system" | "developer" => {
                let text = content_text(&content);
                if !text.is_empty() {
                    system.push(json!({"type":"text", "text":text}));
                }
            }
            "tool" => {
                anthropic_messages.push(json!({
                    "role":"user",
                    "content":[{
                        "type":"tool_result",
                        "tool_use_id":string_field(&message, "tool_call_id").unwrap_or(""),
                        "content":content_text(&content)
                    }]
                }));
            }
            "user" | "assistant" => {
                let mut blocks = openai_content_to_anthropic(&content);
                if role == "assistant"
                    && let Some(tool_calls) = message.get("tool_calls").and_then(Value::as_array)
                {
                    for call in tool_calls {
                        let function = call.get("function").unwrap_or(&Value::Null);
                        blocks.push(json!({
                            "type":"tool_use",
                            "id":string_field(call, "id").unwrap_or(""),
                            "name":string_field(function, "name").unwrap_or(""),
                            "input":parse_arguments(function.get("arguments").unwrap_or(&Value::Null))
                        }));
                    }
                }
                anthropic_messages.push(json!({"role":role, "content":blocks}));
            }
            _ => return Err(format!("unsupported OpenAI message role: {role}")),
        }
    }

    let mut result = Map::new();
    result.insert("model".to_string(), Value::String(model.to_string()));
    result.insert("messages".to_string(), Value::Array(anthropic_messages));
    result.insert(
        "max_tokens".to_string(),
        object.remove("max_tokens").unwrap_or(json!(4096)),
    );
    for field in ["temperature", "top_p", "stop_sequences", "stream"] {
        if let Some(value) = object.remove(field) {
            result.insert(field.to_string(), value);
        }
    }
    if let Some(stop) = object.remove("stop") {
        result.insert(
            "stop_sequences".to_string(),
            if stop.is_string() {
                Value::Array(vec![stop])
            } else {
                stop
            },
        );
    }
    if !system.is_empty() {
        result.insert("system".to_string(), Value::Array(system));
    }
    if let Some(tools) = object
        .remove("tools")
        .and_then(|value| value.as_array().cloned())
    {
        result.insert(
            "tools".to_string(),
            Value::Array(
                tools
                    .iter()
                    .filter_map(|tool| tool.get("function"))
                    .map(|function| {
                        let mut converted = json!({
                            "name":string_field(function, "name").unwrap_or(""),
                            "input_schema":function.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"}))
                        });
                        if let Some(description) = tool_description(function) {
                            converted["description"] = json!(description);
                        }
                        converted
                    })
                    .collect(),
            ),
        );
    }
    Ok(Value::Object(result))
}

pub fn anthropic_to_openai_chat(mut request: Value, model: &str) -> Result<Value, String> {
    let object = request
        .as_object_mut()
        .ok_or_else(|| "Anthropic request must be an object".to_string())?;
    let source_messages = object
        .remove("messages")
        .and_then(|value| value.as_array().cloned())
        .ok_or_else(|| "Anthropic request messages must be an array".to_string())?;
    let mut messages = Vec::new();

    if let Some(system) = object.remove("system") {
        let text = content_text(&system);
        if !text.is_empty() {
            messages.push(json!({"role":"system", "content":text}));
        }
    }
    for message in source_messages {
        let role = string_field(&message, "role").unwrap_or("");
        let content = message.get("content").cloned().unwrap_or(Value::Null);
        let blocks = match content {
            Value::Array(blocks) => blocks,
            Value::String(text) => vec![json!({"type":"text", "text":text})],
            _ => Vec::new(),
        };
        if role == "user" {
            let text = blocks
                .iter()
                .filter(|block| string_field(block, "type") == Some("text"))
                .filter_map(|block| string_field(block, "text"))
                .collect::<Vec<_>>()
                .join("\n");
            if !text.is_empty() {
                messages.push(json!({"role":"user", "content":text}));
            }
            for block in blocks
                .iter()
                .filter(|block| string_field(block, "type") == Some("tool_result"))
            {
                messages.push(json!({
                    "role":"tool",
                    "tool_call_id":string_field(block, "tool_use_id").unwrap_or(""),
                    "content":content_text(block.get("content").unwrap_or(&Value::Null))
                }));
            }
        } else if role == "assistant" {
            let text = blocks
                .iter()
                .filter(|block| string_field(block, "type") == Some("text"))
                .filter_map(|block| string_field(block, "text"))
                .collect::<Vec<_>>()
                .join("\n");
            let tool_calls: Vec<Value> = blocks
                .iter()
                .filter(|block| string_field(block, "type") == Some("tool_use"))
                .map(|block| {
                    json!({
                        "id":string_field(block, "id").unwrap_or(""),
                        "type":"function",
                        "function":{
                            "name":string_field(block, "name").unwrap_or(""),
                            "arguments":serde_json::to_string(block.get("input").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".to_string())
                        }
                    })
                })
                .collect();
            let mut converted = json!({"role":"assistant", "content":text});
            if !tool_calls.is_empty() {
                converted["tool_calls"] = Value::Array(tool_calls);
            }
            messages.push(converted);
        } else {
            return Err(format!("unsupported Anthropic message role: {role}"));
        }
    }

    let mut result = Map::new();
    result.insert("model".to_string(), Value::String(model.to_string()));
    result.insert("messages".to_string(), Value::Array(messages));
    if let Some(max_tokens) = object.remove("max_tokens") {
        result.insert("max_tokens".to_string(), max_tokens);
    }
    for field in ["temperature", "top_p", "stream"] {
        if let Some(value) = object.remove(field) {
            result.insert(field.to_string(), value);
        }
    }
    if let Some(stop) = object.remove("stop_sequences") {
        result.insert("stop".to_string(), stop);
    }
    if let Some(tools) = object
        .remove("tools")
        .and_then(|value| value.as_array().cloned())
    {
        result.insert(
            "tools".to_string(),
            Value::Array(
                tools
                    .iter()
                    .map(|tool| {
                        let mut function = json!({
                            "name":string_field(tool, "name").unwrap_or(""),
                            "parameters":tool.get("input_schema").cloned().unwrap_or_else(|| json!({"type":"object"}))
                        });
                        if let Some(description) = tool_description(tool) {
                            function["description"] = json!(description);
                        }
                        json!({"type":"function", "function":function})
                    })
                    .collect(),
            ),
        );
    }
    // 推理强度：Claude Code 的 /effort 与 thinking 预算都在这里落成 reasoning_effort。
    // 模型不在白名单时整字段省略，免得给不支持的上游塞出 400。
    if let Some(effort) =
        anthropic_reasoning_effort(object).and_then(|effort| clamp_effort_for_model(effort, model))
    {
        result.insert("reasoning_effort".to_string(), json!(effort));
    }
    Ok(Value::Object(result))
}

pub fn anthropic_response_to_openai(response: Value) -> Value {
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for block in response
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match string_field(block, "type") {
            Some("text") => text.push_str(string_field(block, "text").unwrap_or("")),
            Some("tool_use") => tool_calls.push(json!({
                "id":string_field(block, "id").unwrap_or(""),
                "type":"function",
                "function":{
                    "name":string_field(block, "name").unwrap_or(""),
                    "arguments":serde_json::to_string(block.get("input").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".to_string())
                }
            })),
            _ => {}
        }
    }
    let finish_reason = match string_field(&response, "stop_reason") {
        Some("tool_use") => "tool_calls",
        Some("max_tokens") => "length",
        _ => "stop",
    };
    let mut message = json!({"role":"assistant", "content":text});
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let input = response["usage"]["input_tokens"].as_u64().unwrap_or(0);
    let output = response["usage"]["output_tokens"].as_u64().unwrap_or(0);
    json!({
        "id":response.get("id").cloned().unwrap_or_else(|| json!("msg_unknown")),
        "object":"chat.completion",
        "model":response.get("model").cloned().unwrap_or(Value::Null),
        "choices":[{"index":0, "message":message, "finish_reason":finish_reason}],
        "usage":{"prompt_tokens":input, "completion_tokens":output, "total_tokens":input + output}
    })
}

pub fn openai_response_to_anthropic(response: Value) -> Value {
    let choice = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first())
        .cloned()
        .unwrap_or(Value::Null);
    let message = choice.get("message").cloned().unwrap_or(Value::Null);
    let mut content = openai_content_to_anthropic(message.get("content").unwrap_or(&Value::Null));
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let function = call.get("function").unwrap_or(&Value::Null);
        content.push(json!({
            "type":"tool_use",
            "id":string_field(call, "id").unwrap_or(""),
            "name":string_field(function, "name").unwrap_or(""),
            "input":parse_arguments(function.get("arguments").unwrap_or(&Value::Null))
        }));
    }
    let stop_reason = match string_field(&choice, "finish_reason") {
        Some("tool_calls") => "tool_use",
        Some("length") => "max_tokens",
        _ => "end_turn",
    };
    json!({
        "id":response.get("id").cloned().unwrap_or_else(|| json!("msg_unknown")),
        "type":"message",
        "role":"assistant",
        "model":response.get("model").cloned().unwrap_or(Value::Null),
        "content":content,
        "stop_reason":stop_reason,
        "stop_sequence":null,
        "usage":{
            "input_tokens":response["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            "output_tokens":response["usage"]["completion_tokens"].as_u64().unwrap_or(0)
        }
    })
}

pub fn openai_chat_to_gemini(request: &Value) -> Result<Value, String> {
    let messages = request
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "OpenAI request messages must be an array".to_string())?;
    let mut system_parts = Vec::new();
    let mut contents = Vec::new();
    for message in messages {
        let role = string_field(message, "role").unwrap_or("");
        let text = content_text(message.get("content").unwrap_or(&Value::Null));
        if matches!(role, "system" | "developer") {
            if !text.is_empty() {
                system_parts.push(json!({"text":text}));
            }
            continue;
        }
        let mut parts = Vec::new();
        if !text.is_empty() {
            parts.push(json!({"text":text}));
        }
        if role == "assistant" {
            for call in message
                .get("tool_calls")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let function = call.get("function").unwrap_or(&Value::Null);
                parts.push(json!({
                    "functionCall":{
                        "name":string_field(function, "name").unwrap_or(""),
                        "args":parse_arguments(function.get("arguments").unwrap_or(&Value::Null))
                    }
                }));
            }
        } else if role == "tool" {
            parts.push(json!({
                "functionResponse":{
                    "name":string_field(message, "name").or_else(|| string_field(message, "tool_call_id")).unwrap_or("tool"),
                    "response":{"result":text}
                }
            }));
        } else if role != "user" {
            return Err(format!("unsupported OpenAI message role: {role}"));
        }
        contents.push(json!({
            "role":if role == "assistant" { "model" } else { "user" },
            "parts":parts
        }));
    }

    let mut result = json!({"contents":contents});
    if !system_parts.is_empty() {
        result["systemInstruction"] = json!({"parts":system_parts});
    }
    let mut generation = Map::new();
    for (source, target) in [
        ("max_tokens", "maxOutputTokens"),
        ("temperature", "temperature"),
        ("top_p", "topP"),
        ("stop", "stopSequences"),
    ] {
        if let Some(value) = request.get(source) {
            generation.insert(target.to_string(), value.clone());
        }
    }
    if !generation.is_empty() {
        result["generationConfig"] = Value::Object(generation);
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        result["tools"] = json!([{
            "functionDeclarations":tools.iter().filter_map(|tool| tool.get("function")).map(|function| {
                let mut declaration = json!({
                    "name":string_field(function, "name").unwrap_or(""),
                    "parameters":function.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"}))
                });
                if let Some(description) = tool_description(function) {
                    declaration["description"] = json!(description);
                }
                declaration
            }).collect::<Vec<_>>()
        }]);
    }
    Ok(result)
}

pub fn gemini_response_to_openai(response: Value, model: &str) -> Value {
    let candidate = response
        .get("candidates")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or(Value::Null);
    let mut text = String::new();
    let mut tool_calls = Vec::new();
    for (index, part) in candidate["content"]["parts"]
        .as_array()
        .into_iter()
        .flatten()
        .enumerate()
    {
        if let Some(value) = string_field(part, "text") {
            text.push_str(value);
        }
        if let Some(call) = part.get("functionCall") {
            tool_calls.push(json!({
                "id":format!("call_{index}"),
                "type":"function",
                "function":{
                    "name":string_field(call, "name").unwrap_or(""),
                    "arguments":serde_json::to_string(call.get("args").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".to_string())
                }
            }));
        }
    }
    let mut message = json!({"role":"assistant", "content":text});
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let finish = match string_field(&candidate, "finishReason") {
        Some("MAX_TOKENS") => "length",
        _ if !message.get("tool_calls").unwrap_or(&Value::Null).is_null() => "tool_calls",
        _ => "stop",
    };
    json!({
        "id":format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
        "object":"chat.completion",
        "model":model,
        "choices":[{"index":0, "message":message, "finish_reason":finish}],
        "usage":{
            "prompt_tokens":response["usageMetadata"]["promptTokenCount"].as_u64().unwrap_or(0),
            "completion_tokens":response["usageMetadata"]["candidatesTokenCount"].as_u64().unwrap_or(0),
            "total_tokens":response["usageMetadata"]["totalTokenCount"].as_u64().unwrap_or(0)
        }
    })
}

pub fn gemini_to_openai_chat(request: &Value, model: &str) -> Result<Value, String> {
    let contents = request
        .get("contents")
        .and_then(Value::as_array)
        .ok_or_else(|| "Gemini request contents must be an array".to_string())?;
    let mut messages = Vec::new();
    if let Some(system) = request.get("systemInstruction") {
        let text = system["parts"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            messages.push(json!({"role":"system", "content":text}));
        }
    }
    for content in contents {
        let role = match content
            .get("role")
            .and_then(Value::as_str)
            .unwrap_or("user")
        {
            "model" => "assistant",
            _ => "user",
        };
        let parts = content["parts"].as_array().cloned().unwrap_or_default();
        let text = parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("");
        let tool_calls: Vec<Value> = parts
            .iter()
            .enumerate()
            .filter_map(|(index, part)| part.get("functionCall").map(|call| (index, call)))
            .map(|(index, call)| {
                json!({
                    "id":format!("call_{index}"),
                    "type":"function",
                    "function":{
                        "name":string_field(call, "name").unwrap_or(""),
                        "arguments":serde_json::to_string(call.get("args").unwrap_or(&json!({}))).unwrap_or_else(|_| "{}".to_string())
                    }
                })
            })
            .collect();
        let function_responses: Vec<&Value> = parts
            .iter()
            .filter_map(|part| part.get("functionResponse"))
            .collect();
        if !text.is_empty() || !tool_calls.is_empty() {
            let mut message = json!({"role":role, "content":text});
            if !tool_calls.is_empty() {
                message["tool_calls"] = Value::Array(tool_calls);
            }
            messages.push(message);
        }
        for response in function_responses {
            messages.push(json!({
                "role":"tool",
                "name":string_field(response, "name").unwrap_or("tool"),
                "tool_call_id":string_field(response, "name").unwrap_or("tool"),
                "content":serde_json::to_string(response.get("response").unwrap_or(&Value::Null)).unwrap_or_else(|_| "null".to_string())
            }));
        }
    }

    let mut result = json!({"model":model, "messages":messages});
    if let Some(config) = request.get("generationConfig") {
        for (source, target) in [
            ("maxOutputTokens", "max_tokens"),
            ("temperature", "temperature"),
            ("topP", "top_p"),
            ("stopSequences", "stop"),
        ] {
            if let Some(value) = config.get(source) {
                result[target] = value.clone();
            }
        }
    }
    if let Some(declarations) = request
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|tool| tool.get("functionDeclarations").and_then(Value::as_array))
        .next()
    {
        result["tools"] = Value::Array(
            declarations
                .iter()
                .map(|function| {
                    let mut converted = json!({
                        "name":string_field(function, "name").unwrap_or(""),
                        "parameters":function.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"}))
                    });
                    if let Some(description) = tool_description(function) {
                        converted["description"] = json!(description);
                    }
                    json!({"type":"function", "function":converted})
                })
                .collect(),
        );
    }
    if request.get("stream").and_then(Value::as_bool) == Some(true) {
        result["stream"] = Value::Bool(true);
    }
    Ok(result)
}

pub fn openai_response_to_gemini(response: Value) -> Value {
    let choice = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or(Value::Null);
    let message = &choice["message"];
    let mut parts = Vec::new();
    let text = content_text(message.get("content").unwrap_or(&Value::Null));
    if !text.is_empty() {
        parts.push(json!({"text":text}));
    }
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let function = &call["function"];
        parts.push(json!({
            "functionCall":{
                "name":string_field(function, "name").unwrap_or(""),
                "args":parse_arguments(function.get("arguments").unwrap_or(&Value::Null))
            }
        }));
    }
    let finish_reason = match choice.get("finish_reason").and_then(Value::as_str) {
        Some("length") => "MAX_TOKENS",
        _ => "STOP",
    };
    json!({
        "candidates":[{"content":{"role":"model", "parts":parts}, "finishReason":finish_reason, "index":0}],
        "usageMetadata":{
            "promptTokenCount":response["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            "candidatesTokenCount":response["usage"]["completion_tokens"].as_u64().unwrap_or(0),
            "totalTokenCount":response["usage"]["total_tokens"].as_u64().unwrap_or(0)
        }
    })
}

fn responses_content_to_chat(content: &Value) -> Result<Value, String> {
    let Some(parts) = content.as_array() else {
        return Ok(content.clone());
    };
    let mut output = Vec::new();
    for part in parts {
        match part["type"].as_str().unwrap_or("text") {
            "input_text" | "output_text" | "text" => {
                output.push(json!({"type":"text","text":part["text"]}))
            }
            "input_image" => {
                if !part["image_url"].is_string() {
                    return Err("Chat 兼容模式图片需要 image_url，不能仅传 file_id".to_string());
                }
                let mut image = json!({"url":part["image_url"]});
                if let Some(detail) = part.get("detail") {
                    // detail:"original" 只在 Responses API 里合法。Codex 声明
                    // supports_image_detail_original 时会给 view_image 之类的工具截图打上这个值，
                    // 原样转发给严格的 OpenAI 兼容网关（方舟/豆包、日日新、GLM、vLLM 等）会整条请求 400。
                    // 而 Codex 每轮都重放完整历史，所以一张图会让这个会话之后每轮都失败。
                    // 这里把它降级成 auto，auto/low/high 原样保留。
                    if detail.as_str() == Some("original") {
                        image["detail"] = json!("auto");
                    } else {
                        image["detail"] = detail.clone();
                    }
                }
                output.push(json!({"type":"image_url","image_url":image}));
            }
            other => return Err(format!("Chat 兼容模式暂不支持输入类型：{other}")),
        }
    }
    if output.iter().all(|part| part["type"] == "text") {
        Ok(json!(
            output
                .iter()
                .filter_map(|p| p["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n")
        ))
    } else {
        Ok(json!(output))
    }
}

pub fn responses_to_openai_chat(request: &Value) -> Result<Value, String> {
    let bridge = crate::tool_bridge::ToolBridge::from_request(request)?;
    let prepared = bridge.prepare(request);
    let request = &prepared;
    if request
        .get("previous_response_id")
        .is_some_and(|v| !v.is_null())
    {
        return Err(
            "Chat 兼容模式不保存服务端 Responses 会话；请在 input 中发送完整会话历史".to_string(),
        );
    }
    let mut messages: Vec<Value> = Vec::new();
    let mut reasoning = String::new();

    // 1. System instructions
    if let Some(instructions) = request.get("instructions").and_then(Value::as_str) {
        if !instructions.trim().is_empty() {
            messages.push(json!({
                "role": "system",
                "content": instructions
            }));
        }
    }

    // 2. Input items
    let input = request
        .get("input")
        .ok_or_else(|| "Responses request input is required".to_string())?;

    match input {
        Value::String(text) => {
            messages.push(json!({"role": "user", "content": text}));
        }
        Value::Array(items) => {
            for item in items {
                let item_type = string_field(item, "type").unwrap_or("message");
                match item_type {
                    "message" => {
                        let role = string_field(item, "role").unwrap_or("user");
                        let role = if role == "developer" { "system" } else { role };
                        let content =
                            responses_content_to_chat(item.get("content").unwrap_or(&Value::Null))?;
                        let mut msg = json!({
                            "role": role,
                            "content": content
                        });
                        if let Some(calls) = item.get("tool_calls") {
                            msg["tool_calls"] = calls.clone();
                            if content == "" {
                                msg["content"] = Value::Null;
                            }
                        }
                        if role == "assistant" && !reasoning.is_empty() {
                            msg["reasoning_content"] = json!(std::mem::take(&mut reasoning));
                        }
                        messages.push(msg);
                    }
                    "reasoning" => {
                        if let Some(summary) = item.get("summary").and_then(Value::as_array) {
                            for part in summary {
                                reasoning.push_str(part["text"].as_str().unwrap_or(""));
                            }
                        }
                    }
                    "function_call" => {
                        let call_id = string_field(item, "call_id")
                            .or_else(|| string_field(item, "id"))
                            .unwrap_or("");
                        let name = string_field(item, "name").unwrap_or("");
                        let arguments = match item.get("arguments") {
                            Some(Value::String(s)) => s.clone(),
                            Some(v) => v.to_string(),
                            None => "{}".to_string(),
                        };
                        let tool_call = json!({
                            "id": call_id,
                            "type": "function",
                            "function": {
                                "name": name,
                                "arguments": arguments
                            }
                        });

                        let attached = if let Some(last) = messages.last_mut() {
                            if last.get("role").and_then(Value::as_str) == Some("assistant") {
                                if let Some(calls) =
                                    last.get_mut("tool_calls").and_then(Value::as_array_mut)
                                {
                                    calls.push(tool_call.clone());
                                    true
                                } else {
                                    last["tool_calls"] = json!([tool_call]);
                                    if last
                                        .get("content")
                                        .and_then(Value::as_str)
                                        .is_some_and(|s| s.is_empty())
                                    {
                                        last["content"] = Value::Null;
                                    }
                                    true
                                }
                            } else {
                                false
                            }
                        } else {
                            false
                        };

                        if !attached {
                            messages.push(
                                json!({"role":"assistant","content":null,"tool_calls":[tool_call]}),
                            );
                        }
                        if !reasoning.is_empty() {
                            messages.last_mut().unwrap()["reasoning_content"] =
                                json!(std::mem::take(&mut reasoning));
                        }
                    }
                    "function_call_output" => {
                        let call_id = string_field(item, "call_id")
                            .or_else(|| string_field(item, "id"))
                            .unwrap_or("");
                        let output = match item.get("output") {
                            Some(Value::String(s)) => s.clone(),
                            Some(v) => v.to_string(),
                            None => String::new(),
                        };
                        messages.push(json!({
                            "role": "tool",
                            "tool_call_id": call_id,
                            "content": output
                        }));
                    }
                    // Codex 0.154+ 的 additional_tools 载体：有 role 但没有 content，
                    // 只用来携带额外工具。ToolBridge::prepare 已经把它摘掉并把里面的工具
                    // 合并进 tools，这里再兜一层，免得它落进下面的 `_` 分支，
                    // 被当成 {"role":"system","content":""} 发给严格的 Chat 网关。
                    "additional_tools" => {}
                    _ => {
                        if let Some(role) = string_field(item, "role") {
                            let content = content_text(item.get("content").unwrap_or(&Value::Null));
                            messages.push(json!({
                                "role": role,
                                "content": content
                            }));
                        }
                    }
                }
            }
        }
        _ => return Err("Responses request input must be text or an array".to_string()),
    }

    let mut chat = json!({
        "model": request.get("model").cloned().unwrap_or(Value::Null),
        "messages": messages
    });

    for (source, target) in [
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("max_output_tokens", "max_tokens"),
        ("stream", "stream"),
    ] {
        if let Some(value) = request.get(source) {
            chat[target] = value.clone();
        }
    }

    // Responses 的 reasoning.effort → Chat 的 reasoning_effort。
    // 同样只在目标模型确实接受推理强度时才写入：经本地路由转发到
    // DeepSeek / GLM 这类 Chat 网关时，多这个字段会被 400。
    if let Some(effort) = request
        .get("reasoning")
        .and_then(|reasoning| reasoning.get("effort"))
        .and_then(Value::as_str)
        .and_then(ReasoningEffort::parse)
        .and_then(|effort| clamp_effort_for_model(effort, chat["model"].as_str().unwrap_or("")))
    {
        chat["reasoning_effort"] = json!(effort);
    }
    if let Some(tools) = request.get("tools").and_then(Value::as_array) {
        let mut converted_tools = Vec::new();
        for tool in tools {
            if tool.get("type").and_then(Value::as_str) != Some("function") {
                return Err("Unsupported Chat tool".into());
            }
            if tool.get("function").is_some() {
                converted_tools.push(tool.clone());
            } else {
                let mut function = tool.clone();
                function.as_object_mut().unwrap().remove("type");
                converted_tools.push(json!({"type":"function","function":function}));
            }
        }
        chat["tools"] = json!(converted_tools);
    }
    if let Some(choice) = request.get("tool_choice") {
        chat["tool_choice"] = if choice["type"] == "function" && choice.get("name").is_some() {
            json!({"type":"function","function":{"name":choice["name"]}})
        } else {
            choice.clone()
        };
    }
    if let Some(parallel) = request.get("parallel_tool_calls") {
        chat["parallel_tool_calls"] = parallel.clone();
    }
    if request["stream"] == true {
        chat["stream_options"] = json!({"include_usage":true});
    }
    Ok(chat)
}

pub fn openai_chat_to_responses(request: Value, model: &str) -> Result<Value, String> {
    let object = request
        .as_object()
        .ok_or_else(|| "OpenAI Chat request must be an object".to_string())?;
    let messages = object
        .get("messages")
        .and_then(Value::as_array)
        .ok_or_else(|| "OpenAI Chat request messages must be an array".to_string())?;
    let mut input = Vec::new();
    for message in messages {
        let role = string_field(message, "role").unwrap_or("user");
        if role == "tool" {
            input.push(json!({
                "type":"function_call_output",
                "call_id":string_field(message, "tool_call_id").unwrap_or(""),
                "output":content_text(message.get("content").unwrap_or(&Value::Null))
            }));
            continue;
        }
        let content = content_text(message.get("content").unwrap_or(&Value::Null));
        if !content.is_empty() {
            input.push(json!({"type":"message", "role":role, "content":content}));
        }
        for call in message
            .get("tool_calls")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let function = call.get("function").unwrap_or(&Value::Null);
            input.push(json!({
                "type":"function_call",
                "call_id":string_field(call, "id").unwrap_or(""),
                "name":string_field(function, "name").unwrap_or(""),
                "arguments":function.get("arguments").cloned().unwrap_or_else(|| json!("{}"))
            }));
        }
    }

    let mut result = json!({"model":model, "input":input});
    for (source, target) in [
        ("temperature", "temperature"),
        ("top_p", "top_p"),
        ("max_tokens", "max_output_tokens"),
        ("stream", "stream"),
    ] {
        if let Some(value) = object.get(source) {
            result[target] = value.clone();
        }
    }
    // Responses API 规定 max_output_tokens 最小是 16。Claude Desktop 探测"这个模型能不能用"
    // 时会发 max_tokens=1，原样映射过去上游直接报错，前端就显示成「模型不可用」。
    // 这里只把 1..16 的值抬到 16；非数字、缺失或已经 >=16 的值一律不动，
    // 免得掩盖上游真正的参数校验错误。
    if let Some(limit) = result.get("max_output_tokens").and_then(Value::as_u64) {
        if limit < 16 {
            result["max_output_tokens"] = json!(16);
        }
    }
    // Chat 的 reasoning_effort → Responses 的 reasoning.effort。
    // 这里的 model 是最终发给上游的模型名，所以按它再夹一次档位。
    if let Some(effort) = object
        .get("reasoning_effort")
        .and_then(Value::as_str)
        .and_then(ReasoningEffort::parse)
        .and_then(|effort| clamp_effort_for_model(effort, model))
    {
        result["reasoning"] = json!({"effort": effort});
    }
    if let Some(tools) = object.get("tools").and_then(Value::as_array) {
        result["tools"] = Value::Array(
            tools
                .iter()
                .filter_map(|tool| tool.get("function"))
                .map(|function| {
                    let mut converted = json!({
                        "type":"function",
                        "name":string_field(function, "name").unwrap_or(""),
                        "parameters":function.get("parameters").cloned().unwrap_or_else(|| json!({"type":"object"}))
                    });
                    if let Some(description) = tool_description(function) {
                        converted["description"] = json!(description);
                    }
                    converted
                })
                .collect(),
        );
    }
    Ok(result)
}

pub fn responses_response_to_openai_chat(response: Value) -> Value {
    let mut text = Vec::new();
    let mut tool_calls = Vec::new();
    for item in response
        .get("output")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        match string_field(item, "type") {
            Some("message") => {
                let content = content_text(item.get("content").unwrap_or(&Value::Null));
                if !content.is_empty() {
                    text.push(content);
                }
            }
            Some("function_call") => tool_calls.push(json!({
                "id":string_field(item, "call_id").or_else(|| string_field(item, "id")).unwrap_or(""),
                "type":"function",
                "function":{
                    "name":string_field(item, "name").unwrap_or(""),
                    "arguments":item.get("arguments").cloned().unwrap_or_else(|| json!("{}"))
                }
            })),
            _ => {}
        }
    }
    let mut message = json!({"role":"assistant", "content":text.join("\n")});
    if !tool_calls.is_empty() {
        message["tool_calls"] = Value::Array(tool_calls);
    }
    let finish_reason = if message.get("tool_calls").is_some() {
        "tool_calls"
    } else if response["incomplete_details"]["reason"] == "max_output_tokens" {
        "length"
    } else {
        "stop"
    };
    json!({
        "id":response.get("id").cloned().unwrap_or_else(|| json!(format!("chatcmpl_{}", uuid::Uuid::new_v4().simple()))),
        "object":"chat.completion",
        "model":response.get("model").cloned().unwrap_or(Value::Null),
        "choices":[{"index":0, "message":message, "finish_reason":finish_reason}],
        "usage":{
            "prompt_tokens":response["usage"]["input_tokens"].as_u64().unwrap_or(0),
            "completion_tokens":response["usage"]["output_tokens"].as_u64().unwrap_or(0),
            "total_tokens":response["usage"]["total_tokens"].as_u64().unwrap_or(0)
        }
    })
}

pub fn openai_chat_response_to_responses(response: Value) -> Value {
    let choice = response
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|items| items.first())
        .cloned()
        .unwrap_or(Value::Null);
    let incomplete = matches!(
        choice["finish_reason"].as_str(),
        Some("length" | "content_filter")
    );
    let status = if incomplete {
        "incomplete"
    } else {
        "completed"
    };
    let message = choice.get("message").cloned().unwrap_or(Value::Null);
    let mut output = vec![json!({
        "type":"message",
        "id":format!("msg_{}", uuid::Uuid::new_v4().simple()),
        "status":status,
        "role":"assistant",
        "content":[{"type":"output_text", "text":content_text(message.get("content").unwrap_or(&Value::Null)), "annotations":[]}]
    })];
    if let Some(reasoning) = message.get("reasoning_content").and_then(Value::as_str) {
        output.insert(0, json!({"id":format!("rs_{}",uuid::Uuid::new_v4().simple()),"type":"reasoning","summary":[{"type":"summary_text","text":reasoning}]}));
    }
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let function = call.get("function").unwrap_or(&Value::Null);
        output.push(json!({
            "type":"function_call", "status":status,
            "id":string_field(call, "id").unwrap_or(""),
            "call_id":string_field(call, "id").unwrap_or(""),
            "name":string_field(function, "name").unwrap_or(""),
            "arguments":function.get("arguments").cloned().unwrap_or_else(|| json!("{}"))
        }));
    }
    let input_tokens = response["usage"]["prompt_tokens"].as_u64().unwrap_or(0);
    let output_tokens = response["usage"]["completion_tokens"].as_u64().unwrap_or(0);
    let mut result = json!({
       "id":response.get("id").cloned().unwrap_or_else(|| json!(format!("resp_{}", uuid::Uuid::new_v4().simple()))),
       "object":"response",
       "status":status,
       "model":response.get("model").cloned().unwrap_or(Value::Null),
       "output":output,
       "usage":{
           "input_tokens":input_tokens,
           "output_tokens":output_tokens,
           "total_tokens":response["usage"]["total_tokens"].as_u64().unwrap_or(input_tokens + output_tokens)
       }
    });
    if incomplete {
        result["incomplete_details"] = json!({"reason":if choice["finish_reason"] == "content_filter" {"content_filter"} else {"max_output_tokens"}});
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_openai_system_tools_and_tool_calls_to_anthropic() {
        let converted = openai_chat_to_anthropic(
            json!({
                "messages":[
                    {"role":"system", "content":"Be concise"},
                    {"role":"user", "content":"Weather?"},
                    {"role":"assistant", "content":null, "tool_calls":[{"id":"call_1", "type":"function", "function":{"name":"weather", "arguments":"{\"city\":\"Shanghai\"}"}}]},
                    {"role":"tool", "tool_call_id":"call_1", "content":"sunny"}
                ],
                "tools":[{"type":"function", "function":{"name":"weather", "description":"Get weather", "parameters":{"type":"object"}}}],
                "max_tokens":123
            }),
            "claude-test",
        )
        .unwrap();
        assert_eq!(converted["model"], "claude-test");
        assert_eq!(converted["system"][0]["text"], "Be concise");
        assert_eq!(converted["messages"][1]["content"][0]["type"], "tool_use");
        assert_eq!(
            converted["messages"][2]["content"][0]["type"],
            "tool_result"
        );
        assert_eq!(converted["tools"][0]["input_schema"]["type"], "object");
    }

    #[test]
    fn round_trips_anthropic_tool_response_and_usage() {
        let openai = anthropic_response_to_openai(json!({
            "id":"msg_1", "model":"claude-test", "stop_reason":"tool_use",
            "content":[{"type":"text", "text":"Checking"}, {"type":"tool_use", "id":"tool_1", "name":"weather", "input":{"city":"Shanghai"}}],
            "usage":{"input_tokens":10, "output_tokens":7}
        }));
        assert_eq!(openai["choices"][0]["finish_reason"], "tool_calls");
        assert_eq!(openai["usage"]["total_tokens"], 17);
        let anthropic = openai_response_to_anthropic(openai);
        assert_eq!(anthropic["content"][1]["name"], "weather");
        assert_eq!(anthropic["usage"]["input_tokens"], 10);
    }

    #[test]
    fn converts_openai_chat_to_gemini_and_back() {
        let gemini = openai_chat_to_gemini(&json!({
            "messages":[{"role":"system", "content":"Be concise"}, {"role":"user", "content":"Hello"}],
            "tools":[{"type":"function", "function":{"name":"lookup", "parameters":{"type":"object"}}}],
            "max_tokens":100
        })).unwrap();
        assert_eq!(
            gemini["systemInstruction"]["parts"][0]["text"],
            "Be concise"
        );
        assert_eq!(gemini["contents"][0]["parts"][0]["text"], "Hello");
        assert_eq!(gemini["generationConfig"]["maxOutputTokens"], 100);

        let openai = gemini_response_to_openai(
            json!({
                "candidates":[{"content":{"parts":[{"text":"Hi"}]}, "finishReason":"STOP"}],
                "usageMetadata":{"promptTokenCount":3, "candidatesTokenCount":2, "totalTokenCount":5}
            }),
            "gemini-test",
        );
        assert_eq!(openai["choices"][0]["message"]["content"], "Hi");
        assert_eq!(openai["usage"]["total_tokens"], 5);

        let chat = gemini_to_openai_chat(
            &json!({
                "systemInstruction":{"parts":[{"text":"Be concise"}]},
                "contents":[{"role":"user", "parts":[{"text":"Hello"}]}],
                "generationConfig":{"maxOutputTokens":42}
            }),
            "selected-model",
        )
        .unwrap();
        assert_eq!(chat["messages"][0]["role"], "system");
        assert_eq!(chat["messages"][1]["content"], "Hello");
        assert_eq!(chat["max_tokens"], 42);

        let gemini_response = openai_response_to_gemini(openai);
        assert_eq!(
            gemini_response["candidates"][0]["content"]["parts"][0]["text"],
            "Hi"
        );
        assert_eq!(gemini_response["usageMetadata"]["totalTokenCount"], 5);
    }

    #[test]
    fn converts_responses_input_and_output() {
        let chat = responses_to_openai_chat(
            &json!({"model":"gpt", "input":"Hello", "max_output_tokens":20}),
        )
        .unwrap();
        assert_eq!(chat["messages"][0]["content"], "Hello");
        assert_eq!(chat["max_tokens"], 20);
        let response = openai_chat_response_to_responses(json!({
            "id":"chat_1", "model":"gpt", "choices":[{"message":{"role":"assistant", "content":"Hi"}, "finish_reason":"stop"}],
            "usage":{"prompt_tokens":2, "completion_tokens":1, "total_tokens":3}
        }));
        assert_eq!(response["object"], "response");
        assert_eq!(response["output"][0]["content"][0]["text"], "Hi");
        assert_eq!(response["usage"]["total_tokens"], 3);

        let native_request = openai_chat_to_responses(
            json!({
                "messages":[{"role":"user", "content":"Hello"}],
                "tools":[{"type":"function", "function":{"name":"lookup", "parameters":{"type":"object"}}}],
                "max_tokens":20
            }),
            "gpt-native",
        )
        .unwrap();
        assert_eq!(native_request["model"], "gpt-native");
        assert_eq!(native_request["input"][0]["content"], "Hello");
        assert_eq!(native_request["tools"][0]["name"], "lookup");

        let chat_response = responses_response_to_openai_chat(json!({
            "id":"resp_1", "model":"gpt-native",
            "output":[
                {"type":"message", "content":[{"type":"output_text", "text":"Done"}]},
                {"type":"function_call", "call_id":"call_1", "name":"lookup", "arguments":"{}"}
            ],
            "usage":{"input_tokens":4, "output_tokens":2, "total_tokens":6}
        }));
        assert_eq!(chat_response["choices"][0]["message"]["content"], "Done");
        assert_eq!(
            chat_response["choices"][0]["message"]["tool_calls"][0]["function"]["name"],
            "lookup"
        );
        assert_eq!(chat_response["usage"]["total_tokens"], 6);
    }

    #[test]
    fn converts_responses_multiturn_tool_calls_and_instructions() {
        let req = json!({
            "model": "deepseek-chat",
            "instructions": "You are a helpful coding assistant.",
            "input": [
                {"type": "message", "role": "user", "content": "What is the weather?"},
                {"type": "function_call", "call_id": "call_weather_1", "name": "get_weather", "arguments": "{\"city\":\"Beijing\"}"},
                {"type": "function_call_output", "call_id": "call_weather_1", "output": "{\"temp\": 22}"}
            ],
            "tools": [
                {
                    "type": "function",
                    "name": "get_weather",
                    "description": "Get current weather",
                    "parameters": {
                        "type": "object",
                        "properties": {
                            "city": {"type": "string"}
                        }
                    }
                }
            ],
            "temperature": 0.7,
            "max_output_tokens": 1000,
            "stream": true
        });

        let chat = responses_to_openai_chat(&req).unwrap();
        assert_eq!(chat["model"], "deepseek-chat");
        assert_eq!(chat["temperature"], 0.7);
        assert_eq!(chat["max_tokens"], 1000);
        assert_eq!(chat["stream"], true);

        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 4);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(
            messages[0]["content"],
            "You are a helpful coding assistant."
        );

        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "What is the weather?");

        assert_eq!(messages[2]["role"], "assistant");
        assert_eq!(messages[2]["tool_calls"][0]["id"], "call_weather_1");
        assert_eq!(
            messages[2]["tool_calls"][0]["function"]["name"],
            "get_weather"
        );
        assert_eq!(
            messages[2]["tool_calls"][0]["function"]["arguments"],
            "{\"city\":\"Beijing\"}"
        );

        assert_eq!(messages[3]["role"], "tool");
        assert_eq!(messages[3]["tool_call_id"], "call_weather_1");
        assert_eq!(messages[3]["content"], "{\"temp\": 22}");

        let tools = chat["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_weather");
        assert_eq!(tools[0]["function"]["description"], "Get current weather");
    }
    include!("adapters_regression_tests.rs");

    #[test]
    fn maps_reasoning_effort_tiers_to_openai_formats() {
        // /effort xhigh：显式档位原样送达。
        let chat = anthropic_to_openai_chat(
            json!({"messages":[{"role":"user","content":"hi"}], "output_config":{"effort":"xhigh"}}),
            "gpt-5.6",
        )
        .unwrap();
        assert_eq!(chat["reasoning_effort"], "xhigh");

        // 支持独立 max 档的模型原样发 max。
        for model in [
            "gpt-5.6",
            "gpt-5.6-sol",
            "gpt-5.6-terra",
            "gpt-5.6-luna",
            "gpt-6-astra",
        ] {
            let chat = anthropic_to_openai_chat(
                json!({"messages":[{"role":"user","content":"hi"}], "output_config":{"effort":"max"}}),
                model,
            )
            .unwrap();
            assert_eq!(chat["reasoning_effort"], "max", "model = {model}");
        }

        // 其余支持推理的模型把 max 降到 xhigh（xAI 对 max 直接 400）。
        let grok = anthropic_to_openai_chat(
            json!({"messages":[{"role":"user","content":"hi"}], "output_config":{"effort":"max"}}),
            "grok-4.6-build",
        )
        .unwrap();
        assert_eq!(grok["reasoning_effort"], "xhigh");

        // 不支持推理强度的模型整字段省略。
        let deepseek = anthropic_to_openai_chat(
            json!({"messages":[{"role":"user","content":"hi"}], "output_config":{"effort":"high"}}),
            "deepseek-chat",
        )
        .unwrap();
        assert!(deepseek.get("reasoning_effort").is_none());

        // 没有显式档位时回退到 thinking 预算。
        let fallback = anthropic_to_openai_chat(
            json!({"messages":[{"role":"user","content":"hi"}], "thinking":{"type":"enabled","budget_tokens":24000}}),
            "gpt-5.6",
        )
        .unwrap();
        assert_eq!(fallback["reasoning_effort"], "high");
        let disabled = anthropic_to_openai_chat(
            json!({"messages":[{"role":"user","content":"hi"}], "thinking":{"type":"disabled"}}),
            "gpt-5.6",
        )
        .unwrap();
        assert!(disabled.get("reasoning_effort").is_none());

        // Chat → Responses 落到 reasoning.effort。
        let responses = openai_chat_to_responses(
            json!({"messages":[{"role":"user","content":"hi"}], "reasoning_effort":"xhigh"}),
            "gpt-5.6",
        )
        .unwrap();
        assert_eq!(responses["reasoning"]["effort"], "xhigh");

        // Responses → Chat 反向映射。
        let back = responses_to_openai_chat(
            &json!({"model":"grok-4.6", "input":"hi", "reasoning":{"effort":"high"}}),
        )
        .unwrap();
        assert_eq!(back["reasoning_effort"], "high");
    }

    #[test]
    fn clamps_responses_max_output_tokens_to_the_minimum() {
        // Claude Desktop 的模型可用性探针发 max_tokens=1。
        let probe = openai_chat_to_responses(
            json!({"messages":[{"role":"user","content":"hi"}], "max_tokens":1}),
            "gpt-5.6",
        )
        .unwrap();
        assert_eq!(probe["max_output_tokens"], 16);

        // 已经合法的值不动。
        let normal = openai_chat_to_responses(
            json!({"messages":[{"role":"user","content":"hi"}], "max_tokens":4096}),
            "gpt-5.6",
        )
        .unwrap();
        assert_eq!(normal["max_output_tokens"], 4096);
    }

    #[test]
    fn omits_tool_description_when_missing_or_null() {
        let anthropic = openai_chat_to_anthropic(
            json!({"messages":[{"role":"user","content":"hi"}], "max_tokens":16, "tools":[
                {"type":"function","function":{"name":"no_desc","parameters":{"type":"object"}}},
                {"type":"function","function":{"name":"null_desc","description":null,"parameters":{"type":"object"}}},
                {"type":"function","function":{"name":"has_desc","description":"keep","parameters":{"type":"object"}}}
            ]}),
            "claude-test",
        )
        .unwrap();
        let tools = anthropic["tools"].as_array().unwrap();
        assert!(tools[0].get("description").is_none());
        assert!(tools[1].get("description").is_none());
        assert_eq!(tools[2]["description"], "keep");

        let chat = anthropic_to_openai_chat(
            json!({"messages":[{"role":"user","content":"hi"}], "tools":[
                {"name":"web_search","input_schema":{"type":"object"}}
            ]}),
            "gpt-4o",
        )
        .unwrap();
        assert!(chat["tools"][0]["function"].get("description").is_none());
    }

    #[test]
    fn downgrades_original_image_detail_for_chat_upstreams() {
        let chat = responses_to_openai_chat(&json!({
            "model":"deepseek-chat",
            "input":[{"type":"message","role":"user","content":[
                {"type":"input_image","image_url":"data:image/png;base64,AAA","detail":"original"}
            ]}]
        }))
        .unwrap();
        let part = &chat["messages"][0]["content"][0];
        assert_eq!(part["image_url"]["detail"], "auto");

        let kept = responses_to_openai_chat(&json!({
            "model":"deepseek-chat",
            "input":[{"type":"message","role":"user","content":[
                {"type":"input_image","image_url":"data:image/png;base64,AAA","detail":"high"}
            ]}]
        }))
        .unwrap();
        assert_eq!(
            kept["messages"][0]["content"][0]["image_url"]["detail"],
            "high"
        );
    }
}
