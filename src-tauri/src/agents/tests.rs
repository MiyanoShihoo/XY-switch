use super::{codex, models::*, projection::Projection};
use serde_json::{Value, json};

const BASE: i64 = 1_794_307_716_000;

fn record(kind: &str, payload: Value, at: i64) -> Value {
    json!({"type": kind, "timestamp": at, "payload": payload})
}

fn metadata(session: &str) -> Value {
    record("session_meta", json!({"id": session, "timestamp": BASE}), BASE)
}

fn counts(input: i64, output: i64) -> Value {
    json!({"input_tokens": input, "output_tokens": output, "total_tokens": input + output})
}

fn response(session: &str, turn: &str, response: &str, input: i64, output: i64, at: i64) -> Value {
    record("token_usage_record", json!({"thread_id": session, "turn_id": turn,
        "response_id": response, "usage": counts(input, output)}), at)
}

fn parse(records: &[Value]) -> Vec<AgentEvent> {
    let mut state = Value::Null;
    let mut events = Vec::new();
    for (index, value) in records.iter().enumerate() {
        let context = LineContext { source_id: "codex-sessions", client_id: "codex",
            source_path: "rollout-test.jsonl", line_number: index as u64 + 1, observed_at: BASE + 99_000 };
        events.extend(codex::parse_record(value, &context, &mut state));
    }
    events
}

fn snapshot(events: &[AgentEvent]) -> AgentSnapshot {
    Projection::from_events(events).snapshot(&AgentSnapshotQuery::default(), vec![], AgentHistory::default(), 1, BASE + 99_000, vec![])
}

fn event(agent: &str, kind: &str, timestamp: i64, line: u64) -> AgentEvent {
    let (client, session) = agent.split_once(':').unwrap();
    let id = if client == "deepseek_harness" { format!("{agent}:seq:{line}:{kind}") } else { format!("{agent}:{kind}:{timestamp}:{line}") };
    AgentEvent { id, source_id: "codex-sessions".into(),
        source_path: "rollout-test.jsonl".into(), client_id: client.into(), session_id: session.into(),
        agent_id: agent.into(), kind: kind.into(), title: kind.into(), timestamp,
        observed_at: BASE + 99_000, source_line: line, ..Default::default() }
}

fn usage_event(agent: &str, key: &str, input: i64, output: i64, at: i64, line: u64) -> AgentEvent {
    let mut usage = event(agent, "usage", at, line);
    usage.model = Some("model-exact".into());
    usage.task_id = Some(format!("{agent}:turn:turn-one"));
    usage.usage = Some(AgentUsageRecord { key: key.into(), input_tokens: input, output_tokens: output,
        total_tokens: input + output, basis: if agent.starts_with("codex:") { "codex_response_id" } else { "dsh-v4:disjoint-input;optional-counters-default-zero" }.into(), ..Default::default() });
    usage
}

fn task_result_events() -> (Vec<AgentEvent>, Vec<AgentEvent>) {
    let mut child = Vec::new();
    for (kind, turn, at, line, status, summary) in [
        ("task_started", "turn-one", 1000, 1, Some("running"), None),
        ("task_updated", "turn-one", 1050, 2, None, Some("Child prompt")),
        ("task_completed", "turn-one", 2000, 3, Some("completed"), Some("Completed result")),
        ("task_started", "turn-two", 2200, 5, Some("running"), None),
        ("task_updated", "turn-two", 2250, 6, None, Some("Second prompt")),
        ("task_failed", "turn-two", 3000, 7, Some("failed"), Some("Task failed")),
    ] {
        let mut record = event("codex:child", kind, BASE + at, line);
        record.task_id = Some(format!("codex:child:turn:{turn}"));
        record.status = status.map(str::to_owned);
        record.summary = summary.map(str::to_owned);
        child.push(record);
    }
    child.push(usage_event("codex:child", "response-one", 40, 10, BASE + 2100, 4));
    let mut second_usage = usage_event("codex:child", "response-two", 8, 2, BASE + 3100, 8);
    second_usage.task_id = Some("codex:child:turn:turn-two".into());
    child.push(second_usage);
    child.sort_by_key(|event| event.timestamp);
    for record in &mut child { record.source_path = "rollout-child.jsonl".into(); }

    let mut assignment = event("codex:parent", "task_handoff", BASE + 900, 31);
    assignment.task_id = Some("codex:child:assignment:one".into());
    assignment.related_agent_id = Some("codex:child".into());
    assignment.correlation_id = Some("one".into());
    assignment.summary = Some("Original request".into());
    let mut handoff = event("codex:parent", "task_handoff", BASE + 1100, 32);
    handoff.task_id = Some("codex:child:turn:turn-one".into());
    handoff.related_agent_id = assignment.related_agent_id.clone();
    handoff.correlation_id = assignment.correlation_id.clone();
    handoff.summary = Some("Delegated request".into());
    let mut second = event("codex:parent", "task_handoff", BASE + 2400, 33);
    second.task_id = Some("codex:child:turn:turn-two".into());
    second.related_agent_id = assignment.related_agent_id.clone();
    second.correlation_id = Some("two".into());
    second.summary = Some("Second request".into());
    let mut parent = vec![assignment, handoff, second];
    for record in &mut parent { record.source_path = "rollout-parent.jsonl".into(); }
    (child, parent)
}

#[test]
fn task_results_and_errors_survive_late_handoffs_for_multiple_tasks() {
    let (child, parent) = task_result_events();
    let mut chronological = [child.clone(), parent.clone()].concat();
    chronological.sort_by(|left, right| (left.timestamp, &left.id).cmp(&(right.timestamp, &right.id)));
    let expected = snapshot(&chronological);
    assert_eq!(expected.tasks.len(), 2);
    let completed = &expected.tasks[0];
    assert_eq!(completed.title, "Delegated request");
    assert_eq!(completed.summary.as_deref(), Some("Completed result"));
    assert_eq!(completed.status, "completed");
    assert_eq!(completed.error, None);
    assert_eq!(completed.started_at, Some(BASE + 1000));
    assert_eq!(completed.ended_at, Some(BASE + 2000));
    assert_eq!(completed.last_event_at, BASE + 2100);
    assert_eq!(completed.evidence_event_id, parent[0].id);
    assert_eq!(completed.requested_by_agent_id.as_deref(), Some("codex:parent"));
    assert_eq!(completed.usage.total_tokens, 50);
    let failed = &expected.tasks[1];
    assert_eq!(failed.title, "Second request");
    assert_eq!(failed.summary.as_deref(), Some("Second request"));
    assert_eq!(failed.error.as_deref(), Some("Task failed"));
    assert_eq!(failed.status, "failed");
    assert_eq!(failed.usage.total_tokens, 10);
    chronological.reverse();
    for order in [[child.clone(), parent.clone()].concat(), [parent, child].concat(), chronological] {
        let mut live = Projection::default();
        for batch in order.chunks(2) { live.extend(batch); }
        let actual = live.snapshot(&AgentSnapshotQuery::default(), vec![], AgentHistory::default(), 1, BASE + 99_000, vec![]);
        assert_eq!(serde_json::to_value(&actual.tasks).unwrap(), serde_json::to_value(&expected.tasks).unwrap());
    }
}

#[test]
fn task_field_clocks_preserve_results_through_ties_failure_and_recovery() {
    for agent in ["codex:child", "deepseek_harness:child"] {
        let task_id = format!("{agent}:turn:recovery");
        let mut description = event(agent, "task_updated", BASE + 5, 1);
        description.task_id = Some(task_id.clone());
        description.summary = Some("Description A".into());
        let mut description_tie = description.clone();
        description_tie.id.push_str(":z");
        description_tie.summary = Some("Description Z".into());
        let mut started = event(agent, "task_started", BASE + 10, 2);
        started.task_id = Some(task_id.clone());
        started.status = Some("running".into());
        let mut completed = event(agent, "task_completed", BASE + 20, 3);
        completed.task_id = Some(task_id.clone());
        completed.status = Some("completed".into());
        completed.summary = Some("Result A".into());
        let mut result_tie = completed.clone();
        result_tie.id.push_str(":z");
        result_tie.summary = Some("Result Z".into());
        let mut running_tie = event(agent, "task_started", BASE + 20, 3);
        running_tie.task_id = Some(task_id.clone());
        running_tie.status = Some("running".into());
        let mut events = vec![description, description_tie, started, completed, result_tie, running_tie];
        let check = |events: &[AgentEvent], status: &str, error: Option<&str>, ended_at: Option<i64>| {
            let expected = snapshot(events);
            let task = &expected.tasks[0];
            assert_eq!(task.title, "Description Z");
            assert_eq!(task.summary.as_deref(), Some("Result Z"));
            assert_eq!(task.status, status);
            assert_eq!(task.error.as_deref(), error);
            assert_eq!(task.ended_at, ended_at);
            let mut reversed = events.to_vec();
            reversed.reverse();
            let mut chronological = events.to_vec();
            chronological.sort_by(|left, right| (left.timestamp, &left.id).cmp(&(right.timestamp, &right.id)));
            for order in [reversed, chronological] {
                assert_eq!(serde_json::to_value(&snapshot(&order).tasks).unwrap(), serde_json::to_value(&expected.tasks).unwrap());
            }
        };
        check(&events, "completed", None, Some(BASE + 20));
        for (kind, at, line, status, summary) in [
            ("task_failed", 30, 4, "failed", Some("Failure detail")),
            ("task_started", 40, 5, "running", None),
            ("task_completed", 50, 6, "completed", None),
        ] {
            let mut next = event(agent, kind, BASE + at, line);
            next.task_id = Some(task_id.clone());
            next.status = Some(status.into());
            next.summary = summary.map(str::to_owned);
            events.push(next);
            check(&events, status, summary, (status != "running").then_some(BASE + at));
        }
    }
}

#[test]
fn stored_task_projection_matches_child_first_parent_first_and_incremental_replay() {
    let (child, parent) = task_result_events();
    let mut reversed = [child.clone(), parent.clone()].concat();
    reversed.reverse();
    for batches in [vec![child.clone(), parent.clone()], vec![parent, child], reversed.into_iter().map(|event| vec![event]).collect()] {
        let database = crate::database::Database::open(std::path::Path::new(":memory:")).unwrap();
        let store = super::store::EventStore::new(database).unwrap();
        let mut live = Projection::default();
        for batch in batches {
            let cursor = super::store::FileCursor { source_id: batch[0].source_id.clone(), root_key: "c".repeat(64),
                path: batch[0].source_path.clone(), ..Default::default() };
            let inserted = store.append(&batch, &cursor).unwrap();
            assert_eq!(inserted.len(), batch.len());
            live.extend(&inserted);
        }
        let restored = snapshot(&store.load_events(Some(BASE + 3100)).unwrap());
        let live = live.snapshot(&AgentSnapshotQuery::default(), vec![], AgentHistory::default(), 1, BASE + 99_000, vec![]);
        assert_eq!(restored.tasks.len(), 2);
        assert_eq!(restored.tasks[0].summary.as_deref(), Some("Completed result"));
        assert_eq!(restored.tasks[1].error.as_deref(), Some("Task failed"));
        assert_eq!(restored.tasks[0].usage.total_tokens, 50);
        assert_eq!(restored.tasks[1].usage.total_tokens, 10);
        assert_eq!(serde_json::to_value(&live.tasks).unwrap(), serde_json::to_value(&restored.tasks).unwrap());
    }
}

#[test]
fn codex_seconds_and_milliseconds_preserve_failure_and_duration() {
    let events = parse(&[
        metadata("main"),
        record("event_msg", json!({"type":"task_started", "turn_id":"turn-one", "started_at":BASE / 1000}), BASE),
        record("event_msg", json!({"type":"item_completed", "turn_id":"turn-one", "completed_at_ms":BASE + 700,
            "item":{"type":"AgentMessage", "phase":"commentary", "content":[{"type":"Text", "text":"Still working"}]}}), BASE + 800),
        record("event_msg", json!({"type":"task_complete", "turn_id":"turn-one", "completed_at":BASE / 1000 + 9,
            "error":{"message":"Transport closed"}}), BASE + 9_100),
    ]);
    assert_eq!(events.iter().find(|event| event.kind == "message").unwrap().timestamp, BASE + 700);
    let view = snapshot(&events);
    let task = &view.tasks[0];
    assert_eq!(task.started_at, Some(BASE));
    assert_eq!(task.ended_at, Some(BASE + 9_000));
    assert_eq!(task.status, "failed");
    assert_eq!(task.error.as_deref(), Some("Transport closed"));
}

#[test]
fn inherited_history_and_foreign_owner_usage_never_count_for_child() {
    let events = parse(&[
        record("session_meta", json!({"id":"child", "timestamp":BASE, "parent_thread_id":"main",
            "forked_from_id":"main", "subagent_history_start_ordinal":4}), BASE),
        metadata("main"),
        response("main", "inherited", "copied", 9_000, 90, BASE - 1_000),
        record("event_msg", json!({"type":"task_complete", "turn_id":"inherited"}), BASE - 500),
        record("response_item", json!({"type":"message", "role":"user", "content":"Inherited prompt"}), BASE - 100),
        record("event_msg", json!({"type":"task_started", "turn_id":"child-turn", "started_at":BASE / 1000 + 1}), BASE + 1_000),
        response("main", "foreign", "foreign", 8_000, 80, BASE + 2_000),
        response("child", "child-turn", "owned", 100, 20, BASE + 3_000),
    ]);
    let view = snapshot(&events);
    let child = view.agents.iter().find(|node| node.id == "codex:child").unwrap();
    assert_eq!(child.parent_id.as_deref(), Some("codex:main"));
    assert_eq!(view.totals.total_tokens, 120);
    assert_eq!(view.tasks.len(), 1);
    assert_eq!(view.teams.len(), 1);
    assert!(!events.iter().any(|event| event.summary.as_deref() == Some("Inherited prompt")));
}

#[test]
fn fork_reference_alone_does_not_create_a_parent_relationship() {
    let events = parse(&[record("session_meta", json!({"id":"copy", "forked_from_id":"main", "timestamp":BASE}), BASE)]);
    assert!(snapshot(&events).edges.is_empty());
    assert_eq!(snapshot(&events).agents[0].parent_id, None);
}

#[test]
fn modern_response_ledger_replaces_legacy_and_deduplicates_globally() {
    let mut events = parse(&[
        metadata("main"),
        record("event_msg", json!({"type":"token_count", "info":{"model_context_window":1000,
            "last_token_usage":counts(200, 20), "total_token_usage":counts(900, 90)}}), BASE + 1_000),
        response("main", "turn-one", "unique-response", 200, 20, BASE + 2_000),
        response("main", "turn-one", "unique-response", 200, 20, BASE + 2_000),
    ]);
    events.push(usage_event("codex:child", "codex:response:unique-response", 200, 20, BASE + 3_000, 4));
    let view = snapshot(&events);
    assert_eq!(view.totals.total_tokens, 220);
    assert_eq!(view.totals.records, 1);
    let main = view.agents.iter().find(|node| node.id == "codex:main").unwrap();
    assert_eq!(main.context.as_ref().unwrap().used_tokens, 200);
    assert_eq!(main.context.as_ref().unwrap().window_tokens, Some(1000));
    assert_eq!(main.usage.unknown_cost_records, 1);
}

#[test]
fn legacy_rollback_rebases_without_negative_or_duplicate_tokens() {
    let mut records = vec![metadata("legacy")];
    for (index, input) in [100, 160, 120, 190].into_iter().enumerate() {
        records.push(record("event_msg", json!({"type":"token_count", "info":{
            "total_token_usage":counts(input, 0), "last_token_usage":counts(50, 0)}}), BASE + index as i64 * 1000));
    }
    let view = snapshot(&parse(&records));
    assert_eq!(view.totals.total_tokens, 230);
    assert!(!view.warnings.is_empty());
}

#[test]
fn context_compaction_clears_measurement_without_clearing_cumulative_usage() {
    let view = snapshot(&parse(&[metadata("main"), response("main", "turn-one", "one", 200, 20, BASE + 1_000),
        record("compacted", json!({}), BASE + 2_000)]));
    assert!(view.agents[0].context.is_none());
    assert_eq!(view.totals.total_tokens, 220);
}

#[test]
fn silence_and_source_disconnect_do_not_infer_agent_termination() {
    let mut started = event("codex:main", "task_started", BASE, 2);
    started.task_id = Some("codex:main:turn:one".into());
    started.status = Some("running".into());
    let projection = Projection::from_events(&[started]);
    for connection in ["connected", "disconnected", "unavailable"] {
        let view = projection.snapshot(&AgentSnapshotQuery::default(), vec![AgentSource { id:"codex-sessions".into(),
            connection: connection.into(), ..Default::default() }], AgentHistory::default(), 1, BASE + 86_400_000, vec![]);
        assert_eq!(view.agents[0].status, "running");
        assert_eq!(view.agents[0].connection, connection);
        assert_eq!(view.agents[0].ended_at, None);
    }
}

#[test]
fn dsh_waiting_after_turn_completion_is_stable_under_replay_order() {
    let mut completed = event("deepseek_harness:session", "task_completed", BASE, 10);
    completed.task_id = Some("deepseek_harness:session:task:1".into());
    completed.status = Some("completed".into());
    let mut waiting = event("deepseek_harness:session", "agent_waiting", BASE, 10);
    waiting.status = Some("waiting".into());
    for events in [vec![completed.clone(), waiting.clone()], vec![waiting, completed]] {
        let view = snapshot(&events);
        assert_eq!(view.agents[0].status, "waiting");
        assert_eq!(view.tasks[0].status, "completed");
        assert_eq!(view.agents[0].ended_at, None);
    }
}

#[test]
fn dsh_settlement_revision_replaces_old_usage_and_cost_in_every_dimension() {
    let mut original = usage_event("deepseek_harness:session", "settlement-one", 100, 20, BASE, 10);
    original.usage.as_mut().unwrap().estimated_cost_usd = Some(0.25);
    let unrelated = usage_event("deepseek_harness:session", "settlement-two", 50, 5, BASE + 1_000, 11);
    let revised = usage_event("deepseek_harness:session", "settlement-one", 150, 30, BASE + 2_000, 12);
    let view = snapshot(&[original.clone(), unrelated, revised, original]);
    assert_eq!(view.totals.total_tokens, 235);
    assert_eq!(view.totals.records, 2);
    assert_eq!(view.totals.estimated_cost_usd, None);
    assert_eq!(view.totals.actual_cost_usd, None);
    assert_eq!(view.totals.unknown_cost_records, 2);
    for group in &view.usage_groups {
        assert_eq!(group.usage.total_tokens, 235, "{}", group.dimension);
        assert_eq!(group.usage.records, 2);
        assert_eq!(group.usage.estimated_cost_usd, None);
    }
}

#[test]
fn dsh_revision_and_context_use_native_sequence_despite_clock_or_file_reordering() {
    let mut old = usage_event("deepseek_harness:session", "same", 100, 20, BASE + 1000, 9);
    let mut new = usage_event("deepseek_harness:session", "same", 150, 30, BASE, 12);
    old.source_line = 200;
    new.source_line = 2;
    old.context = Some(AgentContext { used_tokens: 100, window_tokens: None, measured_at: old.timestamp, basis: "source-reported".into() });
    new.context = Some(AgentContext { used_tokens: 150, window_tokens: None, measured_at: new.timestamp, basis: "source-reported".into() });
    for events in [vec![old.clone(), new.clone()], vec![new, old]] {
        let view = snapshot(&events);
        assert_eq!(view.totals.total_tokens, 180);
        assert_eq!(view.agents[0].context.as_ref().unwrap().used_tokens, 150);
    }
}

#[test]
fn multilevel_relationships_reject_cycles_and_keep_unrelated_agents_separate() {
    let mut child = event("codex:child", "agent_created", BASE, 1);
    child.parent_agent_id = Some("codex:main".into());
    let mut grandchild = event("codex:grandchild", "agent_created", BASE, 2);
    grandchild.parent_agent_id = Some("codex:child".into());
    let mut cycle = event("codex:main", "agent_created", BASE, 3);
    cycle.parent_agent_id = Some("codex:grandchild".into());
    let unrelated = event("codex:unrelated", "agent_created", BASE, 4);
    let projection = Projection::from_events(&[child, grandchild, cycle, unrelated]);
    assert_eq!(projection.members("codex:main").len(), 3);
    let view = projection.snapshot(&AgentSnapshotQuery::default(), vec![], AgentHistory::default(), 0, BASE, vec![]);
    assert_eq!(view.teams.len(), 2);
    assert_eq!(view.edges.len(), 2);
    assert!(!view.warnings.is_empty());
}

#[test]
fn handoff_placeholder_merges_only_with_matching_explicit_submission() {
    let mut assignment = event("codex:main", "task_handoff", BASE, 1);
    assignment.task_id = Some("codex:child:assignment:call".into());
    assignment.related_agent_id = Some("codex:child".into());
    assignment.correlation_id = Some("call".into());
    assignment.summary = Some("Inspect the parser".into());
    let mut resolved = assignment.clone();
    resolved.id.push_str(":submission");
    resolved.task_id = Some("codex:child:turn:submission".into());
    let mut started = event("codex:child", "task_started", BASE + 1000, 2);
    started.task_id = resolved.task_id.clone();
    started.status = Some("running".into());
    let view = snapshot(&[assignment, started, resolved]);
    assert_eq!(view.tasks.len(), 1);
    assert_eq!(view.tasks[0].status, "running");
    assert_eq!(view.tasks[0].title, "Inspect the parser");
    assert_eq!(view.tasks[0].requested_by_agent_id.as_deref(), Some("codex:main"));
    assert_eq!(view.edges[0].task_id, Some("codex:child:turn:submission".into()));
}

#[test]
fn public_messages_are_scrubbed_and_private_reasoning_is_not_collected() {
    let events = parse(&[metadata("main"),
        record("event_msg", json!({"type":"item_completed", "item":{"type":"Reasoning", "summary_text":"HIDDEN_REASONING", "raw_content":"HIDDEN_REASONING"}}), BASE + 100),
        record("response_item", json!({"type":"message", "role":"assistant", "channel":"analysis", "content":[{"text":"HIDDEN_ANALYSIS"}]}), BASE + 200),
        record("response_item", json!({"type":"message", "role":"assistant", "phase":"commentary", "content":[{"text":"API_KEY=sk-proj-abcdefghijklmnopqrstuvwxyz0123456789; Public status"}]}), BASE + 300),
    ]);
    let serialized = serde_json::to_string(&events).unwrap();
    for forbidden in ["HIDDEN_REASONING", "HIDDEN_ANALYSIS", "sk-proj-abcdefghijklmnopqrstuvwxyz0123456789"] {
        assert!(!serialized.contains(forbidden));
    }
    assert!(serialized.contains("Public status"));
}

#[test]
fn failing_tool_does_not_mark_the_agent_as_failed() {
    let events = parse(&[metadata("main"),
        record("event_msg", json!({"type":"task_started", "turn_id":"turn-one"}), BASE),
        record("event_msg", json!({"type":"item_completed", "turn_id":"turn-one", "item":{
            "type":"CommandExecution", "id":"tool-one", "status":"completed", "exit_code":1}}), BASE + 1_000),
    ]);
    assert_eq!(events.iter().find(|event| event.kind == "tool_completed").unwrap().status.as_deref(), Some("failed"));
    assert_eq!(snapshot(&events).agents[0].status, "running");
}

#[test]
fn team_filter_and_resource_dimensions_never_double_count_parent_and_children() {
    let mut child = event("codex:child", "agent_created", BASE, 1);
    child.parent_agent_id = Some("codex:main".into());
    let projection = Projection::from_events(&[child,
        usage_event("codex:main", "main-usage", 100, 20, BASE + 100, 2),
        usage_event("codex:child", "child-usage", 200, 40, BASE + 200, 3),
        usage_event("codex:other", "other-usage", 300, 60, BASE + 300, 4)]);
    let view = projection.snapshot(&AgentSnapshotQuery { team_id:Some("codex:main".into()), at:None }, vec![], AgentHistory::default(), 0, BASE, vec![]);
    assert_eq!(view.agents.len(), 2);
    assert_eq!(view.totals.total_tokens, 360);
    for dimension in ["agent", "task", "session", "team", "client", "model"] {
        assert_eq!(view.usage_groups.iter().filter(|group| group.dimension == dimension).map(|group| group.usage.total_tokens).sum::<i64>(), 360, "{dimension}");
    }
}

#[test]
fn thousand_agent_projection_keeps_explicit_edges_and_exact_totals() {
    let mut events = Vec::new();
    for index in 0..1_000 {
        let id = format!("codex:agent-{index}");
        let mut created = event(&id, "agent_created", BASE, index + 1);
        if index > 0 { created.parent_agent_id = Some(format!("codex:agent-{}", (index - 1) / 10)); }
        events.push(created);
        events.push(usage_event(&id, &format!("usage-{index}"), 100, 20, BASE + 1000, index + 1));
    }
    let started = std::time::Instant::now();
    let view = snapshot(&events);
    assert_eq!(view.agents.len(), 1_000);
    assert_eq!(view.edges.len(), 999);
    assert_eq!(view.teams.len(), 1);
    assert_eq!(view.totals.total_tokens, 120_000);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
}

#[test]
fn estimates_require_exact_model_and_complete_rates_and_do_not_replace_actual_cost() {
    let database = crate::database::Database::open(std::path::Path::new(":memory:")).unwrap();
    crate::model_pricing::save(&database, &crate::model_pricing::PricingInput {
        id: "model-exact".into(), input_per_million: 2.0, output_per_million: 4.0,
        cache_read_per_million: Some(1.0), cache_creation_per_million: Some(3.0),
    }).unwrap();
    let mut event = usage_event("codex:main", "priced", 1_000_000, 1_000_000, BASE, 1);
    let usage = event.usage.as_mut().unwrap();
    usage.cache_read_tokens = 500_000;
    usage.cache_write_tokens = 250_000;
    super::privacy::price_event(&database, &mut event).unwrap();
    assert_eq!(event.usage.as_ref().unwrap().estimated_cost_usd, Some(5.75));
    assert_eq!(event.usage.as_ref().unwrap().actual_cost_usd, None);
    let mut alias = usage_event("codex:main", "alias", 100, 20, BASE, 2);
    alias.model = Some("provider/model-exact".into());
    super::privacy::price_event(&database, &mut alias).unwrap();
    assert_eq!(alias.usage.as_ref().unwrap().estimated_cost_usd, None);
    let mut actual = usage_event("codex:main", "actual", 100, 20, BASE, 3);
    actual.usage.as_mut().unwrap().actual_cost_usd = Some(0.15);
    super::privacy::price_event(&database, &mut actual).unwrap();
    assert_eq!(actual.usage.as_ref().unwrap().estimated_cost_usd, None);
    assert_eq!(actual.usage.as_ref().unwrap().actual_cost_usd, Some(0.15));
    crate::model_pricing::save(&database, &crate::model_pricing::PricingInput {
        id: "no-cache-rate".into(), input_per_million: 2.0, output_per_million: 4.0,
        cache_read_per_million: None, cache_creation_per_million: None,
    }).unwrap();
    let mut incomplete = usage_event("codex:main", "incomplete", 100, 20, BASE, 4);
    incomplete.model = Some("no-cache-rate".into());
    incomplete.usage.as_mut().unwrap().cache_read_tokens = 50;
    super::privacy::price_event(&database, &mut incomplete).unwrap();
    assert_eq!(incomplete.usage.as_ref().unwrap().estimated_cost_usd, None);
}

fn parse_harness(records: &[Value]) -> Vec<AgentEvent> {
    let mut state = Value::Null;
    records.iter().enumerate().flat_map(|(index, value)| {
        let context = LineContext { source_id: "deepseek-harness", client_id: "deepseek_harness",
            source_path: "session.v4.jsonl", line_number: index as u64 + 1, observed_at: BASE + 99_000 };
        super::harness::parse_record(value, &context, &mut state)
    }).collect()
}

#[test]
fn harness_current_task_is_independent_of_agent_status_rank_and_arrival_order() {
    let header = json!({"type":"session", "version":4, "id":"parent", "createdAt":BASE,
        "isSeeded":false, "delegationDepth":0});
    let started = json!({"type":"turn/start", "seq":0, "time":BASE + 1, "data":{"turn":1}});
    let ended = json!({"type":"turn/end", "seq":1, "time":BASE + 2, "data":{"turn":1, "reason":{"kind":"completed"}}});
    let resumed = json!({"type":"turn/start", "seq":2, "time":BASE + 3, "data":{"turn":2}});
    for (records, task, status) in [
        (vec![header.clone(), started.clone()], Some("deepseek_harness:parent:task:0"), "running"),
        (vec![header.clone(), started.clone(), ended.clone()], None, "waiting"),
        (vec![header, started, ended, resumed], Some("deepseek_harness:parent:task:2"), "running"),
    ] {
        let events = parse_harness(&records);
        let mut reversed = events.clone();
        reversed.reverse();
        for order in [&events, &reversed] {
            let view = snapshot(order);
            assert_eq!(view.agents[0].current_task_id.as_deref(), task);
            assert_eq!(view.agents[0].status, status);
            if view.tasks.len() > 1 { assert_eq!(view.tasks.iter().find(|task| task.id.ends_with(":task:0")).unwrap().status, "completed"); }
        }
    }
}

#[test]
fn completing_an_older_task_preserves_the_other_active_task() {
    let mut started = event("deepseek_harness:parent", "task_started", BASE + 1, 1);
    started.task_id = Some("deepseek_harness:parent:task:1".into());
    started.status = Some("running".into());
    let mut newer = event("deepseek_harness:parent", "task_started", BASE + 2, 2);
    newer.task_id = Some("deepseek_harness:parent:task:2".into());
    newer.status = Some("running".into());
    let mut ended = event("deepseek_harness:parent", "task_completed", BASE + 3, 3);
    ended.task_id = started.task_id.clone();
    ended.status = Some("completed".into());
    for events in [vec![started.clone(), newer.clone(), ended.clone()], vec![ended, newer, started]] {
        assert_eq!(snapshot(&events).agents[0].current_task_id.as_deref(), Some("deepseek_harness:parent:task:2"));
    }
}

#[test]
fn harness_header_metadata_survives_catalog_order_and_sqlite_replay() {
    let catalog = parse_harness(&[
        json!({"type":"session", "version":4, "id":"z-parent", "createdAt":BASE, "isSeeded":false, "delegationDepth":0}),
        json!({"type":"subagent/catalog", "seq":0, "time":BASE + 3, "data":{
            "version":1, "mode":"continuable", "childId":"a-child", "childCreatedAt":BASE + 2, "label":"Auditor"}}),
    ]);
    let header = parse_harness(&[json!({"type":"session", "version":4, "id":"a-child", "createdAt":BASE + 2,
        "isSeeded":false, "delegationDepth":1, "origin":"subagent", "parentSession":"z-parent", "cwd":"E:/review-child"})]);
    let mut live = Projection::from_events(&catalog);
    live.extend(&header);
    let mut events = catalog;
    events.extend(header);
    let database = crate::database::Database::open(std::path::Path::new(":memory:")).unwrap();
    let store = super::store::EventStore::new(database).unwrap();
    let cursor = super::store::FileCursor { source_id:"deepseek-harness".into(), root_key:"a".repeat(64),
        path:"session.v4.jsonl".into(), ..Default::default() };
    store.append(&events, &cursor).unwrap();
    let restored = snapshot(&store.load_events(None).unwrap());
    let live = live.snapshot(&AgentSnapshotQuery::default(), vec![], AgentHistory::default(), 1, BASE + 99_000, vec![]);
    let child = restored.agents.iter().find(|node| node.session_id == "a-child").unwrap();
    assert_eq!(child.workspace.as_deref(), Some("E:/review-child"));
    assert_eq!(child.role.as_deref(), Some("subagent"));
    assert_eq!(child.name, "Auditor");
    assert_eq!(child.evidence_event_id, "deepseek_harness:a-child:header:agent_created");
    assert_eq!(serde_json::to_value(&live.agents).unwrap(), serde_json::to_value(&restored.agents).unwrap());
    assert_eq!(serde_json::to_value(&live.edges).unwrap(), serde_json::to_value(&restored.edges).unwrap());
}

#[test]
fn credential_uris_and_connection_strings_are_redacted_in_stored_events() {
    let database = crate::database::Database::open(std::path::Path::new(":memory:")).unwrap();
    let store = super::store::EventStore::new(database.clone()).unwrap();
    let mut sample = event("codex:privacy", "message", BASE, 1);
    sample.summary = Some("postgresql://review_user:REVIEW_ONLY_PASSWORD@localhost:5432/app; redis://:REDIS_TEST_PASSWORD@localhost:6379/0; mongodb+srv://mongo_user:MONGO_TEST_PASSWORD@host.test/db?auth=QUERY_SECRET; https://web_user:WEB_TEST_PASSWORD@host.test/x?signature=SIGNATURE_TEST; Server=localhost; User ID=SQL_TEST_USER; Pwd=SQL_TEST_PASSWORD".into());
    let cursor = super::store::FileCursor { source_id:sample.source_id.clone(), root_key:"b".repeat(64),
        path:sample.source_path.clone(), ..Default::default() };
    let sanitized = store.append(&[sample], &cursor).unwrap();
    let arc = database.conn();
    let connection = arc.lock().unwrap();
    let stored: String = connection.query_row("SELECT event_json FROM agent_observation_events", [], |row| row.get(0)).unwrap();
    for value in ["review_user", "REVIEW_ONLY_PASSWORD", "REDIS_TEST_PASSWORD", "mongo_user", "MONGO_TEST_PASSWORD",
        "QUERY_SECRET", "web_user", "WEB_TEST_PASSWORD", "SIGNATURE_TEST", "SQL_TEST_USER", "SQL_TEST_PASSWORD"] {
        assert!(!stored.contains(value), "{value}");
        assert!(!sanitized[0].summary.as_ref().unwrap().contains(value), "{value}");
    }
    assert!(stored.contains("localhost:5432/app"));
    assert!(stored.contains("host.test/x"));
}
