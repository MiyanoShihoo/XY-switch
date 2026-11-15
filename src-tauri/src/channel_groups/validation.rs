use super::types::*;
use std::collections::BTreeSet;

pub const MAX_AMOUNT: u64 = 1_000_000_000_000;

pub fn validate_identity(value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 256 || value.trim() != value || value.chars().any(char::is_control) {
        return Err("invalid_identity".into());
    }
    Ok(())
}

fn key(value: &str) -> Result<(), String> {
    validate_identity(value)?;
    if value.len() > 128 || !value.bytes().all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte)) {
        return Err("invalid_configuration_id".into());
    }
    Ok(())
}

pub fn amount(value: Option<u64>) -> Result<(), String> {
    if value.is_some_and(|value| value > MAX_AMOUNT) { return Err("amount_out_of_range".into()); }
    Ok(())
}

pub fn validate_request(request: &RouteRequest) -> Result<(), String> {
    validate_identity(&request.id)?;
    key(&request.group_id)?;
    if request.reserve_tokens == 0 || request.reserve_tokens > MAX_AMOUNT { return Err("invalid_token_reservation".into()); }
    amount(request.reserve_microusd)?;
    for value in [&request.session_key, &request.previous_response_id, &request.conversation_id].into_iter().flatten() {
        validate_identity(value)?;
    }
    Ok(())
}

pub fn validate(configuration: &Configuration) -> Result<(), String> {
    if configuration.groups.len() > 64 || configuration.projects.len() > 128
        || configuration.groups.iter().map(|group| group.channels.len()).sum::<usize>() > 512 {
        return Err("configuration_too_large".into());
    }
    let mut projects = BTreeSet::new();
    for project in &configuration.projects {
        key(&project.id)?;
        amount(project.daily_microusd)?;
        if project.revision >= MAX_AMOUNT || !projects.insert(&project.id) { return Err("invalid_project_revision_or_duplicate".into()); }
    }
    let mut groups = BTreeSet::new();
    let mut models = BTreeSet::new();
    let mut channels = BTreeSet::new();
    for group in &configuration.groups {
        key(&group.id)?;
        validate_identity(&group.name)?;
        let suffix = group.virtual_model.strip_prefix(MODEL_PREFIX).ok_or("virtual_model_namespace_required")?;
        key(suffix)?;
        if !groups.insert(&group.id) || !models.insert(&group.virtual_model) { return Err("duplicate_group_or_virtual_model".into()); }
        if group.revision >= MAX_AMOUNT || !(60..=2_592_000).contains(&group.affinity_ttl_seconds) { return Err("invalid_group_revision_or_affinity_ttl".into()); }
        if group.project_id.as_ref().is_some_and(|id| !projects.contains(id)) { return Err("project_not_configured".into()); }
        if group.channels.is_empty() || group.channels.len() > 32 { return Err("invalid_channel_count".into()); }
        for channel in &group.channels {
            key(&channel.id)?;
            validate_identity(&channel.provider_id)?;
            validate_identity(&channel.upstream_model)?;
            if super::is_virtual_model(&channel.upstream_model) { return Err("nested_virtual_model_not_allowed".into()); }
            if let Some(account) = &channel.account_id { validate_identity(account)?; }
            if let Some(pool) = &channel.quota_pool_id { key(pool)?; }
            if !channels.insert(&channel.id) { return Err("duplicate_channel_id".into()); }
            if channel.weight == 0 || channel.weight > 1000 { return Err("invalid_channel_weight".into()); }
            if channel.quota_independent && channel.quota_pool_id.is_some() { return Err("conflicting_quota_relationship".into()); }
            for limit in [channel.limits.rpm, channel.limits.tpm] {
                amount(limit)?;
                if limit == Some(0) { return Err("invalid_rate_limit".into()); }
            }
            amount(channel.limits.daily_microusd)?;
            if channel.limits.concurrency.is_some_and(|value| value == 0 || value > 100_000) { return Err("invalid_concurrency_limit".into()); }
            let cooldown = &channel.cooldown;
            if !(1..=86_400).contains(&cooldown.rate_limit_seconds) || !(1..=86_400).contains(&cooldown.failure_seconds)
                || !(cooldown.rate_limit_seconds..=604_800).contains(&cooldown.max_backoff_seconds) {
                return Err("invalid_cooldown_policy".into());
            }
        }
    }
    Ok(())
}
