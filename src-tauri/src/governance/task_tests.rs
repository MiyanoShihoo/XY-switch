use super::*;
use crate::governance::{save, test_support as fixture};
use serde_json::json;

fn body() -> Value {
    json!({"model":"gov-model","messages":[{"role":"user","content":"OK"}],"max_tokens":16})
}

fn identity(id: &str) -> Identity {
    let mut headers = HeaderMap::new();
    headers.insert("x-xy-task-id", id.parse().unwrap());
    Identity::from_request(&headers, b"{}", "local_loopback", false).unwrap()
}

fn context(db: &Database, id: &str) -> Context {
    Context::new(db, identity(id), Cancellation::default()).unwrap()
}

fn begin(db: &Database, context: &Context) -> Result<String, AdmissionError> {
    admit_and_start(
        db,
        context,
        &fixture::provider("http://127.0.0.1:1"),
        &mut body(),
        "chat",
        "proxy",
        |_| Ok(()),
        |_, id, _| id,
    )
}

fn record_attempt_event(db: &Database, context: &Context, attempt_id: &str) -> i64 {
    db.save_provider(&fixture::provider("http://127.0.0.1:1"))
        .unwrap();
    db.record_request(&crate::database::RequestEvent {
        provider_id: "gov-test".into(),
        route: "chat".into(),
        status_code: Some(200),
        success: true,
        affects_health: false,
        latency_ms: 1,
        retry_count: 0,
        error_kind: None,
        model: Some("gov-model".into()),
        input_tokens: None,
        output_tokens: None,
        cache_creation_tokens: None,
        cache_read_tokens: None,
        cost: None,
        price_source: None,
        price_version: None,
        created_at: now(),
        diagnostics: Some(RequestDiagnostics {
            attempt_id: Some(attempt_id.into()),
            task_request_id: Some(context.request_id.clone()),
            task: Some(summary(db, &context.identity.id).unwrap()),
            send_attempts: 1,
            ..Default::default()
        }),
    })
    .unwrap()
    .event_id
}

struct StoreFile(PathBuf);
impl StoreFile {
    fn new() -> Self {
        let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/task-store-tests");
        std::fs::create_dir_all(&directory).unwrap();
        Self(directory.join(format!("{}.db", uuid::Uuid::new_v4())))
    }
}
impl Drop for StoreFile {
    fn drop(&mut self) {
        for suffix in ["", ".tasks.lock", "-journal", "-wal", "-shm"] {
            let mut path = self.0.as_os_str().to_owned();
            path.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(path));
        }
    }
}

fn run_store_process(path: &Path, mode: &str) {
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "governance::tasks::tests::task_store_process_helper",
            "--nocapture",
        ])
        .env("XY_SWITCH_TASK_STORE_TEST_PATH", path)
        .env("XY_SWITCH_TASK_STORE_TEST_MODE", mode)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn old_policies_gain_bounded_task_defaults() {
    let policy: budget::BudgetPolicy = serde_json::from_value(json!({
        "enabled":true,"dailyTokens":100000,"dailyMicrousd":1000000,
        "inputReservation":1024,"maxOutputTokens":64
    }))
    .unwrap();
    assert_eq!(
        (policy.task_max_attempts, policy.task_max_microusd),
        (12, 250000)
    );
}

#[test]
fn native_turns_explicit_ids_and_auth_scopes_group_correctly() {
    let requests = fixture::native_codex_requests();
    let ids: Vec<_> = requests
        .iter()
        .map(|request| {
            Identity::from_request(
                &HeaderMap::new(),
                &serde_json::to_vec(request).unwrap(),
                "scope",
                false,
            )
            .unwrap()
        })
        .collect();
    assert_eq!(ids[0].id, ids[1].id);
    assert_eq!(ids[0].id, ids[2].id);
    assert_ne!(ids[0].id, ids[3].id);
    assert_eq!(ids[0].source, "codex_turn");
    let mut headers = HeaderMap::new();
    headers.insert("idempotency-key", "same-task".parse().unwrap());
    let first = Identity::from_request(&headers, b"{}", "scope-one", false).unwrap();
    let second =
        Identity::from_request(&headers, br#"{"changed":true}"#, "scope-one", false).unwrap();
    let other = Identity::from_request(&headers, b"{}", "scope-two", false).unwrap();
    assert_eq!(first.id, second.id);
    assert_ne!(first.id, other.id);
    headers.insert("x-xy-task-id", "different-task".parse().unwrap());
    assert_eq!(
        Identity::from_request(&headers, b"{}", "scope", false).unwrap_err(),
        "task_identity_ambiguous"
    );
    headers.remove("idempotency-key");
    headers.append("x-xy-task-id", "duplicate-task".parse().unwrap());
    assert_eq!(
        Identity::from_request(&headers, b"{}", "scope", false).unwrap_err(),
        "task_identity_ambiguous"
    );
}

#[test]
fn missing_or_conflicting_turn_identity_is_rejected() {
    let db = fixture::database();
    fixture::budget(&db);
    let anonymous = Identity::from_request(&HeaderMap::new(), b"{}", "local", false).unwrap();
    assert_eq!(
        Context::new(&db, anonymous, Cancellation::default())
            .err()
            .unwrap(),
        "task_identity_required"
    );
    assert!(list(&db).unwrap().is_empty());
    assert_eq!(budget::status(&db).unwrap().attempts, 0);
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-codex-turn-metadata",
        r#"{"session_id":"s","turn_id":"t2"}"#.parse().unwrap(),
    );
    let request = br#"{"client_metadata":{"session_id":"s","turn_id":"t1"}}"#;
    assert_eq!(
        Identity::from_request(&headers, request, "local", false).unwrap_err(),
        "task_identity_ambiguous"
    );
}

#[test]
fn concurrent_task_attempt_and_fee_limits_are_exact() {
    for use_cost in [false, true] {
        let db = fixture::database();
        let mut policy = fixture::budget(&db);
        if use_cost {
            policy.task_max_microusd = 3 * 1056;
        } else {
            policy.task_max_attempts = 3;
        }
        save(&db, budget::KEY, &policy).unwrap();
        let handles: Vec<_> = (0..32)
            .map(|_| {
                let db = db.clone();
                std::thread::spawn(move || begin(&db, &context(&db, "concurrent-task")).is_ok())
            })
            .collect();
        assert_eq!(
            handles
                .into_iter()
                .filter_map(|handle| handle.join().ok())
                .filter(|accepted| *accepted)
                .count(),
            3
        );
        let task = detail(&db, &identity("concurrent-task").id).unwrap();
        assert_eq!((task.task.attempts, task.task.reserved_microusd), (3, 3168));
        assert_eq!(
            task.attempts
                .iter()
                .map(|attempt| attempt.ordinal)
                .collect::<Vec<_>>(),
            vec![1, 2, 3]
        );
        assert_eq!(budget::status(&db).unwrap().attempts, 3);
    }
}

#[test]
fn local_build_failure_and_rejection_add_no_send_or_reservation() {
    let db = fixture::database();
    let mut policy = fixture::budget(&db);
    let context = context(&db, "build-task");
    let sends = std::sync::atomic::AtomicUsize::new(0);
    let result = admit_and_start(
        &db,
        &context,
        &fixture::provider("http://127.0.0.1:1"),
        &mut body(),
        "chat",
        "proxy",
        |_| Err::<(), _>("invalid local header".into()),
        |_, _, _| {
            sends.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        },
    );
    assert_eq!(result.unwrap_err().code, "request_build");
    assert_eq!(sends.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(summary(&db, &context.identity.id).unwrap().attempts, 0);
    assert_eq!(budget::status(&db).unwrap().attempts, 0);
    policy.task_max_attempts = 0;
    save(&db, budget::KEY, &policy).unwrap();
    let rejected = self::context(&db, "build-task");
    assert_eq!(
        begin(&db, &rejected).unwrap_err().reason,
        "task_attempt_budget_exhausted"
    );
    assert_eq!(budget::status(&db).unwrap().attempts, 0);
}

#[test]
fn task_store_process_helper() {
    let Some(path) = std::env::var_os("XY_SWITCH_TASK_STORE_TEST_PATH") else {
        return;
    };
    let path = PathBuf::from(path);
    match std::env::var("XY_SWITCH_TASK_STORE_TEST_MODE")
        .unwrap()
        .as_str()
    {
        "locked" => assert_eq!(Database::open(&path).err().unwrap(), "task_database_in_use"),
        "abandon" => {
            let db = Database::open(&path).unwrap();
            let mut policy = fixture::budget(&db);
            policy.task_max_attempts = 1;
            save(&db, budget::KEY, &policy).unwrap();
            begin(&db, &context(&db, "process-task")).unwrap();
            std::process::exit(0);
        }
        mode => panic!("unknown process test mode: {mode}"),
    }
}

#[test]
fn duplicate_database_handles_share_live_cancellation_and_exclude_other_processes() {
    let store = StoreFile::new();
    let first_db = Database::open(&store.0).unwrap();
    fixture::budget(&first_db);
    let first = context(&first_db, "shared-store-task");
    begin(&first_db, &first).unwrap();
    let second_db = Database::open(&store.0).unwrap();
    assert!(Arc::ptr_eq(&first_db.tasks, &second_db.tasks));
    assert_eq!(
        detail(&second_db, &first.identity.id).unwrap().attempts[0].status,
        "sending"
    );
    let second = context(&second_db, "shared-store-task");
    begin(&second_db, &second).unwrap();
    run_store_process(&store.0, "locked");
    cancel(&second_db, &first.identity.id).unwrap();
    assert!(first.signal.is_cancelled() && second.signal.is_cancelled());
    assert_eq!(
        begin(&first_db, &first).unwrap_err().code,
        "request_cancelled"
    );
    let task = detail(&second_db, &first.identity.id).unwrap();
    assert!(task.task.cancelled);
    assert_eq!((task.task.attempts, task.task.active_attempts), (2, 0));
    assert!(
        task.attempts
            .iter()
            .all(|attempt| attempt.status == "cancelled")
    );
}

#[test]
fn terminated_process_recovery_preserves_reservations_and_task_limits() {
    let store = StoreFile::new();
    run_store_process(&store.0, "abandon");
    let db = Database::open(&store.0).unwrap();
    let context = context(&db, "process-task");
    let task = detail(&db, &context.identity.id).unwrap();
    assert_eq!(
        (
            task.task.attempts,
            task.task.reserved_microusd,
            task.task.active_attempts
        ),
        (1, 1056, 0)
    );
    assert_eq!(task.attempts[0].status, "interrupted");
    assert_eq!(
        task.attempts[0].error_kind.as_deref(),
        Some("process_restart")
    );
    assert_eq!(
        (
            task.attempts[0].usage_state.as_str(),
            task.attempts[0].cost_state.as_str()
        ),
        ("missing", "unknown")
    );
    assert_eq!(
        begin(&db, &context).unwrap_err().reason,
        "task_attempt_budget_exhausted"
    );
    assert_eq!(budget::status(&db).unwrap().attempts, 1);
}

#[test]
fn task_limits_survive_day_rollover_tightening_loosening_and_disabled_policy() {
    let db = fixture::database();
    let mut policy = fixture::budget(&db);
    policy.task_max_attempts = 4;
    policy.task_max_microusd = 4 * 1056;
    save(&db, budget::KEY, &policy).unwrap();
    let first = context(&db, "persistent-budget-task");
    begin(&db, &first).unwrap();
    db.conn()
        .lock()
        .unwrap()
        .execute("UPDATE budget_ledger SET day=?1", [now() / 86400 - 1])
        .unwrap();
    assert_eq!(budget::status(&db).unwrap().attempts, 0);
    policy.task_max_attempts = 2;
    policy.task_max_microusd = 2 * 1056;
    save(&db, budget::KEY, &policy).unwrap();
    let tightened = context(&db, "persistent-budget-task");
    begin(&db, &tightened).unwrap();
    policy.task_max_attempts = 12;
    policy.task_max_microusd = 250000;
    save(&db, budget::KEY, &policy).unwrap();
    let widened = context(&db, "persistent-budget-task");
    assert_eq!(
        begin(&db, &widened).unwrap_err().reason,
        "task_attempt_budget_exhausted"
    );
    policy.enabled = false;
    save(&db, budget::KEY, &policy).unwrap();
    let disabled = context(&db, "persistent-budget-task");
    assert_eq!(
        begin(&db, &disabled).unwrap_err().reason,
        "task_attempt_budget_exhausted"
    );
    let task = summary(&db, &first.identity.id).unwrap();
    assert_eq!(
        (task.max_attempts, task.max_microusd, task.attempts),
        (Some(2), Some(2112), 2)
    );
    assert_eq!(budget::status(&db).unwrap().attempts, 1);
}

#[test]
fn disabling_global_budget_keeps_cost_reservation_for_existing_tasks() {
    let db = fixture::database();
    let mut policy = fixture::budget(&db);
    policy.task_max_microusd = 2112;
    save(&db, budget::KEY, &policy).unwrap();
    let first = context(&db, "disabled-cost-task");
    begin(&db, &first).unwrap();
    policy.enabled = false;
    policy.daily_tokens = 0;
    policy.daily_microusd = 0;
    save(&db, budget::KEY, &policy).unwrap();
    let disabled = context(&db, "disabled-cost-task");
    begin(&db, &disabled).unwrap();
    assert_eq!(
        begin(&db, &disabled).unwrap_err().reason,
        "task_cost_budget_exhausted"
    );
    assert_eq!(
        summary(&db, &first.identity.id).unwrap().reserved_microusd,
        2112
    );
    assert_eq!(budget::status(&db).unwrap().attempts, 2);
}

#[test]
fn cancelled_tasks_cannot_be_revived_by_late_responses_or_settlement() {
    let db = fixture::database();
    fixture::budget(&db);
    let first = context(&db, "cancel-late-task");
    let first_id = begin(&db, &first).unwrap();
    let event_id = record_attempt_event(&db, &first, &first_id);
    let concurrent = context(&db, "cancel-late-task");
    begin(&db, &concurrent).unwrap();
    cancel(&db, &first.identity.id).unwrap();
    finish_attempt(&db, &first_id, "response_pending", Some(200), None).unwrap();
    db.update_request_usage(
        event_id,
        &RequestUsage {
            model: Some("gov-model".into()),
            input_tokens: Some(4),
            output_tokens: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
    let retry = context(&db, "cancel-late-task");
    assert_eq!(begin(&db, &retry).unwrap_err().code, "request_cancelled");
    let task = detail(&db, &first.identity.id).unwrap();
    assert_eq!(
        (
            task.task.attempts,
            task.task.known_microusd,
            task.task.active_attempts
        ),
        (2, 6, 0)
    );
    assert!(task.task.cancelled);
    assert!(
        task.attempts
            .iter()
            .all(|attempt| attempt.status == "cancelled")
    );
    assert_eq!(task.attempts[0].cost_state, "known");
    assert_eq!(budget::status(&db).unwrap().attempts, 2);
}

#[test]
fn missing_and_partial_usage_keep_unknown_cost_and_full_reservation() {
    let db = fixture::database();
    let mut policy = fixture::budget(&db);
    policy.task_max_microusd = 2112;
    save(&db, budget::KEY, &policy).unwrap();
    for (input, output, state) in [(None, None, "missing"), (Some(4), None, "partial")] {
        let context = context(&db, "unknown-cost-task");
        let id = begin(&db, &context).unwrap();
        let event_id = record_attempt_event(&db, &context, &id);
        db.update_request_usage(
            event_id,
            &RequestUsage {
                model: Some("gov-model".into()),
                input_tokens: input,
                output_tokens: output,
                ..Default::default()
            },
        )
        .unwrap();
        let task = detail(&db, &context.identity.id).unwrap();
        let attempt = task.attempts.last().unwrap();
        assert_eq!(
            (attempt.usage_state.as_str(), attempt.cost_state.as_str()),
            (state, "unknown")
        );
        assert_eq!(attempt.actual_microusd, None);
    }
    let context = context(&db, "unknown-cost-task");
    assert_eq!(
        begin(&db, &context).unwrap_err().reason,
        "task_cost_budget_exhausted"
    );
    let task = summary(&db, &context.identity.id).unwrap();
    assert_eq!(
        (
            task.reserved_microusd,
            task.committed_microusd,
            task.unknown_cost_attempts
        ),
        (2112, 2112, 2)
    );
}

#[test]
fn actual_cost_above_reservation_reduces_remaining_task_capacity() {
    let db = fixture::database();
    let mut policy = fixture::budget(&db);
    policy.task_max_microusd = 3000;
    save(&db, budget::KEY, &policy).unwrap();
    let context = context(&db, "overage-task");
    let id = begin(&db, &context).unwrap();
    let event_id = record_attempt_event(&db, &context, &id);
    db.update_request_usage(
        event_id,
        &RequestUsage {
            model: Some("gov-model".into()),
            input_tokens: Some(2000),
            output_tokens: Some(50),
            ..Default::default()
        },
    )
    .unwrap();
    let task = summary(&db, &context.identity.id).unwrap();
    assert_eq!(
        (
            task.reserved_microusd,
            task.known_microusd,
            task.committed_microusd
        ),
        (1056, 2100, 2100)
    );
    assert_eq!(
        begin(&db, &context).unwrap_err().reason,
        "task_cost_budget_exhausted"
    );
    assert_eq!(budget::status(&db).unwrap().attempts, 1);
}

#[test]
fn enabling_cost_limits_rejects_historical_unreserved_attempts() {
    let db = fixture::database();
    let context = context(&db, "unreserved-task");
    begin(&db, &context).unwrap();
    fixture::budget(&db);
    assert_eq!(
        begin(&db, &context).unwrap_err().reason,
        "task_unreserved_cost"
    );
    let task = summary(&db, &context.identity.id).unwrap();
    assert_eq!(
        (
            task.attempts,
            task.unreserved_attempts,
            task.reserved_microusd
        ),
        (1, 1, 0)
    );
    assert_eq!(task.max_attempts, Some(12));
    assert_eq!(budget::status(&db).unwrap().attempts, 0);
}

#[test]
fn auxiliary_reservation_failure_rolls_back_both_ledgers_and_does_not_send() {
    let db = fixture::database();
    fixture::budget(&db);
    let context = context(&db, "atomic-extra-task");
    let sends = std::sync::atomic::AtomicUsize::new(0);
    let result = admit_and_start_on(
        &db,
        &context,
        &fixture::provider("http://127.0.0.1:1"),
        &mut body(),
        "chat",
        "model_probe",
        |_| Ok(()),
        |connection| {
            super::super::event_on(connection, "test_auxiliary_reservation", "reserved").unwrap();
            Err::<(), _>("auxiliary_budget_exhausted".into())
        },
        |_, _, _, _| {
            sends.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        },
    );
    assert_eq!(result.unwrap_err().reason, "auxiliary_budget_exhausted");
    assert_eq!(sends.load(std::sync::atomic::Ordering::SeqCst), 0);
    assert_eq!(summary(&db, &context.identity.id).unwrap().attempts, 0);
    assert_eq!(budget::status(&db).unwrap().attempts, 0);
    assert_eq!(
        db.conn()
            .lock()
            .unwrap()
            .query_row(
                "SELECT COUNT(*) FROM governance_events WHERE kind='test_auxiliary_reservation'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
}

#[test]
fn microusd_rounding_keeps_exact_boundaries_and_rejects_invalid_costs() {
    for (cost, expected) in [
        (0.0, 0),
        (0.000001, 1),
        (0.0010560000000000003, 1056),
        (0.0000010000000001, 2),
        (0.0010560001, 1057),
    ] {
        assert_eq!(microusd(cost), Some(expected), "{cost}");
    }
    for cost in [
        -0.01,
        f64::NAN,
        f64::INFINITY,
        i64::MAX as f64 / 1_000_000.0,
    ] {
        assert_eq!(microusd(cost), None);
    }
}
