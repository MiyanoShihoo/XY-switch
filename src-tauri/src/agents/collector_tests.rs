use super::*;
use crate::agents::harness;
use crate::database::Database;
use serde_json::json;
use std::io::Write;

struct Fixture {
    base: PathBuf,
    path: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let base = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("out/xy-switch_agents_20261110");
        let path = base.join(format!("collector-tests-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        Self { base, path }
    }

    fn logs(&self) -> PathBuf {
        self.path.join("logs")
    }

    fn store(&self) -> EventStore {
        EventStore::new(Database::open(&self.path.join("observation.sqlite")).unwrap()).unwrap()
    }

    fn root(&self, client: &str, adapter: ParseRecord) -> SourceRoot {
        SourceRoot {
            id: "fixture".into(),
            client_id: client.into(),
            path: self.logs(),
            adapter,
        }
    }

    fn write(&self, relative: &str, bytes: &[u8]) -> PathBuf {
        let path = self.logs().join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        assert_eq!(self.path.parent(), Some(self.base.as_path()));
        assert!(
            self.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("collector-tests-")
        );
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn line(record: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&record).unwrap();
    bytes.push(b'\n');
    bytes
}

fn append(path: &Path, bytes: &[u8]) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(bytes).unwrap();
    file.flush().unwrap();
}

fn zstd_frame(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).unwrap();
    encoder.include_checksum(true).unwrap();
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}

fn zstd_records(records: impl IntoIterator<Item = Value>) -> Vec<u8> {
    records
        .into_iter()
        .flat_map(|record| zstd_frame(&line(record)))
        .collect()
}

fn fixture_noise(length: usize) -> String {
    let mut value = 0x1234_5678u32;
    (0..length)
        .map(|_| {
            value ^= value << 13;
            value ^= value >> 17;
            value ^= value << 5;
            (b'a' + (value % 26) as u8) as char
        })
        .collect()
}

fn drain_history(collector: &mut Collector, mut now: i64) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    for _ in 0..100 {
        let result = collector.poll(now).unwrap();
        assert!(result.bytes_read + result.decoded_bytes <= MAX_POLL_BYTES as u64);
        assert!(
            result
                .sources
                .iter()
                .all(|source| source.connection == "connected")
        );
        events.extend(result.events);
        if result.pending_files == 0 {
            return events;
        }
        now += 2_000;
    }
    panic!("compressed fixture did not finish within its bounded work allowance");
}

fn fixture_adapter(record: &Value, ctx: &LineContext<'_>, state: &mut Value) -> Vec<AgentEvent> {
    let Some(id) = record["id"].as_str() else {
        return Vec::new();
    };
    *state = json!({"seen":state["seen"].as_u64().unwrap_or(0) + 1});
    vec![AgentEvent {
        id: format!("codex:fixture:{id}"),
        source_id: ctx.source_id.into(),
        source_path: ctx.source_path.into(),
        source_line: ctx.line_number,
        client_id: ctx.client_id.into(),
        agent_id: "codex:fixture".into(),
        session_id: "fixture".into(),
        timestamp: record["at"].as_i64().unwrap_or(1_000),
        observed_at: ctx.observed_at,
        kind: "agent_created".into(),
        title: record["title"].as_str().unwrap_or("Fixture record").into(),
        summary: record["summary"].as_str().map(str::to_owned),
        ..Default::default()
    }]
}

fn harness_header(id: &str, seeded: bool) -> Value {
    json!({"type":"session","version":4,"id":id,"createdAt":1_000,"isSeeded":seeded,"delegationDepth":0})
}

fn harness_record(seq: u64, kind: &str, data: Value) -> Value {
    json!({"type":kind,"seq":seq,"time":1_001 + seq,"data":data})
}

fn harness_usage(turn: u64, input: i64) -> Value {
    json!({"turn":turn,"step":1,"stream":[{"type":"chunk","time":1_002,
        "chunk":{"type":"usage","usage":{"inputTokens":input,"outputTokens":20,
        "cacheReadTokens":0,"cacheWriteTokens":0,"reasoningTokens":8}}}]})
}

#[test]
fn partial_utf8_json_restart_and_archive_replay_commit_only_complete_lines() {
    let fixture = Fixture::new();
    let first = line(json!({"id":"first"}));
    let second = line(json!({"id":"second","title":"开始"}));
    let split = second.iter().position(|byte| *byte >= 0x80).unwrap() + 1;
    let mut initial = first.clone();
    initial.extend_from_slice(&second[..split]);
    let path = fixture.write("rollout-live.jsonl", &initial);
    let root = fixture.root("codex", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let result = collector.poll(10_000).unwrap();
    assert_eq!(result.events.len(), 1);
    assert_eq!(store.cursors().unwrap()[0].offset, first.len() as u64);
    assert_eq!(fs::read(&path).unwrap(), initial);
    drop(collector);
    drop(store);

    let store = fixture.store();
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    append(&path, &second[split..second.len() - 1]);
    assert!(collector.poll(12_000).unwrap().events.is_empty());
    assert_eq!(store.cursors().unwrap()[0].offset, first.len() as u64);
    append(&path, b"\n");
    assert_eq!(collector.poll(14_000).unwrap().events.len(), 1);
    append(&path, &second);
    assert!(collector.poll(16_000).unwrap().events.is_empty());
    let saved = &store.cursors().unwrap()[0];
    assert_eq!(saved.line_number, 3);
    assert_eq!(saved.offset, fs::metadata(&path).unwrap().len());
    assert!(!collector.poll(18_000).unwrap().changed);
    fixture.write("archive/rollout-copy.jsonl", &fs::read(&path).unwrap());
    assert!(collector.poll(30_000).unwrap().events.is_empty());
    assert_eq!(store.load_events(None).unwrap().len(), 2);
    assert_eq!(store.cursors().unwrap().len(), 2);
}

#[test]
fn missing_silent_and_disappeared_roots_have_persistent_connection_states() {
    let fixture = Fixture::new();
    let root = fixture.root("codex", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    assert_eq!(
        collector.poll(1_000).unwrap().sources[0].connection,
        "unavailable"
    );
    fs::create_dir(fixture.logs()).unwrap();
    let connected = collector.poll(3_000).unwrap();
    assert_eq!(connected.sources[0].connection, "connected");
    assert_eq!(connected.sources[0].last_checked_at, Some(3_000));
    assert!(connected.sources[0].last_event_at.is_none());
    assert!(!collector.poll(5_000).unwrap().changed);
    fs::remove_dir(fixture.logs()).unwrap();
    assert_eq!(
        collector.poll(7_000).unwrap().sources[0].connection,
        "disconnected"
    );
    drop(collector);
    drop(store);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    assert_eq!(
        collector.poll(9_000).unwrap().sources[0].connection,
        "disconnected"
    );
    fs::create_dir(fixture.logs()).unwrap();
    assert_eq!(
        collector.poll(11_000).unwrap().sources[0].connection,
        "connected"
    );
    assert!(store.load_events(None).unwrap().is_empty());
}

#[test]
fn store_failure_retries_without_committing_the_file_cursor() {
    let fixture = Fixture::new();
    fixture.write("rollout-live.jsonl", &line(json!({"id":"retry"})));
    let store = fixture.store();
    store
        .database()
        .conn()
        .lock()
        .unwrap()
        .execute_batch(
            "CREATE TRIGGER agent_observation_test_abort BEFORE INSERT ON agent_observation_cursors
         BEGIN SELECT RAISE(ABORT, 'PRIVATE_SQL_ERROR'); END;",
        )
        .unwrap();
    let mut collector =
        Collector::new(vec![fixture.root("codex", fixture_adapter)], store.clone()).unwrap();
    let failed = collector.poll(1_000).unwrap();
    assert!(failed.events.is_empty());
    assert_eq!(failed.sources[0].connection, "disconnected");
    assert!(store.cursors().unwrap().is_empty());
    assert!(store.load_events(None).unwrap().is_empty());
    assert!(!failed.warnings.join(" ").contains("PRIVATE_SQL_ERROR"));
    store
        .database()
        .conn()
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER agent_observation_test_abort;")
        .unwrap();
    let recovered = collector.poll(3_000).unwrap();
    assert_eq!(recovered.events.len(), 1);
    assert_eq!(recovered.sources[0].connection, "connected");
    assert_eq!(store.cursors().unwrap()[0].line_number, 1);
}

#[test]
fn rotation_truncation_and_equal_size_rewrites_reset_safely() {
    let fixture = Fixture::new();
    let mut bytes = line(json!({"id":"alpha"}));
    bytes.extend(line(json!({"id":"bravo"})));
    let path = fixture.write("rollout-live.jsonl", &bytes);
    let store = fixture.store();
    let mut collector =
        Collector::new(vec![fixture.root("codex", fixture_adapter)], store.clone()).unwrap();
    assert_eq!(collector.poll(1_000).unwrap().events.len(), 2);
    let mut rewritten = line(json!({"id":"charl"}));
    rewritten.extend(line(json!({"id":"bravo"})));
    assert_eq!(rewritten.len(), bytes.len());
    fs::write(&path, &rewritten).unwrap();
    let result = collector.poll(17_000).unwrap();
    assert_eq!(result.events.len(), 1);
    assert!(!result.warnings.is_empty());
    fs::write(&path, line(json!({"id":"delta"}))).unwrap();
    assert_eq!(collector.poll(19_000).unwrap().events.len(), 1);
    let old_identity = store.cursors().unwrap()[0].file_identity.clone();
    fs::rename(&path, fixture.path.join("retired.txt")).unwrap();
    fs::write(&path, line(json!({"id":"echoo"}))).unwrap();
    assert_eq!(collector.poll(21_000).unwrap().events.len(), 1);
    assert_ne!(store.cursors().unwrap()[0].file_identity, old_identity);
    assert_eq!(store.load_events(None).unwrap().len(), 5);
}

#[test]
fn committed_tail_hash_detects_rewrites_past_the_header() {
    let fixture = Fixture::new();
    let header = line(json!({"id":"first","padding":"x".repeat(1_200)}));
    let mut bytes = header.clone();
    bytes.extend(line(json!({"id":"tail-one"})));
    let path = fixture.write("rollout-live.jsonl", &bytes);
    let store = fixture.store();
    let mut collector =
        Collector::new(vec![fixture.root("codex", fixture_adapter)], store.clone()).unwrap();
    assert_eq!(collector.poll(1_000).unwrap().events.len(), 2);
    let mut changed = header;
    changed.extend(line(json!({"id":"tail-two"})));
    assert_eq!(changed.len(), bytes.len());
    assert_eq!(&changed[..HEADER_BYTES], &bytes[..HEADER_BYTES]);
    fs::write(&path, &changed).unwrap();
    let result = collector.poll(17_000).unwrap();
    assert_eq!(result.events.len(), 1);
    assert!(result.events[0].id.ends_with("tail-two"));
    assert_eq!(fs::read(&path).unwrap(), changed);
}

#[test]
fn malformed_and_oversized_partial_lines_are_bounded_across_restart() {
    let fixture = Fixture::new();
    let mut bytes = b"{PRIVATE_MALFORMED_LINE\n \t\r\n\xff\n".to_vec();
    let committed = bytes.len();
    bytes.extend(vec![b'x'; MAX_FILE_BYTES * 3]);
    let path = fixture.write("rollout-live.jsonl", &bytes);
    let root = fixture.root("codex", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let first = collector.poll(1_000).unwrap();
    assert!(first.events.is_empty());
    assert!(first.bytes_read <= MAX_FILE_BYTES as u64);
    let cursor = &store.cursors().unwrap()[0];
    assert_eq!(cursor.offset, committed as u64);
    assert!(cursor.discard_bytes > 0);
    assert_eq!(cursor.malformed_lines, 2);
    drop(collector);
    drop(store);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    for round in 0..5 {
        let result = collector.poll(3_000 + round * 2_000).unwrap();
        assert!(result.events.is_empty());
        assert!(result.bytes_read <= MAX_FILE_BYTES as u64);
    }
    assert_eq!(store.cursors().unwrap()[0].offset, committed as u64);
    let mut suffix = b"\n".to_vec();
    suffix.extend(line(json!({"id":"after-long-line"})));
    append(&path, &suffix);
    let result = collector.poll(15_000).unwrap();
    assert_eq!(result.events.len(), 1);
    let cursor = &store.cursors().unwrap()[0];
    assert_eq!(cursor.malformed_lines, 3);
    assert_eq!(cursor.line_number, 5);
    assert_eq!(cursor.offset, fs::metadata(&path).unwrap().len());
    assert_eq!(cursor.discard_bytes, 0);
    assert!(
        !serde_json::to_string(cursor)
            .unwrap()
            .contains("PRIVATE_MALFORMED_LINE")
    );
}

#[test]
fn discovery_is_throttled_and_ignores_non_rollout_files() {
    let fixture = Fixture::new();
    for name in [
        "auth.json",
        "config.json",
        "other.jsonl",
        "rollout.jsonl",
        "rollout-.jsonl",
    ] {
        fixture.write(name, &line(json!({"id":"must-not-read"})));
    }
    let store = fixture.store();
    let mut collector =
        Collector::new(vec![fixture.root("codex", fixture_adapter)], store.clone()).unwrap();
    let result = collector.poll(1_000).unwrap();
    assert!(result.events.is_empty());
    assert_eq!(result.sources[0].file_count, 0);
    fixture.write("nested/rollout-new.jsonl", &line(json!({"id":"new-file"})));
    assert!(collector.poll(3_000).unwrap().events.is_empty());
    let result = collector.poll(16_000).unwrap();
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.sources[0].file_count, 1);
    assert_eq!(store.history().unwrap().event_count, 1);
}

#[test]
fn whole_poll_io_is_bounded_with_many_backlogged_files() {
    let fixture = Fixture::new();
    for index in 0..10 {
        fixture.write(
            &format!("rollout-{index}.jsonl"),
            &vec![b'x'; MAX_FILE_BYTES + MAX_LINE_BYTES],
        );
    }
    let store = fixture.store();
    let mut collector =
        Collector::new(vec![fixture.root("codex", fixture_adapter)], store).unwrap();
    let result = collector.poll(1_000).unwrap();
    assert!(result.bytes_read > MAX_FILE_BYTES as u64);
    assert!(result.bytes_read <= MAX_POLL_BYTES as u64);
    assert!(result.pending_files > 0);
    assert_eq!(result.sources[0].file_count, 10);
    assert_eq!(result.sources[0].connection, "connected");
}

#[test]
fn dsh_parser_gap_is_visible_after_restart_and_clears_on_repair_or_generation() {
    for compressed in [false, true] {
        let fixture = Fixture::new();
        let mut records = vec![
            harness_header("gap", false),
            harness_record(0, "turn/start", json!({"turn":1})),
            harness_record(
                1,
                "turn/end",
                json!({"turn":1,"reason":{"kind":"completed"}}),
            ),
            harness_record(3, "assistant/attempt", harness_usage(1, 30)),
            harness_record(4, "turn/start", json!({"turn":2})),
        ];
        let encode = |records: &[Value]| {
            if compressed {
                zstd_records(records.iter().cloned())
            } else {
                records.iter().cloned().flat_map(line).collect()
            }
        };
        let relative = if compressed {
            "project/session.jsonl.zstd"
        } else {
            "project/session.jsonl"
        };
        let path = fixture.write(relative, &encode(&records));
        let root = fixture.root("deepseek_harness", harness::parse_record);
        let store = fixture.store();
        let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
        let result = collector.poll(10_000).unwrap();
        let gap_reported = |result: &CollectResult| {
            assert_eq!(result.sources[0].connection, "connected");
            assert!(
                result
                    .warnings
                    .iter()
                    .any(|value| value.contains("序列缺口"))
            );
            assert!(result.sources[0].detail.contains("修复"));
            assert!(result.sources[0].capabilities.iter().any(|capability| {
                capability.feature == "parser_coverage" && capability.mode == "unsupported"
            }));
            assert!(
                !result
                    .events
                    .iter()
                    .any(|event| event.kind == "agent_failed")
            );
        };
        gap_reported(&result);
        assert!(
            result
                .events
                .iter()
                .any(|event| event.kind == "task_completed")
        );
        let terminal = result
            .events
            .iter()
            .find(|event| event.kind == "agent_waiting")
            .unwrap()
            .id
            .clone();
        drop(collector);
        let mut collector = Collector::new(vec![root], store.clone()).unwrap();
        let restarted = collector.poll(12_000).unwrap();
        gap_reported(&restarted);
        assert!(restarted.events.is_empty());
        records.insert(3, harness_record(2, "model/set", json!({})));
        fs::write(&path, encode(&records)).unwrap();
        let repaired = collector.poll(14_000).unwrap();
        assert_eq!(repaired.sources[0].connection, "connected");
        assert!(
            !repaired
                .warnings
                .iter()
                .any(|value| value.contains("序列缺口"))
        );
        assert!(
            repaired
                .events
                .iter()
                .any(|event| event.kind == "task_started" && event.id.contains(":seq:4:"))
        );
        assert!(
            store
                .load_events(None)
                .unwrap()
                .iter()
                .any(|event| event.id == terminal)
        );

        append(
            &path,
            &encode(&[harness_record(6, "turn/start", json!({"turn":3}))]),
        );
        gap_reported(&collector.poll(16_000).unwrap());
        let newest = if compressed {
            "project/session.v1.jsonl.zstd"
        } else {
            "project/session.v1.jsonl"
        };
        fixture.write(newest, &encode(&records));
        let next_generation = collector.poll(30_000).unwrap();
        assert!(
            !next_generation.sources[0]
                .capabilities
                .iter()
                .any(|capability| capability.feature == "parser_coverage")
        );
        assert!(
            !next_generation
                .warnings
                .iter()
                .any(|value| value.contains("序列缺口"))
        );
    }
}

#[test]
fn dsh_partial_and_unsupported_parser_state_has_persistent_capabilities() {
    let fixture = Fixture::new();
    let mut records = vec![
        harness_header("partial", false),
        harness_record(0, "subagent/descriptor", json!({"version":99})),
        harness_record(1, "team/member", json!({})),
    ];
    for seq in 2..68 {
        records.push(harness_record(seq, "turn/start", json!({"turn":seq})));
    }
    fixture.write("session.jsonl.zstd", &zstd_records(records));
    let root = fixture.root("deepseek_harness", harness::parse_record);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    for restart in [false, true] {
        if restart {
            collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
        }
        let result = collector.poll(10_000).unwrap();
        assert_eq!(result.sources[0].connection, "connected");
        for feature in [
            "complete_relationships",
            "team_protocol",
            "model_descriptor",
        ] {
            let capability = result.sources[0]
                .capabilities
                .iter()
                .find(|capability| capability.feature == feature)
                .unwrap();
            assert_eq!(capability.mode, "unsupported");
            assert!(result.warnings.contains(&capability.reason));
        }
    }
}

#[test]
fn real_harness_adapter_preserves_slot_revisions_through_restart() {
    let fixture = Fixture::new();
    let mut records = vec![
        harness_header("parent", false),
        harness_record(0, "turn/start", json!({"turn":1})),
        harness_record(1, "assistant/attempt", harness_usage(1, 30)),
        harness_record(2, "assistant/attempt", harness_usage(1, 40)),
    ];
    records[3]["time"] = json!(900);
    let bytes: Vec<_> = records.into_iter().flat_map(line).collect();
    let path = fixture.write("project/session.v4.jsonl", &bytes);
    let root = fixture.root("deepseek_harness", harness::parse_record);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let events = collector.poll(10_000).unwrap().events;
    let usage: Vec<_> = events
        .iter()
        .filter_map(|event| event.usage.as_ref())
        .collect();
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0].key, usage[1].key);
    drop(collector);
    drop(store);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    assert!(collector.poll(12_000).unwrap().events.is_empty());
    append(
        &path,
        &line(harness_record(3, "assistant/attempt", harness_usage(1, 50))),
    );
    let result = collector.poll(14_000).unwrap();
    assert_eq!(
        result
            .events
            .iter()
            .filter(|event| event.usage.is_some())
            .count(),
        1
    );
    let stored = store.load_events(None).unwrap();
    assert_eq!(
        stored.iter().filter(|event| event.usage.is_some()).count(),
        3
    );
    fixture.write("archive/session.v4.jsonl", &fs::read(&path).unwrap());
    assert!(collector.poll(30_000).unwrap().events.is_empty());
}

#[test]
fn seeded_prescan_is_restartable_and_uses_the_last_marker_before_replay() {
    let fixture = Fixture::new();
    let padding = "FIXTURE_SEED_ARGUMENT ".repeat(9_000);
    let mut inherited_usage = harness_usage(1, 999);
    inherited_usage["arguments"] = json!(padding);
    let records = vec![
        harness_header("child", true),
        harness_record(0, "session/end-seed", json!({"inherited":true})),
        harness_record(1, "turn/start", json!({"turn":1,"arguments":padding})),
        harness_record(2, "assistant/attempt", inherited_usage),
        harness_record(3, "fixture/ignored", json!({"arguments":padding})),
        harness_record(4, "session/end-seed", json!({"inherited":true})),
        harness_record(5, "turn/end", json!({"turn":1,"reason":{"kind":"forked"}})),
        harness_record(6, "turn/start", json!({"turn":2})),
        harness_record(7, "assistant/attempt", harness_usage(2, 800)),
    ];
    let bytes: Vec<_> = records.into_iter().flat_map(line).collect();
    assert!(bytes.len() > MAX_FILE_BYTES);
    let path = fixture.write("session.jsonl", &bytes);
    let root = fixture.root("deepseek_harness", harness::parse_record);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    assert!(collector.poll(10_000).unwrap().events.is_empty());
    let cursor = &store.cursors().unwrap()[0];
    assert_eq!(cursor.offset, 0);
    let scan = cursor.seed_scan.as_ref().unwrap();
    assert!(scan.offset > 0 && !scan.complete);
    assert!(
        !serde_json::to_string(cursor)
            .unwrap()
            .contains("FIXTURE_SEED_ARGUMENT")
    );
    drop(collector);
    drop(store);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    assert!(collector.poll(12_000).unwrap().events.is_empty());
    let cursor = &store.cursors().unwrap()[0];
    assert!(cursor.seed_scan.as_ref().unwrap().complete);
    assert_eq!(
        cursor.parser_state["seedBoundary"],
        json!({"sessionId":"child","seq":4})
    );
    let extension = vec![
        harness_record(8, "session/end-seed", json!({"inherited":true})),
        harness_record(9, "turn/end", json!({"turn":2,"reason":{"kind":"forked"}})),
        harness_record(10, "turn/start", json!({"turn":3})),
        harness_record(11, "assistant/attempt", harness_usage(3, 40)),
        harness_record(12, "assistant/attempt", harness_usage(3, 50)),
    ];
    append(
        &path,
        &extension.into_iter().flat_map(line).collect::<Vec<_>>(),
    );
    assert!(collector.poll(14_000).unwrap().events.is_empty());
    assert_eq!(
        store.cursors().unwrap()[0].parser_state["seedBoundary"],
        json!({"sessionId":"child","seq":8})
    );
    for round in 0..4 {
        assert!(
            collector.poll(16_000 + round * 2_000).unwrap().bytes_read <= MAX_FILE_BYTES as u64
        );
    }
    let records = store.load_events(None).unwrap();
    let usage: Vec<_> = records
        .iter()
        .filter_map(|event| event.usage.as_ref())
        .collect();
    assert_eq!(usage.len(), 2);
    assert!(
        usage
            .iter()
            .all(|usage| usage.key == "deepseek_harness:child:usage:11")
    );
    assert_eq!(
        usage
            .iter()
            .map(|usage| usage.input_tokens)
            .collect::<Vec<_>>(),
        vec![40, 50]
    );
    assert_eq!(
        store.cursors().unwrap()[0].offset,
        fs::metadata(&path).unwrap().len()
    );
}

#[test]
fn seeded_logs_wait_for_a_complete_boundary_line() {
    let fixture = Fixture::new();
    let records = vec![
        harness_header("child", true),
        harness_record(0, "turn/start", json!({"turn":1})),
        harness_record(1, "assistant/attempt", harness_usage(1, 999)),
    ];
    let path = fixture.write(
        "session.jsonl",
        &records.into_iter().flat_map(line).collect::<Vec<_>>(),
    );
    let store = fixture.store();
    let mut collector = Collector::new(
        vec![fixture.root("deepseek_harness", harness::parse_record)],
        store.clone(),
    )
    .unwrap();
    assert!(collector.poll(1_000).unwrap().events.is_empty());
    let marker = line(harness_record(
        2,
        "session/end-seed",
        json!({"inherited":true}),
    ));
    append(&path, &marker[..marker.len() - 1]);
    assert!(collector.poll(3_000).unwrap().events.is_empty());
    assert!(
        !store.cursors().unwrap()[0]
            .seed_scan
            .as_ref()
            .unwrap()
            .complete
    );
    append(&path, b"\n");
    append(
        &path,
        &line(harness_record(3, "turn/start", json!({"turn":2}))),
    );
    append(
        &path,
        &line(harness_record(4, "assistant/attempt", harness_usage(2, 30))),
    );
    assert!(collector.poll(5_000).unwrap().events.is_empty());
    let result = collector.poll(7_000).unwrap();
    let usage: Vec<_> = result
        .events
        .iter()
        .filter_map(|event| event.usage.as_ref())
        .collect();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].input_tokens, 30);
}

#[test]
fn newest_compressed_generation_imports_history_without_falling_back() {
    let fixture = Fixture::new();
    fixture.write("project/session.jsonl", &line(harness_header("old", false)));
    fixture.write(
        "project/session.v2.jsonl",
        &line(harness_header("selected", false)),
    );
    fixture.write(
        "project/session.v3.jsonl",
        &line(harness_header("plain-shadowed", false)),
    );
    let compressed = fixture.write(
        "project/session.v3.jsonl.zst",
        &zstd_records([harness_header("compressed", false)]),
    );
    let store = fixture.store();
    let mut collector = Collector::new(
        vec![fixture.root("deepseek_harness", harness::parse_record)],
        store.clone(),
    )
    .unwrap();
    let result = collector.poll(1_000).unwrap();
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].agent_id, "deepseek_harness:compressed");
    assert_eq!(
        store.cursors().unwrap()[0].path,
        "project/session.v3.jsonl.zst"
    );
    assert_eq!(result.sources[0].connection, "connected");
    assert_eq!(result.sources[0].mode, "history");
    assert_eq!(
        result.sources[0]
            .capabilities
            .iter()
            .find(|capability| capability.feature == "compressed_history")
            .unwrap()
            .mode,
        "history"
    );
    fs::remove_file(compressed).unwrap();
    let result = collector.poll(16_000).unwrap();
    assert_eq!(result.events.len(), 1);
    assert_eq!(result.events[0].agent_id, "deepseek_harness:plain-shadowed");
    assert_eq!(result.events[0].source_path, "project/session.v3.jsonl");
}

#[test]
fn compressed_multiframe_harness_revisions_restart_and_archive_are_idempotent() {
    let fixture = Fixture::new();
    let records = [
        harness_header("compressed-parent", false),
        harness_record(0, "turn/start", json!({"turn":1})),
        harness_record(1, "assistant/attempt", harness_usage(1, 30)),
        harness_record(2, "assistant/attempt", harness_usage(1, 40)),
    ];
    let path = fixture.write("live/session.v2.jsonl.zstd", &zstd_records(records));
    let root = fixture.root("deepseek_harness", harness::parse_record);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let first = collector.poll(10_000).unwrap();
    let usage: Vec<_> = first
        .events
        .iter()
        .filter_map(|event| event.usage.as_ref())
        .collect();
    assert_eq!(usage.len(), 2);
    assert_eq!(usage[0].key, usage[1].key);
    drop(collector);
    drop(store);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    let unchanged = collector.poll(12_000).unwrap();
    assert!(unchanged.events.is_empty());
    assert_eq!(unchanged.decoded_bytes, 0);
    append(
        &path,
        &zstd_records([harness_record(3, "assistant/attempt", harness_usage(1, 50))]),
    );
    let next = collector.poll(14_000).unwrap();
    assert_eq!(
        next.events
            .iter()
            .filter(|event| event.usage.is_some())
            .count(),
        1
    );
    fixture.write("archive/session.v2.jsonl.zstd", &fs::read(&path).unwrap());
    assert!(collector.poll(30_000).unwrap().events.is_empty());
    let stored = store.load_events(None).unwrap();
    let usage: Vec<_> = stored
        .iter()
        .filter_map(|event| event.usage.as_ref())
        .collect();
    assert_eq!(
        usage
            .iter()
            .map(|usage| usage.input_tokens)
            .collect::<Vec<_>>(),
        vec![30, 40, 50]
    );
    assert!(usage.iter().all(|sample| sample.key == usage[0].key));
}

fn verify_large_harness_headers_survive_restart(compressed: bool) {
    let fixture = Fixture::new();
    let secret = "LARGE_HEADER_CREDENTIAL_MUST_NOT_BE_STORED";
    let padding = fixture_noise(310_402);
    let header = |seq| {
        harness_record(seq, "request/header", json!({"header":{
            "config":{"model":"large-header-model","provider":"fixture"},
            "authorization":secret,"padding":padding
        }}))
    };
    let records = vec![
        harness_header("large-header-parent", false),
        harness_record(0, "turn/start", json!({"turn":1})),
        header(1),
        header(2),
        harness_record(3, "assistant/attempt", harness_usage(1, 50)),
    ];
    let header_bytes = line(records[2].clone());
    assert!(header_bytes.len() > 256 * 1024);
    assert!(header_bytes.len() < MAX_LINE_BYTES);
    let bytes = if compressed {
        zstd_records(records)
    } else {
        records.into_iter().flat_map(line).collect()
    };
    let relative = if compressed {
        "live/session.v4.jsonl.zstd"
    } else {
        "live/session.v4.jsonl"
    };
    let path = fixture.write(relative, &bytes);
    let root = fixture.root("deepseek_harness", harness::parse_record);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let first = collector.poll(10_000).unwrap();
    assert!(first.bytes_read + first.decoded_bytes <= MAX_FILE_BYTES as u64);
    assert!(first.pending_files > 0);
    drop(collector);
    drop(store);

    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    drain_history(&mut collector, 12_000);
    let stored = store.load_events(None).unwrap();
    let usage: Vec<_> = stored.iter().filter_map(|event| event.usage.as_ref()).collect();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].input_tokens, 50);
    assert_eq!(usage[0].output_tokens, 20);
    let cursor = store.cursors().unwrap().remove(0);
    assert_eq!(cursor.parser_state["lastSeq"], json!(3));
    assert!(cursor.parser_state["blocked"].is_null());
    assert_eq!(cursor.malformed_lines, 0);
    for persisted in [
        serde_json::to_string(&stored).unwrap(),
        serde_json::to_string(&cursor.parser_state).unwrap(),
    ] {
        assert!(!persisted.contains(secret));
        assert!(!persisted.contains(&padding[..200]));
    }
    assert_eq!(fs::read(path).unwrap(), bytes);
    drop(collector);
    drop(store);

    let store = fixture.store();
    let mut collector = Collector::new(vec![root], store).unwrap();
    assert!(collector.poll(250_000).unwrap().events.is_empty());
}

#[test]
fn large_plain_harness_headers_keep_sequence_usage_and_privacy_through_restart() {
    verify_large_harness_headers_survive_restart(false);
}

#[test]
fn large_compressed_harness_headers_keep_sequence_usage_and_privacy_through_restart() {
    verify_large_harness_headers_survive_restart(true);
}

#[test]
fn compressed_torn_frame_waits_without_importing_its_complete_json_lines() {
    let fixture = Fixture::new();
    let first = zstd_frame(&line(json!({"id":"verified"})));
    let second = zstd_frame(&line(json!({"id":"unverified"})));
    let split = second.len() - 3;
    let initial = [first.as_slice(), &second[..split]].concat();
    let path = fixture.write("session.jsonl.zstd", &initial);
    let root = fixture.root("deepseek_harness", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let first_poll = collector.poll(1_000).unwrap();
    assert_eq!(first_poll.events.len(), 1);
    assert_eq!(first_poll.events[0].id, "codex:fixture:verified");
    let saved = store.cursors().unwrap().remove(0);
    let compressed = saved.compressed.as_ref().unwrap();
    assert!(compressed.incomplete);
    assert_eq!(compressed.verified_offset, first.len() as u64);
    assert_eq!(first_poll.sources[0].connection, "connected");
    drop(collector);
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    let unchanged = collector.poll(3_000).unwrap();
    assert_eq!(unchanged.decoded_bytes, 0);
    assert!(unchanged.events.is_empty());
    append(&path, &second[split..]);
    let completed = collector.poll(5_000).unwrap();
    assert_eq!(completed.events.len(), 1);
    assert_eq!(completed.events[0].id, "codex:fixture:unverified");
    assert!(
        !store.cursors().unwrap()[0]
            .compressed
            .as_ref()
            .unwrap()
            .incomplete
    );
    assert_eq!(fs::read(path).unwrap(), [first, second].concat());
}

#[test]
fn compressed_checksum_failure_stops_at_verified_prefix_and_recovers_on_rewrite() {
    let fixture = Fixture::new();
    let good = zstd_frame(&line(json!({"id":"good"})));
    let middle = zstd_frame(&line(
        json!({"id":"middle","arguments":"PRIVATE_CORRUPT_FRAME"}),
    ));
    let mut bad = middle.clone();
    *bad.last_mut().unwrap() ^= 0x80;
    let last = zstd_frame(&line(json!({"id":"last"})));
    let path = fixture.write(
        "session.jsonl.zstd",
        &[good.as_slice(), bad.as_slice(), last.as_slice()].concat(),
    );
    let store = fixture.store();
    let mut collector = Collector::new(
        vec![fixture.root("deepseek_harness", fixture_adapter)],
        store.clone(),
    )
    .unwrap();
    let failed = collector.poll(1_000).unwrap();
    assert_eq!(failed.events.len(), 1);
    assert_eq!(failed.sources[0].connection, "connected");
    assert!(!failed.warnings.join(" ").contains("PRIVATE_CORRUPT_FRAME"));
    assert!(
        store.cursors().unwrap()[0]
            .compressed
            .as_ref()
            .unwrap()
            .failed
    );
    assert_eq!(collector.poll(17_000).unwrap().decoded_bytes, 0);
    fs::write(&path, [good, middle, last].concat()).unwrap();
    File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(100))
        .unwrap();
    let recovered = collector.poll(19_000).unwrap();
    assert_eq!(recovered.events.len(), 2);
    assert!(
        !store.cursors().unwrap()[0]
            .compressed
            .as_ref()
            .unwrap()
            .failed
    );
    assert_eq!(store.load_events(None).unwrap().len(), 3);
}

#[test]
fn compressed_large_single_frame_resumes_bounded_validation_and_parser_replay() {
    let fixture = Fixture::new();
    let data: Vec<_> = (0..12)
        .flat_map(|index| {
            line(json!({
                "id":format!("large-{index}"), "arguments":"MEMORY_ONLY_ARGUMENT ".repeat(8_000)
            }))
        })
        .collect();
    let packed = zstd_frame(&data);
    fixture.write("session.jsonl.zstd", &packed);
    let root = fixture.root("deepseek_harness", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let first = collector.poll(1_000).unwrap();
    assert!(first.events.is_empty());
    assert!(first.decoded_bytes > 0);
    assert!(first.bytes_read + first.decoded_bytes <= MAX_FILE_BYTES as u64);
    assert_eq!(
        store.cursors().unwrap()[0]
            .compressed
            .as_ref()
            .unwrap()
            .verified_offset,
        0
    );
    drop(collector);
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let mut restarted_replay = false;
    let mut finished = false;
    for round in 1..100 {
        let result = collector.poll(1_000 + round * 2_000).unwrap();
        assert!(result.bytes_read + result.decoded_bytes <= MAX_FILE_BYTES as u64);
        assert_eq!(result.sources[0].connection, "connected");
        if !result.events.is_empty() && !restarted_replay {
            assert!(result.pending_files > 0);
            collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
            restarted_replay = true;
        } else if result.pending_files == 0 {
            finished = true;
            break;
        }
    }
    assert!(finished && restarted_replay);
    assert_eq!(store.load_events(None).unwrap().len(), 12);
    let cursor = store.cursors().unwrap().remove(0);
    assert_eq!(cursor.offset, data.len() as u64);
    assert_eq!(cursor.file_len, packed.len() as u64);
    assert_eq!(cursor.parser_state["seen"], 12);
    assert!(
        !serde_json::to_string(&cursor)
            .unwrap()
            .contains("MEMORY_ONLY_ARGUMENT")
    );
    let mut collector = Collector::new(vec![root], store).unwrap();
    assert_eq!(collector.poll(300_000).unwrap().decoded_bytes, 0);
}

#[test]
fn compressed_restart_uses_frame_checkpoint_instead_of_redecoding_prior_history() {
    let fixture = Fixture::new();
    let padding = fixture_noise(96_000);
    let bytes = zstd_records(
        (0..32).map(|index| json!({"id":format!("frame-{index}"),"arguments":padding})),
    );
    fixture.write("session.jsonl.zstd", &bytes);
    let store = fixture.store();
    let root = fixture.root("deepseek_harness", fixture_adapter);
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    for round in 0..8 {
        let result = collector.poll(1_000 + round * 2_000).unwrap();
        assert!(result.bytes_read + result.decoded_bytes <= MAX_FILE_BYTES as u64);
    }
    let before = store.cursors().unwrap().remove(0);
    assert!(before.compressed.as_ref().unwrap().replay_decoded > MAX_FILE_BYTES as u64);
    drop(collector);
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    let next = collector.poll(17_000).unwrap();
    assert!(!next.events.is_empty());
    assert!(store.cursors().unwrap()[0].offset > before.offset);
    drain_history(&mut collector, 19_000);
    assert_eq!(store.load_events(None).unwrap().len(), 32);
}

#[test]
fn compressed_partial_utf8_json_across_frames_waits_through_restart() {
    let fixture = Fixture::new();
    let first = line(json!({"id":"first"}));
    let second = line(json!({"id":"second","title":"开始"}));
    let split = second.iter().position(|byte| *byte >= 0x80).unwrap() + 1;
    let path = fixture.write(
        "session.jsonl.zstd",
        &zstd_frame(&[first.as_slice(), &second[..split]].concat()),
    );
    let store = fixture.store();
    let root = fixture.root("deepseek_harness", fixture_adapter);
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    assert_eq!(collector.poll(1_000).unwrap().events.len(), 1);
    assert_eq!(store.cursors().unwrap()[0].offset, first.len() as u64);
    drop(collector);
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    assert_eq!(collector.poll(3_000).unwrap().decoded_bytes, 0);
    append(&path, &zstd_frame(&second[split..second.len() - 1]));
    assert!(collector.poll(5_000).unwrap().events.is_empty());
    append(&path, &zstd_frame(b"\n"));
    let completed = collector.poll(7_000).unwrap();
    assert_eq!(completed.events.len(), 1);
    assert_eq!(completed.events[0].source_line, 2);
    assert_eq!(
        store.cursors().unwrap()[0].offset,
        (first.len() + second.len()) as u64
    );
}

#[test]
fn compressed_seed_boundary_prescan_survives_restart_and_new_last_marker() {
    let fixture = Fixture::new();
    let padding = "COMPRESSED_SEED_ARGUMENT ".repeat(7_000);
    let records = [
        harness_header("compressed-child", true),
        harness_record(0, "session/end-seed", json!({"inherited":true})),
        harness_record(1, "turn/start", json!({"turn":1,"arguments":padding})),
        harness_record(2, "assistant/attempt", harness_usage(1, 999)),
        harness_record(3, "fixture/ignored", json!({"arguments":padding})),
        harness_record(4, "session/end-seed", json!({"inherited":true})),
        harness_record(5, "turn/end", json!({"turn":1,"reason":{"kind":"forked"}})),
        harness_record(6, "turn/start", json!({"turn":2,"arguments":padding})),
        harness_record(7, "assistant/attempt", harness_usage(2, 800)),
    ];
    let path = fixture.write("session.jsonl.zstd", &zstd_records(records));
    let root = fixture.root("deepseek_harness", harness::parse_record);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    let first = collector.poll(1_000).unwrap();
    assert!(first.events.is_empty());
    let saved = store.cursors().unwrap().remove(0);
    assert!(!saved.seed_scan.as_ref().unwrap().complete);
    assert!(saved.seed_scan.as_ref().unwrap().offset > 0);
    assert_eq!(saved.offset, 0);
    drop(collector);
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    let mut now = 3_000;
    for _ in 0..30 {
        assert!(collector.poll(now).unwrap().events.is_empty());
        now += 2_000;
        if store.cursors().unwrap()[0]
            .seed_scan
            .as_ref()
            .unwrap()
            .complete
        {
            break;
        }
    }
    assert_eq!(
        store.cursors().unwrap()[0].parser_state["seedBoundary"],
        json!({"sessionId":"compressed-child","seq":4})
    );
    append(
        &path,
        &zstd_records([
            harness_record(8, "session/end-seed", json!({"inherited":true})),
            harness_record(9, "turn/end", json!({"turn":2,"reason":{"kind":"forked"}})),
            harness_record(10, "turn/start", json!({"turn":3})),
            harness_record(11, "assistant/attempt", harness_usage(3, 40)),
            harness_record(12, "assistant/attempt", harness_usage(3, 50)),
        ]),
    );
    assert!(collector.poll(now).unwrap().events.is_empty());
    assert_eq!(
        store.cursors().unwrap()[0].parser_state["seedBoundary"],
        json!({"sessionId":"compressed-child","seq":8})
    );
    drain_history(&mut collector, now + 2_000);
    let events = store.load_events(None).unwrap();
    let usage: Vec<_> = events
        .iter()
        .filter_map(|event| event.usage.as_ref())
        .collect();
    assert_eq!(
        usage
            .iter()
            .map(|usage| usage.input_tokens)
            .collect::<Vec<_>>(),
        vec![40, 50]
    );
    assert!(
        usage
            .iter()
            .all(|usage| usage.key == "deepseek_harness:compressed-child:usage:11")
    );
    assert!(
        !serde_json::to_string(&store.cursors().unwrap())
            .unwrap()
            .contains("COMPRESSED_SEED_ARGUMENT")
    );
}

#[test]
fn compressed_seed_does_not_choose_an_earlier_marker_before_partial_last_line() {
    let fixture = Fixture::new();
    let marker = line(harness_record(
        3,
        "session/end-seed",
        json!({"inherited":true}),
    ));
    let mut packed = zstd_records([
        harness_header("seed-lines", true),
        harness_record(0, "session/end-seed", json!({"inherited":true})),
        harness_record(1, "turn/start", json!({"turn":1})),
        harness_record(2, "assistant/attempt", harness_usage(1, 999)),
    ]);
    packed.extend(zstd_frame(&marker[..marker.len() - 1]));
    let path = fixture.write("session.jsonl.zstd", &packed);
    let store = fixture.store();
    let root = fixture.root("deepseek_harness", harness::parse_record);
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    assert!(collector.poll(1_000).unwrap().events.is_empty());
    assert!(
        !store.cursors().unwrap()[0]
            .seed_scan
            .as_ref()
            .unwrap()
            .complete
    );
    drop(collector);
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    assert_eq!(collector.poll(3_000).unwrap().decoded_bytes, 0);
    append(&path, &zstd_frame(b"\n"));
    append(
        &path,
        &zstd_records([
            harness_record(4, "turn/end", json!({"turn":1,"reason":{"kind":"forked"}})),
            harness_record(5, "turn/start", json!({"turn":2})),
            harness_record(6, "assistant/attempt", harness_usage(2, 30)),
        ]),
    );
    assert!(collector.poll(5_000).unwrap().events.is_empty());
    assert_eq!(
        store.cursors().unwrap()[0].parser_state["seedBoundary"]["seq"],
        3
    );
    let events = drain_history(&mut collector, 7_000);
    let usage: Vec<_> = events
        .iter()
        .filter_map(|event| event.usage.as_ref())
        .collect();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].input_tokens, 30);
}

#[test]
fn compressed_sqlite_failure_discards_uncommitted_decoder_state() {
    let fixture = Fixture::new();
    fixture.write(
        "session.jsonl.zstd",
        &zstd_records([json!({"id":"retry-compressed"})]),
    );
    let store = fixture.store();
    store.database().conn().lock().unwrap().execute_batch(
        "CREATE TRIGGER agent_observation_compressed_abort BEFORE INSERT ON agent_observation_cursors
         BEGIN SELECT RAISE(ABORT, 'PRIVATE_COMPRESSED_SQL_ERROR'); END;"
    ).unwrap();
    let mut collector = Collector::new(
        vec![fixture.root("deepseek_harness", fixture_adapter)],
        store.clone(),
    )
    .unwrap();
    let failed = collector.poll(1_000).unwrap();
    assert!(failed.events.is_empty());
    assert!(store.cursors().unwrap().is_empty());
    assert!(store.load_events(None).unwrap().is_empty());
    assert!(collector.compressed.is_empty());
    assert!(
        !failed
            .warnings
            .join(" ")
            .contains("PRIVATE_COMPRESSED_SQL_ERROR")
    );
    store
        .database()
        .conn()
        .lock()
        .unwrap()
        .execute_batch("DROP TRIGGER agent_observation_compressed_abort;")
        .unwrap();
    let recovered = collector.poll(3_000).unwrap();
    assert_eq!(recovered.events.len(), 1);
    assert_eq!(store.cursors().unwrap()[0].parser_state["seen"], 1);
}

#[test]
fn compressed_oversized_line_can_be_discarded_incrementally_without_raw_spooling() {
    let fixture = Fixture::new();
    let packed: Vec<_> = (0..8)
        .flat_map(|_| zstd_frame(&vec![b'x'; 128 * 1024]))
        .collect();
    let path = fixture.write("session.jsonl.zstd", &packed);
    let root = fixture.root("deepseek_harness", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    drain_history(&mut collector, 1_000);
    let saved = store.cursors().unwrap().remove(0);
    assert_eq!(saved.offset, 0);
    assert_eq!(saved.seed_scan.as_ref().unwrap().discard_bytes, 1024 * 1024);
    drop(collector);
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    append(
        &path,
        &zstd_frame(&[b"\n".as_slice(), &line(json!({"id":"after-long-line"}))].concat()),
    );
    let resumed = collector.poll(31_000).unwrap();
    assert_eq!(resumed.events.len(), 1);
    assert_eq!(store.cursors().unwrap()[0].malformed_lines, 1);
    assert_eq!(store.cursors().unwrap()[0].line_number, 2);
    assert_eq!(fs::read_dir(fixture.logs()).unwrap().count(), 1);
}

#[test]
fn compressed_empty_frames_and_many_files_share_decode_and_io_budget() {
    let fixture = Fixture::new();
    let empty = zstd_frame(b"");
    let mut packed: Vec<_> = (0..MAX_ZSTD_STEPS + 20)
        .flat_map(|_| empty.clone())
        .collect();
    packed.extend(zstd_records([json!({"id":"after-empty-frames"})]));
    fixture.write("empty/session.jsonl.zstd", &packed);
    for index in 0..12 {
        let bytes = zstd_records((0..8).map(
            |line| json!({"id":format!("many-{index}-{line}"),"arguments":"x".repeat(192_000)}),
        ));
        fixture.write(&format!("many-{index}/session.jsonl.zstd"), &bytes);
    }
    let store = fixture.store();
    let mut collector = Collector::new(
        vec![fixture.root("deepseek_harness", fixture_adapter)],
        store.clone(),
    )
    .unwrap();
    let first = collector.poll(1_000).unwrap();
    assert!(first.pending_files > 0);
    assert!(first.decoded_bytes > 0);
    assert!(first.bytes_read + first.decoded_bytes <= MAX_POLL_BYTES as u64);
    assert!(collector.compressed.len() <= MAX_COMPRESSED_STREAMS);
    drain_history(&mut collector, 3_000);
    assert_eq!(store.load_events(None).unwrap().len(), 97);
}

#[test]
fn compressed_window_limit_is_reported_without_disconnect_or_unverified_records() {
    let fixture = Fixture::new();
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 1).unwrap();
    encoder.window_log(ZSTD_WINDOW_LOG + 1).unwrap();
    encoder.write_all(&vec![b' '; 9 * 1024 * 1024]).unwrap();
    let packed = encoder.finish().unwrap();
    fixture.write("session.jsonl.zstd", &packed);
    let store = fixture.store();
    let mut collector = Collector::new(
        vec![fixture.root("deepseek_harness", fixture_adapter)],
        store.clone(),
    )
    .unwrap();
    let result = collector.poll(1_000).unwrap();
    assert!(result.events.is_empty());
    assert_eq!(result.sources[0].connection, "connected");
    assert!(
        store.cursors().unwrap()[0]
            .compressed
            .as_ref()
            .unwrap()
            .failed
    );
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("窗口"))
    );
    assert_eq!(collector.poll(17_000).unwrap().decoded_bytes, 0);
}

#[test]
fn compressed_truncation_and_identity_replacement_reset_without_duplicate_events() {
    let fixture = Fixture::new();
    let initial = zstd_records([json!({"id":"first"}), json!({"id":"second"})]);
    let path = fixture.write("session.jsonl.zstd", &initial);
    let root = fixture.root("deepseek_harness", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    assert_eq!(drain_history(&mut collector, 1_000).len(), 2);
    fs::write(&path, zstd_records([json!({"id":"first"})])).unwrap();
    let truncated = collector.poll(3_000).unwrap();
    assert!(truncated.events.is_empty());
    assert!(
        truncated
            .warnings
            .iter()
            .any(|value| value.contains("重置"))
    );
    let identity = store.cursors().unwrap()[0].file_identity.clone();
    let retired = fixture.path.join("retired.zstd");
    assert_eq!(retired.parent(), Some(fixture.path.as_path()));
    fs::rename(&path, &retired).unwrap();
    let replacement = zstd_records([json!({"id":"first"}), json!({"id":"third"})]);
    fs::write(&path, &replacement).unwrap();
    drop(collector);
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    let events = drain_history(&mut collector, 5_000);
    assert_eq!(events.len(), 1);
    assert!(events[0].id.ends_with("third"));
    assert_ne!(store.cursors().unwrap()[0].file_identity, identity);
    assert_eq!(store.load_events(None).unwrap().len(), 3);
    assert_eq!(fs::read(path).unwrap(), replacement);
}

#[test]
fn compressed_change_during_read_retries_without_false_disconnect() {
    static CHANGING_LOG: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);
    fn changing_adapter(
        record: &Value,
        ctx: &LineContext<'_>,
        state: &mut Value,
    ) -> Vec<AgentEvent> {
        if let Some(path) = CHANGING_LOG.lock().unwrap().take() {
            append(&path, &zstd_records([json!({"id":"second"})]));
        }
        fixture_adapter(record, ctx, state)
    }
    let fixture = Fixture::new();
    let path = fixture.write("session.jsonl.zstd", &zstd_records([json!({"id":"first"})]));
    *CHANGING_LOG.lock().unwrap() = Some(path);
    let store = fixture.store();
    let mut collector = Collector::new(
        vec![fixture.root("deepseek_harness", changing_adapter)],
        store.clone(),
    )
    .unwrap();
    let changed = collector.poll(1_000).unwrap();
    assert_eq!(changed.sources[0].connection, "connected");
    assert!(changed.events.is_empty());
    assert_eq!(changed.pending_files, 1);
    assert!(changed.warnings.iter().any(|value| value.contains("变动")));
    assert!(store.cursors().unwrap().is_empty());
    assert!(store.load_events(None).unwrap().is_empty());
    assert_eq!(drain_history(&mut collector, 3_000).len(), 2);
    assert_eq!(store.cursors().unwrap()[0].parser_state["seen"], json!(2));
}

#[test]
fn compressed_backlog_does_not_starve_a_newer_log_when_cache_is_full() {
    let fixture = Fixture::new();
    let old_line = line(json!({"id":"old", "padding":"x".repeat(MAX_LINE_BYTES / 2)}));
    let backlog = zstd_frame(&old_line.repeat(32));
    for index in 0..MAX_COMPRESSED_STREAMS {
        let path = fixture.write(&format!("history-{index}/session.jsonl.zstd"), &backlog);
        OpenOptions::new()
            .write(true)
            .open(path)
            .unwrap()
            .set_times(
                fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(100)),
            )
            .unwrap();
    }
    let root = fixture.root("deepseek_harness", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root], store).unwrap();
    let first = collector.poll(1_000).unwrap();
    assert!(first.events.is_empty());
    assert_eq!(collector.compressed.len(), MAX_COMPRESSED_STREAMS);
    let path = fixture.write(
        "live/session.jsonl.zstd",
        &zstd_records([json!({"id":"current"})]),
    );
    OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(UNIX_EPOCH + std::time::Duration::from_secs(200)),
        )
        .unwrap();
    let result = collector.poll(17_000).unwrap();
    assert_eq!(result.events.len(), 1);
    assert!(result.events[0].id.ends_with("current"));
    assert_eq!(result.sources[0].connection, "connected");
    assert!(result.pending_files > 0);
    assert!(collector.compressed.len() <= MAX_COMPRESSED_STREAMS);
    assert!(result.bytes_read + result.decoded_bytes <= MAX_POLL_BYTES as u64);
}

#[test]
fn compressed_credentials_are_redacted_and_reconnect_keeps_retention_cursor() {
    let fixture = Fixture::new();
    let packed = zstd_records([json!({"id":"private-compressed",
        "title":"password=COMPRESSED_PASSWORD", "summary":"authorization=COMPRESSED_AUTH",
        "arguments":"COMPRESSED_RAW_ARGUMENT", "config":{"apiKey":"COMPRESSED_RAW_KEY"}})]);
    let path = fixture.write("session.jsonl.zstd", &packed);
    let root = fixture.root("deepseek_harness", fixture_adapter);
    let store = fixture.store();
    let mut collector = Collector::new(vec![root.clone()], store.clone()).unwrap();
    assert_eq!(collector.poll(1_000).unwrap().events.len(), 1);
    let saved = store.cursors().unwrap();
    let persisted = serde_json::to_string(&(store.load_events(None).unwrap(), &saved)).unwrap();
    let database = fs::read(fixture.path.join("observation.sqlite")).unwrap();
    let database = String::from_utf8_lossy(&database);
    for secret in [
        "COMPRESSED_PASSWORD",
        "COMPRESSED_AUTH",
        "COMPRESSED_RAW_ARGUMENT",
        "COMPRESSED_RAW_KEY",
    ] {
        assert!(!persisted.contains(secret));
        assert!(!database.contains(secret));
    }
    assert_eq!(fs::read(&path).unwrap(), packed);
    let parked = fixture.path.join("parked-logs");
    assert_eq!(parked.parent(), Some(fixture.path.as_path()));
    fs::rename(fixture.logs(), &parked).unwrap();
    let offline = collector.poll(3_000).unwrap();
    assert_eq!(offline.sources[0].connection, "disconnected");
    assert!(offline.events.is_empty());
    assert_eq!(store.cursors().unwrap(), saved);
    store.set_retention_days(1).unwrap();
    assert_eq!(store.prune(172_800_000).unwrap(), 1);
    drop(collector);
    fs::rename(&parked, fixture.logs()).unwrap();
    let mut collector = Collector::new(vec![root], store.clone()).unwrap();
    let recovered = collector.poll(172_802_000).unwrap();
    assert_eq!(recovered.sources[0].connection, "connected");
    assert_eq!(recovered.decoded_bytes, 0);
    assert!(recovered.events.is_empty());
    assert!(store.load_events(None).unwrap().is_empty());
    assert_eq!(fs::read(path).unwrap(), packed);
}

#[test]
fn persisted_database_and_cursors_exclude_fixture_credentials_and_arguments() {
    let fixture = Fixture::new();
    let bytes = line(json!({"id":"redacted", "title":"password=FIXTURE_PASSWORD",
        "summary":"authorization=FIXTURE_AUTH https://user:pass@host.test/log?token=FIXTURE_QUERY",
        "arguments":"FIXTURE_RAW_ARGUMENTS", "config":{"apiKey":"FIXTURE_RAW_KEY"}}));
    let path = fixture.write("rollout-safe.jsonl", &bytes);
    let store = fixture.store();
    let mut collector =
        Collector::new(vec![fixture.root("codex", fixture_adapter)], store.clone()).unwrap();
    assert_eq!(collector.poll(1_000).unwrap().events.len(), 1);
    assert_eq!(fs::read(path).unwrap(), bytes);
    let persisted =
        serde_json::to_string(&(store.load_events(None).unwrap(), store.cursors().unwrap()))
            .unwrap();
    let db_bytes = fs::read(fixture.path.join("observation.sqlite")).unwrap();
    let db_text = String::from_utf8_lossy(&db_bytes);
    for secret in [
        "FIXTURE_PASSWORD",
        "FIXTURE_AUTH",
        "FIXTURE_QUERY",
        "FIXTURE_RAW_ARGUMENTS",
        "FIXTURE_RAW_KEY",
        "user:pass",
    ] {
        assert!(!persisted.contains(secret));
        assert!(!db_text.contains(secret));
    }
    assert!(!persisted.contains(&fixture.path.to_string_lossy().replace('\\', "\\\\")));
}

#[cfg(any(windows, unix))]
#[test]
fn refuses_linked_roots_and_nested_reparse_directories() {
    let fixture = Fixture::new();
    let target = fixture.path.join("outside-root");
    fs::create_dir(&target).unwrap();
    fs::write(
        target.join("rollout-private.jsonl"),
        line(json!({"id":"must-not-read"})),
    )
    .unwrap();
    fs::write(
        target.join("session.jsonl.zstd"),
        zstd_records([json!({"id":"must-not-decode"})]),
    )
    .unwrap();
    fs::create_dir(fixture.logs()).unwrap();
    let link = fixture.logs().join("linked");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, &link).unwrap();
    #[cfg(windows)]
    {
        let result = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(&link)
            .arg(&target)
            .output()
            .unwrap();
        assert!(result.status.success(), "fixture junction creation failed");
    }
    assert!(is_link(&fs::symlink_metadata(&link).unwrap()));
    let store = fixture.store();
    let mut collector =
        Collector::new(vec![fixture.root("codex", fixture_adapter)], store.clone()).unwrap();
    let result = collector.poll(1_000).unwrap();
    assert!(result.events.is_empty());
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("链接"))
    );
    let mut root = fixture.root("codex", fixture_adapter);
    root.id = "linked-root".into();
    root.path = link.clone();
    let mut linked = Collector::new(vec![root], store.clone()).unwrap();
    let result = linked.poll(1_000).unwrap();
    assert!(result.events.is_empty());
    assert_eq!(result.sources[0].connection, "unavailable");
    assert!(store.load_events(None).unwrap().is_empty());
    assert!(target.join("rollout-private.jsonl").is_file());
    let mut compressed = Collector::new(
        vec![fixture.root("deepseek_harness", fixture_adapter)],
        store.clone(),
    )
    .unwrap();
    let result = compressed.poll(1_000).unwrap();
    assert!(result.events.is_empty());
    assert_eq!(result.decoded_bytes, 0);
    assert!(
        result
            .warnings
            .iter()
            .any(|warning| warning.contains("链接"))
    );
    let mut linked_root = fixture.root("deepseek_harness", fixture_adapter);
    linked_root.id = "compressed-linked-root".into();
    linked_root.path = link.clone();
    let mut compressed = Collector::new(vec![linked_root], store.clone()).unwrap();
    let result = compressed.poll(1_000).unwrap();
    assert_eq!(result.sources[0].connection, "unavailable");
    assert!(result.events.is_empty());
    assert_eq!(result.decoded_bytes, 0);
    assert!(store.load_events(None).unwrap().is_empty());
    assert!(target.join("session.jsonl.zstd").is_file());
    #[cfg(unix)]
    fs::remove_file(link).unwrap();
    #[cfg(windows)]
    fs::remove_dir(link).unwrap();
}
