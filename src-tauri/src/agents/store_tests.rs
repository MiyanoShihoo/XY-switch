use super::*;
use crate::agents::{AgentEventScope, AgentMonitor, models::{AgentSnapshotQuery, AgentUsageRecord}};
use serde_json::json;

fn memory_store() -> EventStore {
    EventStore::new(Database::open(Path::new(":memory:")).unwrap()).unwrap()
}

fn cursor() -> FileCursor {
    FileCursor {
        source_id: "fixture".into(),
        root_key: digest(b"isolated-root"),
        path: "rollout-fixture.jsonl".into(),
        offset: 64,
        file_len: 64,
        line_number: 1,
        parser_state: json!({"sessionId":"fixture"}),
        ..Default::default()
    }
}

fn event(id: &str, agent: &str, timestamp: i64) -> AgentEvent {
    AgentEvent {
        id: id.into(),
        source_id: "fixture".into(),
        source_path: "rollout-fixture.jsonl".into(),
        source_line: 1,
        client_id: "codex".into(),
        session_id: "fixture".into(),
        agent_id: agent.into(),
        timestamp,
        observed_at: timestamp,
        kind: "task_started".into(),
        title: "Observation".into(),
        ..Default::default()
    }
}

fn usage_event(id: &str, client: &str, timestamp: i64, input: i64) -> AgentEvent {
    let mut event = event(id, &format!("{client}:fixture"), timestamp);
    event.client_id = client.into();
    event.kind = "usage".into();
    event.model = Some("fixture-unpriced-model".into());
    event.usage = Some(AgentUsageRecord {
        key: format!("{client}:slot"),
        input_tokens: input,
        output_tokens: 20,
        total_tokens: input + 20,
        basis: "fixture".into(),
        ..Default::default()
    });
    event
}

fn append_to_monitor(monitor: &AgentMonitor, events: &[AgentEvent]) {
    let inserted = monitor.shared.store.append(events, &cursor()).unwrap();
    let history = monitor.shared.store.history().unwrap();
    let mut state = monitor.shared.view.lock().unwrap();
    state.projection.extend(&inserted);
    state.history = history;
    state.revision += 1;
}

fn membership_monitor() -> (AgentMonitor, AgentEvent) {
    let monitor = AgentMonitor::with_roots(Database::open(Path::new(":memory:")).unwrap(), vec![]).unwrap();
    let mut parent = event("parent-header", "codex:parent", 1000);
    parent.kind = "agent_created".into();
    let mut child = event("child-header", "codex:child", 1001);
    child.kind = "agent_created".into();
    let mut usage = event("child-usage", "codex:child", 1500);
    usage.kind = "usage".into();
    usage.task_id = Some("codex:child:turn:one".into());
    usage.usage = Some(AgentUsageRecord { key: "child-response".into(), input_tokens: 5,
        output_tokens: 2, total_tokens: 7, basis: "codex_response_id".into(), ..Default::default() });
    append_to_monitor(&monitor, &[parent, child, usage]);
    let mut spawn = event("parent-spawn", "codex:parent", 3000);
    spawn.kind = "agent_spawned".into();
    spawn.related_agent_id = Some("codex:child".into());
    (monitor, spawn)
}

#[test]
fn monitor_historical_membership_matches_snapshot_across_pages_and_late_spawn() {
    let (monitor, spawn) = membership_monitor();
    let mut query = AgentEventQuery { team_id: Some("codex:child".into()), until: Some(2000),
        limit: Some(1), ..Default::default() };
    let first = monitor.events(query.clone()).unwrap();
    assert_eq!(first.total, 2);
    assert_eq!(first.events[0].id, "child-usage");
    query.cursor = Some(first.next_cursor.unwrap());
    append_to_monitor(&monitor, &[spawn]);
    let second = monitor.events(query).unwrap();
    assert_eq!(second.total, 2);
    assert_eq!(second.events[0].id, "child-header");
    assert!(second.next_cursor.is_none());

    let child = monitor.snapshot(AgentSnapshotQuery { team_id: Some("codex:child".into()), at: Some(2000) }).unwrap();
    assert_eq!(child.agents.len(), 1);
    assert_eq!(child.agents[0].id, "codex:child");
    assert_eq!(child.totals.total_tokens, 7);
    let parent = monitor.snapshot(AgentSnapshotQuery { team_id: Some("codex:parent".into()), at: Some(2000) }).unwrap();
    assert_eq!(parent.agents.len(), 1);
    assert_eq!(parent.agents[0].id, "codex:parent");
    assert_eq!(parent.totals.total_tokens, 0);
    let parent_events = monitor.events(AgentEventQuery { team_id: Some("codex:parent".into()), until: Some(2000), ..Default::default() }).unwrap();
    assert_eq!(parent_events.total, 1);
    assert_eq!(parent_events.events[0].id, "parent-header");
    let current = monitor.snapshot(AgentSnapshotQuery { team_id: Some("codex:parent".into()), at: None }).unwrap();
    assert_eq!(current.agents.len(), 2);
    assert_eq!(current.totals.total_tokens, 7);
    assert_eq!(monitor.events(AgentEventQuery { team_id: Some("codex:child".into()), ..Default::default() }).unwrap().total, 0);
}

#[test]
fn monitor_event_scope_separates_replay_membership_from_dates_and_binds_cursors() {
    let (monitor, spawn) = membership_monitor();
    append_to_monitor(&monitor, &[spawn]);
    let scope = AgentEventScope { at: Some(3500) };
    let query = AgentEventQuery { team_id: Some("codex:parent".into()), since: Some(1000),
        until: Some(1500), limit: Some(1), ..Default::default() };
    let replay = monitor.snapshot(AgentSnapshotQuery { team_id: query.team_id.clone(), at: scope.at }).unwrap();
    assert_eq!(replay.agents.len(), 2);
    let first = monitor.events_in_scope(query.clone(), scope).unwrap();
    assert_eq!(first.total, 3);
    assert_eq!(first.events[0].id, "child-usage");
    let mut next = query.clone();
    next.cursor = Some(first.next_cursor.unwrap());
    assert!(monitor.events_in_scope(next.clone(), AgentEventScope { at: Some(2000) }).is_err());
    let second = monitor.events_in_scope(next.clone(), scope).unwrap();
    assert_eq!(second.total, 3);
    assert_eq!(second.events[0].id, "child-header");
    next.cursor = Some(second.next_cursor.unwrap());
    let third = monitor.events_in_scope(next, scope).unwrap();
    assert_eq!(third.total, 3);
    assert_eq!(third.events[0].id, "parent-header");
    assert!(third.next_cursor.is_none());

    assert_eq!(monitor.events(query.clone()).unwrap().total, 1);
    let live_scope: AgentEventScope = serde_json::from_value(json!({})).unwrap();
    assert_eq!(monitor.events_in_scope(query.clone(), live_scope).unwrap().total, 3);
    let scoped_task = AgentEventQuery { agent_id: Some("codex:child".into()), task_id: Some("codex:child:turn:one".into()), ..query.clone() };
    assert_eq!(monitor.events_in_scope(scoped_task, scope).unwrap().total, 1);
    let outsider = AgentEventQuery { agent_id: Some("codex:outsider".into()), ..query };
    assert_eq!(monitor.events_in_scope(outsider, scope).unwrap().total, 0);
    for until in [None, Some(4000)] {
        let page = monitor.events_in_scope(AgentEventQuery { until, ..Default::default() }, AgentEventScope { at: Some(2000) }).unwrap();
        assert_eq!(page.total, 3);
        assert!(page.events.iter().all(|event| event.timestamp <= 2000));
    }
    assert!(monitor.events_in_scope(AgentEventQuery::default(), AgentEventScope { at: Some(-1) }).is_err());
}

#[test]
fn event_and_cursor_roll_back_together_on_sql_failure() {
    let store = memory_store();
    let initial = cursor();
    store.append(&[], &initial).unwrap();
    store
        .database()
        .conn()
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER agent_observation_test_abort BEFORE UPDATE ON agent_observation_cursors
         BEGIN SELECT RAISE(ABORT, 'PRIVATE_DATABASE_ERROR'); END;",
        )
        .unwrap();
    let mut next = initial.clone();
    next.offset = 128;
    next.file_len = 128;
    next.line_number = 2;
    let record = event("new", "codex:fixture", 100);
    let error = store
        .append(std::slice::from_ref(&record), &next)
        .unwrap_err();
    assert!(!error.contains("PRIVATE_DATABASE_ERROR"));
    assert!(store.load_events(None).unwrap().is_empty());
    assert_eq!(store.cursors().unwrap(), vec![initial]);
    store
        .database()
        .conn()
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER agent_observation_test_abort;")
        .unwrap();
    assert_eq!(
        store
            .append(std::slice::from_ref(&record), &next)
            .unwrap()
            .len(),
        1
    );
    assert!(store.clone().append(&[record], &next).unwrap().is_empty());
    assert_eq!(store.cursors().unwrap(), vec![next]);
}

#[test]
fn dsh_revisions_survive_old_index_migration_and_codex_keys_stay_unique() {
    let store = memory_store();
    let first = usage_event(
        "deepseek_harness:fixture:seq:1:usage",
        "deepseek_harness",
        200,
        30,
    );
    store
        .append(std::slice::from_ref(&first), &cursor())
        .unwrap();
    store
        .database()
        .conn()
        .lock()
        .unwrap()
        .execute_batch(
            "DROP INDEX agent_observation_usage_key_immutable;
         CREATE UNIQUE INDEX agent_observation_usage_key
           ON agent_observation_events(client_id,usage_key) WHERE usage_key IS NOT NULL;",
        )
        .unwrap();
    let store = EventStore::new(store.database().clone()).unwrap();
    let revision = usage_event(
        "deepseek_harness:fixture:seq:2:usage",
        "deepseek_harness",
        100,
        40,
    );
    let codex = usage_event("codex:response:a", "codex", 300, 50);
    let duplicate_key = usage_event("codex:response:b", "codex", 400, 60);
    let inserted = store
        .append(&[first, revision, codex, duplicate_key], &cursor())
        .unwrap();
    assert_eq!(inserted.len(), 2);
    let stored = store.load_events(None).unwrap();
    assert_eq!(stored.len(), 3);
    assert_eq!(
        stored
            .iter()
            .map(|event| event.timestamp)
            .collect::<Vec<_>>(),
        vec![100, 200, 300]
    );
    let dsh: Vec<_> = stored
        .iter()
        .filter(|event| event.client_id == "deepseek_harness")
        .collect();
    assert_eq!(dsh.len(), 2);
    assert_eq!(
        dsh[0].usage.as_ref().unwrap().key,
        dsh[1].usage.as_ref().unwrap().key
    );
    assert_ne!(dsh[0].id, dsh[1].id);
    assert_eq!(store.load_events(Some(200)).unwrap().len(), 2);
}

#[test]
fn pages_have_stable_ties_totals_and_filter_bound_cursors() {
    let store = memory_store();
    let mut records: Vec<_> = ["a", "b", "c"]
        .into_iter()
        .map(|id| {
            let mut record = event(id, "codex:one", 100);
            record.task_id = Some("task-one".into());
            record
        })
        .collect();
    records.push(event("d", "codex:one", 101));
    records.push(event("z", "codex:two", 102));
    store.append(&records, &cursor()).unwrap();
    let query = AgentEventQuery {
        limit: Some(2),
        ..Default::default()
    };
    let first = store.event_page(&query, None).unwrap();
    assert_eq!(first.total, 5);
    assert_eq!(
        first
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec!["z", "d"]
    );
    store
        .append(&[event("new", "codex:one", 500)], &cursor())
        .unwrap();
    let second_query = AgentEventQuery {
        cursor: first.next_cursor.clone(),
        ..query.clone()
    };
    let second = store.event_page(&second_query, None).unwrap();
    assert_eq!(second.total, 6);
    assert_eq!(
        second
            .events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "b"]
    );
    let third = store
        .event_page(
            &AgentEventQuery {
                cursor: second.next_cursor,
                ..query.clone()
            },
            None,
        )
        .unwrap();
    assert_eq!(third.events[0].id, "a");
    assert!(third.next_cursor.is_none());

    let mut allowed: Vec<_> = (0..1500)
        .map(|index| format!("codex:other-{index}"))
        .collect();
    allowed.push("codex:one".into());
    let filtered = AgentEventQuery {
        team_id: Some("team-one".into()),
        agent_id: Some("codex:one".into()),
        task_id: Some("task-one".into()),
        since: Some(100),
        until: Some(100),
        limit: Some(2),
        ..Default::default()
    };
    let page = store.event_page(&filtered, Some(&allowed)).unwrap();
    assert_eq!(page.total, 3);
    assert_eq!(page.events.len(), 2);
    allowed.reverse();
    let next = store
        .event_page(
            &AgentEventQuery {
                cursor: page.next_cursor,
                ..filtered.clone()
            },
            Some(&allowed),
        )
        .unwrap();
    assert_eq!(next.events[0].id, "a");
    assert_eq!(store.event_page(&filtered, Some(&[])).unwrap().total, 0);
    assert!(store.event_page(&filtered, None).is_err());
    assert!(
        store
            .event_page(
                &AgentEventQuery {
                    cursor: first.next_cursor,
                    ..filtered
                },
                Some(&allowed)
            )
            .is_err()
    );
    for token in ["", "v1.00", "v1.zz", "v2.7b7d", "v1.7"] {
        assert!(
            store
                .event_page(
                    &AgentEventQuery {
                        cursor: Some(token.into()),
                        ..query.clone()
                    },
                    None
                )
                .is_err()
        );
    }
    for limit in [0, 501] {
        assert!(
            store
                .event_page(
                    &AgentEventQuery {
                        limit: Some(limit),
                        ..Default::default()
                    },
                    None
                )
                .is_err()
        );
    }
    assert!(
        store
            .event_page(
                &AgentEventQuery {
                    since: Some(2),
                    until: Some(1),
                    ..Default::default()
                },
                None
            )
            .is_err()
    );
}

#[test]
fn retention_keeps_active_ancestors_cursors_and_business_records() {
    let store = memory_store();
    assert_eq!(store.retention_days().unwrap(), 30);
    assert!(store.set_retention_days(0).is_err());
    assert!(store.set_retention_days(366).is_err());
    store.set_retention_days(1).unwrap();
    store
        .database()
        .conn()
        .lock()
        .unwrap()
        .execute(
            "INSERT INTO app_settings(key,value) VALUES('collector-test-business','untouched')",
            [],
        )
        .unwrap();
    let mut grandparent = event("grandparent", "codex:grandparent", 1);
    grandparent.kind = "agent_created".into();
    let mut parent = event("parent", "codex:parent", 2);
    parent.kind = "agent_created".into();
    parent.parent_agent_id = Some(grandparent.agent_id.clone());
    let mut child = event("child", "codex:child", 3);
    child.kind = "agent_created".into();
    child.parent_agent_id = Some(parent.agent_id.clone());
    let mut orphan = event("orphan", "codex:orphan", 4);
    orphan.kind = "agent_created".into();
    let now = DAY_MS * 60;
    let recent = event("recent", "codex:child", now);
    let expired = event("expired", "codex:child", now - 2 * DAY_MS);
    let cursor = cursor();
    store
        .append(
            &[grandparent, parent, child, orphan, expired, recent],
            &cursor,
        )
        .unwrap();
    assert_eq!(store.prune(now).unwrap(), 2);
    assert_eq!(store.prune(now).unwrap(), 0);
    assert_eq!(
        store
            .load_events(None)
            .unwrap()
            .iter()
            .map(|event| event.id.as_str())
            .collect::<Vec<_>>(),
        vec!["grandparent", "parent", "child", "recent"]
    );
    assert_eq!(store.cursors().unwrap(), vec![cursor]);
    let history = store.history().unwrap();
    assert_eq!(history.event_count, 4);
    assert_eq!(history.oldest_at, Some(1));
    assert!(history.truncated);
    assert_eq!(
        EventStore::new(store.database().clone())
            .unwrap()
            .retention_days()
            .unwrap(),
        1
    );
    let value: String = store
        .database()
        .conn()
        .lock()
        .unwrap()
        .query_row(
            "SELECT value FROM app_settings WHERE key='collector-test-business'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(value, "untouched");
}

#[test]
fn capacity_limit_keeps_recent_event_and_its_ancestor_metadata() {
    let store = memory_store();
    let mut records = Vec::new();
    for (id, parent) in [
        ("grandparent", None),
        ("parent", Some("grandparent")),
        ("child", Some("parent")),
    ] {
        let mut created = event(id, &format!("codex:{id}"), 1);
        created.kind = "agent_created".into();
        created.parent_agent_id = parent.map(|id| format!("codex:{id}"));
        records.push(created);
    }
    for index in 0..6 {
        records.push(event(&format!("run-{index}"), "codex:child", 10 + index));
    }
    store.append(&records, &cursor()).unwrap();
    let arc = store.database().conn();
    let connection = arc.lock().unwrap();
    assert_eq!(enforce_event_limit(&connection, 4).unwrap(), 5);
    assert_eq!(count_on(&connection).unwrap(), 4);
    drop(connection);
    let records = store.load_events(None).unwrap();
    assert_eq!(
        records
            .iter()
            .filter(|event| event.kind == "agent_created")
            .count(),
        3
    );
    assert!(records.iter().any(|event| event.id == "run-5"));
    assert_eq!(MAX_STORED_EVENTS, 200_000);
}

#[test]
fn retention_and_capacity_preserve_spawn_only_ancestry_for_replay() {
    use crate::agents::projection::Projection;

    let now = DAY_MS * 60;
    let mut records = Vec::new();
    for id in ["grandparent", "parent", "child", "orphan"] {
        let mut created = event(id, &format!("codex:{id}"), 1);
        created.kind = "agent_created".into();
        records.push(created);
    }
    for (parent, child) in [("grandparent", "parent"), ("parent", "child")] {
        let mut spawned = event(&format!("spawn-{child}"), &format!("codex:{parent}"), 2);
        spawned.kind = "agent_spawned".into();
        spawned.related_agent_id = Some(format!("codex:{child}"));
        records.push(spawned);
    }
    records.push(event("old-activity", "codex:child", 3));
    records.push(event("recent", "codex:child", now));
    let relationships = |events: &[AgentEvent]| {
        Projection::from_events(events)
            .snapshot(
                &Default::default(),
                Vec::new(),
                Default::default(),
                1,
                now,
                Vec::new(),
            )
            .agents
            .into_iter()
            .map(|node| (node.id, (node.parent_id, node.team_id)))
            .collect::<HashMap<_, _>>()
    };
    let before = relationships(&records);
    assert_eq!(before["codex:child"].0.as_deref(), Some("codex:parent"));
    assert_eq!(before["codex:child"].1, "codex:grandparent");
    for retention in [true, false] {
        let store = memory_store();
        store.append(&records, &cursor()).unwrap();
        if retention {
            store.set_retention_days(1).unwrap();
            assert_eq!(store.prune(now).unwrap(), 2);
            assert_eq!(store.prune(now).unwrap(), 0);
        } else {
            let arc = store.database().conn();
            assert_eq!(enforce_event_limit(&arc.lock().unwrap(), 6).unwrap(), 2);
        }
        let retained = store.load_events(None).unwrap();
        assert_eq!(retained.len(), 6);
        assert!(retained.iter().any(|event| event.id == "recent"));
        let after = relationships(&retained);
        for id in ["codex:grandparent", "codex:parent", "codex:child"] {
            assert_eq!(after[id], before[id]);
        }
        assert!(!after.contains_key("codex:orphan"));
        assert_eq!(store.cursors().unwrap(), vec![cursor()]);
    }
}

#[test]
fn compressed_cursors_validate_logical_offsets_against_verified_frames() {
    let store = memory_store();
    let mut saved = cursor();
    saved.path = "session.jsonl.zstd".into();
    saved.offset = 10_000;
    saved.file_len = 100;
    saved.compressed = Some(CompressedCursor {
        verified_offset: 100,
        verified_decoded: 10_000,
        inspected_offset: 100,
        replay_offset: 100,
        replay_decoded: 10_000,
        waiting_for_change: true,
        ..Default::default()
    });
    store.append(&[], &saved).unwrap();
    assert_eq!(store.cursors().unwrap(), vec![saved.clone()]);
    let mut invalid = saved.clone();
    invalid.offset += 1;
    assert!(store.append(&[], &invalid).is_err());
    invalid = saved.clone();
    invalid.compressed.as_mut().unwrap().verified_offset += 1;
    assert!(store.append(&[], &invalid).is_err());
    invalid = saved.clone();
    invalid.compressed.as_mut().unwrap().replay_decoded += 1;
    assert!(store.append(&[], &invalid).is_err());
    invalid = saved.clone();
    invalid.compressed.as_mut().unwrap().replay_offset = 0;
    assert!(store.append(&[], &invalid).is_err());
    assert_eq!(store.cursors().unwrap(), vec![saved]);
}

#[test]
fn append_scrubs_before_persistence_and_rejects_invalid_inputs_atomically() {
    let store = memory_store();
    let mut record = usage_event("safe", "codex", 100, 20);
    record.title = "password=FIXTURE_PASSWORD".into();
    record.summary = Some(
        "authorization=FIXTURE_AUTH https://user:pass@host.test/path?token=FIXTURE_QUERY".into(),
    );
    record.usage.as_mut().unwrap().basis = "api_key=FIXTURE_BASIS".into();
    let inserted = store.append(&[record.clone()], &cursor()).unwrap();
    let persisted = serde_json::to_string(&store.load_events(None).unwrap()).unwrap();
    for secret in [
        "FIXTURE_PASSWORD",
        "FIXTURE_AUTH",
        "FIXTURE_QUERY",
        "FIXTURE_BASIS",
        "user:pass",
    ] {
        assert!(!persisted.contains(secret));
    }
    assert!(
        inserted[0]
            .usage
            .as_ref()
            .unwrap()
            .estimated_cost_usd
            .is_none()
    );
    let initial = store.cursors().unwrap();
    let mut invalid = cursor();
    invalid.path = "../auth.json".into();
    assert!(store.append(&[], &invalid).is_err());
    invalid = cursor();
    invalid.offset = invalid.file_len + 1;
    assert!(store.append(&[], &invalid).is_err());
    record.id = "invalid-usage".into();
    record.usage.as_mut().unwrap().input_tokens = -1;
    assert!(store.append(&[record], &cursor()).is_err());
    assert_eq!(store.history().unwrap().event_count, 1);
    assert_eq!(store.cursors().unwrap(), initial);
}
