use super::{SessionToolEvent, SessionToolKind, SessionToolStatus};
use serde_json::Value;

pub(super) struct ToolRecord {
    pub role: &'static str,
    pub content: String,
    pub event: SessionToolEvent,
}

fn field(value: &Value, key: &str) -> Option<String> {
    value.get(key)?.as_str().filter(|text| !text.trim().is_empty()).map(str::to_owned)
}

fn call_id(value: &Value) -> Option<String> {
    field(value, "call_id").or_else(|| field(value, "callId"))
}

pub(super) fn content(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Array(blocks) if blocks.iter().all(|block| matches!(block["type"].as_str(),
            Some("text" | "input_text" | "output_text" | "image" | "input_image" | "image_url" | "audio" | "input_audio"))) => {
            blocks.iter().map(content).filter(|text| !text.is_empty()).collect::<Vec<_>>().join("\n")
        }
        Value::Object(_) if matches!(value["type"].as_str(), Some("text" | "input_text" | "output_text")) => {
            value["text"].as_str().unwrap_or_default().to_owned()
        }
        Value::Object(_) if matches!(value["type"].as_str(), Some("image" | "input_image" | "image_url")) => "[image]".into(),
        Value::Object(_) if matches!(value["type"].as_str(), Some("audio" | "input_audio")) => "[audio]".into(),
        _ => value.to_string(),
    }
}

fn status(value: &Value, default: SessionToolStatus) -> SessionToolStatus {
    match value["status"].as_str() {
        Some("in_progress" | "running" | "executing") => SessionToolStatus::Running,
        Some("completed") => SessionToolStatus::Completed,
        Some("succeeded" | "success") => SessionToolStatus::Succeeded,
        Some("failed" | "error") => SessionToolStatus::Failed,
        Some("cancelled" | "canceled") => SessionToolStatus::Cancelled,
        Some("pending_approval" | "waiting_for_approval") => SessionToolStatus::PendingApproval,
        Some("approved" | "accepted") => SessionToolStatus::Approved,
        Some("denied" | "rejected") => SessionToolStatus::Denied,
        _ => default,
    }
}

fn result_status(value: &Value) -> SessionToolStatus {
    let explicit = value.get("is_error").or_else(|| value.get("isError")).and_then(Value::as_bool);
    if explicit == Some(true) {
        return SessionToolStatus::Failed;
    }
    status(value, match explicit {
        Some(true) => SessionToolStatus::Failed,
        Some(false) => SessionToolStatus::Succeeded,
        None => SessionToolStatus::ResultReceived,
    })
}

fn approval_status(value: &Value) -> SessionToolStatus {
    if let Some(approved) = value["approve"].as_bool() {
        return if approved { SessionToolStatus::Approved } else { SessionToolStatus::Denied };
    }
    match value.get("decision").and_then(Value::as_str) {
        Some("approved" | "approved_for_session" | "accept" | "accept_for_session") => SessionToolStatus::Approved,
        Some("denied" | "declined" | "reject") => SessionToolStatus::Denied,
        Some("abort" | "cancelled") => SessionToolStatus::Cancelled,
        _ => status(value, SessionToolStatus::Unknown),
    }
}

fn record(role: &'static str, event_kind: SessionToolKind, call_id: Option<String>, name: Option<String>, content: String, status: SessionToolStatus) -> ToolRecord {
    ToolRecord { role, content, event: SessionToolEvent { call_id, approval_request_id: None, name, event_kind, status } }
}

fn invocation(name: Option<&str>, arguments: &Value) -> String {
    format!("[tool: {}]\n{}", name.unwrap_or("unknown"), content(arguments))
}

pub(super) fn codex_response(payload: &Value) -> Vec<ToolRecord> {
    let kind = payload["type"].as_str().unwrap_or_default();
    let id = call_id(payload);
    let name = field(payload, "name");
    let approval_request_id = match kind {
        "mcp_approval_request" => field(payload, "id"),
        "mcp_approval_response" | "mcp_call" => field(payload, "approval_request_id"),
        _ => None,
    };
    let mut event = match kind {
        "function_call" | "custom_tool_call" | "tool_search_call" => {
            let search = kind == "tool_search_call";
            let name = if search { Some("tool_search".into()) } else { name };
            let arguments = payload.get("arguments").or_else(|| payload.get("input")).unwrap_or(&Value::Null);
            record("assistant", if search { SessionToolKind::Search } else { SessionToolKind::Call }, id,
                name.clone(), invocation(name.as_deref(), arguments), status(payload, SessionToolStatus::Requested))
        }
        "function_call_output" | "custom_tool_call_output" => {
            record("tool", SessionToolKind::Result, id, name, content(&payload["output"]), status(payload, SessionToolStatus::ResultReceived))
        }
        "tool_search_output" | "mcp_list_tools" => {
            record("tool", SessionToolKind::Result, id.or_else(|| field(payload, "id")),
                Some("tool_search".into()), content(&payload["tools"]), status(payload, SessionToolStatus::ResultReceived))
        }
        "mcp_approval_request" => {
            record("system", SessionToolKind::Approval, id, name,
                content(&payload["arguments"]), SessionToolStatus::PendingApproval)
        }
        "mcp_approval_response" => {
            record("system", SessionToolKind::Approval, id, name,
                content(&payload["reason"]), approval_status(payload))
        }
        "mcp_call" => {
            let id = id.or_else(|| field(payload, "id"));
            let mut records = vec![record("assistant", SessionToolKind::Call, id.clone(), name.clone(),
                invocation(name.as_deref(), &payload["arguments"]), status(payload, SessionToolStatus::Requested))];
            if payload.get("output").is_some_and(|output| !output.is_null()) || payload.get("error").is_some_and(|error| !error.is_null()) {
                let result = payload.get("output").filter(|output| !output.is_null()).unwrap_or(&payload["error"]);
                let outcome = if payload.get("error").is_some_and(|error| !error.is_null()) { SessionToolStatus::Failed } else { status(payload, SessionToolStatus::ResultReceived) };
                records.push(record("tool", SessionToolKind::Result, id, name, content(result), outcome));
            }
            for record in &mut records {
                record.event.approval_request_id.clone_from(&approval_request_id);
            }
            return records;
        }
        _ => return Vec::new(),
    };
    event.event.approval_request_id = approval_request_id;
    vec![event]
}

pub(super) fn codex_runtime(payload: &Value) -> Option<ToolRecord> {
    let kind = payload["type"].as_str().unwrap_or_default();
    let id = call_id(payload);
    let name = match kind {
        "exec_command_begin" | "exec_command_end" | "exec_approval_request" | "exec_approval_response" | "exec_approval" => Some("exec_command".into()),
        "patch_apply_begin" | "patch_apply_end" | "apply_patch_approval_request" | "apply_patch_approval_response" | "apply_patch_approval" => Some("apply_patch".into()),
        "mcp_tool_call_begin" | "mcp_tool_call_end" => {
            let invocation = &payload["invocation"];
            field(invocation, "server").zip(field(invocation, "tool")).map(|(server, tool)| format!("mcp__{server}__{tool}"))
        }
        _ => field(payload, "name"),
    };
    Some(match kind {
        "exec_command_begin" | "patch_apply_begin" | "mcp_tool_call_begin" => {
            let arguments = payload.get("command").or_else(|| payload["invocation"].get("arguments")).or_else(|| payload.get("changes")).unwrap_or(&Value::Null);
            record("assistant", SessionToolKind::Call, id, name.clone(), invocation(name.as_deref(), arguments), SessionToolStatus::Running)
        }
        "exec_command_end" => {
            let outcome = match payload["exit_code"].as_i64() {
                Some(0) => SessionToolStatus::Succeeded,
                Some(_) => SessionToolStatus::Failed,
                None => status(payload, SessionToolStatus::ResultReceived),
            };
            let output = payload.get("aggregated_output").or_else(|| payload.get("stdout")).unwrap_or(&Value::Null);
            record("tool", SessionToolKind::Result, id, name, content(output), outcome)
        }
        "patch_apply_end" => {
            let outcome = match payload["success"].as_bool() {
                Some(true) => SessionToolStatus::Succeeded,
                Some(false) => SessionToolStatus::Failed,
                None => status(payload, SessionToolStatus::ResultReceived),
            };
            let output = payload.get("stdout").or_else(|| payload.get("stderr")).unwrap_or(&Value::Null);
            record("tool", SessionToolKind::Result, id, name, content(output), outcome)
        }
        "mcp_tool_call_end" => {
            let result = &payload["result"];
            let (body, outcome) = if let Some(error) = result.get("Err") { (error, SessionToolStatus::Failed) }
                else { let result = result.get("Ok").unwrap_or(result); (result.get("content").unwrap_or(result), result_status(result)) };
            record("tool", SessionToolKind::Result, id, name, content(body), outcome)
        }
        "exec_approval_request" | "apply_patch_approval_request" => {
            let body = payload.get("reason").or_else(|| payload.get("command")).unwrap_or(&Value::Null);
            record("system", SessionToolKind::Approval, id, name, content(body), SessionToolStatus::PendingApproval)
        }
        "exec_approval_response" | "exec_approval" | "apply_patch_approval_response" | "apply_patch_approval" => {
            record("system", SessionToolKind::Approval, id, name, content(&payload["decision"]), approval_status(payload))
        }
        _ => return None,
    })
}

pub(super) fn claude_block(block: &Value) -> Option<ToolRecord> {
    Some(match block["type"].as_str()? {
        "tool_use" => {
            let name = field(block, "name");
            record("assistant", SessionToolKind::Call, field(block, "id"), name.clone(),
                invocation(name.as_deref(), &block["input"]), status(block, SessionToolStatus::Requested))
        }
        "tool_result" => record("tool", SessionToolKind::Result, field(block, "tool_use_id"), None, content(&block["content"]), result_status(block)),
        _ => return None,
    })
}

pub(super) fn gemini_call(call: &Value) -> Vec<ToolRecord> {
    let id = field(call, "id").or_else(|| call_id(call));
    let name = field(call, "name");
    let mut records = vec![record("assistant", SessionToolKind::Call, id.clone(), name.clone(),
        invocation(name.as_deref(), &call["args"]), status(call, SessionToolStatus::Requested))];
    if let Some(result) = call.get("result") {
        records.push(record("tool", SessionToolKind::Result, id, name, content(result), result_status(call)));
    }
    records
}

pub(super) fn status_rank(status: SessionToolStatus) -> u8 {
    match status {
        SessionToolStatus::Unknown => 0,
        SessionToolStatus::Requested => 1,
        SessionToolStatus::PendingApproval => 2,
        SessionToolStatus::Approved => 3,
        SessionToolStatus::Running => 4,
        SessionToolStatus::ResultReceived => 5,
        SessionToolStatus::Completed => 6,
        SessionToolStatus::Succeeded | SessionToolStatus::Failed | SessionToolStatus::Denied | SessionToolStatus::Cancelled => 7,
    }
}
