use super::{engine::Engine, ledger, types::*};
pub use super::validation::{validate, validate_identity, validate_request};
use crate::{accounts::{AccountInfo, AccountKind, AccountStatus}, database::{Database, Provider}};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::{BTreeMap, BTreeSet};

const KEY: &str = "channel_groups.pilot.v1";

pub fn configuration_on(connection: &Connection) -> Result<Configuration, String> {
    let value: Option<String> = connection.query_row("SELECT value FROM app_settings WHERE key=?1", [KEY], |row| row.get(0))
        .optional().map_err(|error| error.to_string())?;
    let configuration = match value {
        None => Configuration::default(),
        Some(value) if value.len() <= 2_000_000 => serde_json::from_str(&value).map_err(|_| "invalid_saved_channel_groups")?,
        Some(_) => return Err("configuration_too_large".into()),
    };
    validate(&configuration)?;
    Ok(configuration)
}

fn facts_on(connection: &Connection, providers: &[Provider], accounts: &[AccountInfo]) -> Result<BTreeMap<String, ProviderFacts>, String> {
    let policies_exist: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='account_pool_policies')", [], |row| row.get(0)).map_err(|error| error.to_string())?;
    let mut facts = BTreeMap::new();
    for provider in providers {
        let mut value = ProviderFacts {
            origin: reqwest::Url::parse(&provider.endpoint).ok().map(|url| url.origin().ascii_serialization()).unwrap_or_default(),
            enabled: provider.enabled,
            ..Default::default()
        };
        if policies_exist {
            value.account_pool_configured = connection.query_row("SELECT EXISTS(SELECT 1 FROM account_pool_policies WHERE provider_id=?1)", [&provider.id], |row| row.get(0)).map_err(|error| error.to_string())?;
        }
        for account in accounts.iter().filter(|account| account.provider_id == provider.id) {
            value.account_pool_configured = true;
            value.account_ids.push(account.id.clone());
            value.account_models.insert(account.id.clone(), account.available_models.clone());
            if !account.enabled || !account.destination_approved || account.kind == AccountKind::NativeManaged
                || !matches!(account.status, AccountStatus::Available | AccountStatus::CoolingDown | AccountStatus::QuotaLimited) {
                value.unavailable_accounts.push(account.id.clone());
            }
            if let Some(until) = account.cooldown_until { value.account_cooldowns.insert(account.id.clone(), until.saturating_mul(1000)); }
            if let Some(quota) = &account.quota { value.account_quotas.insert(account.id.clone(), quota.clone()); }
        }
        facts.insert(provider.id.clone(), value);
    }
    Ok(facts)
}

fn provider_ids(configuration: &Configuration) -> BTreeSet<String> {
    configuration.groups.iter().flat_map(|group| group.channels.iter().map(|channel| channel.provider_id.clone())).collect()
}

fn engine_on(connection: &Connection, configuration: Configuration, providers: &[Provider], accounts: &[AccountInfo]) -> Result<Engine, String> {
    let facts = facts_on(connection, providers, accounts)?;
    let baseline = ledger::read(connection, &provider_ids(&configuration))?;
    Engine::new(configuration, facts, baseline)
}

pub fn list(database: &Database, accounts: &[AccountInfo]) -> Result<Snapshot, String> {
    let providers = database.list_providers()?;
    let connection = database.conn();
    let connection = connection.lock().map_err(|_| "channel_group_store_unavailable")?;
    let configuration = configuration_on(&connection)?;
    let mut engine = engine_on(&connection, configuration.clone(), &providers, accounts)?;
    let now = crate::model_verification::now_ms();
    let events = ledger::events(&connection, &provider_ids(&configuration), 2048)?;
    for event in events.iter().rev() { engine.replay(event)?; }
    engine.advance(now)?;
    let mut warnings = engine.warnings.clone();
    if events.len() == 2048 { warnings.push("health_history_limited_to_latest_2048_events".into()); }
    if events.iter().any(|event| event.status == Some(429)) { warnings.push("recorded_headers_unavailable_configured_cooldown_used".into()); }
    Ok(Snapshot { phase: "offlinePilot", live_enabled: false, billing_hold: true, configuration, channels: engine.views(), warnings, generated_at_ms: now })
}

fn validate_references(connection: &Connection, group: &ChannelGroup, accounts: &[AccountInfo]) -> Result<(), String> {
    for channel in &group.channels {
        let exists: bool = connection.query_row("SELECT EXISTS(SELECT 1 FROM providers WHERE id=?1)", [&channel.provider_id], |row| row.get(0)).map_err(|error| error.to_string())?;
        if !exists { return Err("provider_missing".into()); }
        if let Some(id) = &channel.account_id {
            let account = accounts.iter().find(|account| &account.id == id && account.provider_id == channel.provider_id).ok_or("account_missing_or_wrong_provider")?;
            if account.kind == AccountKind::NativeManaged { return Err("native_account_not_http_channel".into()); }
            if !account.available_models.is_empty() && !account.available_models.contains(&channel.upstream_model) { return Err("model_not_available_to_account".into()); }
        }
    }
    Ok(())
}

fn persist(connection: &Connection, configuration: &Configuration) -> Result<(), String> {
    validate(configuration)?;
    let value = serde_json::to_string(configuration).map_err(|error| error.to_string())?;
    connection.execute("INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP", params![KEY, value]).map_err(|error| error.to_string())?;
    Ok(())
}

pub fn save(database: &Database, accounts: &[AccountInfo], mut input: SaveGroup) -> Result<Snapshot, String> {
    {
        let connection = database.conn();
        let mut connection = connection.lock().map_err(|_| "channel_group_store_unavailable")?;
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let mut configuration = configuration_on(&transaction)?;
        if let Some(mut project) = input.project {
            if input.group.project_id.as_deref() != Some(&project.id) { return Err("project_reference_mismatch".into()); }
            let previous = configuration.projects.iter().find(|value| value.id == project.id).map_or(0, |value| value.revision);
            if project.revision != previous { return Err("project_revision_conflict".into()); }
            project.revision = project.revision.checked_add(1).ok_or("revision_overflow")?;
            configuration.projects.retain(|value| value.id != project.id);
            configuration.projects.push(project);
        }
        let previous = configuration.groups.iter().find(|value| value.id == input.group.id).map_or(0, |value| value.revision);
        if input.group.revision != previous { return Err("group_revision_conflict".into()); }
        input.group.revision = input.group.revision.checked_add(1).ok_or("revision_overflow")?;
        validate_references(&transaction, &input.group, accounts)?;
        configuration.groups.retain(|value| value.id != input.group.id);
        configuration.groups.push(input.group);
        persist(&transaction, &configuration)?;
        transaction.commit().map_err(|error| error.to_string())?;
    }
    list(database, accounts)
}

pub fn set_enabled(database: &Database, accounts: &[AccountInfo], group_id: &str, enabled: bool, expected_revision: u64) -> Result<Snapshot, String> {
    {
        let connection = database.conn();
        let mut connection = connection.lock().map_err(|_| "channel_group_store_unavailable")?;
        let transaction = connection.transaction().map_err(|error| error.to_string())?;
        let mut configuration = configuration_on(&transaction)?;
        let group = configuration.groups.iter_mut().find(|group| group.id == group_id).ok_or("group_missing")?;
        if group.revision != expected_revision { return Err("group_revision_conflict".into()); }
        group.enabled = enabled;
        group.revision = group.revision.checked_add(1).ok_or("revision_overflow")?;
        persist(&transaction, &configuration)?;
        transaction.commit().map_err(|error| error.to_string())?;
    }
    list(database, accounts)
}

pub fn recorded_events(database: &Database, group_id: &str, limit: usize) -> Result<Vec<RecordedEvent>, String> {
    if !(1..=200).contains(&limit) { return Err("invalid_event_limit".into()); }
    let connection = database.conn();
    let connection = connection.lock().map_err(|_| "channel_group_store_unavailable")?;
    let configuration = configuration_on(&connection)?;
    let group = configuration.groups.iter().find(|group| group.id == group_id).ok_or("group_missing")?;
    ledger::events(&connection, &group.channels.iter().map(|channel| channel.provider_id.clone()).collect(), limit)
}

pub fn simulate(database: &Database, accounts: &[AccountInfo], input: TrialInput) -> Result<TrialResult, String> {
    if input.steps.len() > 4096 || input.replay_event_ids.len() > 200 { return Err("trial_too_large".into()); }
    let providers = database.list_providers()?;
    let (mut engine, mut events) = {
        let connection = database.conn();
        let connection = connection.lock().map_err(|_| "channel_group_store_unavailable")?;
        let configuration = configuration_on(&connection)?;
        let group = configuration.groups.iter().find(|group| group.id == input.group_id).ok_or("group_missing")?;
        let allowed: BTreeSet<_> = group.channels.iter().map(|channel| channel.provider_id.clone()).collect();
        let mut ids = BTreeSet::new();
        let mut events = Vec::new();
        for id in &input.replay_event_ids {
            if !ids.insert(id) { return Err("duplicate_recorded_event".into()); }
            let event = connection.query_row("SELECT id,provider_id,created_at,status_code,latency_ms,total_tokens,input_tokens,output_tokens,cost,error_kind FROM request_events WHERE id=?1", [id], ledger::event_row)
                .optional().map_err(|error| error.to_string())?.ok_or("recorded_event_missing")?;
            if !allowed.contains(&event.provider_id) { return Err("recorded_event_outside_group".into()); }
            events.push(event);
        }
        (engine_on(&connection, configuration, &providers, accounts)?, events)
    };
    events.sort_by_key(|event| (event.at_ms, event.id));
    for event in &events { engine.replay(event)?; }
    let mut decisions = Vec::new();
    for step in input.steps {
        match step {
            TrialStep::Request { at_ms, request } => { engine.advance(at_ms)?; decisions.push(engine.select(request)?); }
            TrialStep::StreamStarted { at_ms, request_id } => { engine.advance(at_ms)?; engine.stream_started(&request_id)?; }
            TrialStep::Complete { at_ms, request_id, result } => { engine.advance(at_ms)?; engine.complete(&request_id, result)?; }
        }
    }
    let mut warnings = engine.warnings.clone();
    if events.iter().any(|event| event.status == Some(429)) { warnings.push("recorded_headers_unavailable_configured_cooldown_used".into()); }
    Ok(TrialResult { source: "offlineSimulation", live_dispatches: 0, ledger_writes: 0, decisions, replayed_event_ids: events.iter().map(|event| event.id).collect(), channels: engine.views(), warnings })
}
