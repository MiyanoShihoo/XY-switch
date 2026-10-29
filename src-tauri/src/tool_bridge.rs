use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

/// Codex 0.154 起用来携带额外工具的 input 项类型。
const ADDITIONAL_TOOLS_ITEM: &str = "additional_tools";

/// 取出 Responses 请求 input 里 `additional_tools` 载体项携带的工具定义。
///
/// Codex 0.154 起把额外工具（functions / collaboration 执行命名空间、插件）放进
/// `{"type":"additional_tools","role":"developer","tools":[…]}` —— 它**有 role 但没有
/// content**。Codex 自己的 Responses→Chat 转换器把所有带 role 的项当成消息，于是在
/// messages[1] 生成 `{"role":"system","content":null}`，严格的 Chat 网关报
/// `messages[1]: missing field "content"` 后拒绝**整条请求**；与此同时载体里的工具
/// 在 Chat 与 Anthropic 两条转换路径上都被悄悄丢掉，模型根本看不到它们。
///
/// 这里把载体里的工具抽出来，交给和顶层 tools 完全相同的命名空间展开 / 别名逻辑，
/// 载体本身则在 [`ToolBridge::prepare`] 里从 input 中摘掉。
fn additional_tools(request: &Value) -> Vec<Value> {
    request["input"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|item| item["type"] == ADDITIONAL_TOOLS_ITEM)
        .filter_map(|item| item["tools"].as_array())
        .flatten()
        .cloned()
        .collect()
}

// Request-local mapping: no conversation state or credentials are retained here.
#[derive(Clone, Default)]
pub(crate) struct ToolBridge {
    bindings: BTreeMap<String, Binding>,
}
#[derive(Clone)]
struct Binding {
    name: String,
    namespace: Option<String>,
    custom: bool,
}
impl ToolBridge {
    fn alias(namespace: Option<&str>, name: &str) -> String {
        match namespace {
            None => name.to_string(),
            Some(ns) => {
                let hash = Sha256::digest(format!("{ns}\0{name}"));
                let suffix: String = name
                    .chars()
                    .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .take(32)
                    .collect();
                format!("xy_{}_{suffix}", &format!("{hash:x}")[..24])
            }
        }
    }
    pub(crate) fn from_request(request: &Value) -> Result<Self, String> {
        let mut bridge = Self::default();
        if let Some(value) = request.get("tools").filter(|value| !value.is_null()) {
            let tools = value.as_array().ok_or("Tools must be an array")?;
            bridge.collect(tools, None, false)?;
        }
        // 载体里的工具与顶层 tools 去重合并，同名以**顶层声明为准**：
        // 顶层是用户/客户端显式声明的那一份，载体只是 Codex 附带的执行命名空间。
        let extra = additional_tools(request);
        if !extra.is_empty() {
            bridge.collect(&extra, None, true)?;
        }
        Ok(bridge)
    }
    /// `keep_existing` 为 true 时，已存在的别名直接跳过而不是报重复错误。
    /// 只有合并 `additional_tools` 载体时才这么做——顶层 tools 内部重名仍然是硬错误。
    fn collect(
        &mut self,
        tools: &[Value],
        namespace: Option<&str>,
        keep_existing: bool,
    ) -> Result<(), String> {
        for tool in tools {
            let kind = tool["type"].as_str().unwrap_or("");
            let definition = if kind == "function" {
                tool.get("function").unwrap_or(tool)
            } else {
                tool
            };
            if !definition.is_object() {
                return Err("Tool definition must be an object".into());
            }
            if kind == "custom" && tool.get("function").is_some() {
                return Err("Custom tools cannot contain a function wrapper".into());
            }
            let name = definition["name"].as_str().unwrap_or("");
            if kind == "namespace" {
                if namespace.is_some() || name.is_empty() {
                    return Err("Invalid nested tool namespace".into());
                }
                self.collect(
                    tool["tools"]
                        .as_array()
                        .ok_or("Namespace tools must be an array")?,
                    Some(name),
                    keep_existing,
                )?;
            } else if kind == "function" || kind == "custom" {
                if name.is_empty() {
                    return Err("Tool name is required".into());
                }
                let alias = Self::alias(namespace, name);
                if keep_existing && self.bindings.contains_key(&alias) {
                    continue;
                }
                if self
                    .bindings
                    .insert(
                        alias,
                        Binding {
                            name: name.into(),
                            namespace: namespace.map(str::to_string),
                            custom: kind == "custom",
                        },
                    )
                    .is_some()
                {
                    return Err("Duplicate or colliding tool names".into());
                }
            } else {
                return Err(format!(
                    "Chat bridge does not support hosted tool type '{kind}'; disable it or use native Responses"
                ));
            }
        }
        Ok(())
    }
    fn flatten(tools: &[Value], namespace: Option<&str>, output: &mut Vec<Value>) {
        for tool in tools {
            if tool["type"] == "namespace" {
                Self::flatten(
                    tool["tools"].as_array().unwrap(),
                    tool["name"].as_str(),
                    output,
                );
                continue;
            }
            let mut function = tool
                .get("function")
                .cloned()
                .unwrap_or_else(|| tool.clone());
            let name = function["name"].as_str().unwrap();
            function["name"] = json!(Self::alias(namespace, name));
            if tool["type"] == "custom" {
                function = json!({"type":"function","name":function["name"],"description":format!("{}\nPass the raw tool input verbatim in the input string. Tool format: {}", tool["description"].as_str().unwrap_or(""),tool["format"]),"parameters":{"type":"object","properties":{"input":{"type":"string"}},"required":["input"],"additionalProperties":false}});
            } else {
                function["type"] = json!("function");
            }
            output.push(function);
        }
    }
    pub(crate) fn prepare(&self, request: &Value) -> Value {
        let mut result = request.clone();
        let extra = additional_tools(request);
        if request["tools"].is_array() || !extra.is_empty() {
            let mut flat = Vec::new();
            if let Some(tools) = request["tools"].as_array() {
                Self::flatten(tools, None, &mut flat);
            }
            if !extra.is_empty() {
                Self::flatten(&extra, None, &mut flat);
                // 顶层先入列，所以"保留第一次出现"就等于"同名以顶层为准"。
                // 没有载体时不跑这一步，保证不带载体的请求转换结果逐字节不变。
                let mut seen = BTreeSet::new();
                flat.retain(|tool| seen.insert(tool["name"].as_str().unwrap_or("").to_string()));
            }
            result["tools"] = json!(flat);
        }
        if let Some(items) = result["input"].as_array_mut() {
            // 摘掉 additional_tools 载体：它有 role 没有 content，留在 input 里会被
            // 当成消息转换成 {"role":"system","content":null}，严格的 Chat 网关 400 整条请求。
            // 它携带的工具上面已经并进 tools 了，这里丢掉的只是壳。
            items.retain(|item| item["type"] != ADDITIONAL_TOOLS_ITEM);
            for item in items.iter_mut() {
                match item["type"].as_str().unwrap_or("") {
                    "function_call" | "custom_tool_call" => {
                        let name = item["name"].as_str().unwrap_or("");
                        item["name"] = json!(Self::alias(item["namespace"].as_str(), name));
                        if item["type"] == "custom_tool_call" {
                            item["arguments"] = json!(json!({"input":item["input"]}).to_string());
                            item["type"] = json!("function_call");
                            item.as_object_mut().unwrap().remove("input");
                        }
                        item.as_object_mut().unwrap().remove("namespace");
                    }
                    "custom_tool_call_output" => item["type"] = json!("function_call_output"),
                    _ => {}
                }
            }
        }
        if matches!(
            result["tool_choice"]["type"].as_str(),
            Some("function" | "custom")
        ) {
            let choice = &mut result["tool_choice"];
            choice["name"] = json!(Self::alias(
                choice["namespace"].as_str(),
                choice["name"].as_str().unwrap_or("")
            ));
            choice["type"] = json!("function");
            choice.as_object_mut().unwrap().remove("namespace");
        }
        result
    }
    fn custom_input(arguments: &Value) -> Result<Value, String> {
        let parsed: Value = serde_json::from_str(arguments.as_str().unwrap_or(""))
            .map_err(|_| "Invalid JSON arguments for custom tool")?;
        parsed
            .get("input")
            .filter(|v| v.is_string())
            .cloned()
            .ok_or_else(|| "Custom tool arguments must contain a string input".into())
    }
    pub(crate) fn restore_item(&self, item: &mut Value) -> Result<(), String> {
        if item["type"] != "function_call" {
            return Ok(());
        }
        let Some(binding) = item["name"]
            .as_str()
            .and_then(|name| self.bindings.get(name))
        else {
            return Ok(());
        };
        item["name"] = json!(binding.name);
        if let Some(namespace) = &binding.namespace {
            item["namespace"] = json!(namespace);
        }
        if binding.custom {
            item["input"] = if item["status"] == "in_progress" {
                json!("")
            } else {
                Self::custom_input(&item["arguments"])?
            };
            item["type"] = json!("custom_tool_call");
            item.as_object_mut().unwrap().remove("arguments");
        }
        Ok(())
    }
    pub(crate) fn restore_response(&self, response: &mut Value) -> Result<(), String> {
        if let Some(items) = response["output"].as_array_mut() {
            for item in items {
                self.restore_item(item)?;
            }
        }
        Ok(())
    }
    pub(crate) fn restore_event(
        &self,
        event: &mut Value,
        alias: Option<&str>,
    ) -> Result<bool, String> {
        let kind = event["type"].as_str().unwrap_or("").to_string();
        if kind == "response.output_item.added" || kind == "response.output_item.done" {
            self.restore_item(&mut event["item"])?;
        }
        if kind == "response.completed" || kind == "response.incomplete" {
            self.restore_response(&mut event["response"])?;
        }
        if let Some(binding) = alias.and_then(|name| self.bindings.get(name)) {
            if binding.custom {
                if kind == "response.function_call_arguments.delta" {
                    return Ok(false);
                }
                if kind == "response.function_call_arguments.done" {
                    event["type"] = json!("response.custom_tool_call_input.done");
                    event["input"] = Self::custom_input(&event["arguments"])?;
                    event.as_object_mut().unwrap().remove("arguments");
                }
            }
            if event.get("name").is_some() {
                event["name"] = json!(binding.name);
            }
        }
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn namespace_and_custom_tool_round_trip_preserves_identity_and_raw_input() {
        let request = json!({"input":[{"type":"custom_tool_call","namespace":"files","name":"patch","call_id":"c1","input":"line\n\"中文\""},{"type":"custom_tool_call_output","call_id":"c1","output":"ok"}],"tools":[{"type":"namespace","name":"files","tools":[{"type":"custom","name":"patch","description":"Edit files","format":{"type":"text"}},{"type":"function","name":"read","parameters":{"type":"object"}}]}],"tool_choice":{"type":"custom","namespace":"files","name":"patch"}});
        let bridge = ToolBridge::from_request(&request).unwrap();
        let prepared = bridge.prepare(&request);
        let alias = prepared["tools"][0]["name"].as_str().unwrap();
        assert!(alias.len() <= 64);
        assert_eq!(prepared["input"][0]["name"], alias);
        assert_eq!(prepared["input"][0]["type"], "function_call");
        assert_eq!(prepared["input"][1]["type"], "function_call_output");
        assert_eq!(prepared["tool_choice"]["name"], alias);
        let mut output = json!({"type":"function_call","status":"completed","name":alias,"call_id":"c1","arguments":prepared["input"][0]["arguments"]});
        bridge.restore_item(&mut output).unwrap();
        assert_eq!(output["namespace"], "files");
        assert_eq!(output["name"], "patch");
        assert_eq!(output["type"], "custom_tool_call");
        assert_eq!(output["input"], "line\n\"中文\"");
        let chat = crate::adapters::responses_to_openai_chat(&request).unwrap();
        assert_eq!(
            chat["messages"][0]["tool_calls"][0]["function"]["name"],
            alias
        );
        assert_eq!(chat["messages"][1]["role"], "tool");
    }
    #[test]
    fn namespace_aliases_are_stable_and_do_not_collide_for_equal_function_names() {
        let request = json!({"tools":[{"type":"namespace","name":"one","tools":[{"type":"function","name":"read"}]},{"type":"namespace","name":"two","tools":[{"type":"function","name":"read"}]}]});
        let bridge = ToolBridge::from_request(&request).unwrap();
        let prepared = bridge.prepare(&request);
        assert_ne!(prepared["tools"][0]["name"], prepared["tools"][1]["name"]);
        let mut reversed = request.clone();
        reversed["tools"].as_array_mut().unwrap().reverse();
        assert_eq!(
            ToolBridge::from_request(&reversed)
                .unwrap()
                .prepare(&reversed)["tools"][1]["name"],
            prepared["tools"][0]["name"]
        );
        let mut item =
            json!({"type":"function_call","name":prepared["tools"][1]["name"],"arguments":"{}"});
        bridge.restore_item(&mut item).unwrap();
        assert_eq!(item["namespace"], "two");
        assert_eq!(item["name"], "read");
    }
    #[test]
    fn custom_tool_stream_events_require_valid_json_and_string_input() {
        let bridge =
            ToolBridge::from_request(&json!({"tools":[{"type":"custom","name":"patch"}]})).unwrap();
        let mut delta =
            json!({"type":"response.function_call_arguments.delta","delta":"{\"input\""});
        assert!(!bridge.restore_event(&mut delta, Some("patch")).unwrap());
        let mut done = json!({"type":"response.function_call_arguments.done","arguments":"{\"input\":\"abc\"}"});
        assert!(bridge.restore_event(&mut done, Some("patch")).unwrap());
        assert_eq!(done["type"], "response.custom_tool_call_input.done");
        assert_eq!(done["input"], "abc");
        for bad in ["not-json", "{}", "{\"input\":42}"] {
            let mut item =
                json!({"type":"function_call","name":"patch","status":"completed","arguments":bad});
            assert!(bridge.restore_item(&mut item).is_err());
        }
    }
    #[test]
    fn additional_tools_carrier_is_merged_and_never_becomes_a_message() {
        // 带载体：载体里的工具要并进 tools，载体本身不能变成消息。
        let request = json!({
            "input":[
                {"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]},
                {"type":"additional_tools","role":"developer","tools":[
                    {"type":"namespace","name":"functions","tools":[{"type":"function","name":"exec_command","parameters":{"type":"object"}}]},
                    {"type":"function","name":"shared","parameters":{"type":"object"}}
                ]}
            ],
            "tools":[{"type":"function","name":"shared","description":"顶层声明优先","parameters":{"type":"object"}}]
        });
        let bridge = ToolBridge::from_request(&request).unwrap();
        let prepared = bridge.prepare(&request);

        // 载体已从 input 摘掉，只剩那条真正的用户消息。
        let items = prepared["input"].as_array().unwrap();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0]["role"], "user");

        // 同名工具以顶层为准，命名空间下的工具被展开成独立条目。
        let tools = prepared["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0]["name"], "shared");
        assert_eq!(tools[0]["description"], "顶层声明优先");
        let namespaced = tools[1]["name"].as_str().unwrap().to_string();
        assert_ne!(namespaced, "exec_command");

        // 响应侧能把展开后的名字还原成 Codex 认识的形式。
        let mut item = json!({"type":"function_call","name":namespaced,"arguments":"{}"});
        bridge.restore_item(&mut item).unwrap();
        assert_eq!(item["namespace"], "functions");
        assert_eq!(item["name"], "exec_command");

        // 转成 Chat 时不能出现 content 为空/为 null 的多余系统消息。
        let chat = crate::adapters::responses_to_openai_chat(&request).unwrap();
        let messages = chat["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(chat["tools"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn requests_without_the_carrier_convert_byte_for_byte_as_before() {
        let request = json!({
            "model":"gpt",
            "input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}],
            "tools":[{"type":"function","name":"read","parameters":{"type":"object"}}]
        });
        let bridge = ToolBridge::from_request(&request).unwrap();
        let prepared = bridge.prepare(&request);
        assert_eq!(
            serde_json::to_string(&prepared).unwrap(),
            serde_json::to_string(&json!({
                "model":"gpt",
                "input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}],
                "tools":[{"type":"function","name":"read","parameters":{"type":"object"}}]
            }))
            .unwrap()
        );
    }

    #[test]
    fn unsupported_hosted_and_duplicate_tools_are_not_silently_dropped() {
        for request in [
            json!({"tools":[{"type":"web_search"}]}),
            json!({"tools":[{"type":"function","name":"same"},{"type":"custom","name":"same"}]}),
            json!({"tools":[{"type":"function"}]}),
        ] {
            assert!(ToolBridge::from_request(&request).is_err());
        }
    }
}
