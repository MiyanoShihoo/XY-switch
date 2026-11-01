use serde_json::{Map, Value};

const INVALID_INPUT: &str = "budget_invalid_input";
const INVALID_TOOLS: &str = "budget_invalid_tools";

fn fields<'a>(
    value: &'a Value,
    allowed: &[&str],
    invalid: &str,
) -> Result<&'a Map<String, Value>, String> {
    let object = value.as_object().ok_or(invalid)?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        return Err("budget_unbounded_parameter".into());
    }
    Ok(object)
}

fn optional(
    object: &Map<String, Value>,
    key: &str,
    accepts: fn(&Value) -> bool,
    invalid: &str,
) -> Result<(), String> {
    if object
        .get(key)
        .is_none_or(|value| value.is_null() || accepts(value))
    {
        Ok(())
    } else {
        Err(invalid.into())
    }
}

fn required(
    object: &Map<String, Value>,
    key: &str,
    accepts: fn(&Value) -> bool,
    invalid: &str,
) -> Result<(), String> {
    if object.get(key).is_some_and(accepts) {
        Ok(())
    } else {
        Err(invalid.into())
    }
}

fn name(value: &Value) -> bool {
    value.as_str().is_some_and(|name| !name.is_empty())
}

fn client_execution(object: &Map<String, Value>) -> Result<(), String> {
    if object.get("execution").and_then(Value::as_str) == Some("client") {
        Ok(())
    } else {
        Err("budget_server_tools_blocked".into())
    }
}

fn tools(value: &Value) -> Result<(), String> {
    for tool in value.as_array().ok_or(INVALID_TOOLS)? {
        match tool.get("type").and_then(Value::as_str) {
            Some("function") => {
                let object = fields(
                    tool,
                    &[
                        "type",
                        "name",
                        "description",
                        "parameters",
                        "strict",
                        "defer_loading",
                    ],
                    INVALID_TOOLS,
                )?;
                required(object, "name", name, INVALID_TOOLS)?;
                optional(object, "description", Value::is_string, INVALID_TOOLS)?;
                optional(object, "parameters", Value::is_object, INVALID_TOOLS)?;
                optional(object, "strict", Value::is_boolean, INVALID_TOOLS)?;
                optional(object, "defer_loading", Value::is_boolean, INVALID_TOOLS)?;
            }
            Some("custom") => {
                let object = fields(
                    tool,
                    &["type", "name", "description", "format", "defer_loading"],
                    INVALID_TOOLS,
                )?;
                required(object, "name", name, INVALID_TOOLS)?;
                optional(object, "description", Value::is_string, INVALID_TOOLS)?;
                optional(object, "defer_loading", Value::is_boolean, INVALID_TOOLS)?;
                if let Some(format) = object.get("format").filter(|value| !value.is_null()) {
                    match format.get("type").and_then(Value::as_str) {
                        Some("text") => {
                            fields(format, &["type"], INVALID_TOOLS)?;
                        }
                        Some("grammar") => {
                            let grammar =
                                fields(format, &["type", "syntax", "definition"], INVALID_TOOLS)?;
                            if !matches!(
                                grammar.get("syntax").and_then(Value::as_str),
                                Some("lark" | "regex")
                            ) {
                                return Err(INVALID_TOOLS.into());
                            }
                            required(grammar, "definition", Value::is_string, INVALID_TOOLS)?;
                        }
                        _ => return Err(INVALID_TOOLS.into()),
                    }
                }
            }
            Some("namespace") => {
                let object = fields(
                    tool,
                    &["type", "name", "description", "tools"],
                    INVALID_TOOLS,
                )?;
                required(object, "name", name, INVALID_TOOLS)?;
                optional(object, "description", Value::is_string, INVALID_TOOLS)?;
                tools(object.get("tools").ok_or(INVALID_TOOLS)?)?;
            }
            Some("tool_search") => {
                let object = fields(
                    tool,
                    &["type", "execution", "description", "parameters"],
                    INVALID_TOOLS,
                )?;
                client_execution(object)?;
                optional(object, "description", Value::is_string, INVALID_TOOLS)?;
                optional(object, "parameters", Value::is_object, INVALID_TOOLS)?;
            }
            _ => return Err("budget_server_tools_blocked".into()),
        }
    }
    Ok(())
}

fn text_part(value: &Value) -> Result<(), String> {
    let (allowed, text) = match value.get("type").and_then(Value::as_str) {
        Some("input_text") => (&["type", "text"][..], "text"),
        Some("output_text") => (&["type", "text", "annotations", "logprobs"][..], "text"),
        Some("refusal") => (&["type", "refusal"][..], "refusal"),
        _ => return Err("budget_unbounded_input".into()),
    };
    let object = fields(value, allowed, INVALID_INPUT)?;
    required(object, text, Value::is_string, INVALID_INPUT)?;
    optional(object, "annotations", Value::is_array, INVALID_INPUT)?;
    optional(object, "logprobs", Value::is_array, INVALID_INPUT)
}

fn text_content(value: &Value) -> Result<(), String> {
    if value.is_string() {
        return Ok(());
    }
    for part in value.as_array().ok_or(INVALID_INPUT)? {
        text_part(part)?;
    }
    Ok(())
}

fn reasoning_parts(value: &Value, kind: &str) -> Result<(), String> {
    for part in value.as_array().ok_or(INVALID_INPUT)? {
        let object = fields(part, &["type", "text"], INVALID_INPUT)?;
        if object.get("type").and_then(Value::as_str) != Some(kind) {
            return Err("budget_unbounded_input".into());
        }
        required(object, "text", Value::is_string, INVALID_INPUT)?;
    }
    Ok(())
}

fn input_item(value: &Value) -> Result<(), String> {
    let object = match value.get("type").and_then(Value::as_str) {
        None | Some("message") => {
            let object = fields(
                value,
                &["type", "id", "role", "content", "status", "phase"],
                INVALID_INPUT,
            )?;
            if !matches!(
                object.get("role").and_then(Value::as_str),
                Some("user" | "assistant" | "developer" | "system")
            ) {
                return Err(INVALID_INPUT.into());
            }
            if object.get("type").is_some_and(|kind| kind != "message") {
                return Err(INVALID_INPUT.into());
            }
            text_content(object.get("content").ok_or(INVALID_INPUT)?)?;
            optional(object, "phase", Value::is_string, INVALID_INPUT)?;
            object
        }
        Some("function_call" | "custom_tool_call") => {
            let content = if value["type"] == "function_call" {
                "arguments"
            } else {
                "input"
            };
            let object = fields(
                value,
                &[
                    "type",
                    "id",
                    "call_id",
                    "name",
                    "namespace",
                    content,
                    "status",
                ],
                INVALID_INPUT,
            )?;
            required(object, "call_id", name, INVALID_INPUT)?;
            required(object, "name", name, INVALID_INPUT)?;
            required(object, content, Value::is_string, INVALID_INPUT)?;
            optional(object, "namespace", Value::is_string, INVALID_INPUT)?;
            object
        }
        Some("function_call_output" | "custom_tool_call_output") => {
            let object = fields(
                value,
                &["type", "id", "call_id", "output", "status"],
                INVALID_INPUT,
            )?;
            required(object, "call_id", name, INVALID_INPUT)?;
            text_content(object.get("output").ok_or(INVALID_INPUT)?)?;
            object
        }
        Some("tool_search_call") => {
            let object = fields(
                value,
                &["type", "id", "call_id", "execution", "arguments", "status"],
                INVALID_INPUT,
            )?;
            client_execution(object)?;
            required(
                object,
                "arguments",
                |arguments| arguments.is_object() || arguments.is_string(),
                INVALID_INPUT,
            )?;
            optional(object, "call_id", Value::is_string, INVALID_INPUT)?;
            object
        }
        Some("tool_search_output") => {
            let object = fields(
                value,
                &["type", "id", "call_id", "execution", "tools", "status"],
                INVALID_INPUT,
            )?;
            client_execution(object)?;
            tools(object.get("tools").ok_or(INVALID_TOOLS)?)?;
            optional(object, "call_id", Value::is_string, INVALID_INPUT)?;
            object
        }
        Some("additional_tools") => {
            let object = fields(value, &["type", "role", "tools"], INVALID_INPUT)?;
            if object.get("role").is_some_and(|role| role != "developer") {
                return Err(INVALID_INPUT.into());
            }
            tools(object.get("tools").ok_or(INVALID_TOOLS)?)?;
            object
        }
        Some("reasoning") => {
            let object = fields(
                value,
                &[
                    "type",
                    "id",
                    "summary",
                    "content",
                    "encrypted_content",
                    "status",
                ],
                INVALID_INPUT,
            )?;
            if object
                .get("encrypted_content")
                .is_some_and(|content| !content.is_null() && content.as_str() != Some(""))
            {
                return Err("budget_unbounded_input".into());
            }
            if let Some(summary) = object.get("summary") {
                reasoning_parts(summary, "summary_text")?;
            }
            if let Some(content) = object.get("content") {
                reasoning_parts(content, "reasoning_text")?;
            }
            object
        }
        _ => return Err("budget_unbounded_input".into()),
    };
    optional(object, "id", Value::is_string, INVALID_INPUT)?;
    optional(object, "status", Value::is_string, INVALID_INPUT)
}

fn tool_choice(value: &Value, client_search: bool) -> Result<(), String> {
    if matches!(value.as_str(), Some("auto" | "none" | "required")) {
        return Ok(());
    }
    match value.get("type").and_then(Value::as_str) {
        Some("function" | "custom") => {
            let object = fields(
                value,
                &["type", "name", "namespace"],
                "budget_invalid_tool_choice",
            )?;
            required(object, "name", name, "budget_invalid_tool_choice")?;
            optional(
                object,
                "namespace",
                Value::is_string,
                "budget_invalid_tool_choice",
            )
        }
        Some("tool_search") => {
            fields(value, &["type"], "budget_invalid_tool_choice")?;
            if !client_search {
                return Err("budget_server_tools_blocked".into());
            }
            Ok(())
        }
        Some("allowed_tools") => {
            let object = fields(
                value,
                &["type", "mode", "tools"],
                "budget_invalid_tool_choice",
            )?;
            if !matches!(
                object.get("mode").and_then(Value::as_str),
                Some("auto" | "required")
            ) {
                return Err("budget_invalid_tool_choice".into());
            }
            for choice in object
                .get("tools")
                .and_then(Value::as_array)
                .ok_or("budget_invalid_tool_choice")?
            {
                if !choice.is_object()
                    || choice.get("type").and_then(Value::as_str) == Some("allowed_tools")
                {
                    return Err("budget_invalid_tool_choice".into());
                }
                tool_choice(choice, client_search)?;
            }
            Ok(())
        }
        _ => Err("budget_server_tools_blocked".into()),
    }
}

fn text_config(value: &Value) -> Result<(), String> {
    let object = fields(value, &["format", "verbosity"], "budget_invalid_request")?;
    optional(
        object,
        "verbosity",
        Value::is_string,
        "budget_invalid_request",
    )?;
    if let Some(format) = object.get("format") {
        match format.get("type").and_then(Value::as_str) {
            Some("text" | "json_object") => {
                fields(format, &["type"], "budget_invalid_request")?;
            }
            Some("json_schema") => {
                let schema = fields(
                    format,
                    &["type", "name", "description", "schema", "strict"],
                    "budget_invalid_request",
                )?;
                required(schema, "name", name, "budget_invalid_request")?;
                required(schema, "schema", Value::is_object, "budget_invalid_request")?;
                optional(
                    schema,
                    "description",
                    Value::is_string,
                    "budget_invalid_request",
                )?;
                optional(
                    schema,
                    "strict",
                    Value::is_boolean,
                    "budget_invalid_request",
                )?;
            }
            _ => return Err("budget_unbounded_parameter".into()),
        }
    }
    Ok(())
}

pub(super) fn validate(payload: &Value) -> Result<(), String> {
    let object = payload.as_object().ok_or("budget_invalid_request")?;
    if [
        "previous_response_id",
        "conversation",
        "prompt",
        "context_management",
    ]
    .iter()
    .any(|key| object.contains_key(*key))
    {
        return Err("budget_unbounded_input".into());
    }
    if let Some(input) = object.get("input") {
        if !input.is_string() {
            for item in input.as_array().ok_or(INVALID_INPUT)? {
                input_item(item)?;
            }
        }
    }
    if let Some(definitions) = object.get("tools") {
        tools(definitions)?;
    }
    let client_search = object
        .get("tools")
        .and_then(Value::as_array)
        .is_some_and(|definitions| {
            definitions
                .iter()
                .any(|tool| tool["type"] == "tool_search" && tool["execution"] == "client")
        });
    if let Some(choice) = object.get("tool_choice") {
        tool_choice(choice, client_search)?;
    }
    if let Some(text) = object.get("text") {
        text_config(text)?;
    }
    if let Some(reasoning) = object.get("reasoning") {
        let reasoning = fields(
            reasoning,
            &["effort", "summary", "generate_summary"],
            "budget_invalid_request",
        )?;
        for key in ["effort", "summary", "generate_summary"] {
            optional(reasoning, key, Value::is_string, "budget_invalid_request")?;
        }
    }
    if let Some(include) = object.get("include") {
        if !include
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string))
        {
            return Err("budget_invalid_request".into());
        }
    }
    for key in [
        "instructions",
        "user",
        "truncation",
        "safety_identifier",
        "prompt_cache_key",
    ] {
        optional(object, key, Value::is_string, "budget_invalid_request")?;
    }
    for key in ["metadata", "client_metadata"] {
        optional(object, key, Value::is_object, "budget_invalid_request")?;
    }
    for key in ["stream", "store", "parallel_tool_calls"] {
        optional(object, key, Value::is_boolean, "budget_invalid_request")?;
    }
    Ok(())
}
