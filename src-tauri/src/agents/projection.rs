use super::models::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
struct NodeState {
    node: AgentNode,
    status_clock: (i64, u64, u8),
    model_clock: (i64, u64),
    context_clock: (i64, u64),
    creation_clock: Option<(u8, String)>,
    name_clock: Option<(u8, i64, u64, String)>,
    role_clock: Option<(u8, i64, u64, String)>,
    workspace_clock: Option<(u8, i64, u64, String)>,
    active_tasks: BTreeMap<String, (i64, u64)>,
}

#[derive(Default)]
struct TaskState {
    task: AgentTask,
    status_clock: Option<(i64, u64, u8, String)>,
    title_clock: Option<(i64, u64, String)>,
    summary_clock: Option<(i64, u64, String)>,
    requester_clock: Option<(i64, u64, String)>,
    evidence_clock: Option<(i64, String)>,
    description: Option<String>,
    start_clock: Option<(i64, u64)>,
}

impl TaskState {
    fn describe(&mut self, description: &str, clock: (i64, u64, String)) {
        if !description.trim().is_empty() && self.title_clock.as_ref().is_none_or(|previous| &clock > previous) {
            self.task.title = super::privacy::text(description, 120);
            self.description = Some(description.into());
            self.title_clock = Some(clock);
        }
    }

    fn merge_assignment(&mut self, previous: Self) {
        if let (Some(description), Some(clock)) = (previous.description.as_deref(), previous.title_clock) {
            self.describe(description, clock);
        }
        if previous.requester_clock.as_ref() > self.requester_clock.as_ref() {
            self.task.requested_by_agent_id = previous.task.requested_by_agent_id;
            self.requester_clock = previous.requester_clock;
        }
        if let Some(clock) = previous.evidence_clock {
            if self.evidence_clock.as_ref().is_none_or(|earliest| &clock < earliest) {
                self.task.evidence_event_id = previous.task.evidence_event_id;
                self.evidence_clock = Some(clock);
            }
        }
        self.task.last_event_at = self.task.last_event_at.max(previous.task.last_event_at);
    }
}

#[derive(Clone)]
struct UsageEntry {
    agent: String,
    task: Option<String>,
    model: String,
    usage: AgentUsageRecord,
    clock: (i64, u64),
}

#[derive(Default)]
pub struct Projection {
    nodes: BTreeMap<String, NodeState>,
    tasks: BTreeMap<String, TaskState>,
    edges: BTreeMap<String, AgentEdge>,
    usage: BTreeMap<String, UsageEntry>,
    model_usage: BTreeMap<(String, String), AgentUsage>,
    modern_agents: BTreeSet<String>,
    price_counts: BTreeMap<(String, String, String), (usize, usize)>,
    global_tokens: i64,
    aliases: BTreeMap<String, String>,
    warnings: BTreeSet<String>,
}

fn terminal(status: &str) -> bool { matches!(status, "completed" | "failed" | "interrupted") }

fn event_clock(event: &AgentEvent) -> (i64, u64) {
    if event.client_id == "deepseek_harness" {
        let prefix = format!("{}:seq:", event.agent_id);
        let sequence = event.id.strip_prefix(&prefix).and_then(|suffix| suffix.split(':').next())
            .and_then(|sequence| sequence.parse::<u64>().ok()).unwrap_or(0);
        (0, sequence)
    } else { (event.timestamp, event.source_line) }
}

fn metadata_field(target: &mut Option<String>, field_clock: &mut Option<(u8, i64, u64, String)>, value: &Option<String>, clock: &(u8, i64, u64, String)) {
    if value.is_some() && field_clock.as_ref().is_none_or(|previous| clock > previous) {
        *target = value.clone();
        *field_clock = Some(clock.clone());
    }
}

fn add_usage(total: &mut AgentUsage, prices: &mut (usize, usize), record: &AgentUsageRecord, sign: i64) {
    total.input_tokens += sign * record.input_tokens;
    total.output_tokens += sign * record.output_tokens;
    total.cache_read_tokens += sign * record.cache_read_tokens;
    total.cache_write_tokens += sign * record.cache_write_tokens;
    total.reasoning_tokens += sign * record.reasoning_tokens;
    total.total_tokens += sign * record.total_tokens;
    total.records = (total.records as i64 + sign).max(0) as usize;
    if let Some(cost) = record.actual_cost_usd { prices.0 = (prices.0 as i64 + sign).max(0) as usize; total.actual_cost_usd = Some(total.actual_cost_usd.unwrap_or(0.0) + sign as f64 * cost); }
    if let Some(cost) = record.estimated_cost_usd { prices.1 = (prices.1 as i64 + sign).max(0) as usize; total.estimated_cost_usd = Some(total.estimated_cost_usd.unwrap_or(0.0) + sign as f64 * cost); }
    if prices.0 == 0 { total.actual_cost_usd = None; }
    if prices.1 == 0 { total.estimated_cost_usd = None; }
    if record.actual_cost_usd.is_none() && record.estimated_cost_usd.is_none() {
        total.unknown_cost_records = (total.unknown_cost_records as i64 + sign).max(0) as usize;
    }
    if total.records == 0 { *total = AgentUsage::default(); }
}

fn merge_usage(total: &mut AgentUsage, other: &AgentUsage) {
    total.input_tokens += other.input_tokens;
    total.output_tokens += other.output_tokens;
    total.cache_read_tokens += other.cache_read_tokens;
    total.cache_write_tokens += other.cache_write_tokens;
    total.reasoning_tokens += other.reasoning_tokens;
    total.total_tokens += other.total_tokens;
    total.records += other.records;
    total.unknown_cost_records += other.unknown_cost_records;
    if let Some(cost) = other.actual_cost_usd { total.actual_cost_usd = Some(total.actual_cost_usd.unwrap_or(0.0) + cost); }
    if let Some(cost) = other.estimated_cost_usd { total.estimated_cost_usd = Some(total.estimated_cost_usd.unwrap_or(0.0) + cost); }
}

impl Projection {
    pub fn from_events(events: &[AgentEvent]) -> Self {
        let mut projection = Self::default();
        projection.extend(events);
        projection
    }

    pub fn extend(&mut self, events: &[AgentEvent]) {
        for event in events { self.apply(event); }
    }

    fn ensure_node(&mut self, agent_id: &str, event: &AgentEvent) {
        self.nodes.entry(agent_id.into()).or_insert_with(|| {
            let (client, session) = agent_id.split_once(':').unwrap_or((&event.client_id, &event.session_id));
            NodeState { node: AgentNode {
                id: agent_id.into(), session_id: session.into(), client_id: client.into(), source_id: event.source_id.clone(),
                name: format!("Agent {}", session.chars().rev().take(8).collect::<String>().chars().rev().collect::<String>()),
                team_id: agent_id.into(), status: "unknown".into(), connection: "unavailable".into(), created_at: event.timestamp,
                last_event_at: event.timestamp, evidence_event_id: event.id.clone(), ..Default::default()
            }, ..Default::default() }
        });
    }

    fn ensure_task(&mut self, id: &str, owner: &str, event: &AgentEvent) {
        let state = self.tasks.entry(id.into()).or_insert_with(|| TaskState { task: AgentTask {
            id: id.into(), agent_id: owner.into(), kind: if id.contains(":assignment:") { "assignment" } else { "turn" }.into(),
            title: if id.contains(":assignment:") { "已派发任务" } else { "Agent 任务" }.into(), status: "unknown".into(),
            last_event_at: event.timestamp, evidence_event_id: event.id.clone(), ..Default::default()
        }, ..Default::default() });
        state.task.last_event_at = state.task.last_event_at.max(event.timestamp);
        let evidence_clock = (event.timestamp, event.id.clone());
        if state.evidence_clock.as_ref().is_none_or(|earliest| &evidence_clock < earliest) {
            state.task.evidence_event_id = event.id.clone();
            state.evidence_clock = Some(evidence_clock);
        }
    }

    fn parent(&mut self, child: &str, parent: &str, event: &AgentEvent) {
        self.ensure_node(parent, event);
        self.ensure_node(child, event);
        let mut cursor = Some(parent);
        let mut visited = BTreeSet::new();
        while let Some(current) = cursor {
            if current == child || !visited.insert(current) {
                self.warnings.insert("检测到矛盾的循环派生关系，未纳入拓扑。".into()); return;
            }
            cursor = self.nodes.get(current).and_then(|state| state.node.parent_id.as_deref());
        }
        let node = &mut self.nodes.get_mut(child).expect("ensured node").node;
        if node.parent_id.as_deref().is_some_and(|existing| existing != parent) {
            self.warnings.insert("同一 Agent 出现冲突父级，保留首个明确来源关系。".into()); return;
        }
        node.parent_id = Some(parent.into());
        let id = format!("spawn:{parent}:{child}");
        let edge = self.edges.entry(id.clone()).or_insert_with(|| AgentEdge { id, source_agent_id: parent.into(), target_agent_id: child.into(),
            kind: "spawn".into(), task_id: None, timestamp: event.timestamp, evidence_event_id: event.id.clone() });
        if (event.timestamp, &event.id) < (edge.timestamp, &edge.evidence_event_id) {
            edge.timestamp = event.timestamp;
            edge.evidence_event_id = event.id.clone();
        }
    }

    fn set_status(&mut self, event: &AgentEvent, status: &str) {
        let state = self.nodes.get_mut(&event.agent_id).expect("ensured node");
        let rank = u8::from(event.kind.starts_with("agent_"));
        let (major, minor) = event_clock(event);
        let clock = (major, minor, rank);
        if clock < state.status_clock { return; }
        state.status_clock = clock;
        state.node.status = status.into();
        if terminal(status) { state.node.ended_at = Some(event.lifecycle_at.unwrap_or(event.timestamp)); }
        else { state.node.ended_at = None; }
    }

    fn ledger(&mut self, event: &AgentEvent) {
        let Some(record) = event.usage.as_ref() else { return; };
        if !super::privacy::valid_usage(record) { return; }
        if let Some(previous) = self.usage.get(&record.key) {
            if event.client_id != "deepseek_harness" || previous.agent != event.agent_id || event_clock(event) <= previous.clock { return; }
            let previous = self.usage.remove(&record.key).expect("known revision");
            self.adjust(&previous, -1);
        }
        let modern = record.basis == "codex_response_id";
        let legacy = record.basis == "codex_cumulative_delta";
        if modern && self.modern_agents.insert(event.agent_id.clone()) {
            let keys: Vec<_> = self.usage.iter().filter(|(_, value)| value.agent == event.agent_id && value.usage.basis == "codex_cumulative_delta")
                .map(|(key, _)| key.clone()).collect();
            if !keys.is_empty() {
                self.warnings.insert("同一会话出现逐响应与旧累计账本，已采用逐响应记录避免重复；较早区间的覆盖可能不完整。".into());
            }
            for key in keys {
                if let Some(entry) = self.usage.remove(&key) { self.adjust(&entry, -1); }
            }
        }
        if legacy && self.modern_agents.contains(&event.agent_id) { return; }
        if self.global_tokens.checked_add(record.total_tokens).is_none_or(|sum| sum > 9_007_199_254_740_991) {
            self.warnings.insert("用量超过可精确表示的范围，异常记录未纳入统计。".into()); return;
        }
        let entry = UsageEntry { agent: event.agent_id.clone(), task: event.task_id.clone(), model: event.model.clone().unwrap_or_else(|| "unknown".into()), usage: record.clone(), clock: event_clock(event) };
        if let Some(task) = &entry.task { self.ensure_task(task, &entry.agent, event); }
        self.adjust(&entry, 1);
        self.usage.insert(record.key.clone(), entry);
    }

    fn adjust(&mut self, entry: &UsageEntry, sign: i64) {
        self.global_tokens += sign * entry.usage.total_tokens;
        if let Some(state) = self.nodes.get_mut(&entry.agent) {
            let prices = self.price_counts.entry(("agent".into(), entry.agent.clone(), String::new())).or_default();
            add_usage(&mut state.node.usage, prices, &entry.usage, sign);
        }
        if let Some(id) = &entry.task {
            if let Some(task) = self.tasks.get_mut(id) {
                let prices = self.price_counts.entry(("task".into(), id.clone(), String::new())).or_default();
                add_usage(&mut task.task.usage, prices, &entry.usage, sign);
            }
        }
        let prices = self.price_counts.entry(("model".into(), entry.agent.clone(), entry.model.clone())).or_default();
        add_usage(self.model_usage.entry((entry.agent.clone(), entry.model.clone())).or_default(), prices, &entry.usage, sign);
    }

    fn assignment(&mut self, event: &AgentEvent) {
        let Some(receiver) = event.related_agent_id.as_deref() else { return; };
        let Some(original_id) = event.task_id.as_deref() else { return; };
        self.ensure_node(receiver, event);
        let alias_key = format!("{}:{}", receiver, event.correlation_id.as_deref().unwrap_or(&event.id));
        let mut id = original_id.to_owned();
        let mut previous_assignment = None;
        if original_id.contains(":turn:") {
            if let Some(previous) = self.aliases.insert(alias_key, id.clone()) {
                if previous != id && previous.contains(":assignment:") { previous_assignment = self.tasks.remove(&previous); }
            }
        } else if let Some(resolved) = self.aliases.get(&alias_key) { id = resolved.clone(); }
        else { self.aliases.insert(alias_key, id.clone()); }
        self.ensure_task(&id, receiver, event);
        let state = self.tasks.get_mut(&id).expect("ensured task");
        if let Some(previous) = previous_assignment { state.merge_assignment(previous); }
        let clock = event_clock(event);
        let field_clock = (clock.0, clock.1, event.id.clone());
        if state.requester_clock.as_ref().is_none_or(|previous| &field_clock > previous) {
            state.task.requested_by_agent_id = Some(event.agent_id.clone());
            state.requester_clock = Some(field_clock.clone());
        }
        if let Some(summary) = &event.summary { state.describe(summary, field_clock); }
        let edge_id = format!("{}:{}:{receiver}", event.kind, event.correlation_id.as_deref().unwrap_or(&event.id));
        self.edges.insert(edge_id.clone(), AgentEdge { id: edge_id, source_agent_id: event.agent_id.clone(), target_agent_id: receiver.into(),
            kind: if event.kind == "task_handoff" { "handoff" } else { "assignment" }.into(), task_id: Some(id), timestamp: event.timestamp, evidence_event_id: event.id.clone() });
    }

    fn apply(&mut self, event: &AgentEvent) {
        self.ensure_node(&event.agent_id, event);
        let clock = event_clock(event);
        {
            let state = self.nodes.get_mut(&event.agent_id).expect("ensured node");
            state.node.last_event_at = state.node.last_event_at.max(event.timestamp);
            if matches!(event.kind.as_str(), "agent_created" | "agent_updated") {
                let own_header = event.client_id != "deepseek_harness" || event.id == format!("{}:header:agent_created", event.agent_id);
                let priority = if event.kind == "agent_updated" { 3 } else if own_header { 2 } else { 1 };
                let metadata_clock = (priority, clock.0, clock.1, event.id.clone());
                if event.kind == "agent_created" {
                    let creation_clock = (priority, event.id.clone());
                    if state.creation_clock.as_ref().is_none_or(|previous| &creation_clock > previous) {
                        state.node.created_at = event.timestamp;
                        state.node.source_id = event.source_id.clone();
                        state.node.evidence_event_id = event.id.clone();
                        state.creation_clock = Some(creation_clock);
                    }
                }
                if let Some(name) = &event.name {
                    if state.name_clock.as_ref().is_none_or(|previous| &metadata_clock > previous) {
                        state.node.name = name.clone();
                        state.name_clock = Some(metadata_clock.clone());
                    }
                }
                metadata_field(&mut state.node.role, &mut state.role_clock, &event.role, &metadata_clock);
                metadata_field(&mut state.node.workspace, &mut state.workspace_clock, &event.workspace, &metadata_clock);
            }
            if event.kind != "agent_spawned" && event.model.is_some() && state.model_clock <= clock {
                state.node.model = event.model.clone(); state.model_clock = clock;
            }
            if event.context.is_some() && state.context_clock <= clock {
                state.node.context = event.context.clone(); state.context_clock = clock;
            }
            if event.kind == "context_compacted" && state.context_clock <= clock {
                state.node.context = None; state.context_clock = clock;
            }
        }
        if let Some(parent) = &event.parent_agent_id { self.parent(&event.agent_id, parent, event); }
        if event.kind == "agent_spawned" {
            if let Some(child) = &event.related_agent_id {
                self.parent(child, &event.agent_id, event);
                if let Some(name) = &event.name {
                    let state = self.nodes.get_mut(child).expect("ensured child");
                    let name_clock = (1, clock.0, clock.1, event.id.clone());
                    if state.name_clock.as_ref().is_none_or(|previous| &name_clock > previous) {
                        state.node.name = name.clone();
                        state.name_clock = Some(name_clock);
                    }
                }
            }
        }
        if matches!(event.kind.as_str(), "task_assigned" | "task_handoff") { self.assignment(event); return; }
        if let Some(task_id) = &event.task_id {
            if matches!(event.kind.as_str(), "task_started" | "task_completed" | "task_failed" | "task_interrupted" | "task_updated") {
                self.ensure_task(task_id, &event.agent_id, event);
                let state = self.tasks.get_mut(task_id).expect("ensured task");
                let field_clock = (clock.0, clock.1, event.id.clone());
                if event.kind == "task_updated" {
                    state.describe(event.summary.as_deref().unwrap_or(&event.title), field_clock.clone());
                }
                if event.kind == "task_started" {
                    let started_at = event.lifecycle_at.unwrap_or(event.timestamp);
                    state.task.started_at = Some(state.task.started_at.map_or(started_at, |start| start.min(started_at)));
                    state.start_clock = Some(state.start_clock.map_or(clock, |start| start.min(clock)));
                    let node = &mut self.nodes.get_mut(&event.agent_id).expect("ensured node").node;
                    node.started_at = Some(node.started_at.map_or(started_at, |start| start.min(started_at)));
                }
                if let Some(status) = &event.status {
                    if terminal(status) && status != "failed" {
                        if let Some(summary) = event.summary.as_ref().filter(|summary| !summary.trim().is_empty()) {
                            if state.summary_clock.as_ref().is_none_or(|previous| &field_clock > previous) {
                                state.task.summary = Some(summary.clone());
                                state.summary_clock = Some(field_clock);
                            }
                        }
                    }
                    let status_clock = (clock.0, clock.1, u8::from(terminal(status)), event.id.clone());
                    if state.status_clock.as_ref().is_none_or(|previous| &status_clock > previous) {
                        state.status_clock = Some(status_clock); state.task.status = status.clone();
                        state.task.ended_at = terminal(status).then_some(event.lifecycle_at.unwrap_or(event.timestamp));
                        state.task.error = if status == "failed" { event.summary.clone() } else { None };
                    }
                }
                let node_state = self.nodes.get_mut(&event.agent_id).expect("ensured node");
                if !terminal(&state.task.status) {
                    if let Some(start) = state.start_clock { node_state.active_tasks.insert(task_id.clone(), start); }
                } else { node_state.active_tasks.remove(task_id); }
                node_state.node.current_task_id = node_state.active_tasks.iter().max_by_key(|(id, start)| (**start, *id)).map(|(id, _)| id.clone());
                if let Some(status) = &event.status { self.set_status(event, status); }
            }
        }
        if matches!(event.kind.as_str(), "agent_status" | "agent_waiting" | "agent_resumed" | "agent_started" | "agent_ended" | "agent_failed" | "agent_interrupted") {
            if let Some(status) = &event.status { self.set_status(event, status); }
        }
        if event.kind == "agent_closed" && !terminal(&self.nodes[&event.agent_id].node.status) { self.set_status(event, "interrupted"); }
        if event.kind == "usage_gap" { self.warnings.insert("部分旧格式累计用量缺少可靠基线；未确认的区间未计入。".into()); }
        self.ledger(event);
    }

    fn roots(&self) -> BTreeMap<String, String> {
        let mut roots = BTreeMap::<String, String>::new();
        for id in self.nodes.keys() {
            let mut path = Vec::new(); let mut cursor = id.as_str();
            let root = loop {
                if let Some(root) = roots.get(cursor) { break root.clone(); }
                path.push(cursor.to_owned());
                match self.nodes.get(cursor).and_then(|state| state.node.parent_id.as_deref()) {
                    Some(parent) if path.len() <= self.nodes.len() => cursor = parent,
                    _ => break cursor.to_owned(),
                }
            };
            for member in path { roots.insert(member, root.clone()); }
        }
        roots
    }

    pub fn members(&self, team: &str) -> Vec<String> { self.roots().into_iter().filter(|(_, root)| root == team).map(|(id, _)| id).collect() }

    pub fn snapshot(&self, query: &AgentSnapshotQuery, sources: Vec<AgentSource>, history: AgentHistory, revision: u64, now: i64, mut warnings: Vec<String>) -> AgentSnapshot {
        let roots = self.roots();
        let mut teams = BTreeMap::<String, AgentTeam>::new();
        let mut agents = Vec::new(); let mut totals = AgentUsage::default();
        let mut groups = BTreeMap::<(String, String), AgentUsageGroup>::new();
        let mut group = |dimension: &str, id: &str, label: &str, usage: &AgentUsage| {
            let entry = groups.entry((dimension.into(), id.into())).or_insert_with(|| AgentUsageGroup { dimension: dimension.into(), id: id.into(), label: label.into(), usage: AgentUsage::default() });
            merge_usage(&mut entry.usage, usage);
        };
        for (id, state) in &self.nodes {
            let root = roots.get(id).unwrap_or(id);
            let team = teams.entry(root.clone()).or_insert_with(|| AgentTeam { id: root.clone(), name: self.nodes.get(root).map(|root| root.node.name.clone()).unwrap_or_else(|| root.clone()), client_id: state.node.client_id.clone(), ..Default::default() });
            team.agent_count += 1; team.running_count += usize::from(state.node.status == "running");
            team.last_event_at = team.last_event_at.max(state.node.last_event_at); merge_usage(&mut team.usage, &state.node.usage);
            if query.team_id.as_deref().is_some_and(|selected| selected != root) { continue; }
            let mut node = state.node.clone(); node.team_id = root.clone();
            node.connection = sources.iter().find(|source| source.id == node.source_id).map(|source| source.connection.clone()).unwrap_or_else(|| "unavailable".into());
            merge_usage(&mut totals, &node.usage);
            group("agent", id, &node.name, &node.usage); group("session", &node.session_id, &node.session_id, &node.usage);
            group("client", &node.client_id, &node.client_id, &node.usage); agents.push(node);
        }
        let selected: BTreeSet<_> = agents.iter().map(|node| node.id.as_str()).collect();
        let tasks: Vec<_> = self.tasks.values().filter(|state| selected.contains(state.task.agent_id.as_str())).map(|state| {
            let mut task = state.task.clone();
            if task.summary.is_none() { task.summary = state.description.clone(); }
            task
        }).collect();
        for task in &tasks { group("task", &task.id, &task.title, &task.usage); }
        for ((agent, model), usage) in &self.model_usage { if selected.contains(agent.as_str()) && usage.records > 0 { group("model", model, model, usage); } }
        for team in teams.values() { if query.team_id.as_deref().is_none_or(|selected| selected == team.id) { group("team", &team.id, &team.name, &team.usage); } }
        let edges = self.edges.values().filter(|edge| selected.contains(edge.source_agent_id.as_str()) && selected.contains(edge.target_agent_id.as_str())).cloned().collect();
        warnings.extend(self.warnings.iter().cloned());
        if query.at.is_some() { warnings.push("历史回放按事件时间重建；数据源连接指示当前采集状态。".into()); }
        let mut teams: Vec<_> = teams.into_values().collect(); teams.sort_by_key(|team| std::cmp::Reverse(team.last_event_at));
        AgentSnapshot { version: 1, revision, generated_at: now, replay_at: query.at, sources, teams, agents, edges, tasks,
            usage_groups: groups.into_values().collect(), totals, history, warnings }
    }
}
