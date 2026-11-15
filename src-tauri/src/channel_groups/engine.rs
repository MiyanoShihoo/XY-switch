use super::{headers, types::*};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Default)]
struct Runtime {
    window: Vec<WindowUse>,
    active: u32,
    errors: u64,
    observations: u64,
    latency: Option<f64>,
    auth_disabled: bool,
    last_unknown: bool,
    unknown_results: u64,
    usage: UsageTotals,
}

#[derive(Clone)]
struct WindowUse {
    at: i64,
    tokens: Option<u64>,
    request_id: String,
}

#[derive(Clone)]
struct Remaining {
    value: u64,
    reset: Option<i64>,
    valid_until: i64,
}

#[derive(Clone, Default)]
struct Quota {
    windows: Vec<Remaining>,
}

impl Quota {
    fn remaining(&self) -> Option<u64> {
        self.windows.iter().map(|window| window.value).min()
    }

    fn reset_at(&self) -> Option<i64> {
        let minimum = self.remaining()?;
        self.deadline_for(|window| window.value == minimum)
    }

    fn deadline_for(&self, matches: impl Fn(&Remaining) -> bool) -> Option<i64> {
        let mut deadline = None;
        for window in self.windows.iter().filter(|window| matches(window)) {
            let reset = window.reset?;
            deadline = Some(deadline.map_or(reset, |previous: i64| previous.max(reset)));
        }
        deadline
    }

    fn consume(&mut self, amount: u64) {
        for window in &mut self.windows { window.value = window.value.saturating_sub(amount); }
    }

    fn refund(&mut self, amount: u64) {
        for window in &mut self.windows { window.value = window.value.saturating_add(amount); }
    }

    fn observe(&mut self, observation: Remaining) {
        if let Some(window) = self.windows.iter_mut().find(|window| window.reset == observation.reset && window.valid_until == observation.valid_until) {
            window.value = window.value.min(observation.value);
        } else {
            self.windows.push(observation);
        }
    }
}

#[derive(Clone, Default)]
struct Pool {
    generation: u64,
    cooldown_until: Option<i64>,
    reason: Option<String>,
    strikes: u32,
    requests: Quota,
    tokens: Quota,
    source: Option<String>,
}

#[derive(Clone)]
struct Attempt {
    request: RouteRequest,
    channel_id: String,
    started: i64,
    output_started: bool,
    finished: bool,
    held: bool,
    committed: Option<u64>,
    not_sent: bool,
    quota_generation: u64,
    reserved_known_requests: bool,
    reserved_known_tokens: bool,
    charged_tokens: u64,
}

#[derive(Clone)]
struct Binding {
    channel_id: String,
    expires: i64,
}

pub struct Engine {
    configuration: Configuration,
    facts: BTreeMap<String, ProviderFacts>,
    ledger: BTreeMap<String, LedgerBaseline>,
    channels: BTreeMap<String, (String, Channel)>,
    pool_for: BTreeMap<String, String>,
    members: BTreeMap<String, Vec<String>>,
    runtime: BTreeMap<String, Runtime>,
    pools: BTreeMap<String, Pool>,
    attempts: BTreeMap<String, Attempt>,
    considered: BTreeSet<String>,
    bindings: BTreeMap<String, Binding>,
    cursor: BTreeMap<String, usize>,
    weights: BTreeMap<String, i64>,
    replayed: BTreeSet<i64>,
    seeded_accounts: BTreeSet<String>,
    pub warnings: Vec<String>,
    now: i64,
}

impl Engine {
    pub fn new(configuration: Configuration, facts: BTreeMap<String, ProviderFacts>, ledger: BTreeMap<String, LedgerBaseline>) -> Result<Self, String> {
        super::store::validate(&configuration)?;
        let mut channels = BTreeMap::new();
        for group in &configuration.groups {
            for channel in &group.channels {
                channels.insert(channel.id.clone(), (group.id.clone(), channel.clone()));
            }
        }
        let ids: Vec<_> = channels.keys().cloned().collect();
        let mut roots: Vec<usize> = (0..ids.len()).collect();
        let mut warnings = BTreeSet::new();
        for left in 0..ids.len() {
            for right in left + 1..ids.len() {
                let left_channel = &channels[&ids[left]].1;
                let right_channel = &channels[&ids[right]].1;
                let same_origin = facts.get(&left_channel.provider_id).zip(facts.get(&right_channel.provider_id))
                    .is_some_and(|(first, second)| !first.origin.is_empty() && first.origin == second.origin);
                let shared = left_channel.quota_pool_id.is_some() && left_channel.quota_pool_id == right_channel.quota_pool_id;
                let both_unknown = !left_channel.quota_independent && left_channel.quota_pool_id.is_none()
                    && !right_channel.quota_independent && right_channel.quota_pool_id.is_none();
                if left_channel.provider_id == right_channel.provider_id || same_origin || shared || both_unknown {
                    let old = roots[right];
                    let new = roots[left];
                    for root in &mut roots { if *root == old { *root = new; } }
                    if same_origin && (left_channel.quota_independent || right_channel.quota_independent) {
                        warnings.insert("same_origin_shared_conservatively".to_string());
                    }
                }
            }
        }
        let mut pool_for = BTreeMap::new();
        let mut members: BTreeMap<String, Vec<String>> = BTreeMap::new();
        for (index, id) in ids.iter().enumerate() {
            let pool_id = format!("pool:{}", ids[roots[index]]);
            pool_for.insert(id.clone(), pool_id.clone());
            members.entry(pool_id).or_default().push(id.clone());
            let channel = &channels[id].1;
            if !channel.quota_independent && channel.quota_pool_id.is_none() {
                warnings.insert("quota_relationship_unknown_shared_conservatively".into());
            }
            if channel.account_id.is_some() { warnings.insert("account_usage_attributed_to_provider_conservatively".into()); }
        }
        warnings.insert("provider_history_is_not_group_attributed".into());
        warnings.insert("routing_metrics_are_replay_and_simulation_not_live_counters".into());
        warnings.insert("superapi_525_billing_unresolved_live_dispatch_disabled".into());
        Ok(Self {
            runtime: ids.iter().map(|id| (id.clone(), Runtime::default())).collect(),
            pools: members.keys().map(|id| (id.clone(), Pool::default())).collect(),
            configuration, facts, ledger, channels, pool_for, members,
            attempts: BTreeMap::new(), considered: BTreeSet::new(), bindings: BTreeMap::new(),
            cursor: BTreeMap::new(), weights: BTreeMap::new(), replayed: BTreeSet::new(),
            seeded_accounts: BTreeSet::new(), warnings: warnings.into_iter().collect(), now: 0,
        })
    }

    pub fn advance(&mut self, at: i64) -> Result<(), String> {
        if at < self.now || !(0..=253_402_300_799_000).contains(&at) { return Err("trial_time_must_be_monotonic".into()); }
        self.now = at;
        for runtime in self.runtime.values_mut() { runtime.window.retain(|entry| entry.at > at.saturating_sub(60_000)); }
        for pool in self.pools.values_mut() {
            pool.requests.windows.retain(|window| window.valid_until > at);
            pool.tokens.windows.retain(|window| window.valid_until > at);
            if pool.cooldown_until.is_some_and(|until| until <= at) { pool.cooldown_until = None; pool.reason = None; }
            if pool.requests.windows.is_empty() && pool.tokens.windows.is_empty() { pool.source = None; }
        }
        self.seed_accounts();
        Ok(())
    }

    fn seed_accounts(&mut self) {
        use crate::accounts::{QuotaSource, QuotaUnit};
        let channels: Vec<_> = self.channels.values().map(|(_, channel)| channel.clone()).collect();
        for channel in channels {
            let Some(account) = &channel.account_id else { continue; };
            let Some(facts) = self.facts.get(&channel.provider_id) else { continue; };
            let pool_id = &self.pool_for[&channel.id];
            let mut reserved_requests = 0u64;
            let mut reserved_tokens = 0u64;
            for attempt in self.attempts.values().filter(|attempt| !attempt.not_sent && &self.pool_for[&attempt.channel_id] == pool_id) {
                reserved_requests = reserved_requests.saturating_add(1);
                reserved_tokens = reserved_tokens.saturating_add(attempt.charged_tokens);
            }
            let pool = self.pools.get_mut(pool_id).expect("configured pool");
            if let Some(until) = facts.account_cooldowns.get(account).copied().filter(|until| *until > self.now) {
                if pool.cooldown_until.is_none_or(|previous| previous < until) {
                    pool.cooldown_until = Some(until); pool.reason = Some("account_cooling_down".into());
                }
            }
            let Some(snapshot) = facts.account_quotas.get(account) else { continue; };
            if snapshot.source == QuotaSource::Unknown || snapshot.observed_at.saturating_mul(1000) > self.now
                || snapshot.valid_until.saturating_mul(1000) <= self.now { continue; }
            let key = format!("{}:{}:{}", channel.provider_id, account, snapshot.observed_at);
            if !self.seeded_accounts.insert(key) { continue; }
            for window in &snapshot.windows {
                let Some(remaining) = window.remaining.filter(|value| value.is_finite() && *value >= 0.0 && *value <= 9_007_199_254_740_991.0) else { continue; };
                let reset = window.resets_at.map(|at| at.saturating_mul(1000));
                if reset.is_some_and(|at| at <= self.now) { continue; }
                let (target, reserved) = match window.unit {
                    QuotaUnit::Requests => (&mut pool.requests, reserved_requests),
                    QuotaUnit::Tokens => (&mut pool.tokens, reserved_tokens),
                    QuotaUnit::Percent => continue,
                };
                let value = (remaining.floor() as u64).saturating_sub(reserved);
                let valid_until = snapshot.valid_until.saturating_mul(1000).min(reset.unwrap_or(i64::MAX));
                target.observe(Remaining { value, reset, valid_until });
                pool.source = Some("recordedAccountSnapshot".into());
            }
            pool.generation = pool.generation.saturating_add(1);
        }
    }

    fn channel_reason(&self, channel: &Channel) -> Option<&'static str> {
        if !channel.enabled { return Some("channel_disabled"); }
        let Some(facts) = self.facts.get(&channel.provider_id) else { return Some("provider_missing"); };
        if !facts.enabled { return Some("provider_disabled"); }
        if let Some(account) = &channel.account_id {
            if !facts.account_ids.contains(account) { return Some("account_missing"); }
            if facts.unavailable_accounts.contains(account) { return Some("account_unavailable"); }
            if facts.account_models.get(account).is_some_and(|models| !models.is_empty() && !models.contains(&channel.upstream_model)) { return Some("model_not_available_to_account"); }
        } else if facts.account_pool_configured { return Some("explicit_account_required"); }
        if self.runtime[&channel.id].auth_disabled { return Some("authentication_disabled"); }
        None
    }

    fn pool_window(&self, channel_id: &str) -> (u64, u64, bool, u32, Option<i64>) {
        let mut requests = 0u64;
        let mut tokens = 0u64;
        let mut unknown = false;
        let mut active = 0u32;
        let mut next = None;
        for id in &self.members[&self.pool_for[channel_id]] {
            let state = &self.runtime[id];
            active = active.saturating_add(state.active);
            for entry in &state.window {
                requests = requests.saturating_add(1);
                tokens = tokens.saturating_add(entry.tokens.unwrap_or(0));
                unknown |= entry.tokens.is_none();
                let expires = entry.at.saturating_add(60_000);
                next = Some(next.map_or(expires, |previous: i64| previous.min(expires)));
            }
        }
        (requests, tokens, unknown, active, next)
    }

    fn pool_limits(&self, channel_id: &str) -> Limits {
        let channels: Vec<_> = self.members[&self.pool_for[channel_id]].iter().map(|id| &self.channels[id].1).collect();
        Limits {
            rpm: channels.iter().filter_map(|channel| channel.limits.rpm).min(),
            tpm: channels.iter().filter_map(|channel| channel.limits.tpm).min(),
            concurrency: channels.iter().filter_map(|channel| channel.limits.concurrency).min(),
            daily_microusd: None,
        }
    }

    fn commitment(&self, providers: &BTreeSet<String>, project: Option<&str>, channel_id: &str) -> (u64, bool) {
        let mut committed = 0u64;
        let mut unknown = false;
        let day = self.now / DAY_MS;
        for provider in providers {
            if let Some(baseline) = self.ledger.get(provider).and_then(|baseline| baseline.days.get(&day)) {
                committed = committed.saturating_add(baseline.committed_microusd);
                unknown |= baseline.unpriced_requests > 0;
            }
        }
        for attempt in self.attempts.values().filter(|attempt| attempt.started / DAY_MS == day) {
            let (group_id, _) = &self.channels[&attempt.channel_id];
            let belongs = match project {
                Some(project_id) => self.configuration.groups.iter().any(|group| &group.id == group_id && group.project_id.as_deref() == Some(project_id)),
                None => attempt.channel_id == channel_id,
            };
            if belongs {
                committed = committed.saturating_add(attempt.committed.unwrap_or(0));
                unknown |= attempt.committed.is_none();
            }
        }
        (committed, unknown)
    }

    fn budget_reason(&self, channel: &Channel, request: &RouteRequest) -> Option<&'static str> {
        let group = self.configuration.groups.iter().find(|group| group.id == request.group_id)?;
        let project = group.project_id.as_ref().and_then(|id| self.configuration.projects.iter().find(|project| &project.id == id));
        for (limit, project_id) in [(channel.limits.daily_microusd, None), (project.and_then(|value| value.daily_microusd), group.project_id.as_deref())] {
            let Some(limit) = limit else { continue; };
            let Some(reserve) = request.reserve_microusd else { return Some("cost_reservation_required"); };
            let providers = match project_id {
                None => BTreeSet::from([channel.provider_id.clone()]),
                Some(id) => self.configuration.groups.iter().filter(|group| group.project_id.as_deref() == Some(id))
                    .flat_map(|group| group.channels.iter().map(|channel| channel.provider_id.clone())).collect(),
            };
            let (committed, unknown) = self.commitment(&providers, project_id, &channel.id);
            if unknown { return Some("unresolved_ledger_cost"); }
            if committed.checked_add(reserve).is_none_or(|total| total > limit) {
                return Some(if project_id.is_some() { "project_budget_exhausted" } else { "channel_budget_exhausted" });
            }
        }
        None
    }

    fn exclusion(&self, channel: &Channel, request: &RouteRequest) -> Option<Exclusion> {
        let rejected = |reason: &str, retry_at_ms| Some(Exclusion { channel_id: channel.id.clone(), reason: reason.into(), retry_at_ms });
        if let Some(reason) = self.channel_reason(channel) { return rejected(reason, None); }
        let pool = &self.pools[&self.pool_for[&channel.id]];
        if let Some(until) = pool.cooldown_until { return rejected("cooling_down", Some(until)); }
        let (requests, tokens, unknown, active, next) = self.pool_window(&channel.id);
        let limits = self.pool_limits(&channel.id);
        if limits.rpm.is_some_and(|limit| requests >= limit) { return rejected("rpm_limit", next); }
        if limits.tpm.is_some() && unknown { return rejected("token_window_unknown", next); }
        if limits.tpm.is_some_and(|limit| tokens.checked_add(request.reserve_tokens).is_none_or(|total| total > limit)) { return rejected("tpm_limit", next); }
        if limits.concurrency.is_some_and(|limit| active >= limit) { return rejected("concurrency_limit", None); }
        if pool.requests.remaining() == Some(0) { return rejected("request_quota_exhausted", pool.requests.deadline_for(|window| window.value == 0)); }
        if pool.tokens.remaining().is_some_and(|value| value < request.reserve_tokens) { return rejected("token_quota_exhausted", pool.tokens.deadline_for(|window| window.value < request.reserve_tokens)); }
        if let Some(reason) = self.budget_reason(channel, request) { return rejected(reason, None); }
        None
    }

    fn binding_key(group_id: &str, kind: &str, value: &str) -> String {
        format!("{group_id}:{kind}:{:x}", Sha256::digest(value.as_bytes()))
    }

    fn affinity(&self, request: &RouteRequest) -> Result<Option<String>, &'static str> {
        if request.stateful && request.session_key.is_none() && request.previous_response_id.is_none() && request.conversation_id.is_none() {
            return Err("stateful_identity_required");
        }
        let mut pinned = None;
        for (kind, identity, required) in [("session", &request.session_key, false), ("response", &request.previous_response_id, true), ("conversation", &request.conversation_id, true)] {
            let Some(identity) = identity else { continue; };
            match self.bindings.get(&Self::binding_key(&request.group_id, kind, identity)) {
                Some(binding) => {
                    if binding.expires <= self.now { return Err("affinity_expired"); }
                    if pinned.as_ref().is_some_and(|previous| previous != &binding.channel_id) { return Err("affinity_conflict"); }
                    pinned = Some(binding.channel_id.clone());
                }
                None if required => return Err("affinity_unknown"),
                None => (),
            }
        }
        Ok(pinned)
    }

    pub fn select(&mut self, request: RouteRequest) -> Result<Decision, String> {
        super::store::validate_request(&request)?;
        let mut decision = Decision { request_id: request.id.clone(), channel_id: None, upstream_model: None, reason: String::new(), excluded: Vec::new(), dispatch_allowed: false };
        if !self.considered.insert(request.id.clone()) { decision.reason = "request_already_considered".into(); return Ok(decision); }
        let Some(group) = self.configuration.groups.iter().find(|group| group.id == request.group_id).cloned() else { decision.reason = "group_missing".into(); return Ok(decision); };
        if !group.enabled { decision.reason = "group_disabled".into(); return Ok(decision); }
        let pinned = match self.affinity(&request) { Ok(pinned) => pinned, Err(reason) => { decision.reason = reason.into(); return Ok(decision); } };
        let mut eligible = Vec::new();
        for (index, channel) in group.channels.iter().enumerate() {
            if let Some(exclusion) = self.exclusion(channel, &request) { decision.excluded.push(exclusion); }
            else { eligible.push(index); }
        }
        if let Some(id) = &pinned {
            eligible.retain(|index| &group.channels[*index].id == id);
            if eligible.is_empty() { decision.reason = "affinity_unavailable".into(); return Ok(decision); }
        } else if let Some(priority) = eligible.iter().map(|index| group.channels[*index].priority).min() {
            eligible.retain(|index| group.channels[*index].priority == priority);
        }
        if eligible.is_empty() { decision.reason = "no_eligible_channel".into(); return Ok(decision); }
        let cursor = self.cursor.get(&group.id).copied().unwrap_or(0) % group.channels.len();
        eligible.sort_by_key(|index| (index + group.channels.len() - cursor) % group.channels.len());
        let mut chosen = eligible[0];
        match group.strategy {
            Strategy::RoundRobin => (),
            Strategy::WeightedRoundRobin => {
                let mut total = 0i64;
                for index in &eligible {
                    let channel = &group.channels[*index];
                    let weight = i64::from(channel.weight);
                    *self.weights.entry(channel.id.clone()).or_default() += weight;
                    total += weight;
                }
                for index in &eligible {
                    if self.weights[&group.channels[*index].id] > self.weights[&group.channels[chosen].id] { chosen = *index; }
                }
                *self.weights.get_mut(&group.channels[chosen].id).expect("eligible weight") -= total;
            }
            Strategy::LeastLoad => {
                let load = |index: usize| {
                    let channel = &group.channels[index];
                    f64::from(self.runtime[&channel.id].active) / f64::from(channel.limits.concurrency.unwrap_or(1))
                };
                for index in &eligible { if load(*index) < load(chosen) { chosen = *index; } }
            }
            Strategy::LatencyFirst => {
                let latency = |index: usize| self.runtime[&group.channels[index].id].latency.unwrap_or(f64::INFINITY);
                for index in &eligible { if latency(*index) < latency(chosen) { chosen = *index; } }
            }
            Strategy::QuotaAware => {
                let capacity = |index: usize| {
                    let pool = &self.pools[&self.pool_for[&group.channels[index].id]];
                    pool.requests.remaining().into_iter()
                        .chain(pool.tokens.remaining().map(|value| value / request.reserve_tokens)).min()
                };
                for index in &eligible { if capacity(*index) > capacity(chosen) { chosen = *index; } }
            }
        }
        let channel = &group.channels[chosen];
        self.cursor.insert(group.id.clone(), (chosen + 1) % group.channels.len());
        let runtime = self.runtime.get_mut(&channel.id).expect("configured channel");
        runtime.active = runtime.active.saturating_add(1);
        runtime.usage.requests = runtime.usage.requests.saturating_add(1);
        runtime.window.push(WindowUse { at: self.now, tokens: Some(request.reserve_tokens), request_id: request.id.clone() });
        let pool = self.pools.get_mut(&self.pool_for[&channel.id]).expect("configured pool");
        let quota_generation = pool.generation;
        let reserved_known_requests = pool.requests.remaining().is_some();
        let reserved_known_tokens = pool.tokens.remaining().is_some();
        pool.requests.consume(1);
        pool.tokens.consume(request.reserve_tokens);
        if let Some(session) = &request.session_key {
            self.bindings.insert(Self::binding_key(&group.id, "session", session), Binding { channel_id: channel.id.clone(), expires: self.now.saturating_add((group.affinity_ttl_seconds * 1000) as i64) });
        }
        decision.channel_id = Some(channel.id.clone());
        decision.upstream_model = Some(channel.upstream_model.clone());
        decision.reason = if pinned.is_some() { "session_affinity" } else { "selected_for_simulation" }.into();
        self.attempts.insert(request.id.clone(), Attempt { channel_id: channel.id.clone(), started: self.now, output_started: false, finished: false, held: true, committed: request.reserve_microusd, not_sent: false, quota_generation, reserved_known_requests, reserved_known_tokens, charged_tokens: request.reserve_tokens, request });
        Ok(decision)
    }

    pub fn stream_started(&mut self, id: &str) -> Result<(), String> {
        let attempt = self.attempts.get_mut(id).ok_or("request_not_selected")?;
        if attempt.finished { return Err("request_already_completed".into()); }
        attempt.output_started = true;
        Ok(())
    }

    fn observe(&mut self, channel_id: &str, status: Option<u16>, evidence: headers::HeaderEvidence, current_id: Option<&str>, source: &str) {
        let channel = self.channels[channel_id].1.clone();
        if matches!(status, Some(401 | 403)) {
            for (id, (_, candidate)) in &self.channels {
                if candidate.provider_id == channel.provider_id && candidate.account_id == channel.account_id {
                    self.runtime.get_mut(id).expect("configured channel").auth_disabled = true;
                }
            }
        }
        let pool_id = self.pool_for[channel_id].clone();
        let mut held_requests = 0u64;
        let mut held_tokens = 0u64;
        for (id, attempt) in &self.attempts {
            if Some(id.as_str()) != current_id && attempt.held && self.pool_for[&attempt.channel_id] == pool_id {
                held_requests = held_requests.saturating_add(1);
                held_tokens = held_tokens.saturating_add(attempt.charged_tokens);
            }
        }
        let pool = self.pools.get_mut(&pool_id).expect("configured pool");
        if evidence.requests.is_some() || evidence.tokens.is_some() { pool.generation = pool.generation.saturating_add(1); }
        let merge = |previous: &mut Quota, value: Option<u64>, reset: Option<i64>, held: u64| {
            if let Some(value) = value {
                let value = value.saturating_sub(held);
                let valid_until = reset.unwrap_or(if value == 0 { i64::MAX } else { self.now.saturating_add(60_000) });
                if valid_until > self.now {
                    previous.observe(Remaining { value, reset, valid_until });
                }
            }
        };
        merge(&mut pool.requests, evidence.requests, evidence.requests_reset, held_requests);
        merge(&mut pool.tokens, evidence.tokens, evidence.tokens_reset, held_tokens);
        if evidence.requests.is_some() || evidence.tokens.is_some() { pool.source = Some(source.into()); }
        let cooling = if status == Some(429) {
            pool.strikes = pool.strikes.saturating_add(1);
            let explicit = [evidence.retry_at, evidence.requests_reset, evidence.tokens_reset,
                pool.requests.reset_at(), pool.tokens.reset_at()]
                .into_iter().flatten().filter(|until| *until > self.now).max();
            let fallback = channel.cooldown.rate_limit_seconds.saturating_mul(1u64 << pool.strikes.saturating_sub(1).min(20)).min(channel.cooldown.max_backoff_seconds);
            Some((explicit.unwrap_or_else(|| self.now.saturating_add((fallback * 1000) as i64)), "rate_limited"))
        } else if status.is_none() || status.is_some_and(|code| code >= 500) {
            Some((self.now.saturating_add((channel.cooldown.failure_seconds * 1000) as i64), "upstream_result_unknown"))
        } else { None };
        if let Some((until, reason)) = cooling {
            if pool.cooldown_until.is_none_or(|previous| until >= previous) {
                pool.cooldown_until = Some(until);
                pool.reason = Some(reason.into());
            }
        }
    }

    pub fn complete(&mut self, id: &str, result: Completion) -> Result<(), String> {
        let attempt = self.attempts.get(id).ok_or("request_not_selected")?.clone();
        if attempt.finished { return Err("request_already_completed".into()); }
        super::validation::amount(result.usage.tokens)?;
        super::validation::amount(result.usage.microusd)?;
        if result.outcome == Outcome::Http && !result.status.is_some_and(|status| (200..=599).contains(&status)) { return Err("http_status_required".into()); }
        if result.outcome != Outcome::Http && result.status.is_some() { return Err("unexpected_http_status".into()); }
        if result.outcome == Outcome::NotSent && (attempt.output_started || result.usage.tokens.is_some() || result.usage.microusd.is_some() || !result.headers.is_empty()) { return Err("not_sent_evidence_conflict".into()); }
        let evidence = headers::parse(&result.headers, self.now)?;
        let success = result.outcome == Outcome::Http && result.status.is_some_and(|status| (200..300).contains(&status));
        let unknown = matches!(result.outcome, Outcome::Timeout | Outcome::TransportUnknown | Outcome::Cancelled) || result.status.is_some_and(|status| status >= 500);
        let group = self.configuration.groups.iter().find(|group| group.id == attempt.request.group_id).expect("configured group").clone();
        let expires = self.now.saturating_add((group.affinity_ttl_seconds * 1000) as i64);
        let mut new_bindings = Vec::new();
        for (kind, identity) in [("response", result.response_id.as_deref()), ("conversation", result.conversation_id.as_deref())] {
            if let Some(identity) = identity {
                super::store::validate_identity(identity)?;
                if !success { return Err("state_id_requires_success".into()); }
                let key = Self::binding_key(&group.id, kind, identity);
                if self.bindings.get(&key).is_some_and(|binding| binding.channel_id != attempt.channel_id) { return Err("response_affinity_conflict".into()); }
                new_bindings.push(key);
            }
        }
        for key in new_bindings { self.bindings.insert(key, Binding { channel_id: attempt.channel_id.clone(), expires }); }
        if result.outcome != Outcome::NotSent {
            if let Some(actual) = result.usage.tokens.filter(|actual| *actual > attempt.request.reserve_tokens) {
                self.pools.get_mut(&self.pool_for[&attempt.channel_id]).expect("configured pool").tokens.consume(actual - attempt.request.reserve_tokens);
            }
            self.observe(&attempt.channel_id, result.status, evidence, Some(id), "simulatedResponse");
        } else {
            let pool = self.pools.get_mut(&self.pool_for[&attempt.channel_id]).expect("configured pool");
            if pool.generation == attempt.quota_generation {
                if attempt.reserved_known_requests { pool.requests.refund(1); }
                if attempt.reserved_known_tokens { pool.tokens.refund(attempt.request.reserve_tokens); }
            }
        }
        let runtime = self.runtime.get_mut(&attempt.channel_id).expect("configured channel");
        if !unknown { runtime.active = runtime.active.saturating_sub(1); }
        runtime.last_unknown = unknown;
        if unknown { runtime.unknown_results = runtime.unknown_results.saturating_add(1); }
        if result.outcome == Outcome::NotSent {
            runtime.window.retain(|entry| entry.request_id != id);
            runtime.usage.requests = runtime.usage.requests.saturating_sub(1);
        } else {
            runtime.observations = runtime.observations.saturating_add(1);
            if !success { runtime.errors = runtime.errors.saturating_add(1); }
            let latency = self.now.saturating_sub(attempt.started) as f64;
            runtime.latency = Some(runtime.latency.map_or(latency, |old| old * 0.8 + latency * 0.2));
            if let Some(tokens) = result.usage.tokens {
                runtime.usage.known_tokens = runtime.usage.known_tokens.saturating_add(tokens);
                runtime.usage.token_known_requests = runtime.usage.token_known_requests.saturating_add(1);
                if let Some(entry) = runtime.window.iter_mut().find(|entry| entry.request_id == id) { entry.tokens = Some(tokens); }
            }
            if let Some(cost) = result.usage.microusd {
                runtime.usage.known_microusd = runtime.usage.known_microusd.saturating_add(cost);
                runtime.usage.cost_known_requests = runtime.usage.cost_known_requests.saturating_add(1);
            }
        }
        let saved = self.attempts.get_mut(id).expect("selected attempt");
        saved.finished = true;
        saved.held = unknown;
        saved.not_sent = result.outcome == Outcome::NotSent;
        saved.charged_tokens = result.usage.tokens.unwrap_or(attempt.request.reserve_tokens).max(attempt.request.reserve_tokens);
        if result.outcome == Outcome::NotSent { saved.committed = Some(0); }
        else if let Some(cost) = result.usage.microusd { saved.committed = Some(cost.max(saved.committed.unwrap_or(0))); }
        if result.outcome == Outcome::NotSent {
            if let Some(session) = &attempt.request.session_key {
                let used = self.attempts.values().any(|candidate| !candidate.not_sent && candidate.request.group_id == group.id && candidate.request.session_key.as_ref() == Some(session));
                if !used { self.bindings.remove(&Self::binding_key(&group.id, "session", session)); }
            }
        }
        Ok(())
    }

    pub fn replay(&mut self, event: &RecordedEvent) -> Result<(), String> {
        if !self.replayed.insert(event.id) { return Err("duplicate_recorded_event".into()); }
        self.advance(event.at_ms)?;
        let ids: Vec<_> = self.channels.iter().filter(|(_, (_, channel))| channel.provider_id == event.provider_id).map(|(id, _)| id.clone()).collect();
        let mut accounted_pools = BTreeSet::new();
        for id in ids {
            let first_in_pool = accounted_pools.insert(self.pool_for[&id].clone());
            if first_in_pool { self.observe(&id, event.status, headers::HeaderEvidence::default(), None, "recordedResponse"); }
            let runtime = self.runtime.get_mut(&id).expect("configured channel");
            if matches!(event.status, Some(401 | 403)) { runtime.auth_disabled = true; }
            runtime.observations = runtime.observations.saturating_add(1);
            if !event.status.is_some_and(|status| (200..300).contains(&status)) { runtime.errors = runtime.errors.saturating_add(1); }
            if let Some(latency) = event.latency_ms { runtime.latency = Some(latency as f64); }
            runtime.last_unknown = event.status.is_none() || event.status.is_some_and(|status| status >= 500);
            if runtime.last_unknown { runtime.unknown_results = runtime.unknown_results.saturating_add(1); }
            if first_in_pool {
                runtime.window.push(WindowUse { at: event.at_ms, tokens: event.tokens, request_id: format!("recorded:{}", event.id) });
            }
        }
        Ok(())
    }

    pub fn views(&self) -> Vec<ChannelView> {
        self.channels.iter().map(|(id, (group_id, channel))| {
            let state = &self.runtime[id];
            let pool_id = &self.pool_for[id];
            let pool = &self.pools[pool_id];
            let group_enabled = self.configuration.groups.iter().any(|group| &group.id == group_id && group.enabled);
            let health = if !group_enabled { "group_disabled" } else if let Some(reason) = self.channel_reason(channel) { reason }
                else if pool.cooldown_until.is_some() { "cooling_down" } else if state.last_unknown { "unknown_result" }
                else if pool.requests.remaining() == Some(0) || pool.tokens.remaining() == Some(0) { "quota_exhausted" }
                else if state.observations == 0 { "unobserved" } else if state.errors > 0 { "degraded" } else { "ready" };
            ChannelView {
                group_id: group_id.clone(), channel_id: id.clone(), provider_id: channel.provider_id.clone(), pool_id: pool_id.clone(),
                pool_declared: self.members[pool_id].iter().all(|member| { let channel = &self.channels[member].1; channel.quota_independent || channel.quota_pool_id.is_some() }),
                health: health.into(), cooldown_reason: pool.reason.clone(), recovery_at_ms: pool.cooldown_until,
                rpm: state.window.len() as u64, tpm_reserved: state.window.iter().fold(0u64, |sum, entry| sum.saturating_add(entry.tokens.unwrap_or(0))),
                tpm_unknown_requests: state.window.iter().filter(|entry| entry.tokens.is_none()).count() as u64,
                concurrent: state.active, error_rate: (state.observations > 0).then(|| state.errors as f64 / state.observations as f64), latency_ms: state.latency,
                quota: QuotaView { remaining_requests: pool.requests.remaining(), remaining_tokens: pool.tokens.remaining(),
                    requests_reset_at_ms: pool.requests.reset_at(), tokens_reset_at_ms: pool.tokens.reset_at(), source: pool.source.clone() },
                recorded_usage: self.ledger.get(&channel.provider_id).map(|value| value.usage.clone()).unwrap_or_default(),
                simulated_usage: state.usage.clone(), unknown_results: state.unknown_results,
            }
        }).collect()
    }
}
