use super::{engine::Engine, headers, ledger, store, types::*};
use crate::database::Database;
use std::{collections::BTreeMap, path::Path, sync::{Arc, Mutex}};

fn configuration(strategy: Strategy, count: usize) -> (Configuration, BTreeMap<String, ProviderFacts>) {
    let channels: Vec<_> = (0..count).map(|index| Channel {
        id: format!("channel-{index}"), provider_id: format!("provider-{index}"), account_id: None,
        upstream_model: "model-a".into(), enabled: true, priority: 0, weight: 1,
        quota_pool_id: None, quota_independent: true, limits: Limits::default(), cooldown: CooldownPolicy::default(),
    }).collect();
    let facts = channels.iter().map(|channel| (channel.provider_id.clone(), ProviderFacts {
        enabled: true, origin: format!("https://{}.example", channel.provider_id), ..Default::default()
    })).collect();
    (Configuration { groups: vec![ChannelGroup { id: "group-a".into(), name: "Test group".into(), virtual_model: "xy-group/test".into(), enabled: true,
        strategy, project_id: None, affinity_ttl_seconds: 3600, channels, revision: 0 }], projects: vec![] }, facts)
}

fn engine(strategy: Strategy, count: usize) -> Engine {
    let (configuration, facts) = configuration(strategy, count);
    Engine::new(configuration, facts, BTreeMap::new()).unwrap()
}

fn request(id: &str) -> RouteRequest {
    RouteRequest { id: id.into(), group_id: "group-a".into(), reserve_tokens: 100, reserve_microusd: Some(1000),
        stateful: false, session_key: None, previous_response_id: None, conversation_id: None }
}

fn completion(status: u16) -> Completion {
    Completion { outcome: Outcome::Http, status: Some(status), usage: CompletionUsage::default(), headers: BTreeMap::new(), response_id: None, conversation_id: None }
}

fn choose(engine: &mut Engine, id: &str) -> String {
    let decision = engine.select(request(id)).unwrap();
    assert!(!decision.dispatch_allowed);
    decision.channel_id.unwrap()
}

#[test]
fn round_robin_has_one_selection_and_is_fair() {
    let mut engine = engine(Strategy::RoundRobin, 3);
    for index in 0..30 { assert_eq!(choose(&mut engine, &index.to_string()), format!("channel-{}", index % 3)); }
    assert_eq!(engine.views().iter().map(|view| view.simulated_usage.requests).collect::<Vec<_>>(), [10, 10, 10]);
    assert_eq!(engine.select(request("0")).unwrap().reason, "request_already_considered");
}

#[test]
fn weighted_round_robin_uses_smooth_weight_ratio() {
    let (mut configuration, facts) = configuration(Strategy::WeightedRoundRobin, 2);
    configuration.groups[0].channels[0].weight = 3;
    let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap();
    for index in 0..40 { choose(&mut engine, &index.to_string()); }
    assert_eq!(engine.views().iter().map(|view| view.simulated_usage.requests).collect::<Vec<_>>(), [30, 10]);
}

#[test]
fn priority_and_disabled_channels_are_filtered_before_strategy() {
    let (mut configuration, facts) = configuration(Strategy::RoundRobin, 3);
    configuration.groups[0].channels[0].enabled = false;
    configuration.groups[0].channels[1].priority = 9;
    let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap();
    assert_eq!(choose(&mut engine, "first"), "channel-2");
}

#[test]
fn least_load_and_latency_use_observations() {
    let mut load = engine(Strategy::LeastLoad, 2);
    assert_eq!(choose(&mut load, "a"), "channel-0");
    assert_eq!(choose(&mut load, "b"), "channel-1");
    load.complete("b", completion(200)).unwrap();
    assert_eq!(choose(&mut load, "c"), "channel-1");
    let mut latency = engine(Strategy::LatencyFirst, 2);
    latency.replay(&RecordedEvent { id: 1, provider_id: "provider-0".into(), at_ms: 1, status: Some(200), latency_ms: Some(100), tokens: None, cost_microusd: None, error_kind: None }).unwrap();
    latency.replay(&RecordedEvent { id: 2, provider_id: "provider-1".into(), at_ms: 2, status: Some(200), latency_ms: Some(20), tokens: None, cost_microusd: None, error_kind: None }).unwrap();
    assert_eq!(choose(&mut latency, "fast"), "channel-1");
}

#[test]
fn quota_aware_prefers_known_capacity_and_unknown_falls_back_to_rotation() {
    let mut engine = engine(Strategy::QuotaAware, 2);
    assert_eq!(choose(&mut engine, "a"), "channel-0");
    assert_eq!(choose(&mut engine, "b"), "channel-1");
    assert!(engine.views().iter().all(|view| view.quota.remaining_tokens.is_none()));
    let mut result = completion(200);
    result.headers.insert("x-ratelimit-remaining-tokens".into(), "1000".into());
    engine.complete("b", result).unwrap();
    assert_eq!(choose(&mut engine, "c"), "channel-1");
    assert_eq!(engine.views()[1].quota.remaining_tokens, Some(900));
}

#[test]
fn shared_pools_use_minimum_limits_and_do_not_multiply_capacity() {
    for mode in ["provider", "origin", "declared", "unknown"] {
        let (mut configuration, mut facts) = configuration(Strategy::RoundRobin, 2);
        let channels = &mut configuration.groups[0].channels;
        channels[0].limits.rpm = Some(1);
        channels[1].limits.rpm = Some(20);
        match mode {
            "provider" => channels[1].provider_id = channels[0].provider_id.clone(),
            "origin" => facts.get_mut("provider-1").unwrap().origin = facts["provider-0"].origin.clone(),
            "declared" => for channel in channels { channel.quota_pool_id = Some("shared-account".into()); channel.quota_independent = false; },
            _ => for channel in channels { channel.quota_independent = false; },
        }
        let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap();
        choose(&mut engine, "a");
        let blocked = engine.select(request("b")).unwrap();
        assert_eq!(blocked.reason, "no_eligible_channel", "{mode}");
        assert!(blocked.excluded.iter().all(|entry| entry.reason == "rpm_limit"));
        assert_eq!(engine.views()[0].pool_id, engine.views()[1].pool_id);
    }
}

#[test]
fn shared_limits_apply_across_groups() {
    let (mut configuration, facts) = configuration(Strategy::RoundRobin, 1);
    configuration.groups[0].channels[0].limits.concurrency = Some(1);
    let mut second = configuration.groups[0].clone();
    second.id = "group-b".into(); second.virtual_model = "xy-group/b".into(); second.channels[0].id = "channel-b".into();
    configuration.groups.push(second);
    let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap();
    choose(&mut engine, "a");
    let mut other = request("b"); other.group_id = "group-b".into();
    assert_eq!(engine.select(other).unwrap().excluded[0].reason, "concurrency_limit");
}

#[test]
fn atomic_selection_does_not_overbook_concurrency() {
    let (mut configuration, facts) = configuration(Strategy::RoundRobin, 3);
    for channel in &mut configuration.groups[0].channels { channel.limits.concurrency = Some(1); }
    let engine = Arc::new(Mutex::new(Engine::new(configuration, facts, BTreeMap::new()).unwrap()));
    let workers: Vec<_> = (0..24).map(|index| { let engine = engine.clone(); std::thread::spawn(move || engine.lock().unwrap().select(request(&index.to_string())).unwrap().channel_id.is_some()) }).collect();
    assert_eq!(workers.into_iter().filter_map(|worker| worker.join().unwrap().then_some(())).count(), 3);
}

#[test]
fn retry_after_supports_seconds_http_dates_and_compound_resets() {
    assert_eq!(headers::retry_after("120", 1000), Some(121000));
    assert_eq!(headers::retry_after("Thu, 01 Jan 1970 00:02:00 GMT", 1000), Some(120000));
    assert_eq!(headers::reset_duration("1m2.5s250ms", 1000), Some(63750));
    assert_eq!(headers::reset_duration("0.0001s", 0), Some(1));
    for invalid in ["-1", "NaN", "1e9s", "1 s", "2.3.4s", "9999999999999999999999999999d"] { assert_eq!(headers::reset_duration(invalid, 0), None); }
    assert_eq!(headers::retry_after("18446744073709551615", 0), None);
    let duplicate = BTreeMap::from([("Retry-After".into(), "10".into()), ("retry-after".into(), "20".into())]);
    assert_eq!(headers::parse(&duplicate, 0).unwrap_err(), "duplicate_response_header");
}

#[test]
fn rate_limit_honors_longest_upstream_deadline_and_late_success_does_not_clear_it() {
    let mut engine = engine(Strategy::RoundRobin, 1);
    choose(&mut engine, "a"); choose(&mut engine, "late");
    let mut result = completion(429);
    result.headers = BTreeMap::from([("Retry-After".into(), "7200".into()), ("x-ratelimit-reset-tokens".into(), "3h".into())]);
    engine.complete("a", result).unwrap();
    engine.advance(1000).unwrap(); engine.complete("late", completion(200)).unwrap();
    assert_eq!(engine.views()[0].recovery_at_ms, Some(10_800_000));
    assert!(engine.select(request("blocked")).unwrap().channel_id.is_none());
    engine.advance(10_800_000).unwrap();
    assert_eq!(choose(&mut engine, "recovered"), "channel-0");
}

#[test]
fn quota_reset_returns_unknown_without_filling_capacity() {
    let mut engine = engine(Strategy::RoundRobin, 1); choose(&mut engine, "a");
    let mut result = completion(429);
    result.headers = BTreeMap::from([("x-ratelimit-remaining-requests".into(), "0".into()), ("x-ratelimit-reset-requests".into(), "1m".into())]);
    engine.complete("a", result).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_requests, Some(0));
    engine.advance(60_000).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_requests, None);
    assert_eq!(choose(&mut engine, "b"), "channel-0");
}

#[test]
fn authentication_failure_disables_matching_credential_and_does_not_retry() {
    for status in [401, 403] {
        let mut engine = engine(Strategy::RoundRobin, 2);
        choose(&mut engine, "a"); engine.complete("a", completion(status)).unwrap();
        assert_eq!(engine.views()[0].health, "authentication_disabled");
        assert_eq!(engine.views()[0].recovery_at_ms, None);
        assert_eq!(engine.select(request("a")).unwrap().reason, "request_already_considered");
        assert_eq!(choose(&mut engine, "new-operation"), "channel-1");
    }
}

#[test]
fn affinity_requires_known_server_ids_and_pins_followup_to_original_channel() {
    let mut engine = engine(Strategy::RoundRobin, 2);
    let mut first = request("a"); first.stateful = true; first.session_key = Some("session-one".into());
    assert_eq!(engine.select(first).unwrap().channel_id.as_deref(), Some("channel-0"));
    let mut result = completion(200); result.response_id = Some("resp-one".into()); result.conversation_id = Some("conv-one".into());
    engine.complete("a", result).unwrap();
    let mut next = request("b"); next.previous_response_id = Some("resp-one".into()); next.conversation_id = Some("conv-one".into());
    assert_eq!(engine.select(next).unwrap().channel_id.as_deref(), Some("channel-0"));
    engine.complete("b", completion(429)).unwrap();
    let mut pinned = request("c"); pinned.session_key = Some("session-one".into());
    assert_eq!(engine.select(pinned).unwrap().reason, "affinity_unavailable");
    let mut unknown = request("d"); unknown.previous_response_id = Some("never-seen".into());
    assert_eq!(engine.select(unknown).unwrap().reason, "affinity_unknown");
    engine.advance(3_600_000).unwrap();
    let mut expired = request("e"); expired.previous_response_id = Some("resp-one".into());
    assert_eq!(engine.select(expired).unwrap().reason, "affinity_expired");
}

#[test]
fn affinity_conflicts_and_missing_state_identity_are_rejected() {
    let mut engine = engine(Strategy::RoundRobin, 2);
    let mut invalid = request("invalid"); invalid.stateful = true;
    assert_eq!(engine.select(invalid).unwrap().reason, "stateful_identity_required");
    for (id, response) in [("a", "resp-a"), ("b", "resp-b")] {
        choose(&mut engine, id); let mut result = completion(200); result.response_id = Some(response.into()); engine.complete(id, result).unwrap();
    }
    let mut first = request("c"); first.previous_response_id = Some("resp-a".into()); first.session_key = Some("session-c".into()); engine.select(first).unwrap();
    let mut conflict = request("d"); conflict.previous_response_id = Some("resp-b".into()); conflict.session_key = Some("session-c".into());
    assert_eq!(engine.select(conflict).unwrap().reason, "affinity_conflict");
}

#[test]
fn streamed_timeout_and_525_never_redispatch_or_release_unknown_reservations() {
    for outcome in [Outcome::Timeout, Outcome::TransportUnknown, Outcome::Cancelled, Outcome::Http] {
        let (mut configuration, facts) = configuration(Strategy::RoundRobin, 1);
        configuration.groups[0].channels[0].limits.concurrency = Some(1);
        let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap(); choose(&mut engine, "a"); engine.stream_started("a").unwrap();
        let mut impossible = completion(200); impossible.outcome = Outcome::NotSent; impossible.status = None;
        assert_eq!(engine.complete("a", impossible).unwrap_err(), "not_sent_evidence_conflict");
        let mut result = completion(525); result.outcome = outcome; if outcome != Outcome::Http { result.status = None; }
        engine.complete("a", result).unwrap();
        assert_eq!(engine.views()[0].concurrent, 1);
        assert_eq!(engine.views()[0].unknown_results, 1);
        assert_eq!(engine.views()[0].simulated_usage.cost_known_requests, 0);
        assert_eq!(engine.select(request("a")).unwrap().reason, "request_already_considered");
        engine.advance(60_000).unwrap();
        assert!(engine.select(request("different-operation")).unwrap().channel_id.is_none());
    }
}

#[test]
fn not_sent_releases_known_reservations_and_provisional_affinity_only() {
    let mut engine = engine(Strategy::RoundRobin, 1); choose(&mut engine, "seed");
    let mut result = completion(200); result.headers = BTreeMap::from([("x-ratelimit-remaining-requests".into(), "5".into()), ("x-ratelimit-remaining-tokens".into(), "1000".into())]);
    engine.complete("seed", result).unwrap();
    let mut selected = request("never-sent"); selected.session_key = Some("provisional".into()); engine.select(selected).unwrap();
    let mut not_sent = completion(200); not_sent.outcome = Outcome::NotSent; not_sent.status = None;
    engine.complete("never-sent", not_sent).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_requests, Some(5));
    assert_eq!(engine.views()[0].quota.remaining_tokens, Some(1000));
    assert_eq!(engine.views()[0].concurrent, 0);
    assert_eq!(engine.views()[0].simulated_usage.requests, 1);
    assert_eq!(engine.select(request("never-sent")).unwrap().reason, "request_already_considered");
}

#[test]
fn response_quota_subtracts_other_inflight_reservations_and_actual_token_overage() {
    let mut engine = engine(Strategy::RoundRobin, 1); choose(&mut engine, "a"); choose(&mut engine, "b");
    let mut result = completion(200); result.headers.insert("x-ratelimit-remaining-tokens".into(), "1000".into());
    engine.complete("a", result).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_tokens, Some(900));
    let mut result = completion(200); result.usage.tokens = Some(160); engine.complete("b", result).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_tokens, Some(840));
}

#[test]
fn later_quota_observations_reserve_reported_overage_for_unknown_results() {
    let mut engine = engine(Strategy::RoundRobin, 1);
    choose(&mut engine, "held");
    engine.stream_started("held").unwrap();
    let mut result = completion(525);
    result.usage.tokens = Some(400);
    engine.complete("held", result).unwrap();
    engine.advance(30_000).unwrap();
    choose(&mut engine, "observed");
    let mut result = completion(200);
    result.headers.insert("x-ratelimit-remaining-tokens".into(), "1000".into());
    result.headers.insert("x-ratelimit-reset-tokens".into(), "1m".into());
    engine.complete("observed", result).unwrap();
    let view = &engine.views()[0];
    assert_eq!(view.quota.remaining_tokens, Some(600));
    assert_eq!(view.concurrent, 1);
    assert_eq!(view.unknown_results, 1);
    let mut oversized = request("oversized");
    oversized.reserve_tokens = 601;
    let decision = engine.select(oversized).unwrap();
    assert!(decision.channel_id.is_none());
    assert_eq!(decision.excluded[0].reason, "token_quota_exhausted");
}

#[test]
fn channel_budget_is_independent_even_when_provider_is_shared() {
    let (mut configuration, facts) = configuration(Strategy::RoundRobin, 2);
    configuration.groups[0].channels[1].provider_id = "provider-0".into();
    for channel in &mut configuration.groups[0].channels { channel.limits.daily_microusd = Some(1000); }
    let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap();
    assert_eq!(choose(&mut engine, "a"), "channel-0");
    assert_eq!(choose(&mut engine, "b"), "channel-1");
    assert!(engine.select(request("c")).unwrap().channel_id.is_none());
}

#[test]
fn project_budget_is_shared_across_groups_and_rolls_over_at_utc_midnight() {
    let (mut configuration, facts) = configuration(Strategy::RoundRobin, 1);
    configuration.projects.push(ProjectBudget { id: "project".into(), daily_microusd: Some(1000), revision: 0 });
    configuration.groups[0].project_id = Some("project".into());
    let mut second = configuration.groups[0].clone(); second.id = "group-b".into(); second.virtual_model = "xy-group/b".into(); second.channels[0].id = "channel-b".into(); configuration.groups.push(second);
    let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap(); choose(&mut engine, "a");
    let mut other = request("b"); other.group_id = "group-b".into();
    assert_eq!(engine.select(other).unwrap().excluded[0].reason, "project_budget_exhausted");
    engine.advance(DAY_MS).unwrap(); assert_eq!(choose(&mut engine, "new-day"), "channel-0");
}

#[test]
fn unknown_ledger_cost_and_missing_reservation_fail_closed_under_budget() {
    let (mut configuration, facts) = configuration(Strategy::RoundRobin, 1);
    configuration.groups[0].channels[0].limits.daily_microusd = Some(5000);
    let baseline = LedgerBaseline { usage: UsageTotals::default(), days: BTreeMap::from([(0, LedgerDay { committed_microusd: 0, unpriced_requests: 1 })]) };
    let mut engine = Engine::new(configuration, facts, BTreeMap::from([("provider-0".into(), baseline)])).unwrap();
    assert_eq!(engine.select(request("a")).unwrap().excluded[0].reason, "unresolved_ledger_cost");
    let mut missing = request("b"); missing.reserve_microusd = None;
    assert_eq!(engine.select(missing).unwrap().excluded[0].reason, "cost_reservation_required");
}

#[test]
fn replay_deduplicates_shared_pool_and_marks_unknown_tokens_without_adding_usage() {
    let (mut configuration, facts) = configuration(Strategy::RoundRobin, 2);
    configuration.groups[0].channels[1].provider_id = "provider-0".into();
    let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap();
    let event = RecordedEvent { id: 7, provider_id: "provider-0".into(), at_ms: 1000, status: Some(429), latency_ms: Some(10), tokens: None, cost_microusd: None, error_kind: None };
    engine.replay(&event).unwrap();
    assert_eq!(engine.views()[0].recovery_at_ms, Some(61000));
    assert_eq!(engine.views().iter().map(|view| view.rpm).sum::<u64>(), 1);
    assert_eq!(engine.views().iter().map(|view| view.tpm_unknown_requests).sum::<u64>(), 1);
    assert!(engine.views().iter().all(|view| view.simulated_usage.requests == 0));
    assert_eq!(engine.replay(&event).unwrap_err(), "duplicate_recorded_event");
}

#[test]
fn account_snapshot_uses_real_units_source_and_expiry() {
    use crate::accounts::{QuotaSnapshot, QuotaSource, QuotaUnit, QuotaWindow};
    let (mut configuration, mut facts) = configuration(Strategy::QuotaAware, 1);
    configuration.groups[0].channels[0].account_id = Some("account-one".into());
    let provider = facts.get_mut("provider-0").unwrap(); provider.account_ids.push("account-one".into());
    provider.account_quotas.insert("account-one".into(), QuotaSnapshot { source: QuotaSource::ProviderResponse, observed_at: 1, valid_until: 60,
        windows: vec![QuotaWindow { id: "tokens".into(), unit: QuotaUnit::Tokens, remaining: Some(500.0), limit: Some(1000.0), resets_at: Some(60) }] });
    let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap(); engine.advance(1000).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_tokens, Some(500)); choose(&mut engine, "a");
    engine.advance(2000).unwrap(); assert_eq!(engine.views()[0].quota.remaining_tokens, Some(400));
    engine.advance(60_000).unwrap(); assert_eq!(engine.views()[0].quota.remaining_tokens, None);
}

#[test]
fn response_quota_windows_expire_independently_without_releasing_long_limits() {
    for (long_remaining, short_remaining, expected) in [(0, 50, 0), (100, 0, 99)] {
        let mut engine = engine(Strategy::RoundRobin, 1);
        choose(&mut engine, "long-window"); choose(&mut engine, "short-window");
        let mut result = completion(200);
        result.headers = BTreeMap::from([
            ("x-ratelimit-remaining-requests".into(), long_remaining.to_string()),
            ("x-ratelimit-reset-requests".into(), "1h".into()),
        ]);
        engine.complete("long-window", result).unwrap();
        let mut result = completion(200);
        result.headers = BTreeMap::from([
            ("x-ratelimit-remaining-requests".into(), short_remaining.to_string()),
            ("x-ratelimit-reset-requests".into(), "1s".into()),
        ]);
        engine.complete("short-window", result).unwrap();
        assert_eq!(engine.views()[0].quota.remaining_requests, Some(0));
        engine.advance(1000).unwrap();
        assert_eq!(engine.views()[0].quota.remaining_requests, Some(expected));
        engine.advance(3_600_000).unwrap();
        assert_eq!(engine.views()[0].quota.remaining_requests, None);
    }
}

#[test]
fn account_snapshot_keeps_each_window_and_subtracts_existing_trial_reservations() {
    use crate::accounts::{QuotaSnapshot, QuotaSource, QuotaUnit, QuotaWindow};
    let (mut configuration, mut facts) = configuration(Strategy::QuotaAware, 1);
    configuration.groups[0].channels[0].account_id = Some("account-one".into());
    let provider = facts.get_mut("provider-0").unwrap();
    provider.account_ids.push("account-one".into());
    provider.account_quotas.insert("account-one".into(), QuotaSnapshot {
        source: QuotaSource::ProviderResponse, observed_at: 1, valid_until: 7200,
        windows: vec![
            QuotaWindow { id: "minute".into(), unit: QuotaUnit::Tokens, remaining: Some(500.0), limit: Some(1000.0), resets_at: Some(60) },
            QuotaWindow { id: "hour".into(), unit: QuotaUnit::Tokens, remaining: Some(5000.0), limit: Some(10000.0), resets_at: Some(3600) },
        ],
    });
    let mut engine = Engine::new(configuration, facts, BTreeMap::new()).unwrap();
    choose(&mut engine, "already-selected");
    engine.advance(1000).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_tokens, Some(400));
    engine.advance(60_000).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_tokens, Some(4900));
    engine.advance(3_600_000).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_tokens, None);
}

#[test]
fn actual_token_overage_remains_charged_to_earlier_long_window_with_new_headers() {
    let mut engine = engine(Strategy::RoundRobin, 1); choose(&mut engine, "seed");
    let mut result = completion(200);
    result.headers = BTreeMap::from([
        ("x-ratelimit-remaining-tokens".into(), "500".into()),
        ("x-ratelimit-reset-tokens".into(), "1h".into()),
    ]);
    engine.complete("seed", result).unwrap(); choose(&mut engine, "actual");
    let mut result = completion(200); result.usage.tokens = Some(160);
    result.headers = BTreeMap::from([
        ("x-ratelimit-remaining-tokens".into(), "1000".into()),
        ("x-ratelimit-reset-tokens".into(), "1s".into()),
    ]);
    engine.complete("actual", result).unwrap(); engine.advance(1000).unwrap();
    assert_eq!(engine.views()[0].quota.remaining_tokens, Some(340));
}

fn database_group() -> (Database, SaveGroup) {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let (mut configuration, _) = configuration(Strategy::RoundRobin, 1);
    configuration.groups[0].channels[0].provider_id = "openai".into();
    (database, SaveGroup { group: configuration.groups.remove(0), project: None })
}

#[test]
fn configuration_crud_uses_revisions_without_mutating_provider_settings() {
    let (database, input) = database_group();
    let before = serde_json::to_value(database.list_providers().unwrap()).unwrap();
    assert!(store::list(&database, &[]).unwrap().configuration.groups.is_empty());
    let saved = store::save(&database, &[], input.clone()).unwrap();
    assert_eq!(saved.configuration.groups[0].revision, 1); assert!(!saved.live_enabled); assert!(saved.billing_hold);
    assert_eq!(store::save(&database, &[], input).unwrap_err(), "group_revision_conflict");
    let disabled = store::set_enabled(&database, &[], "group-a", false, 1).unwrap(); assert!(!disabled.configuration.groups[0].enabled);
    assert_eq!(store::set_enabled(&database, &[], "group-a", true, 1).unwrap_err(), "group_revision_conflict");
    assert_eq!(before, serde_json::to_value(database.list_providers().unwrap()).unwrap());
}

#[test]
fn project_and_group_save_are_atomic_on_revision_conflict() {
    let (database, mut input) = database_group(); input.group.project_id = Some("project".into());
    input.project = Some(ProjectBudget { id: "project".into(), daily_microusd: Some(1000), revision: 0 });
    let saved = store::save(&database, &[], input.clone()).unwrap();
    input.project.as_mut().unwrap().revision = 1; input.project.as_mut().unwrap().daily_microusd = Some(9999);
    assert_eq!(store::save(&database, &[], input).unwrap_err(), "group_revision_conflict");
    assert_eq!(store::list(&database, &[]).unwrap().configuration.projects, saved.configuration.projects);
}

#[test]
fn ledger_counts_attempt_reservation_once_and_keeps_missing_values_unknown() {
    let (database, input) = database_group(); store::save(&database, &[], input).unwrap();
    let connection = database.conn(); let connection = connection.lock().unwrap();
    connection.execute_batch("INSERT INTO request_events(id,provider_id,route,status_code,created_at) VALUES(7,'openai','chat',525,1000);
        INSERT INTO proxy_attempts(id,task_id,request_id,ordinal,provider_id,model,route,reserved_microusd,status,event_id,started_at) VALUES('attempt-one','task-one','req-one',1,'openai','model','chat',5000,'failed',7,1000);
        INSERT INTO budget_ledger(id,day,provider_id,model,tokens,cost_microusd,source,created_at) VALUES('attempt-one',0,'openai','model',100,5000,'local',1);").unwrap();
    let baseline = ledger::read(&connection, &std::collections::BTreeSet::from(["openai".into()])).unwrap();
    assert_eq!(baseline["openai"].days[&0].committed_microusd, 5000);
    assert_eq!(baseline["openai"].days[&0].unpriced_requests, 0);
    assert_eq!(baseline["openai"].usage.cost_known_requests, 0);
    assert_eq!(baseline["openai"].usage.token_known_requests, 0);
    connection.execute("UPDATE proxy_attempts SET actual_microusd=8000 WHERE id='attempt-one'", []).unwrap();
    assert_eq!(ledger::read(&connection, &std::collections::BTreeSet::from(["openai".into()])).unwrap()["openai"].days[&0].committed_microusd, 8000);
}

#[test]
fn stored_event_replay_and_simulation_have_zero_database_writes() {
    let (database, input) = database_group(); store::save(&database, &[], input).unwrap();
    {
        let connection = database.conn(); let connection = connection.lock().unwrap();
        connection.execute("INSERT INTO request_events(id,provider_id,route,status_code,created_at) VALUES(7,'openai','chat',525,1000)", []).unwrap();
    }
    let events = store::recorded_events(&database, "group-a", 20).unwrap();
    assert_eq!(events[0].tokens, None); assert_eq!(events[0].cost_microusd, None);
    let changes = database.conn().lock().unwrap().total_changes();
    let result = store::simulate(&database, &[], TrialInput { group_id: "group-a".into(), replay_event_ids: vec![7],
        steps: vec![TrialStep::Request { at_ms: 2000, request: request("after-525") }] }).unwrap();
    assert_eq!(result.live_dispatches, 0); assert_eq!(result.ledger_writes, 0);
    assert!(result.decisions[0].channel_id.is_none()); assert_eq!(result.replayed_event_ids, [7]);
    assert_eq!(database.conn().lock().unwrap().total_changes(), changes);
    assert!(store::simulate(&database, &[], TrialInput { group_id: "group-a".into(), replay_event_ids: vec![2], steps: vec![] }).is_err());
}

#[test]
fn validation_rejects_bad_ranges_duplicate_models_and_embedded_credentials() {
    let (mut configuration, _) = configuration(Strategy::RoundRobin, 1);
    configuration.groups[0].channels[0].weight = 0; assert!(store::validate(&configuration).is_err());
    configuration.groups[0].channels[0].weight = 1; configuration.groups[0].channels[0].upstream_model = "xy-group/recursive".into(); assert!(store::validate(&configuration).is_err());
    let mut invalid = request("zero"); invalid.reserve_tokens = 0; assert!(store::validate_request(&invalid).is_err());
    let json = serde_json::to_value(request("input")).unwrap(); let mut object = json.as_object().unwrap().clone(); object.insert("apiKey".into(), "secret".into());
    assert!(serde_json::from_value::<RouteRequest>(serde_json::Value::Object(object)).is_err());
}
