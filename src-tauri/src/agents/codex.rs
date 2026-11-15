use super::{models::{AgentContext, AgentEvent, AgentUsageRecord, LineContext}, privacy};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(default)]
struct PendingTool {
    name: String,
    targets: Vec<String>,
    summary: Option<String>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct CodexState {
    session: String,
    created_at: i64,
    skip_until_line: u64,
    inherited: bool,
    awaiting_new_turn: bool,
    turn: Option<String>,
    model: Option<String>,
    window: Option<i64>,
    modern_usage: bool,
    previous_total: Option<[i64; 6]>,
    legacy_reset_epoch: Option<String>,
    legacy_usage_line: u64,
    prompt_seen: bool,
    pending: BTreeMap<String, PendingTool>,
}

fn timestamp(value: &Value) -> Option<i64> {
    value.as_i64().filter(|number| *number > 0).and_then(|number| {
        if number < 100_000_000_000 { number.checked_mul(1000) } else { Some(number) }
    }).or_else(|| {
        value.as_str().and_then(crate::session_usage_codex::parse_iso_timestamp_ms)
    }).filter(|value| privacy::valid_timestamp(*value))
}

fn id(value: &Value) -> Option<String> { value.as_str().and_then(privacy::identifier) }
fn agent(session: &str) -> String { format!("codex:{session}") }
fn task(session: &str, turn: &str) -> String { format!("codex:{session}:turn:{turn}") }
fn digest(value: &str) -> String { format!("{:x}", Sha256::digest(value.as_bytes())) }

fn base(ctx: &LineContext<'_>, state: &CodexState, at: i64, kind: &str, suffix: &str, title: &str) -> AgentEvent {
    AgentEvent {
        id: format!("codex:{}:{kind}:{suffix}", state.session),
        source_id: ctx.source_id.into(), source_path: ctx.source_path.into(), source_line: ctx.line_number,
        client_id: "codex".into(), session_id: state.session.clone(), agent_id: agent(&state.session),
        task_id: state.turn.as_ref().map(|turn| task(&state.session, turn)), timestamp: at, observed_at: ctx.observed_at,
        kind: kind.into(), title: title.into(), model: state.model.clone(), ..Default::default()
    }
}

fn content(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() { return Some(text.to_owned()); }
    let texts: Vec<&str> = value.as_array()?.iter().filter_map(|part| part.get("text").and_then(Value::as_str)).take(12).collect();
    (!texts.is_empty()).then(|| texts.join(" "))
}

fn counters(value: &Value) -> Option<[i64; 6]> {
    let input = value.get("input_tokens")?.as_i64()?;
    let output = value.get("output_tokens")?.as_i64()?;
    let optional = |key: &str| value.get(key).map(Value::as_i64).unwrap_or(Some(0));
    let cache = optional("cached_input_tokens")?;
    let write = optional("cache_write_input_tokens")?;
    let reasoning = optional("reasoning_output_tokens")?;
    let total = value.get("total_tokens").map(Value::as_i64).unwrap_or(input.checked_add(output))?;
    let sample = sample([input, output, cache, write, reasoning, total], String::new(), "");
    privacy::valid_usage(&sample).then_some([input, output, cache, write, reasoning, total])
}

fn sample(counts: [i64; 6], key: String, basis: &str) -> AgentUsageRecord {
    AgentUsageRecord { key, input_tokens: counts[0], output_tokens: counts[1], cache_read_tokens: counts[2],
        cache_write_tokens: counts[3], reasoning_tokens: counts[4], total_tokens: counts[5], basis: basis.into(), ..Default::default() }
}

fn reported_state(value: &Value) -> Option<(&'static str, Option<String>)> {
    if let Some(status) = value.as_str() {
        return match status {
            "running" => Some(("running", None)),
            "pending_init" => Some(("unknown", None)),
            "waiting" | "idle" => Some(("waiting", None)),
            "shutdown" | "interrupted" => Some(("interrupted", None)),
            "completed" => Some(("completed", None)),
            "errored" | "failed" => Some(("failed", None)),
            _ => None,
        };
    }
    for (key, status) in [("completed", "completed"), ("errored", "failed"), ("interrupted", "interrupted")] {
        if let Some(detail) = value.get(key) {
            return Some((status, detail.as_str().map(|text| privacy::text(text, 512))));
        }
    }
    value.get("status").and_then(reported_state)
}

fn receiver_status(ctx: &LineContext<'_>, state: &CodexState, at: i64, call: &str, receiver: &str, value: &Value) -> Option<AgentEvent> {
    let (status, summary) = reported_state(value)?;
    let mut event = base(ctx, state, at, "agent_status", &format!("{call}:{receiver}:{status}"), "子 Agent 状态报告");
    event.agent_id = agent(receiver);
    event.session_id = receiver.into();
    event.task_id = None;
    event.model = None;
    event.status = Some(status.into());
    event.summary = summary;
    event.correlation_id = Some(call.into());
    Some(event)
}

fn collaboration(ctx: &LineContext<'_>, state: &CodexState, at: i64, call: &str, tool: &str,
    receivers: &[String], summary: Option<String>, result: &Value) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    for receiver in receivers {
        if receiver == &state.session { continue; }
        let target = agent(receiver);
        if tool == "spawn_agent" {
            let mut spawned = base(ctx, state, at, "agent_spawned", &format!("{call}:{receiver}"), "派生子 Agent");
            spawned.related_agent_id = Some(target.clone());
            spawned.correlation_id = Some(call.into());
            spawned.name = result.get("nickname").and_then(Value::as_str).map(|text| privacy::text(text, 80));
            events.push(spawned);
        }
        if matches!(tool, "spawn_agent" | "send_input" | "send_message") {
            let kind = if tool == "spawn_agent" { "task_assigned" } else { "task_handoff" };
            let mut event = base(ctx, state, at, kind, &format!("{call}:{receiver}"), if tool == "spawn_agent" { "分配任务" } else { "任务消息交接" });
            event.related_agent_id = Some(target);
            event.correlation_id = Some(call.into());
            event.summary = summary.clone();
            event.task_id = id(&result["submission_id"]).map(|turn| task(receiver, &turn))
                .or_else(|| Some(format!("codex:{receiver}:assignment:{call}")));
            if let Some(submission) = id(&result["submission_id"]) { event.id.push_str(&format!(":{submission}")); }
            events.push(event);
        }
        if tool == "resume_agent" {
            let mut event = base(ctx, state, at, "agent_resumed", &format!("{call}:{receiver}"), "Agent 恢复");
            event.agent_id = agent(receiver); event.session_id = receiver.into(); event.task_id = None;
            event.status = Some("unknown".into()); event.model = None; event.correlation_id = Some(call.into()); events.push(event);
        }
        let state_value = result.get("agents_states").and_then(|states| states.get(receiver))
            .or_else(|| result.get("status").and_then(|states| states.get(receiver)))
            .or_else(|| (tool == "close_agent").then(|| &result["previous_status"]));
        if let Some(value) = state_value {
            if let Some(event) = receiver_status(ctx, state, at, call, receiver, value) { events.push(event); }
        }
        if tool == "close_agent" && !result.get("previous_status").is_none_or(Value::is_null) {
            let mut event = base(ctx, state, at, "agent_closed", &format!("{call}:{receiver}"), "Agent 已关闭");
            event.agent_id = agent(receiver); event.session_id = receiver.into(); event.task_id = None; event.model = None;
            event.correlation_id = Some(call.into()); events.push(event);
        }
    }
    events
}

fn message_event(ctx: &LineContext<'_>, state: &mut CodexState, at: i64, role: &str, value: &Value, events: &mut Vec<AgentEvent>) {
    let Some(message) = content(value) else { return; };
    let trimmed = message.trim();
    if trimmed.is_empty() || ["# AGENTS.md", "<environment_context>", "<permissions", "<turn_aborted>", "<subagent_notification>"]
        .iter().any(|prefix| trimmed.starts_with(prefix)) { return; }
    let mut event = base(ctx, state, at, "message", &digest(&format!("{}:{role}:{trimmed}", state.turn.as_deref().unwrap_or_default())),
        if role == "user" { "任务消息" } else { "Agent 消息" });
    event.summary = Some(privacy::text(trimmed, 512));
    if role == "user" && !state.prompt_seen && state.turn.is_some() {
        let mut update = event.clone();
        update.id = format!("{}:prompt", update.id);
        update.kind = "task_updated".into();
        update.title = privacy::text(trimmed, 120);
        events.push(update);
        state.prompt_seen = true;
    }
    events.push(event);
}

fn parse_event(payload: &Value, ctx: &LineContext<'_>, state: &mut CodexState, at: i64, suffix: &str, events: &mut Vec<AgentEvent>) {
    if id(&payload["thread_id"]).is_some_and(|owner| owner != state.session) { return; }
    match payload["type"].as_str().unwrap_or_default() {
        "task_started" => {
            let Some(turn) = id(&payload["turn_id"]) else { return; };
            state.prompt_seen = false;
            state.turn = Some(turn.clone());
            state.window = payload["model_context_window"].as_i64().filter(|window| *window > 0).or(state.window);
            let mut event = base(ctx, state, at, "task_started", &turn, "Agent 任务");
            event.lifecycle_at = timestamp(&payload["started_at"]);
            event.status = Some("running".into());
            events.push(event);
        }
        "task_complete" | "task_failed" => {
            let Some(turn) = id(&payload["turn_id"]).or_else(|| state.turn.clone()) else { return; };
            let error = payload.get("error").filter(|error| !error.is_null());
            let failed = error.is_some() || payload["type"] == "task_failed";
            let mut event = base(ctx, state, at,
                if failed { "task_failed" } else { "task_completed" }, &turn, if failed { "任务失败" } else { "任务完成" });
            event.lifecycle_at = timestamp(&payload["completed_at"]);
            event.task_id = Some(task(&state.session, &turn));
            event.status = Some(if failed { "failed" } else { "completed" }.into());
            event.summary = error.and_then(|error| error.as_str().or_else(|| error["message"].as_str()))
                .or_else(|| payload["last_agent_message"].as_str()).map(|text| privacy::text(text, 512));
            events.push(event);
        }
        "turn_aborted" | "task_interrupted" => {
            let turn = id(&payload["turn_id"]).or_else(|| state.turn.clone());
            let mut event = base(ctx, state, at, "task_interrupted", suffix, "任务已中断");
            event.task_id = turn.as_ref().map(|turn| task(&state.session, turn));
            event.status = Some("interrupted".into());
            events.push(event);
        }
        "token_count" => {
            let info = &payload["info"];
            state.window = info["model_context_window"].as_i64().filter(|window| *window > 0).or(state.window);
            if let Some(last) = counters(&info["last_token_usage"]) {
                let mut event = base(ctx, state, at, "context", suffix, "最近请求上下文测量");
                event.context = Some(AgentContext { used_tokens: last[0], window_tokens: state.window, measured_at: at,
                    basis: "最近请求输入；非实时上下文内存".into() });
                events.push(event);
            }
            let Some(total) = counters(&info["total_token_usage"]) else { return; };
            if !state.modern_usage && ctx.line_number <= state.legacy_usage_line { return; }
            let previous = state.previous_total.replace(total);
            if state.modern_usage { return; }
            state.legacy_usage_line = ctx.line_number;
            let delta = match previous {
                Some(previous) if total.iter().zip(previous).all(|(next, previous)| *next >= previous) => {
                    let mut delta = [0; 6];
                    for index in 0..6 { delta[index] = total[index] - previous[index]; }
                    delta
                }
                None if !state.inherited => total,
                _ => {
                    if let Some(previous) = previous {
                        state.legacy_reset_epoch = Some(digest(&format!(
                            "{}:{at}:{previous:?}:{total:?}",
                            state.legacy_reset_epoch.as_deref().unwrap_or("initial"),
                        )));
                    }
                    let event = base(ctx, state, at, "usage_gap", suffix, "累计用量基线重建；该区间未计量");
                    events.push(event);
                    return;
                }
            };
            if delta[5] == 0 { return; }
            let key = match &state.legacy_reset_epoch {
                Some(epoch) => format!("codex:legacy:{}:reset:{}", state.session,
                    digest(&format!("{epoch}:{previous:?}:{total:?}"))),
                None => format!("codex:legacy:{}:{}", state.session, digest(&format!("{total:?}"))),
            };
            let usage = sample(delta, key.clone(), "codex_cumulative_delta");
            if !privacy::valid_usage(&usage) { return; }
            let mut event = base(ctx, state, at, "usage", &key, "累计快照差分 Token 用量");
            event.usage = Some(usage);
            events.push(event);
        }
        "user_message" => message_event(ctx, state, at, "user", &payload["message"], events),
        "agent_message" => {
            if !matches!(payload["phase"].as_str().or(payload["channel"].as_str()), Some("analysis" | "reasoning")) {
                message_event(ctx, state, at, "assistant", &payload["message"], events);
            }
        }
        "item_completed" => {
            let item = &payload["item"];
            let saved_turn = state.turn.clone();
            state.turn = id(&payload["turn_id"]).or_else(|| state.turn.clone());
            parse_item(item, ctx, state, timestamp(&payload["completed_at_ms"]).unwrap_or(at), suffix, events);
            state.turn = saved_turn;
        }
        "context_compacted" => events.push(base(ctx, state, at, "context_compacted", suffix, "上下文已压缩；等待新的输入用量")),
        "error" | "warning" => {
            let mut event = base(ctx, state, at, "exception", suffix, "客户端异常事件");
            event.summary = payload["message"].as_str().map(|text| privacy::text(text, 512));
            events.push(event);
        }
        _ => {}
    }
}

pub fn parse_record(record: &Value, ctx: &LineContext<'_>, state_value: &mut Value) -> Vec<AgentEvent> {
    let mut state: CodexState = serde_json::from_value(state_value.clone()).unwrap_or_default();
    let events = parse(record, ctx, &mut state);
    *state_value = serde_json::to_value(state).unwrap_or(Value::Null);
    events
}

fn tool_name(name: &str) -> &str {
    match name.rsplit('.').next().unwrap_or(name) {
        "wait_agent" => "wait",
        other => other,
    }
}

fn pending_tool(name: &str, arguments: &Value) -> PendingTool {
    let mut targets = Vec::new();
    for key in ["target", "id", "agent_id"] {
        if let Some(target) = id(&arguments[key]) { targets.push(target); }
    }
    for key in ["targets", "ids", "agent_ids"] {
        if let Some(array) = arguments[key].as_array() { targets.extend(array.iter().filter_map(id)); }
    }
    targets.sort(); targets.dedup(); targets.truncate(64);
    let summary = arguments["message"].as_str().or_else(|| arguments["prompt"].as_str())
        .map(|text| privacy::text(text, 512)).or_else(|| content(&arguments["items"]).map(|text| privacy::text(&text, 512)));
    PendingTool { name: tool_name(name).into(), targets, summary }
}

fn tool_event(ctx: &LineContext<'_>, state: &CodexState, at: i64, kind: &str, call: &str, name: &str, status: &str) -> AgentEvent {
    let mut event = base(ctx, state, at, kind, call, &privacy::text(name, 100));
    event.correlation_id = Some(call.into());
    event.status = Some(status.into());
    event
}

fn parse_response(payload: &Value, ctx: &LineContext<'_>, state: &mut CodexState, at: i64, suffix: &str, events: &mut Vec<AgentEvent>) {
    match payload["type"].as_str().unwrap_or_default() {
        "message" => {
            let role = payload["role"].as_str().unwrap_or_default();
            let phase = payload["phase"].as_str().or_else(|| payload["channel"].as_str());
            if role == "user" || (role == "assistant" && !matches!(phase, Some("analysis" | "reasoning"))) {
                message_event(ctx, state, at, role, &payload["content"], events);
            }
        }
        "function_call" => {
            let Some(call) = id(&payload["call_id"]) else { return; };
            let name = payload["name"].as_str().unwrap_or("tool");
            let arguments: Value = payload["arguments"].as_str().and_then(|text| serde_json::from_str(text).ok()).unwrap_or(Value::Null);
            let pending = pending_tool(name, &arguments);
            events.push(tool_event(ctx, state, at, "tool_started", &call, name, "running"));
            if pending.name == "wait" {
                let mut event = base(ctx, state, at, "agent_waiting", &call, "等待子 Agent");
                event.status = Some("waiting".into()); event.correlation_id = Some(call.clone()); events.push(event);
            }
            if state.pending.len() >= 512 {
                if let Some(first) = state.pending.keys().next().cloned() { state.pending.remove(&first); }
            }
            state.pending.insert(call, pending);
        }
        "function_call_output" => {
            let Some(call) = id(&payload["call_id"]) else { return; };
            let Some(pending) = state.pending.remove(&call) else { return; };
            let result: Value = payload["output"].as_str().and_then(|text| serde_json::from_str(text).ok())
                .unwrap_or_else(|| if payload["output"].is_object() { payload["output"].clone() } else { Value::Null });
            let failed = result["is_error"] == true || result.get("error").is_some_and(|error| !error.is_null());
            events.push(tool_event(ctx, state, at, "tool_returned", &call, &pending.name, if failed { "failed" } else { "unknown" }));
            if pending.name == "wait" {
                let mut event = base(ctx, state, at, "agent_resumed", &format!("wait:{call}"), "等待结束，恢复执行");
                event.status = Some("running".into()); event.correlation_id = Some(call.clone()); events.push(event);
            }
            if failed { return; }
            let mut receivers = pending.targets;
            if let Some(receiver) = id(&result["agent_id"]) { receivers.push(receiver); }
            if pending.name == "wait" {
                if let Some(statuses) = result["status"].as_object() { receivers.extend(statuses.keys().filter_map(|key| privacy::identifier(key))); }
            }
            receivers.sort(); receivers.dedup(); receivers.truncate(64);
            events.extend(collaboration(ctx, state, at, &call, &pending.name, &receivers, pending.summary, &result));
        }
        "compaction" => events.push(base(ctx, state, at, "context_compacted", suffix, "上下文已压缩；等待新的输入用量")),
        _ => {}
    }
}

fn parse_item(item: &Value, ctx: &LineContext<'_>, state: &mut CodexState, at: i64, suffix: &str, events: &mut Vec<AgentEvent>) {
    let kind = item["type"].as_str().unwrap_or_default();
    match kind {
        "UserMessage" => message_event(ctx, state, at, "user", &item["content"], events),
        "AgentMessage" => {
            if !matches!(item["phase"].as_str(), Some("analysis" | "reasoning")) {
                message_event(ctx, state, at, "assistant", &item["content"], events);
            }
        }
        "ContextCompaction" => events.push(base(ctx, state, at, "context_compacted", suffix, "上下文已压缩；等待新的输入用量")),
        "CommandExecution" | "McpToolCall" | "CollabAgentToolCall" | "WebSearch" | "ImageView" => {
            let call = id(&item["id"]).unwrap_or_else(|| suffix.into());
            let name = match kind {
                "CommandExecution" => "命令执行",
                "WebSearch" => "网页搜索",
                "ImageView" => "查看图片",
                _ => item["tool"].as_str().unwrap_or(kind),
            };
            let status = if kind == "CommandExecution" && item["exit_code"].as_i64().is_some_and(|code| code != 0) { "failed" }
                else { match item["status"].as_str() { Some("completed") => "completed", Some("failed" | "errored") => "failed",
                    Some("interrupted" | "cancelled") => "interrupted", Some("inProgress" | "in_progress" | "running") => "running", _ => "unknown" } };
            let mut event = tool_event(ctx, state, at, "tool_completed", &call, name, status);
            if kind == "CommandExecution" { event.summary = item["exit_code"].as_i64().map(|code| format!("退出码 {code}")); }
            events.push(event);
            if kind != "CollabAgentToolCall" || matches!(status, "failed" | "interrupted") { return; }
            if id(&item["sender_thread_id"]).is_some_and(|sender| sender != state.session) { return; }
            let tool = tool_name(name);
            let receivers: Vec<String> = item["receiver_thread_ids"].as_array()
                .map(|array| array.iter().filter_map(id).take(64).collect()).unwrap_or_default();
            let summary = item["prompt"].as_str().map(|text| privacy::text(text, 512));
            for receiver in receivers {
                let nickname = item["receiver_agents"].as_array().and_then(|agents| agents.iter().find(|agent| agent["thread_id"] == receiver))
                    .and_then(|agent| agent["agent_nickname"].as_str());
                let result = serde_json::json!({"agents_states": item["agents_states"], "nickname": nickname});
                events.extend(collaboration(ctx, state, at, &call, tool, &[receiver], summary.clone(), &result));
            }
            if tool == "wait" {
                let mut event = base(ctx, state, at, "agent_resumed", &format!("wait:{call}"), "等待结束，恢复执行");
                event.status = Some("running".into()); event.correlation_id = Some(call); events.push(event);
            }
        }
        _ => {}
    }
}

fn parse(record: &Value, ctx: &LineContext<'_>, state: &mut CodexState) -> Vec<AgentEvent> {
    let Some(at) = timestamp(&record["timestamp"]) else { return Vec::new(); };
    let payload = &record["payload"];
    let kind = record["type"].as_str().unwrap_or_default();
    if kind == "session_meta" {
        if !state.session.is_empty() { return Vec::new(); }
        let Some(session) = id(&payload["id"]) else { return Vec::new(); };
        state.session = session;
        state.created_at = timestamp(&payload["timestamp"]).unwrap_or(at);
        let inherited_count = payload["subagent_history_start_ordinal"].as_u64();
        state.skip_until_line = inherited_count.map(|count| count.saturating_add(ctx.line_number)).unwrap_or(0);
        state.inherited = payload.get("forked_from_id").is_some_and(|value| !value.is_null()) || inherited_count.is_some();
        state.awaiting_new_turn = state.inherited && inherited_count.is_none();
        let parent = id(&payload["parent_thread_id"]).or_else(|| payload.pointer("/source/subagent/thread_spawn/parent_thread_id").and_then(id));
        let mut event = base(ctx, state, state.created_at, "agent_created", "metadata", "Agent 会话创建");
        event.parent_agent_id = parent.filter(|parent| parent != &state.session).map(|parent| agent(&parent));
        event.name = payload.get("agent_nickname").and_then(Value::as_str)
            .or_else(|| payload.pointer("/source/subagent/thread_spawn/agent_nickname").and_then(Value::as_str)).map(|name| privacy::text(name, 80));
        event.role = payload.get("agent_role").and_then(Value::as_str)
            .or_else(|| payload.pointer("/source/subagent/thread_spawn/agent_role").and_then(Value::as_str)).map(|role| privacy::text(role, 80));
        event.workspace = payload.get("cwd").and_then(Value::as_str).map(|workspace| privacy::text(workspace, 256));
        return vec![event];
    }
    if state.session.is_empty() || ctx.line_number <= state.skip_until_line { return Vec::new(); }
    if state.awaiting_new_turn {
        if kind == "event_msg" && payload["type"] == "task_started"
            && timestamp(&payload["started_at"]).is_some_and(|started| started >= state.created_at) {
            state.awaiting_new_turn = false;
        } else if kind != "token_usage_record" { return Vec::new(); }
    }
    let suffix = format!("{at}:{}", digest(&format!("{kind}:{}", payload)));
    let mut events = Vec::new();
    if kind == "turn_context" {
        state.turn = id(&payload["turn_id"]).or_else(|| state.turn.clone());
        state.model = payload.get("model").and_then(Value::as_str).map(|model| privacy::text(model, 160)).or_else(|| state.model.clone());
        let mut event = base(ctx, state, at, "agent_updated", &suffix, "模型与运行上下文更新");
        event.model = state.model.clone();
        events.push(event);
    } else if kind == "token_usage_record" {
        if id(&payload["thread_id"]).as_deref() != Some(state.session.as_str()) { return events; }
        let Some(response) = id(&payload["response_id"]) else { return events; };
        let Some(counts) = counters(&payload["usage"]) else { return events; };
        state.modern_usage = true;
        let mut event = base(ctx, state, at, "usage", &response, "响应 Token 用量");
        event.task_id = id(&payload["turn_id"]).map(|turn| task(&state.session, &turn));
        event.correlation_id = Some(response.clone());
        event.usage = Some(sample(counts, format!("codex:response:{response}"), "codex_response_id"));
        event.context = Some(AgentContext { used_tokens: counts[0], window_tokens: state.window, measured_at: at, basis: "最近请求输入；非实时上下文内存".into() });
        events.push(event);
    } else if kind == "compacted" {
        let event = base(ctx, state, at, "context_compacted", &suffix, "上下文已压缩；等待新的输入用量");
        events.push(event);
    } else if kind == "event_msg" {
        parse_event(payload, ctx, state, at, &suffix, &mut events);
    } else if kind == "response_item" {
        parse_response(payload, ctx, state, at, &suffix, &mut events);
    }
    events
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::{
        models::{AgentHistory, AgentSnapshotQuery, AgentUsage},
        projection::Projection,
        store::{EventStore, FileCursor},
    };
    use crate::database::Database;
    use serde_json::json;
    use std::{collections::BTreeSet, path::Path};

    const BASE: i64 = 1_794_307_716_000;
    const SOURCE: &str = "codex-r2";
    const PATH: &str = "rollout-r2.jsonl";
    const SESSION: &str = "legacy-r2";

    #[test]
    fn invalid_codex_dates_do_not_create_nodes_or_advance_parser_state() {
        let mut state = Value::Null;
        let invalid = json!({"type":"session_meta", "timestamp":9_007_199_254_740_991_i64,
            "payload":{"id":"bad-date", "timestamp":9_007_199_254_740_991_i64}});
        assert!(read(&[invalid], &mut state, PATH, 1, BASE).is_empty());
        assert_eq!(state, serde_json::to_value(CodexState::default()).unwrap());
        assert_eq!(timestamp(&json!(8_640_000_000_000_000_i64)), Some(8_640_000_000_000_000));
        assert_eq!(timestamp(&json!(8_640_000_000_000_001_i64)), None);
        assert_eq!(timestamp(&json!(1_794_307_716)), Some(BASE));
        let valid = json!({"type":"session_meta", "timestamp":BASE, "payload":{"id":"valid-date", "timestamp":BASE}});
        let events = read(&[valid], &mut state, PATH, 2, BASE);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].timestamp, BASE);
    }

    fn cumulative(counts: [i64; 6], at: i64) -> Value {
        json!({"type":"event_msg", "timestamp":at, "payload":{
            "type":"token_count", "info":{
                "total_token_usage":{
                    "input_tokens":counts[0], "output_tokens":counts[1],
                    "cached_input_tokens":counts[2], "cache_write_input_tokens":counts[3],
                    "reasoning_output_tokens":counts[4], "total_tokens":counts[5]
                },
                "last_token_usage":{"input_tokens":12, "output_tokens":0, "total_tokens":12},
                "model_context_window":4096
            }
        }})
    }

    #[test]
    fn lifecycle_keeps_precise_event_clock_and_reported_start_end_separate() {
        let mut state = Value::Null;
        let records = [
            json!({"type":"session_meta", "timestamp":BASE, "payload":{"id":SESSION}}),
            json!({"type":"event_msg", "timestamp":BASE + 921,
                "payload":{"type":"task_started", "turn_id":"r10-turn", "started_at":BASE / 1000}}),
            json!({"type":"event_msg", "timestamp":BASE + 1850,
                "payload":{"type":"task_complete", "turn_id":"r10-turn", "completed_at":BASE / 1000 + 1}}),
        ];
        let events = read(&records, &mut state, PATH, 1, BASE + 5000);
        let started = events.iter().find(|event| event.kind == "task_started").unwrap();
        let completed = events.iter().find(|event| event.kind == "task_completed").unwrap();
        assert_eq!((started.timestamp, started.lifecycle_at), (BASE + 921, Some(BASE)));
        assert_eq!((completed.timestamp, completed.lifecycle_at), (BASE + 1850, Some(BASE + 1000)));
        let snapshot = Projection::from_events(&events).snapshot(
            &AgentSnapshotQuery::default(), vec![], AgentHistory::default(), 1, BASE + 5000, vec![],
        );
        let task = snapshot.tasks.iter().find(|task| task.id.ends_with("r10-turn")).unwrap();
        assert_eq!((task.started_at, task.ended_at), (Some(BASE), Some(BASE + 1000)));
        assert_eq!(snapshot.agents[0].ended_at, Some(BASE + 1000));

        let fallback = json!({"type":"event_msg", "timestamp":BASE + 2921,
            "payload":{"type":"task_started", "turn_id":"r10-fallback"}});
        let tail = read(&[fallback], &mut state, PATH, 4, BASE + 5000);
        assert_eq!((tail[0].timestamp, tail[0].lifecycle_at), (BASE + 2921, None));
        let mut legacy = serde_json::to_value(&tail[0]).unwrap();
        legacy.as_object_mut().unwrap().remove("lifecycleAt");
        assert_eq!(serde_json::from_value::<AgentEvent>(legacy).unwrap().lifecycle_at, None);
    }

    #[test]
    fn same_second_lifecycle_survives_arrival_order_batches_and_sqlite_replay() {
        const SECOND: i64 = 1_794_334_151_000;
        const PARENT: &str = "r10-parent";
        const CHILD: &str = "r10-child";
        let mut parent_state = Value::Null;
        let mut child_state = Value::Null;
        let parent_header = json!({"type":"session_meta", "timestamp":SECOND - 1000,
            "payload":{"id":PARENT, "timestamp":SECOND - 1000}});
        let spawn = json!({"type":"event_msg", "timestamp":SECOND + 576, "payload":{
            "type":"item_completed", "completed_at_ms":SECOND + 576, "item":{
                "type":"CollabAgentToolCall", "id":"r10-spawn", "tool":"spawn_agent", "status":"completed",
                "sender_thread_id":PARENT, "receiver_thread_ids":[CHILD], "agents_states":{(CHILD):"pending_init"}
            }
        }});
        let mut parent_events = read(&[parent_header], &mut parent_state, "parent.jsonl", 1, SECOND + 5000);
        parent_events.extend(read(&[spawn], &mut parent_state, "parent.jsonl", 4262, SECOND + 5000));
        let child_records = [
            json!({"type":"session_meta", "timestamp":SECOND + 921,
                "payload":{"id":CHILD, "timestamp":SECOND + 404, "parent_thread_id":PARENT}}),
            json!({"type":"event_msg", "timestamp":SECOND + 921,
                "payload":{"type":"task_started", "turn_id":"r10-turn", "started_at":SECOND / 1000}}),
        ];
        let child_events = read(&child_records, &mut child_state, "child.jsonl", 1, SECOND + 5000);
        let parent_running = json!({"type":"event_msg", "timestamp":SECOND + 1500, "payload":{
            "type":"item_completed", "completed_at_ms":SECOND + 1500, "item":{
                "type":"CollabAgentToolCall", "id":"r10-wait", "tool":"wait_agent", "status":"completed",
                "sender_thread_id":PARENT, "receiver_thread_ids":[CHILD], "agents_states":{(CHILD):"running"}
            }
        }});
        let running_events = read(&[parent_running], &mut parent_state, "parent.jsonl", 4263, SECOND + 5000);
        let complete = json!({"type":"event_msg", "timestamp":SECOND + 1850, "payload":{
            "type":"task_complete", "turn_id":"r10-turn", "completed_at":SECOND / 1000 + 1
        }});
        let completed_events = read(&[complete], &mut child_state, "child.jsonl", 3, SECOND + 5000);
        let snapshot = |projection: &Projection, at: i64| projection.snapshot(
            &AgentSnapshotQuery { at: Some(at), ..Default::default() }, vec![], AgentHistory::default(), 1, at, vec![],
        );
        for child_first in [false, true] {
            for split_batches in [false, true] {
                let store = memory_store();
                let mut projection = Projection::default();
                let mut initial = [(&parent_events, &parent_state, "parent.jsonl"), (&child_events, &child_state, "child.jsonl")];
                if child_first { initial.reverse(); }
                for (events, state, path) in initial {
                    for batch in events.chunks(if split_batches { 1 } else { events.len() }) {
                        projection.extend(&save(&store, batch, state, path, batch.last().unwrap().source_line));
                    }
                }
                let live = snapshot(&projection, SECOND + 1000);
                let node = live.agents.iter().find(|node| node.id == agent(CHILD)).unwrap();
                assert_eq!(node.status, "running");
                assert_eq!(node.parent_id.as_deref(), Some("codex:r10-parent"));
                assert_eq!(node.started_at, Some(SECOND));
                assert_eq!(live.totals.records, 0);
                let before = snapshot(&Projection::from_events(&store.load_events(Some(SECOND + 800)).unwrap()), SECOND + 800);
                assert_eq!(before.agents.iter().find(|node| node.id == agent(CHILD)).unwrap().status, "unknown");
                let replay = snapshot(&Projection::from_events(&store.load_events(Some(SECOND + 1000)).unwrap()), SECOND + 1000);
                assert_eq!(serde_json::to_value(&live.agents).unwrap(), serde_json::to_value(&replay.agents).unwrap());
                assert_eq!(serde_json::to_value(&live.tasks).unwrap(), serde_json::to_value(&replay.tasks).unwrap());

                let mut terminal = [(&running_events, &parent_state, "parent.jsonl"), (&completed_events, &child_state, "child.jsonl")];
                if child_first { terminal.reverse(); }
                for (events, state, path) in terminal {
                    projection.extend(&save(&store, events, state, path, events.last().unwrap().source_line));
                }
                let final_live = snapshot(&projection, SECOND + 2000);
                let final_replay = snapshot(&Projection::from_events(&store.load_events(Some(SECOND + 2000)).unwrap()), SECOND + 2000);
                let node = final_live.agents.iter().find(|node| node.id == agent(CHILD)).unwrap();
                assert_eq!(node.status, "completed");
                assert_eq!(node.ended_at, Some(SECOND + 1000));
                assert_eq!(serde_json::to_value(&final_live.agents).unwrap(), serde_json::to_value(&final_replay.agents).unwrap());
                assert_eq!(serde_json::to_value(&final_live.tasks).unwrap(), serde_json::to_value(&final_replay.tasks).unwrap());
            }
        }
    }

    fn legacy_records(values: &[i64]) -> Vec<Value> {
        let mut records = vec![json!({"type":"session_meta", "timestamp":BASE,
            "payload":{"id":SESSION, "timestamp":BASE}})];
        records.extend(values.iter().enumerate().map(|(index, value)| {
            cumulative([*value, 0, 0, 0, 0, *value], BASE + (index as i64 + 1) * 1000)
        }));
        records
    }

    fn read(records: &[Value], state: &mut Value, path: &str, first_line: u64, observed_at: i64) -> Vec<AgentEvent> {
        let mut events = Vec::new();
        for (index, record) in records.iter().enumerate() {
            let context = LineContext {
                source_id: SOURCE, client_id: "codex", source_path: path,
                line_number: first_line + index as u64, observed_at,
            };
            events.extend(parse_record(record, &context, state));
        }
        events
    }

    fn usage_keys(events: &[AgentEvent]) -> Vec<String> {
        events.iter().filter_map(|event| event.usage.as_ref().map(|usage| usage.key.clone())).collect()
    }

    fn totals(events: &[AgentEvent]) -> AgentUsage {
        Projection::from_events(events).snapshot(
            &AgentSnapshotQuery::default(), vec![], AgentHistory::default(), 1,
            BASE + 99_000, vec![],
        ).totals
    }

    fn memory_store() -> EventStore {
        EventStore::new(Database::open(Path::new(":memory:")).unwrap()).unwrap()
    }

    fn save(store: &EventStore, events: &[AgentEvent], state: &Value, path: &str, line: u64) -> Vec<AgentEvent> {
        store.append(events, &FileCursor {
            source_id: SOURCE.into(), root_key: digest("r2-root"), path: path.into(),
            offset: line * 64, file_len: line * 64, line_number: line,
            parser_state: state.clone(), ..Default::default()
        }).unwrap()
    }

    #[test]
    fn legacy_reset_reused_endpoint_counts_new_interval() {
        let records = legacy_records(&[100, 160, 120, 160]);
        let mut state = Value::Null;
        let events = read(&records, &mut state, PATH, 1, BASE + 99_000);
        let usages: Vec<_> = events.iter().filter_map(|event| event.usage.as_ref()).collect();
        assert_eq!(usages.iter().map(|usage| usage.total_tokens).collect::<Vec<_>>(), vec![100, 60, 40]);
        assert!(usages.iter().all(|usage| usage.basis == "codex_cumulative_delta"));
        assert_eq!(usage_keys(&events).into_iter().collect::<BTreeSet<_>>().len(), 3);
        assert_eq!(usages[0].key, format!("codex:legacy:{SESSION}:{}", digest("[100, 0, 0, 0, 0, 100]")));
        assert_eq!(usages[1].key, format!("codex:legacy:{SESSION}:{}", digest("[160, 0, 0, 0, 0, 160]")));
        assert_eq!(events.iter().filter(|event| event.kind == "usage_gap").count(), 1);
        assert_eq!(totals(&events).total_tokens, 200);
        let store = memory_store();
        save(&store, &events, &state, PATH, records.len() as u64);
        let persisted = totals(&store.load_events(None).unwrap());
        assert_eq!((persisted.input_tokens, persisted.total_tokens, persisted.records), (200, 200, 3));
    }

    #[test]
    fn legacy_reset_rereads_and_archive_copies_deduplicate() {
        let records = legacy_records(&[100, 160, 120, 160]);
        let mut state = Value::Null;
        let events = read(&records, &mut state, PATH, 1, BASE + 99_000);
        let store = memory_store();
        save(&store, &events, &state, PATH, records.len() as u64);
        let saved_state = state.clone();
        let repeated = read(&records, &mut state, PATH, 1, BASE + 199_000);
        assert!(usage_keys(&repeated).is_empty());
        assert_eq!(state, saved_state);
        assert!(save(&store, &repeated, &state, PATH, records.len() as u64).is_empty());

        let mut combined = events.clone();
        for (path, first_line) in [(PATH, 1), ("archived/rollout-r2.jsonl", 41)] {
            let mut fresh = Value::Null;
            let replay = read(&records, &mut fresh, path, first_line, BASE + 299_000);
            assert_eq!(usage_keys(&events), usage_keys(&replay));
            assert!(save(&store, &replay, &fresh, path, first_line + records.len() as u64 - 1).is_empty());
            combined.extend(replay);
        }
        assert_eq!(totals(&combined).total_tokens, 200);
        assert_eq!(totals(&store.load_events(None).unwrap()).total_tokens, 200);

        let next = cumulative([180, 0, 0, 0, 0, 180], BASE + 5000);
        let tail = read(&[next], &mut state, PATH, records.len() as u64 + 1, BASE + 399_000);
        assert_eq!(totals(&tail).total_tokens, 20);
        save(&store, &tail, &state, PATH, records.len() as u64 + 1);
        assert_eq!(totals(&store.load_events(None).unwrap()).total_tokens, 220);
    }

    #[test]
    fn legacy_reset_persisted_cursor_resumes_at_every_boundary() {
        let records = legacy_records(&[100, 160, 120, 160]);
        let mut uninterrupted_state = Value::Null;
        let expected = read(&records, &mut uninterrupted_state, PATH, 1, BASE + 99_000);
        for split in 0..=records.len() {
            let store = memory_store();
            let mut state = Value::Null;
            let prefix = read(&records[..split], &mut state, PATH, 1, BASE + 99_000);
            save(&store, &prefix, &state, PATH, split as u64);
            let reopened = EventStore::new(store.database().clone()).unwrap();
            drop(store);
            let cursor = reopened.cursors().unwrap().pop().unwrap();
            let mut resumed_state = cursor.parser_state;
            let tail = read(&records[split..], &mut resumed_state, PATH, cursor.line_number + 1, BASE + 199_000);
            save(&reopened, &tail, &resumed_state, PATH, records.len() as u64);
            let persisted = reopened.load_events(None).unwrap();
            assert_eq!(usage_keys(&persisted), usage_keys(&expected), "split={split}");
            assert_eq!(totals(&persisted).total_tokens, 200, "split={split}");
            assert_eq!(resumed_state, uninterrupted_state, "split={split}");
            let repeated = read(&records, &mut resumed_state, PATH, 1, BASE + 299_000);
            assert!(save(&reopened, &repeated, &resumed_state, PATH, records.len() as u64).is_empty());
            assert_eq!(totals(&reopened.load_events(None).unwrap()).total_tokens, 200);
        }
    }

    #[test]
    fn legacy_repeated_reset_ranges_with_equal_timestamps_have_distinct_epochs() {
        let mut records = legacy_records(&[100, 160, 120, 160, 120, 160]);
        for record in records.iter_mut().skip(1) { record["timestamp"] = json!(BASE + 1000); }
        let mut state = Value::Null;
        let events = read(&records, &mut state, PATH, 1, BASE + 99_000);
        assert_eq!(usage_keys(&events).into_iter().collect::<BTreeSet<_>>().len(), 4);
        let store = memory_store();
        save(&store, &events, &state, PATH, records.len() as u64);
        assert_eq!(totals(&store.load_events(None).unwrap()).total_tokens, 240);
        let mut fresh = Value::Null;
        let copied = read(&records, &mut fresh, "archived/rollout-r2.jsonl", 101, BASE + 199_000);
        assert_eq!(usage_keys(&events), usage_keys(&copied));
        assert!(save(&store, &copied, &fresh, "archived/rollout-r2.jsonl", records.len() as u64 + 100).is_empty());
    }

    #[test]
    fn legacy_cache_only_reset_preserves_all_counter_deltas_and_context() {
        let mut records = legacy_records(&[]);
        for (index, counts) in [
            [100, 20, 80, 0, 10, 120], [160, 40, 140, 0, 20, 200],
            [160, 40, 120, 0, 20, 200], [200, 50, 150, 0, 25, 250],
        ].into_iter().enumerate() {
            records.push(cumulative(counts, BASE + (index as i64 + 1) * 1000));
        }
        let mut state = Value::Null;
        let events = read(&records, &mut state, PATH, 1, BASE + 99_000);
        assert_eq!(events.iter().filter(|event| event.kind == "usage_gap").count(), 1);
        let store = memory_store();
        save(&store, &events, &state, PATH, records.len() as u64);
        let persisted = store.load_events(None).unwrap();
        let usage = totals(&persisted);
        assert_eq!([usage.input_tokens, usage.output_tokens, usage.cache_read_tokens,
            usage.cache_write_tokens, usage.reasoning_tokens, usage.total_tokens], [200, 50, 170, 0, 25, 250]);
        let context = persisted.iter().filter_map(|event| event.context.as_ref()).last().unwrap();
        assert_eq!((context.used_tokens, context.window_tokens), (12, Some(4096)));
    }

    #[test]
    fn modern_response_identity_and_owner_ignore_legacy_reset_epochs() {
        let mut records = legacy_records(&[100, 160, 120, 160]);
        let response = json!({"type":"token_usage_record", "timestamp":BASE + 100,
            "payload":{"thread_id":SESSION, "turn_id":"turn-r2", "response_id":"r2-response",
                "usage":{"input_tokens":35, "output_tokens":5, "total_tokens":40}}});
        records.insert(1, response.clone());
        let mut duplicate = response.clone();
        duplicate["timestamp"] = json!(BASE + 5000);
        records.push(duplicate);
        let mut foreign = response;
        foreign["payload"]["thread_id"] = json!("other-session");
        foreign["payload"]["response_id"] = json!("other-response");
        records.push(foreign);
        let mut state = Value::Null;
        let events = read(&records, &mut state, PATH, 1, BASE + 99_000);
        assert_eq!(usage_keys(&events), vec!["codex:response:r2-response", "codex:response:r2-response"]);
        for event in events.iter().filter(|event| event.usage.is_some()) {
            assert_eq!(event.id, "codex:legacy-r2:usage:r2-response");
            assert_eq!(event.task_id.as_deref(), Some("codex:legacy-r2:turn:turn-r2"));
            assert_eq!(event.correlation_id.as_deref(), Some("r2-response"));
            assert_eq!(event.usage.as_ref().unwrap().basis, "codex_response_id");
        }
        assert_eq!(state["modern_usage"], json!(true));
        assert!(state["legacy_reset_epoch"].is_null());
        let store = memory_store();
        save(&store, &events, &state, PATH, records.len() as u64);
        let usage = totals(&store.load_events(None).unwrap());
        assert_eq!((usage.input_tokens, usage.output_tokens, usage.total_tokens, usage.records), (35, 5, 40, 1));
        let mut fresh = Value::Null;
        let archive = read(&records, &mut fresh, "archived/rollout-r2.jsonl", 1, BASE + 199_000);
        assert!(save(&store, &archive, &fresh, "archived/rollout-r2.jsonl", records.len() as u64).is_empty());
        assert_eq!(totals(&store.load_events(None).unwrap()).total_tokens, 40);
    }
}
