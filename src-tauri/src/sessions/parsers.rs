use super::{
    Client, MAX_LINE_BYTES, MAX_MESSAGE_BYTES, MAX_MESSAGES, SessionDetail, SessionMessage, warn,
};
use crate::session_usage_codex::parse_iso_timestamp_ms;
use serde_json::Value;
use std::{collections::HashMap, path::Path};

#[derive(Default)]
pub(super) struct Parsed {
    pub id: Option<String>,
    pub title: Option<String>,
    pub project_dir: Option<String>,
    pub created_at: Option<i64>,
    pub last_active_at: Option<i64>,
    pub detail: SessionDetail,
}

fn string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|s| !s.trim().is_empty())
        .map(str::to_owned)
}

fn timestamp(value: &Value) -> Option<i64> {
    if let Some(s) = value.as_str() {
        return parse_iso_timestamp_ms(s);
    }
    let n = value.as_i64()?;
    if n <= 0 {
        None
    } else if n < 1_000_000_000_000 {
        n.checked_mul(1000)
    } else {
        Some(n)
    }
}

fn text(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .map(text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Object(_) => match value.get("type").and_then(Value::as_str).unwrap_or("text") {
            "text" | "input_text" | "output_text" => value
                .get("text")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string(),
            "tool_use" => format!(
                "[tool: {}]\n{}",
                value["name"].as_str().unwrap_or("unknown"),
                value.get("input").unwrap_or(&Value::Null)
            ),
            "tool_result" => value.get("content").map(text).unwrap_or_default(),
            "image" | "input_image" | "image_url" => "[image]".to_string(),
            "audio" | "input_audio" => "[audio]".to_string(),
            // Reasoning, encrypted content and unsupported binary payloads are not conversation text.
            _ => String::new(),
        },
        _ => String::new(),
    }
}

fn title(content: &str) -> Option<String> {
    let trimmed = content.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("# AGENTS.md instructions")
        || trimmed.starts_with("<environment_context>")
    {
        return None;
    }
    Some(
        trimmed
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(120)
            .collect(),
    )
}

fn role(value: &str) -> Option<&'static str> {
    match value {
        "user" => Some("user"),
        "assistant" | "gemini" => Some("assistant"),
        "tool" | "function" => Some("tool"),
        "system" | "developer" | "info" | "error" => Some("system"),
        _ => None,
    }
}

struct Messages {
    records: Vec<(SessionMessage, bool)>,
    bytes: usize,
    limited: bool,
}
impl Messages {
    fn new() -> Self {
        Self {
            records: Vec::new(),
            bytes: 0,
            limited: false,
        }
    }
    fn push(
        &mut self,
        parsed: &mut Parsed,
        role: &str,
        content: String,
        ts: Option<i64>,
        fallback: bool,
        include: bool,
    ) {
        if content.trim().is_empty() {
            return;
        }
        if role == "user" && parsed.title.is_none() {
            parsed.title = title(&content);
        }
        if !include {
            return;
        }
        if self.records.len() >= MAX_MESSAGES
            || self.bytes.saturating_add(content.len()) > MAX_MESSAGE_BYTES
        {
            self.limited = true;
            return;
        }
        self.bytes += content.len();
        self.records.push((
            SessionMessage {
                role: role.into(),
                content,
                ts,
            },
            fallback,
        ));
    }
    fn finish(self, parsed: &mut Parsed) {
        // Codex can record the same user/assistant message as both response_item
        // and event_msg. Match occurrences, not a global set: repeated turns survive.
        let mut primary = HashMap::<(String, String), usize>::new();
        for (m, fallback) in &self.records {
            if !fallback {
                *primary
                    .entry((m.role.clone(), m.content.trim().into()))
                    .or_default() += 1;
            }
        }
        for (m, fallback) in self.records {
            if fallback
                && let Some(count) = primary.get_mut(&(m.role.clone(), m.content.trim().into()))
                && *count > 0
            {
                *count -= 1;
                continue;
            }
            parsed.detail.messages.push(m);
        }
        if self.limited {
            warn(
                &mut parsed.detail.warnings,
                "消息显示已达到安全上限，仅显示前面的部分记录；完整内容请在原客户端查看".into(),
            );
        }
    }
}

pub(super) fn parse(
    client: Client,
    path: &Path,
    bytes: &[u8],
    truncated: bool,
    include: bool,
) -> Result<Parsed, String> {
    if client == Client::Gemini {
        return parse_gemini(bytes, truncated, include);
    }
    let mut parsed = Parsed::default();
    let mut messages = Messages::new();
    let mut invalid = 0usize;
    let mut last_assistant = String::new();
    let mut recognized_claude = false;
    let mut offset = 0usize;
    for line in bytes.split(|c| *c == b'\n') {
        offset += line.len() + 1;
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        if truncated && offset > bytes.len() && !bytes.ends_with(b"\n") {
            break;
        }
        if line.len() > MAX_LINE_BYTES {
            invalid += 1;
            continue;
        }
        let value: Value = match serde_json::from_slice(line) {
            Ok(v) => v,
            Err(_) => {
                invalid += 1;
                continue;
            }
        };
        let ts = value.get("timestamp").and_then(timestamp);
        parsed.created_at = parsed.created_at.or(ts);
        let kind = value["type"].as_str().unwrap_or("");
        if client == Client::Claude {
            if let Some(id) = string(&value, "sessionId") {
                if parsed.id.as_ref().is_some_and(|known| known != &id) {
                    return Err("日志包含冲突的会话 ID".into());
                }
                parsed.id = Some(id);
            }
            parsed.project_dir = parsed.project_dir.or_else(|| string(&value, "cwd"));
            if kind == "custom-title" {
                parsed.title = string(&value, "customTitle").and_then(|s| title(&s));
            }
            if kind == "summary" && parsed.title.is_none() {
                parsed.title = string(&value, "summary").and_then(|s| title(&s));
            }
            if matches!(kind, "user" | "assistant" | "system") {
                recognized_claude = true;
                let message = value.get("message").unwrap_or(&value);
                if let Some(r) = role(message["role"].as_str().unwrap_or(kind)) {
                    if let Some(blocks) = message["content"].as_array() {
                        // Claude places tool_result blocks inside user envelopes.
                        // Keep their order, but never label them as user instructions
                        // or use a tool result as the conversation title.
                        let mut pending = String::new();
                        for block in blocks {
                            if block["type"] == "tool_result" {
                                messages.push(
                                    &mut parsed,
                                    r,
                                    std::mem::take(&mut pending),
                                    ts,
                                    false,
                                    include,
                                );
                                messages.push(&mut parsed, "tool", text(block), ts, false, include);
                            } else {
                                let content = text(block);
                                if !content.is_empty() {
                                    if !pending.is_empty() {
                                        pending.push('\n');
                                    }
                                    pending.push_str(&content);
                                }
                            }
                        }
                        messages.push(&mut parsed, r, pending, ts, false, include);
                    } else {
                        messages.push(
                            &mut parsed,
                            r,
                            text(&message["content"]),
                            ts,
                            false,
                            include,
                        );
                    }
                }
            }
        } else if kind == "session_meta" {
            let payload = &value["payload"];
            if let Some(id) = string(payload, "id") {
                if parsed.id.as_ref().is_some_and(|known| known != &id) {
                    return Err("日志包含冲突的会话 ID".into());
                }
                parsed.id = Some(id);
            }
            parsed.project_dir = string(payload, "cwd");
            parsed.created_at = payload
                .get("timestamp")
                .and_then(timestamp)
                .or(parsed.created_at);
        } else if kind == "response_item" {
            let payload = &value["payload"];
            match payload["type"].as_str().unwrap_or("") {
                "message" => {
                    if let Some(r) = payload["role"].as_str().and_then(role) {
                        let content = text(&payload["content"]);
                        if r == "assistant" {
                            last_assistant.clone_from(&content);
                        }
                        messages.push(&mut parsed, r, content, ts, false, include);
                    }
                }
                "function_call" | "custom_tool_call" => {
                    let content = format!(
                        "[tool: {}]\n{}",
                        payload["name"].as_str().unwrap_or("unknown"),
                        payload
                            .get("arguments")
                            .or_else(|| payload.get("input"))
                            .map(text)
                            .unwrap_or_default()
                    );
                    messages.push(&mut parsed, "assistant", content, ts, false, include);
                }
                "function_call_output" | "custom_tool_call_output" => {
                    messages.push(
                        &mut parsed,
                        "tool",
                        text(&payload["output"]),
                        ts,
                        false,
                        include,
                    );
                }
                _ => {}
            }
        } else if kind == "event_msg" {
            let payload = &value["payload"];
            match payload["type"].as_str().unwrap_or("") {
                "user_message" => messages.push(
                    &mut parsed,
                    "user",
                    text(&payload["message"]),
                    ts,
                    true,
                    include,
                ),
                "agent_message" => {
                    let content = text(&payload["message"]);
                    last_assistant.clone_from(&content);
                    messages.push(&mut parsed, "assistant", content, ts, true, include);
                }
                "task_complete" => {
                    let content = text(&payload["last_agent_message"]);
                    if content != last_assistant {
                        last_assistant.clone_from(&content);
                        messages.push(&mut parsed, "assistant", content, ts, true, include);
                    }
                }
                _ => {}
            }
        }
    }
    // Claude logs written by older versions may omit sessionId; the constrained
    // filename is the native CLI's session identifier, not an IPC-provided path.
    if client == Client::Claude && !recognized_claude && parsed.id.is_none() {
        return Err("不是可识别的 Claude 会话日志".into());
    }
    if client == Client::Claude && parsed.id.is_none() {
        parsed.id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(str::to_string);
    }
    if invalid > 0 {
        warn(
            &mut parsed.detail.warnings,
            format!("跳过 {invalid} 条损坏、未写完或过大的日志记录"),
        );
    }
    messages.finish(&mut parsed);
    Ok(parsed)
}

fn parse_gemini(bytes: &[u8], truncated: bool, include: bool) -> Result<Parsed, String> {
    if truncated {
        return Err("Gemini 会话超过本轮读取上限，未将截断 JSON 当成完整记录".into());
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| "Gemini 会话不是完整的 JSON 文件")?;
    let records = value
        .get("messages")
        .and_then(Value::as_array)
        .ok_or("Gemini 会话缺少 messages 数组")?;
    let mut parsed = Parsed {
        id: string(&value, "sessionId"),
        title: string(&value, "title")
            .or_else(|| string(&value, "summary"))
            .and_then(|s| title(&s)),
        project_dir: string(&value, "cwd").or_else(|| string(&value, "projectDir")),
        created_at: value.get("startTime").and_then(timestamp),
        last_active_at: value.get("lastUpdated").and_then(timestamp),
        ..Parsed::default()
    };
    let mut messages = Messages::new();
    for record in records {
        let Some(r) = record["type"]
            .as_str()
            .or_else(|| record["role"].as_str())
            .and_then(role)
        else {
            continue;
        };
        let ts = record.get("timestamp").and_then(timestamp);
        messages.push(&mut parsed, r, text(&record["content"]), ts, false, include);
        if let Some(calls) = record["toolCalls"].as_array() {
            for call in calls {
                let content = format!(
                    "[tool: {}]\n{}",
                    call["name"].as_str().unwrap_or("unknown"),
                    call.get("args").unwrap_or(&Value::Null)
                );
                messages.push(&mut parsed, "assistant", content, ts, false, include);
                if let Some(result) = call.get("result") {
                    let content = if result.is_string() || result.is_array() {
                        text(result)
                    } else {
                        result.to_string()
                    };
                    messages.push(&mut parsed, "tool", content, ts, false, include);
                }
            }
        }
    }
    messages.finish(&mut parsed);
    Ok(parsed)
}
