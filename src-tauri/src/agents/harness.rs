//! Read-only DSH v4 observation, pinned to upstream 687f282e8625c6b8d11ff58211a90524be2e4317.
//! Collector must pre-scan seeded files and set seedBoundary = { sessionId, seq }
//! to the LAST session/end-seed with inherited:true before replaying from seq 0.
//! usage.key identifies a replaceable settlement slot: project its latest value,
//! not the sum of all revisions. llm/retry-started opens a new slot.
//! Revisions use the numeric native seq in :seq:<seq>:usage, never wall time.

use super::models::{AgentContext, AgentEvent, AgentUsageRecord, LineContext};
use super::privacy::{identifier, text, valid_usage};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const CLIENT: &str = "deepseek_harness";
const STATE_VERSION: u32 = 1;
const MAX_EXACT: u64 = 9_007_199_254_740_991;
const MAX_TURNS: usize = 64;
const MAX_LINKS: usize = 512;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
struct State {
    schema_version: u32,
    seed_boundary: Option<SeedBoundary>,
    header: Option<Header>,
    last_seq: Option<u64>,
    blocked: Option<Block>,
    seed_seen: bool,
    ever_started: bool,
    descriptor_seen: bool,
    model: Option<String>,
    provider: Option<String>,
    route_context: Option<RouteContext>,
    turns: Vec<Turn>,
    calls: Vec<Call>,
    jobs: Vec<Job>,
    usage_slot: Option<UsageSlot>,
    partial_links: bool,
    unsupported_team_events: bool,
    unsupported_descriptor: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SeedBoundary {
    session_id: String,
    seq: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
struct Header {
    id: String,
    created_at: i64,
    cwd: Option<String>,
    parent_session: Option<String>,
    is_seeded: bool,
    subagent: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "snake_case")]
enum Block {
    UnsupportedState,
    InvalidHeader,
    HeaderChanged,
    MissingSeedBoundary,
    InvalidSeedBoundary,
    UnexpectedSeedMarker,
    InvalidEnvelope,
    SequenceGap,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct RouteContext {
    model: String,
    provider: String,
    window: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Turn {
    turn: u64,
    start_seq: u64,
    ended: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Call {
    id: String,
    turn: u64,
    step: u64,
    task_seq: u64,
    name: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Job {
    id: String,
    task_seq: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct UsageSlot {
    turn: u64,
    step: u64,
    first_seq: u64,
}

fn number(value: &Value) -> Option<u64> {
    value.as_u64().filter(|number| *number <= MAX_EXACT)
}

fn timestamp(value: &Value) -> Option<u64> {
    number(value).filter(|value| super::privacy::valid_timestamp(*value as i64))
}

fn native_id(value: &Value) -> Option<String> {
    identifier(value.as_str()?)
}

fn label(value: &Value, limit: usize) -> Option<String> {
    let clean = text(value.as_str()?, limit);
    (!clean.is_empty()).then_some(clean)
}

fn agent_id(native: &str) -> String {
    format!("{CLIENT}:{native}")
}

fn task_id(native: &str, seq: u64) -> String {
    format!("{}:task:{seq}", agent_id(native))
}

fn trim<T>(items: &mut Vec<T>, limit: usize) -> bool {
    let extra = items.len().saturating_sub(limit);
    items.drain(..extra);
    extra != 0
}

fn read_header(record: &Value) -> Option<Header> {
    if record["version"].as_u64()? != 4 {
        return None;
    }
    number(&record["delegationDepth"])?;
    let id = native_id(&record["id"])?;
    let parent_session = match record.get("parentSession") {
        Some(value) => Some(native_id(value).filter(|parent| parent != &id)?),
        None => None,
    };
    let subagent = match record.get("origin") {
        None => false,
        Some(value) if value.as_str() == Some("subagent") => true,
        Some(_) => return None,
    };
    Some(Header {
        id,
        created_at: timestamp(&record["createdAt"])? as i64,
        cwd: label(&record["cwd"], 512),
        parent_session,
        is_seeded: record["isSeeded"].as_bool()?,
        subagent,
    })
}

fn event(
    state: &State,
    ctx: &LineContext<'_>,
    seq: Option<u64>,
    time: i64,
    kind: &str,
    title: &str,
) -> AgentEvent {
    let Some(header) = state.header.as_ref() else {
        return AgentEvent::default();
    };
    let location = seq.map_or_else(|| "header".into(), |value| format!("seq:{value}"));
    AgentEvent {
        id: format!("{}:{location}:{kind}", agent_id(&header.id)),
        source_id: ctx.source_id.into(),
        source_path: ctx.source_path.into(),
        source_line: ctx.line_number,
        client_id: CLIENT.into(),
        session_id: header.id.clone(),
        agent_id: agent_id(&header.id),
        timestamp: time,
        observed_at: ctx.observed_at,
        kind: kind.into(),
        title: text(title, 160),
        parent_agent_id: header.parent_session.as_deref().map(agent_id),
        model: state.model.clone(),
        workspace: header.cwd.clone(),
        ..AgentEvent::default()
    }
}

fn turn_task(state: &State, turn: u64) -> Option<u64> {
    state
        .turns
        .iter()
        .rev()
        .find(|item| item.turn == turn)
        .map(|item| item.start_seq)
}

fn attach_task(event: &mut AgentEvent, task_seq: Option<u64>) {
    event.task_id = task_seq.map(|seq| task_id(&event.session_id, seq));
}

fn route(record_type: &str, data: &Value, state: &mut State) -> bool {
    match record_type {
        "request/header" => {
            let config = &data["header"]["config"];
            let Some(model) = native_id(&config["model"]) else {
                return false;
            };
            state.model = Some(model);
            state.provider = native_id(&config["provider"]);
            true
        }
        "request/context" => {
            let (Some(model), Some(provider)) =
                (native_id(&data["model"]), native_id(&data["provider"]))
            else {
                return false;
            };
            let window = number(&data["contextWindow"])
                .filter(|count| *count > 0)
                .map(|count| count as i64);
            state.model = Some(model.clone());
            state.provider = Some(provider.clone());
            state.route_context = Some(RouteContext {
                model,
                provider,
                window,
            });
            true
        }
        _ => false,
    }
}

fn token_usage(data: &Value, record_type: &str) -> Option<AgentUsageRecord> {
    let usage = if record_type == "assistant/message" && data.get("usage").is_some() {
        &data["usage"]
    } else {
        &data["stream"].as_array()?.iter().rev().find(|part| {
            part["type"].as_str() == Some("chunk")
                && part["chunk"]["type"].as_str() == Some("usage")
        })?["chunk"]["usage"]
    };
    let optional = |name: &str| -> Option<i64> {
        match usage.get(name) {
            None => Some(0),
            Some(value) => number(value).map(|value| value as i64),
        }
    };
    let uncached = number(&usage["inputTokens"])? as i64;
    let output_tokens = number(&usage["outputTokens"])? as i64;
    let cache_read_tokens = optional("cacheReadTokens")?;
    let cache_write_tokens = optional("cacheWriteTokens")?;
    let input_tokens = uncached
        .checked_add(cache_read_tokens)?
        .checked_add(cache_write_tokens)?;
    let total_tokens = match usage.get("totalTokens") {
        Some(value) => number(value)? as i64,
        None => input_tokens.checked_add(output_tokens)?,
    };
    let normalized = AgentUsageRecord {
        input_tokens,
        output_tokens,
        cache_read_tokens,
        cache_write_tokens,
        reasoning_tokens: optional("reasoningTokens")?,
        total_tokens,
        basis: "dsh-v4:disjoint-input;optional-counters-default-zero".into(),
        ..AgentUsageRecord::default()
    };
    valid_usage(&normalized).then_some(normalized)
}

fn summary_of_user(data: &Value) -> Option<String> {
    let message = data;
    if message["role"].as_str() != Some("user")
        || message["source"]["kind"].as_str() != Some("user")
    {
        return None;
    }
    let blocks = message["content"].as_array()?;
    let first_text = blocks
        .iter()
        .find(|block| block["type"].as_str() == Some("text"))?;
    label(&first_text["text"], 160)
}

/// Normalize one complete decoded JSONL record. State and returned events must be
/// committed atomically with the collector cursor. Replays retain native seq IDs.
pub fn parse_record(record: &Value, ctx: &LineContext<'_>, state: &mut Value) -> Vec<AgentEvent> {
    let mut parsed: State = match serde_json::from_value(state.clone()) {
        Ok(value) => value,
        Err(_) if state.is_null() => State::default(),
        Err(_) => State {
            blocked: Some(Block::UnsupportedState),
            ..State::default()
        },
    };
    if parsed.schema_version != 0 && parsed.schema_version != STATE_VERSION {
        parsed = State {
            blocked: Some(Block::UnsupportedState),
            ..State::default()
        };
    }
    parsed.schema_version = STATE_VERSION;
    parsed.partial_links |= trim(&mut parsed.turns, MAX_TURNS);
    parsed.partial_links |= trim(&mut parsed.calls, MAX_LINKS);
    parsed.partial_links |= trim(&mut parsed.jobs, MAX_LINKS);
    let events = parse_inner(record, ctx, &mut parsed);
    *state = serde_json::to_value(parsed).unwrap_or(Value::Null);
    events
}

fn parse_inner(record: &Value, ctx: &LineContext<'_>, state: &mut State) -> Vec<AgentEvent> {
    if record["type"].as_str() == Some("session") {
        if state.blocked.is_some() && state.blocked != Some(Block::MissingSeedBoundary) {
            return Vec::new();
        }
        let Some(header) = read_header(record) else {
            state.blocked = Some(Block::InvalidHeader);
            return Vec::new();
        };
        if let Some(previous) = &state.header {
            if previous != &header {
                state.blocked = Some(Block::HeaderChanged);
            }
            return Vec::new();
        }
        let time = header.created_at;
        let subagent = header.subagent;
        if header.is_seeded && state.seed_boundary.is_none() {
            state.blocked = Some(Block::MissingSeedBoundary);
        }
        state.header = Some(header);
        let mut created = event(state, ctx, None, time, "agent_created", "DSH 会话");
        created.status = Some("unknown".into());
        created.role = subagent.then(|| "subagent".into());
        return vec![created];
    }
    let Some(header) = state.header.clone() else {
        return Vec::new();
    };
    if state.blocked == Some(Block::MissingSeedBoundary) && state.seed_boundary.is_some() {
        state.blocked = None;
    }
    if state.blocked.is_some() {
        return Vec::new();
    }
    let seed_cut = if header.is_seeded {
        let Some(cut) = &state.seed_boundary else {
            state.blocked = Some(Block::MissingSeedBoundary);
            return Vec::new();
        };
        if cut.session_id != header.id || cut.seq > MAX_EXACT {
            state.blocked = Some(Block::InvalidSeedBoundary);
            return Vec::new();
        }
        Some(cut.seq)
    } else {
        None
    };
    let (Some(seq), Some(time), Some(record_type), Some(data)) = (
        number(&record["seq"]),
        timestamp(&record["time"]),
        record["type"].as_str(),
        record.get("data").filter(|data| data.is_object()),
    ) else {
        state.blocked = Some(Block::InvalidEnvelope);
        return Vec::new();
    };
    if state.last_seq.is_some_and(|last| seq <= last) {
        return Vec::new();
    }
    if seq != state.last_seq.map_or(0, |last| last + 1) {
        state.blocked = Some(Block::SequenceGap);
        return Vec::new();
    }
    state.last_seq = Some(seq);
    let inherited_marker =
        record_type == "session/end-seed" && data["inherited"].as_bool() == Some(true);
    if let Some(cut) = seed_cut {
        if seq < cut {
            route(record_type, data, state);
            return Vec::new();
        }
        if seq == cut {
            if !inherited_marker {
                state.blocked = Some(Block::InvalidSeedBoundary);
            } else {
                state.seed_seen = true;
            }
            return Vec::new();
        }
        if !state.seed_seen {
            state.blocked = Some(Block::InvalidSeedBoundary);
            return Vec::new();
        }
    }
    if inherited_marker {
        state.blocked = Some(Block::UnexpectedSeedMarker);
        return Vec::new();
    }
    let time = time as i64;
    let mut events = Vec::new();
    match record_type {
        "request/header" | "request/context" => {
            if route(record_type, data, state) {
                events.push(event(
                    state,
                    ctx,
                    Some(seq),
                    time,
                    "agent_updated",
                    "DSH 模型路由",
                ));
            }
        }
        "subagent/catalog" => {
            let version = number(&data["version"]);
            let mode = data["mode"].as_str();
            let valid = match version {
                Some(0) => matches!(mode, Some("one-shot" | "continuable")),
                Some(1) => matches!(mode, Some("one-shot" | "continuable" | "unknown")),
                Some(2) => mode == Some("external"),
                _ => false,
            };
            if valid {
                if let (Some(child), Some(created_at)) =
                    (native_id(&data["childId"]), timestamp(&data["childCreatedAt"]))
                {
                    if child != header.id {
                        let mut created = event(
                            state,
                            ctx,
                            Some(seq),
                            created_at as i64,
                            "agent_created",
                            "DSH 子会话登记",
                        );
                        created.session_id = child.clone();
                        created.agent_id = agent_id(&child);
                        created.parent_agent_id = Some(agent_id(&header.id));
                        created.name = label(&data["label"], 120);
                        created.role = Some("subagent".into());
                        created.model = None;
                        created.workspace = None;
                        created.status = Some("unknown".into());
                        created.correlation_id = Some(agent_id(&child));
                        events.push(created);
                    }
                }
            }
        }
        "subagent/descriptor" if !state.descriptor_seen => {
            state.descriptor_seen = true;
            let mode = data["mode"].as_str();
            let valid_label = match mode {
                Some("one-shot") => data.get("label").is_none_or(Value::is_string),
                Some("continuable") => data["label"].is_string(),
                _ => false,
            };
            if number(&data["version"]) != Some(3) || !valid_label || !data["provider"].is_string()
            {
                state.unsupported_descriptor = true;
            } else {
                if mode == Some("continuable") {
                    if let Some(model) = native_id(&data["agentModel"]) {
                        state.model = Some(model);
                    }
                }
                let mut updated = event(
                    state,
                    ctx,
                    Some(seq),
                    time,
                    "agent_updated",
                    "DSH 子会话身份",
                );
                updated.name = label(&data["label"], 120);
                updated.role = Some("subagent".into());
                events.push(updated);
            }
        }
        "turn/start" => {
            if let Some(turn) = number(&data["turn"]) {
                if turn_task(state, turn).is_none() {
                    state.turns.push(Turn {
                        turn,
                        start_seq: seq,
                        ended: false,
                    });
                    state.partial_links |= trim(&mut state.turns, MAX_TURNS);
                    let mut task = event(
                        state,
                        ctx,
                        Some(seq),
                        time,
                        "task_started",
                        &format!("DSH turn {turn}"),
                    );
                    attach_task(&mut task, Some(seq));
                    task.status = Some("running".into());
                    task.correlation_id = Some(format!("{}:turn:{turn}", agent_id(&header.id)));
                    let kind = if state.ever_started {
                        "agent_resumed"
                    } else {
                        "agent_started"
                    };
                    let mut started = event(state, ctx, Some(seq), time, kind, "DSH 开始回合");
                    attach_task(&mut started, Some(seq));
                    started.status = Some("running".into());
                    state.ever_started = true;
                    events.extend([started, task]);
                }
            }
        }
        "turn/end" => {
            if let Some(turn) = number(&data["turn"]) {
                let outcome = match data["reason"]["kind"].as_str() {
                    Some("completed") => {
                        Some(("task_completed", "completed", "agent_waiting", "waiting"))
                    }
                    Some("error") => Some(("task_failed", "failed", "agent_failed", "failed")),
                    Some("blocked" | "max-tokens") => Some((
                        "task_interrupted",
                        "interrupted",
                        "agent_waiting",
                        "waiting",
                    )),
                    Some("aborted" | "interrupted") => Some((
                        "task_interrupted",
                        "interrupted",
                        "agent_interrupted",
                        "interrupted",
                    )),
                    _ => None,
                };
                if let (Some(index), Some((task_kind, task_status, agent_kind, agent_status))) = (
                    state
                        .turns
                        .iter()
                        .rposition(|item| item.turn == turn && !item.ended),
                    outcome,
                ) {
                    let task_seq = state.turns[index].start_seq;
                    state.turns[index].ended = true;
                    let mut task = event(
                        state,
                        ctx,
                        Some(seq),
                        time,
                        task_kind,
                        &format!("DSH turn {turn}"),
                    );
                    attach_task(&mut task, Some(task_seq));
                    task.status = Some(task_status.into());
                    task.summary = label(&data["reason"]["kind"], 40);
                    let mut settled =
                        event(state, ctx, Some(seq), time, agent_kind, "DSH 回合结束");
                    attach_task(&mut settled, Some(task_seq));
                    settled.status = Some(agent_status.into());
                    events.extend([task, settled]);
                }
            }
        }
        "user/message" => {
            let mut active = state.turns.iter().filter(|item| !item.ended);
            if let (Some(turn), Some(summary)) = (active.next(), summary_of_user(data)) {
                if active.next().is_none() {
                    let mut updated = event(
                        state,
                        ctx,
                        Some(seq),
                        time,
                        "task_updated",
                        &format!("DSH turn {}", turn.turn),
                    );
                    attach_task(&mut updated, Some(turn.start_seq));
                    updated.summary = Some(summary);
                    updated.correlation_id = native_id(&data["id"])
                        .map(|id| format!("{}:message:{id}", agent_id(&header.id)));
                    events.push(updated);
                }
            }
        }
        "tool/call" => {
            if let (Some(turn), Some(step), Some(id), Some(name)) = (
                number(&data["turn"]),
                number(&data["step"]),
                native_id(&data["callId"]),
                label(&data["name"], 80),
            ) {
                if let Some(task_seq) = turn_task(state, turn) {
                    state.calls.push(Call {
                        id: id.clone(),
                        turn,
                        step,
                        task_seq,
                        name: name.clone(),
                    });
                    state.partial_links |= trim(&mut state.calls, MAX_LINKS);
                    let mut tool = event(
                        state,
                        ctx,
                        Some(seq),
                        time,
                        "tool_started",
                        &format!("工具开始：{name}"),
                    );
                    attach_task(&mut tool, Some(task_seq));
                    tool.correlation_id = Some(format!(
                        "{}:turn:{turn}:step:{step}:call:{id}",
                        agent_id(&header.id)
                    ));
                    tool.status = Some("running".into());
                    events.push(tool);
                }
            }
        }
        "tool/result" => {
            let message = &data["message"];
            if message["role"].as_str() == Some("tool")
                && message["source"]["kind"].as_str() == Some("tool")
            {
                if let (Some(turn), Some(step), Some(id)) = (
                    number(&data["turn"]),
                    number(&data["step"]),
                    native_id(&message["toolCallId"]),
                ) {
                    let is_error = match message.get("isError") {
                        None => Some(false),
                        Some(value) => value.as_bool(),
                    };
                    if message["source"]["callId"].as_str() == Some(id.as_str()) {
                        if let (Some(task_seq), Some(failed)) = (turn_task(state, turn), is_error) {
                            let name = state
                                .calls
                                .iter()
                                .rev()
                                .find(|call| {
                                    call.id == id && call.turn == turn && call.step == step
                                })
                                .map(|call| call.name.as_str())
                                .unwrap_or("tool");
                            let kind = if failed {
                                "tool_failed"
                            } else {
                                "tool_completed"
                            };
                            let mut tool = event(
                                state,
                                ctx,
                                Some(seq),
                                time,
                                kind,
                                &format!("工具结束：{name}"),
                            );
                            attach_task(&mut tool, Some(task_seq));
                            tool.correlation_id = Some(format!(
                                "{}:turn:{turn}:step:{step}:call:{id}",
                                agent_id(&header.id)
                            ));
                            tool.status = Some(if failed { "failed" } else { "completed" }.into());
                            events.push(tool);
                        }
                    }
                }
            }
        }
        "job/registered" | "job/settled" => {
            if data["owner"].as_str() == Some(header.id.as_str()) {
                if let (Some(id), Some(kind), Some(_)) = (
                    native_id(&data["jobId"]),
                    label(&data["kind"], 80),
                    number(&data["startedAt"]),
                ) {
                    let (task_seq, title) = if record_type == "job/registered" {
                        let root_call = data["toolCall"]["rootCallId"].as_str();
                        let task_seq = state
                            .calls
                            .iter()
                            .rev()
                            .find(|call| Some(call.id.as_str()) == root_call)
                            .map(|call| call.task_seq);
                        state.jobs.push(Job {
                            id: id.clone(),
                            task_seq,
                        });
                        state.partial_links |= trim(&mut state.jobs, MAX_LINKS);
                        (task_seq, "作业登记")
                    } else {
                        if number(&data["finishedAt"]).is_none()
                            || data["producerReported"].as_bool().is_none()
                        {
                            return events;
                        }
                        let title = match data["status"].as_str() {
                            Some("completed") => "作业完成",
                            Some("killed") => "作业中断",
                            Some("failed") => "作业失败",
                            _ => return events,
                        };
                        let task_seq = state
                            .jobs
                            .iter()
                            .rev()
                            .find(|job| job.id == id)
                            .and_then(|job| job.task_seq);
                        state.jobs.retain(|job| job.id != id);
                        (task_seq, title)
                    };
                    let mut job = event(
                        state,
                        ctx,
                        Some(seq),
                        time,
                        "message",
                        &format!("{title}：{kind}"),
                    );
                    attach_task(&mut job, task_seq);
                    job.correlation_id = Some(format!("{}:job:{id}", agent_id(&header.id)));
                    events.push(job);
                }
            }
        }
        "llm/retry-started" => {
            if native_id(&data["retryId"]).is_none() || number(&data["retry"]).is_none() {
                return events;
            }
            if let (Some(turn), Some(step)) = (number(&data["turn"]), number(&data["step"])) {
                if state
                    .usage_slot
                    .as_ref()
                    .is_some_and(|slot| slot.turn == turn && slot.step == step)
                {
                    state.usage_slot = None;
                }
            }
        }
        "assistant/message" | "assistant/attempt" => {
            if let (Some(turn), Some(step)) = (number(&data["turn"]), number(&data["step"])) {
                if let Some(task_seq) = turn_task(state, turn) {
                    if !data["stream"].is_array() {
                        return events;
                    }
                    let (model, provider) = if record_type == "assistant/message" {
                        let message = &data["message"];
                        let source = &message["source"];
                        if message["role"].as_str() != Some("assistant")
                            || source["kind"].as_str() != Some("model")
                            || !message["id"].is_string()
                            || !message["content"].is_array()
                            || !source["model"].is_string()
                            || !source["provider"].is_string()
                        {
                            return events;
                        }
                        (native_id(&source["model"]), native_id(&source["provider"]))
                    } else {
                        (state.model.clone(), state.provider.clone())
                    };
                    if record_type == "assistant/message" {
                        let mut message =
                            event(state, ctx, Some(seq), time, "message", "DSH 模型响应");
                        attach_task(&mut message, Some(task_seq));
                        message.model = model.clone();
                        message.correlation_id = native_id(&data["message"]["id"])
                            .map(|id| format!("{}:message:{id}", agent_id(&header.id)));
                        events.push(message);
                    }
                    if let Some(mut usage) = token_usage(data, record_type) {
                        let first_seq = match state.usage_slot.as_ref() {
                            Some(slot) if slot.turn == turn && slot.step == step => slot.first_seq,
                            _ => seq,
                        };
                        state.usage_slot = Some(UsageSlot {
                            turn,
                            step,
                            first_seq,
                        });
                        usage.key = format!("{}:usage:{first_seq}", agent_id(&header.id));
                        let mut sample =
                            event(state, ctx, Some(seq), time, "usage", "DSH 用量结算");
                        attach_task(&mut sample, Some(task_seq));
                        sample.model = model.clone();
                        sample.correlation_id = Some(usage.key.clone());
                        let window = state
                            .route_context
                            .as_ref()
                            .filter(|route| {
                                Some(route.model.as_str()) == model.as_deref()
                                    && Some(route.provider.as_str()) == provider.as_deref()
                            })
                            .and_then(|route| route.window);
                        let mut context =
                            event(state, ctx, Some(seq), time, "context", "DSH 最近请求上下文");
                        attach_task(&mut context, Some(task_seq));
                        context.model = model;
                        context.correlation_id = Some(usage.key.clone());
                        context.context = Some(AgentContext {
                            used_tokens: usage.input_tokens,
                            window_tokens: window,
                            measured_at: time,
                            basis: "dsh-v4:latest-settled-request-input;not-live-occupancy".into(),
                        });
                        sample.usage = Some(usage);
                        events.extend([sample, context]);
                    }
                }
            }
        }
        "team/member" | "team/task" | "team/message/queued" | "team/message/delivered" => {
            state.unsupported_team_events = true;
        }
        _ => {}
    }
    events
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(line: u64) -> LineContext<'static> {
        LineContext {
            source_id: CLIENT,
            client_id: CLIENT,
            source_path: "project/session/session.v4.jsonl",
            line_number: line,
            observed_at: 2_000,
        }
    }

    fn header(id: &str, seeded: bool) -> Value {
        json!({"type":"session","version":4,"id":id,"createdAt":1000,"isSeeded":seeded,"delegationDepth":0})
    }

    fn record(seq: u64, kind: &str, data: Value) -> Value {
        json!({"type":kind,"seq":seq,"time":1001+seq,"data":data})
    }

    fn feed(state: &mut Value, seq: u64, kind: &str, data: Value) -> Vec<AgentEvent> {
        parse_record(&record(seq, kind, data), &ctx(seq + 2), state)
    }

    fn start() -> Value {
        let mut state = Value::Null;
        assert_eq!(
            parse_record(&header("parent", false), &ctx(1), &mut state).len(),
            1
        );
        feed(&mut state, 0, "turn/start", json!({"turn":1}));
        state
    }

    fn usage_data(input: i64) -> Value {
        json!({"turn":1,"step":1,"stream":[{"type":"chunk","time":1002,"chunk":{"type":"usage","usage":{"inputTokens":input,"outputTokens":20,"cacheReadTokens":60,"cacheWriteTokens":10,"reasoningTokens":8}}}]})
    }

    #[test]
    fn invalid_harness_dates_are_rejected_in_headers_envelopes_and_catalogs() {
        let mut invalid_header = header("invalid-date", false);
        invalid_header["createdAt"] = json!(9_007_199_254_740_991_i64);
        let mut state = Value::Null;
        assert!(parse_record(&invalid_header, &ctx(1), &mut state).is_empty());
        assert_eq!(serde_json::from_value::<State>(state).unwrap().blocked, Some(Block::InvalidHeader));

        let mut state = start();
        let mut invalid_envelope = record(1, "turn/end", json!({"turn":1}));
        invalid_envelope["time"] = json!(9_007_199_254_740_991_i64);
        assert!(parse_record(&invalid_envelope, &ctx(3), &mut state).is_empty());
        let parsed: State = serde_json::from_value(state).unwrap();
        assert_eq!(parsed.blocked, Some(Block::InvalidEnvelope));
        assert_eq!(parsed.last_seq, Some(0));

        let mut state = start();
        assert!(feed(&mut state, 1, "subagent/catalog", json!({"version":1,"mode":"continuable","childId":"bad-child","childCreatedAt":9_007_199_254_740_991_i64})).is_empty());
        let valid_child = feed(&mut state, 2, "subagent/catalog", json!({"version":1,"mode":"continuable","childId":"good-child","childCreatedAt":1002}));
        assert_eq!(valid_child.len(), 1);
        assert_eq!(valid_child[0].timestamp, 1002);
        assert_eq!(timestamp(&json!(8_640_000_000_000_000_i64)), Some(8_640_000_000_000_000));
        assert_eq!(timestamp(&json!(8_640_000_000_000_001_i64)), None);
    }

    #[test]
    fn turn_seq_owns_tasks_and_tool_results_not_message_names() {
        let mut state = start();
        let call = feed(
            &mut state,
            1,
            "tool/call",
            json!({"turn":1,"step":1,"callId":"call_1","name":"pwsh","arguments":"SECRET_TOOL_ARGS"}),
        );
        assert_eq!(
            call[0].task_id.as_deref(),
            Some("deepseek_harness:parent:task:0")
        );
        let result = feed(
            &mut state,
            2,
            "tool/result",
            json!({"turn":1,"step":1,"message":{"id":"m1","role":"tool","source":{"kind":"tool","callId":"call_1"},"toolCallId":"call_1","isError":true,"content":[{"type":"text","text":"SECRET_TOOL_RESULT"}]}}),
        );
        assert_eq!(result[0].kind, "tool_failed");
        assert_eq!(result[0].correlation_id, call[0].correlation_id);
        assert_eq!(result[0].task_id, call[0].task_id);
        assert!(result[0].summary.is_none());
        assert!(!state.to_string().contains("SECRET"));
        let ended = feed(
            &mut state,
            3,
            "turn/end",
            json!({"turn":1,"reason":{"kind":"completed"}}),
        );
        assert_eq!(ended[0].kind, "task_completed");
        assert_eq!(ended[1].kind, "agent_waiting");
    }

    #[test]
    fn usage_is_disjoint_latest_request_context_and_never_billing() {
        let mut state = start();
        feed(
            &mut state,
            1,
            "request/context",
            json!({"model":"cn:deepseek-v4.1-flash","provider":"haibara","contextWindow":1_000_000}),
        );
        let events = feed(&mut state, 2, "assistant/attempt", usage_data(30));
        let usage = events
            .iter()
            .find_map(|event| event.usage.as_ref())
            .unwrap();
        assert_eq!(
            (usage.input_tokens, usage.output_tokens, usage.total_tokens),
            (100, 20, 120)
        );
        assert_eq!(usage.reasoning_tokens, 8);
        assert!(usage.actual_cost_usd.is_none() && usage.estimated_cost_usd.is_none());
        let context = events
            .iter()
            .find_map(|event| event.context.as_ref())
            .unwrap();
        assert_eq!(context.used_tokens, 100);
        assert_eq!(context.window_tokens, Some(1_000_000));
        assert_eq!(context.measured_at, 1003);
        assert_eq!(events[0].model.as_deref(), Some("cn:deepseek-v4.1-flash"));
    }

    #[test]
    fn settled_slot_replaces_until_an_explicit_retry_and_survives_restart() {
        let mut state = start();
        let first = feed(&mut state, 1, "assistant/attempt", usage_data(30));
        state = serde_json::from_str(&state.to_string()).unwrap();
        assert!(feed(&mut state, 1, "assistant/attempt", usage_data(30)).is_empty());
        let mut revision_record = record(2, "assistant/attempt", usage_data(40));
        revision_record["time"] = json!(900);
        let revision = parse_record(&revision_record, &ctx(2), &mut state);
        assert_eq!(revision[0].id, "deepseek_harness:parent:seq:2:usage");
        assert!(revision[0].timestamp < first[0].timestamp);
        assert!(revision[0].source_line < first[0].source_line);
        assert_eq!(
            first[0].usage.as_ref().unwrap().key,
            revision[0].usage.as_ref().unwrap().key
        );
        assert_ne!(first[0].id, revision[0].id);
        feed(
            &mut state,
            3,
            "llm/retry-started",
            json!({"turn":1,"step":1,"retryId":"retry1","retry":1}),
        );
        let retry = feed(&mut state, 4, "assistant/attempt", usage_data(40));
        assert_ne!(
            first[0].usage.as_ref().unwrap().key,
            retry[0].usage.as_ref().unwrap().key
        );
    }

    #[test]
    fn last_seed_boundary_excludes_ancestor_catalog_tasks_and_usage() {
        let mut state = json!({"seedBoundary":{"sessionId":"child","seq":4}});
        let mut h = header("child", true);
        h["parentSession"] = json!("parent");
        let created = parse_record(&h, &ctx(1), &mut state);
        assert_eq!(
            created[0].parent_agent_id.as_deref(),
            Some("deepseek_harness:parent")
        );
        assert!(feed(&mut state, 0, "session/end-seed", json!({"inherited":true})).is_empty());
        assert!(feed(&mut state, 1, "turn/start", json!({"turn":1})).is_empty());
        assert!(feed(&mut state, 2, "assistant/attempt", usage_data(30)).is_empty());
        assert!(
            feed(
                &mut state,
                3,
                "subagent/catalog",
                json!({"version":0,"childId":"ancestor-child","childCreatedAt":1,"mode":"one-shot"})
            )
            .is_empty()
        );
        assert!(feed(&mut state, 4, "session/end-seed", json!({"inherited":true})).is_empty());
        assert!(
            feed(
                &mut state,
                5,
                "turn/end",
                json!({"turn":1,"reason":{"kind":"forked"}})
            )
            .is_empty()
        );
        let started = feed(&mut state, 6, "turn/start", json!({"turn":2}));
        assert_eq!(
            started[1].task_id.as_deref(),
            Some("deepseek_harness:child:task:6")
        );
        assert!(feed(&mut state, 7, "session/end-seed", json!({})).is_empty());
        let mut own = usage_data(30);
        own["turn"] = json!(2);
        let used = feed(&mut state, 8, "assistant/attempt", own);
        assert_eq!(
            used[0].usage.as_ref().unwrap().key,
            "deepseek_harness:child:usage:8"
        );
        assert!(!state.to_string().contains("ancestor-child"));
    }

    #[test]
    fn missing_seed_boundary_and_sequence_gaps_fail_closed() {
        let mut state = Value::Null;
        parse_record(&header("child", true), &ctx(1), &mut state);
        assert!(feed(&mut state, 0, "turn/start", json!({"turn":1})).is_empty());
        assert_eq!(state["blocked"], json!("missing_seed_boundary"));
        let mut state = start();
        assert!(feed(&mut state, 2, "assistant/attempt", usage_data(30)).is_empty());
        assert_eq!(state["blocked"], json!("sequence_gap"));
        assert!(feed(&mut state, 3, "turn/start", json!({"turn":2})).is_empty());
    }

    #[test]
    fn catalog_has_no_task_assignment_or_parent_model_inheritance() {
        let mut state = start();
        feed(
            &mut state,
            1,
            "request/header",
            json!({"header":{"config":{"model":"parent-model","apiKey":"SECRET_CONFIG"}}}),
        );
        let events = feed(
            &mut state,
            2,
            "subagent/catalog",
            json!({"version":2,"childId":"external-child","childCreatedAt":999,"mode":"external","label":"worker token=SECRET_LABEL"}),
        );
        assert_eq!(events[0].agent_id, "deepseek_harness:external-child");
        assert_eq!(
            events[0].parent_agent_id.as_deref(),
            Some("deepseek_harness:parent")
        );
        assert!(events[0].task_id.is_none() && events[0].model.is_none());
        assert!(
            !serde_json::to_string(&events)
                .unwrap()
                .contains("SECRET_LABEL")
        );
        assert!(!state.to_string().contains("SECRET_CONFIG"));
    }

    #[test]
    fn only_redacted_user_summary_and_allowlisted_state_are_retained() {
        let mut state = start();
        state["rawPrompt"] = json!("SECRET_STATE");
        let events = feed(
            &mut state,
            1,
            "user/message",
            json!({"id":"m-user","role":"user","source":{"kind":"user"},"content":[{"type":"text","text":"检查任务 password=SECRET_PASSWORD"},{"type":"reasoning","text":"SECRET_REASONING"}]}),
        );
        assert_eq!(events[0].kind, "task_updated");
        let serialized = serde_json::to_string(&events).unwrap() + &state.to_string();
        assert!(!serialized.contains("SECRET"));
        assert!(events[0].summary.as_ref().unwrap().contains("检查任务"));
    }

    #[test]
    fn jobs_keep_original_owner_and_exact_call_task_after_turn_end() {
        let mut state = start();
        feed(
            &mut state,
            1,
            "tool/call",
            json!({"turn":1,"step":1,"callId":"root1","name":"pwsh","arguments":"SECRET"}),
        );
        let job = json!({"jobId":"job1","owner":"parent","kind":"pwsh","startedAt":1002,"label":"SECRET_LABEL","toolCall":{"callId":"nested1","rootCallId":"root1"}});
        let registered = feed(&mut state, 2, "job/registered", job.clone());
        assert_eq!(
            registered[0].task_id.as_deref(),
            Some("deepseek_harness:parent:task:0")
        );
        feed(
            &mut state,
            3,
            "turn/end",
            json!({"turn":1,"reason":{"kind":"completed"}}),
        );
        let mut ended = job;
        ended["status"] = json!("completed");
        ended["finishedAt"] = json!(1005);
        ended["producerReported"] = json!(true);
        let settled = feed(&mut state, 4, "job/settled", ended.clone());
        assert_eq!(settled[0].kind, "message");
        assert_eq!(settled[0].task_id, registered[0].task_id);
        assert!(settled[0].status.is_none());
        ended["owner"] = json!("foreign");
        assert!(feed(&mut state, 5, "job/settled", ended).is_empty());
        assert!(!state.to_string().contains("SECRET"));
    }

    #[test]
    fn absent_invalid_usage_and_unmatched_route_never_invent_values() {
        let mut state = start();
        assert!(
            feed(
                &mut state,
                1,
                "assistant/attempt",
                json!({"turn":1,"step":1,"stream":[]})
            )
            .is_empty()
        );
        let mut data = usage_data(30);
        data["stream"][0]["chunk"]["usage"]["totalTokens"] = json!(999);
        assert!(feed(&mut state, 2, "assistant/attempt", data).is_empty());
        feed(
            &mut state,
            3,
            "request/context",
            json!({"model":"model-a","provider":"provider-a","contextWindow":128000}),
        );
        feed(
            &mut state,
            4,
            "request/header",
            json!({"header":{"config":{"model":"model-b","provider":"provider-b"}}}),
        );
        let events = feed(&mut state, 5, "assistant/attempt", usage_data(30));
        assert!(events[1].context.as_ref().unwrap().window_tokens.is_none());
        assert_eq!(events[0].model.as_deref(), Some("model-b"));
    }

    #[test]
    fn unknown_header_versions_and_unseeded_inherited_markers_are_refused() {
        let mut h = header("future", false);
        h["version"] = json!(5);
        let mut state = Value::Null;
        assert!(parse_record(&h, &ctx(1), &mut state).is_empty());
        assert_eq!(state["blocked"], json!("invalid_header"));
        let mut state = start();
        assert!(feed(&mut state, 1, "session/end-seed", json!({"inherited":true})).is_empty());
        assert_eq!(state["blocked"], json!("unexpected_seed_marker"));
    }
    #[test]
    fn the_first_own_descriptor_is_authoritative_even_when_unsupported() {
        let mut state = start();
        assert!(feed(&mut state, 1, "subagent/descriptor", json!({"version":4})).is_empty());
        assert!(feed(&mut state, 2, "subagent/descriptor", json!({"version":3,"mode":"continuable","provider":"spawn","label":"worker","agentModel":"untrusted-later-model"})).is_empty());
        assert_eq!(state["unsupportedDescriptor"], json!(true));
        assert!(state["model"].is_null());
    }

    #[test]
    fn assistant_message_requires_its_real_envelope_and_prefers_committed_usage() {
        let mut state = start();
        let mut data = usage_data(30);
        assert!(feed(&mut state, 1, "assistant/message", data.clone()).is_empty());
        data["message"] = json!({"id":"message1","role":"assistant","source":{"kind":"model","provider":"haibara","model":"cn:deepseek-v4.1-flash"},"content":[{"type":"text","text":"SECRET_RESPONSE"}]});
        data["usage"] = json!({"inputTokens":14439,"outputTokens":94,"cacheReadTokens":78592,"totalTokens":93125});
        let events = feed(&mut state, 2, "assistant/message", data.clone());
        let usage = events
            .iter()
            .find_map(|event| event.usage.as_ref())
            .unwrap();
        assert_eq!(
            (usage.input_tokens, usage.output_tokens, usage.total_tokens),
            (93031, 94, 93125)
        );
        assert_eq!(
            usage.basis,
            "dsh-v4:disjoint-input;optional-counters-default-zero"
        );
        assert!(
            !serde_json::to_string(&events)
                .unwrap()
                .contains("SECRET_RESPONSE")
        );
        data["message"]["role"] = json!("user");
        assert!(feed(&mut state, 3, "assistant/message", data).is_empty());
    }

    #[test]
    fn attempt_has_no_message_identity_and_malformed_retry_does_not_open_a_slot() {
        let mut state = start();
        feed(
            &mut state,
            1,
            "request/context",
            json!({"model":"known-model","provider":"known-provider","contextWindow":1000}),
        );
        let first = feed(&mut state, 2, "assistant/attempt", usage_data(30));
        feed(
            &mut state,
            3,
            "llm/retry-started",
            json!({"turn":1,"step":1}),
        );
        let mut data = usage_data(40);
        data["message"] = json!({"source":{"kind":"model","model":"invented-model","provider":"invented-provider"}});
        let revision = feed(&mut state, 4, "assistant/attempt", data);
        assert_eq!(revision[0].model.as_deref(), Some("known-model"));
        assert_eq!(
            first[0].usage.as_ref().unwrap().key,
            revision[0].usage.as_ref().unwrap().key
        );
        assert!(!state.to_string().contains("invented-model"));
    }
}
