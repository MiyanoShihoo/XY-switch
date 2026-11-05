use super::{NativeAccountSnapshot, NativeProviderConnection};
use crate::accounts::{AccountClient, AccountCredentialInput, AccountError, AccountInfo, AccountPool,
    AccountStatus, AddAccountRequest, NativeAccountState, QuotaSnapshot, QuotaSource, QuotaUnit, QuotaWindow};
use crate::database::Database;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, sync::{Mutex, atomic::{AtomicU64, Ordering}}};
use uuid::Uuid;
use zeroize::Zeroizing;

const STORAGE_KEY: &str = "native_account_bindings_v1";
const CANDIDATE_LIFETIME: i64 = 300;
const BINDING_ERROR: &str = "原生账号绑定已变化或过期，请重新读取账号并确认绑定";
const STORE_ERROR: &str = "原生账号绑定存储不可用，操作已阻断";

#[derive(Clone)]
pub(super) struct NativeIdentity {
    subject: Zeroizing<String>,
    label: String,
}

impl NativeIdentity {
    pub(super) fn codex(account: &serde_json::Value) -> Option<Self> {
        if account["type"].as_str() != Some("chatgpt") { return None; }
        let identifier = account["accountId"].as_str().filter(|value| !value.is_empty()
            && value.len() <= 512 && !value.chars().any(char::is_control));
        let email = account["email"].as_str().filter(|value| value.len() <= 254
            && value.contains('@') && !value.chars().any(|character| character.is_control() || character.is_whitespace()));
        let subject = match (identifier, email) {
            (Some(identifier), _) => format!("id:{identifier}"),
            (None, Some(email)) => format!("email:{email}"),
            _ => return None,
        };
        Some(Self { subject: Zeroizing::new(subject), label: "ChatGPT 订阅".into() })
    }

    pub(super) fn opencode(connection: &NativeProviderConnection) -> Option<Self> {
        let identifier = connection.connection_id.as_deref().filter(|value| !value.is_empty())?;
        if !connection.disconnectable { return None; }
        Some(Self { subject: Zeroizing::new(serde_json::to_string(&[connection.provider_id.as_str(), identifier]).ok()?),
            label: crate::privacy::redact_diagnostic(&connection.label) })
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativePoolCandidate {
    pub id: String,
    pub label: String,
    pub expires_at: i64,
    pub bound_account_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BindNativeAccountRequest {
    pub client_id: String,
    pub candidate_id: String,
    pub alias: String,
    pub provider_id: String,
    pub approved_destination: String,
    pub confirmed: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Binding {
    account_id: String,
    client: AccountClient,
    owner: String,
    scope: String,
    identity: String,
    reference_id: String,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedBindings {
    salt: String,
    bindings: Vec<Binding>,
}

struct Candidate {
    owner: String,
    scope: String,
    identity: String,
    expires_at: i64,
}

struct State {
    saved: SavedBindings,
    candidates: BTreeMap<String, Candidate>,
}

pub(super) struct NativePoolBridge {
    database: Database,
    pool: AccountPool,
    state: Mutex<State>,
}

pub(super) fn owner(client: &str) -> Result<&'static str, String> {
    match client {
        "codex-desktop" | "codex-cli" => Ok("codex"),
        "opencode-desktop" | "opencode-cli" => Ok("opencode"),
        _ => Err("此客户端尚未提供可验证的原生账号绑定接口".into()),
    }
}

pub(super) fn client_id(client: AccountClient) -> Result<&'static str, String> {
    match client {
        AccountClient::CodexDesktop => Ok("codex-desktop"),
        AccountClient::CodexCli => Ok("codex-cli"),
        AccountClient::OpenCodeDesktop => Ok("opencode-desktop"),
        AccountClient::OpenCodeCli => Ok("opencode-cli"),
        _ => Err(BINDING_ERROR.into()),
    }
}

fn account_client(client: &str) -> Result<AccountClient, String> {
    match client {
        "codex-desktop" => Ok(AccountClient::CodexDesktop),
        "codex-cli" => Ok(AccountClient::CodexCli),
        "opencode-desktop" => Ok(AccountClient::OpenCodeDesktop),
        "opencode-cli" => Ok(AccountClient::OpenCodeCli),
        _ => Err(BINDING_ERROR.into()),
    }
}

fn digest(salt: &str, parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    for part in std::iter::once(salt).chain(parts.iter().copied()) {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

impl NativePoolBridge {
    pub(super) fn new(database: Database, pool: AccountPool) -> Result<Self, String> {
        let mut saved: SavedBindings = crate::governance::load(&database, STORAGE_KEY).map_err(|_| STORE_ERROR)?;
        if saved.salt.is_empty() {
            if !saved.bindings.is_empty() { return Err(STORE_ERROR.into()); }
            saved.salt = Uuid::new_v4().to_string();
            crate::governance::save(&database, STORAGE_KEY, &saved).map_err(|_| STORE_ERROR)?;
        }
        if Uuid::parse_str(&saved.salt).is_err() || saved.bindings.len() > 256 { return Err(STORE_ERROR.into()); }
        Ok(Self { database, pool, state: Mutex::new(State { saved, candidates: BTreeMap::new() }) })
    }

    pub(super) fn scope(&self, owner: &str, paths: &[&Path]) -> Result<String, String> {
        let state = self.state.lock().map_err(|_| STORE_ERROR)?;
        let mut parts = vec!["config", owner];
        for path in paths {
            if !path.is_absolute() { return Err(BINDING_ERROR.into()); }
            parts.push(path.to_str().ok_or(BINDING_ERROR)?);
        }
        Ok(digest(&state.saved.salt, &parts))
    }

    fn identities(state: &State, owner: &str, snapshot: &NativeAccountSnapshot) -> BTreeMap<String, String> {
        snapshot.identities.iter().map(|identity| (
            digest(&state.saved.salt, &["identity", owner, identity.subject.as_str()]), identity.label.clone(),
        )).collect()
    }

    fn save(&self, state: &State) -> Result<(), String> {
        crate::governance::save(&self.database, STORAGE_KEY, &state.saved).map_err(|_| STORE_ERROR.into())
    }

    fn active(&self, binding: &Binding) -> Result<Option<AccountInfo>, String> {
        match self.pool.get(&binding.account_id) {
            Ok(info) if info.status == AccountStatus::Revoked => Ok(None),
            Ok(info) => Ok(Some(info)),
            Err(AccountError::NotFound) => Ok(None),
            Err(_) => Err(STORE_ERROR.into()),
        }
    }

    fn synchronize(&self, state: &mut State, owner: &str, scope: &str, snapshot: &NativeAccountSnapshot, renew: bool) -> Result<(), String> {
        let identities = Self::identities(state, owner, snapshot);
        if renew {
            let previous = state.saved.bindings.clone();
            for binding in &mut state.saved.bindings {
                if binding.owner == owner && binding.scope == scope && identities.contains_key(&binding.identity) {
                    binding.reference_id = Uuid::new_v4().to_string();
                }
            }
            if let Err(error) = self.save(state) {
                state.saved.bindings = previous;
                return Err(error);
            }
        }
        for binding in &state.saved.bindings {
            if binding.owner != owner || binding.scope != scope { continue; }
            let Some(account) = self.active(binding)? else { continue; };
            let authorized = identities.contains_key(&binding.identity);
            let updated = match self.pool.update_native_state(&account.id, account.credential_generation, NativeAccountState {
                client: binding.client, reference_id: binding.reference_id.clone(), authorized,
                expires_at: None, available_models: account.available_models,
            }) {
                Ok(updated) => updated,
                Err(AccountError::Revoked) => continue,
                Err(error) => return Err(error.to_string()),
            };
            if authorized && let Some(quota) = native_quota(snapshot) {
                match self.pool.update_quota(&updated.id, updated.credential_generation, quota) {
                    Ok(_) | Err(AccountError::Revoked) => {},
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
        Ok(())
    }

    pub(super) fn observe(&self, owner: &str, scope: &str, mut snapshot: NativeAccountSnapshot, renew: bool) -> Result<NativeAccountSnapshot, String> {
        let mut state = self.state.lock().map_err(|_| STORE_ERROR)?;
        self.synchronize(&mut state, owner, scope, &snapshot, renew)?;
        let timestamp = crate::governance::now();
        state.candidates.retain(|_, candidate| candidate.expires_at > timestamp);
        let identities = Self::identities(&state, owner, &snapshot);
        for (identity, label) in identities {
            let mut bound_account_id = None;
            for binding in &state.saved.bindings {
                if binding.owner == owner && binding.identity == identity && self.active(binding)?.is_some() {
                    bound_account_id = Some(binding.account_id.clone());
                    break;
                }
            }
            if state.candidates.len() >= 512 { state.candidates.clear(); }
            let id = Uuid::new_v4().to_string();
            let expires_at = timestamp + CANDIDATE_LIFETIME;
            state.candidates.insert(id.clone(), Candidate { owner: owner.into(), scope: scope.into(), identity, expires_at });
            snapshot.pool_candidates.push(NativePoolCandidate { id, label, expires_at, bound_account_id });
        }
        snapshot.identities.clear();
        Ok(snapshot)
    }

    pub(super) fn bind(&self, scope: &str, snapshot: &NativeAccountSnapshot, request: BindNativeAccountRequest) -> Result<AccountInfo, String> {
        if !request.confirmed { return Err(AccountError::AuthorizationRequired.to_string()); }
        let owner = owner(&request.client_id)?;
        let client = account_client(&request.client_id)?;
        let mut state = self.state.lock().map_err(|_| STORE_ERROR)?;
        let candidate = state.candidates.remove(&request.candidate_id).ok_or(BINDING_ERROR)?;
        if candidate.expires_at <= crate::governance::now() || candidate.scope != scope || candidate.owner != owner
            || !Self::identities(&state, owner, snapshot).contains_key(&candidate.identity) {
            return Err(BINDING_ERROR.into());
        }
        for binding in &state.saved.bindings {
            if binding.owner == owner && binding.identity == candidate.identity && self.active(binding)?.is_some() {
                return Err("该原生账号已有绑定，请在账号池管理现有引用".into());
            }
        }
        let active_ids: Vec<String> = self.pool.list().map_err(|_| STORE_ERROR)?.into_iter()
            .filter(|account| account.status != AccountStatus::Revoked).map(|account| account.id).collect();
        state.saved.bindings.retain(|binding| active_ids.contains(&binding.account_id));
        let account = self.pool.add(AddAccountRequest {
            alias: request.alias, provider_id: request.provider_id, approved_destination: Some(request.approved_destination),
            credentials: AccountCredentialInput::NativeManaged { client }, available_models: Vec::new(),
            allow_refresh: false, confirmed: true,
        }).map_err(|error| error.to_string())?;
        state.saved.bindings.push(Binding { account_id: account.id.clone(), client, owner: owner.into(),
            scope: scope.into(), identity: candidate.identity, reference_id: Uuid::new_v4().to_string() });
        if let Err(error) = self.save(&state) {
            state.saved.bindings.retain(|binding| binding.account_id != account.id);
            let _ = self.pool.revoke(&account.id, true);
            return Err(error);
        }
        self.synchronize(&mut state, owner, scope, snapshot, false)?;
        self.pool.get(&account.id).map_err(|error| error.to_string())
    }

    pub(super) fn bound_client(&self, id: &str, scope: Option<&str>, snapshot: Option<&NativeAccountSnapshot>) -> Result<String, String> {
        let state = self.state.lock().map_err(|_| STORE_ERROR)?;
        let binding = state.saved.bindings.iter().find(|binding| binding.account_id == id).ok_or(BINDING_ERROR)?;
        let account = self.active(binding)?.ok_or(BINDING_ERROR)?;
        if !account.enabled || !account.destination_approved || scope.is_some_and(|scope| scope != binding.scope)
            || snapshot.is_some_and(|snapshot| !Self::identities(&state, &binding.owner, snapshot).contains_key(&binding.identity)) {
            return Err(BINDING_ERROR.into());
        }
        Ok(client_id(binding.client)?.into())
    }

    pub(super) fn account(&self, id: &str) -> Result<AccountInfo, String> {
        self.pool.get(id).map_err(|error| error.to_string())
    }

    pub(super) fn invalidate(&self, scope: &str) -> Result<(), String> {
        let state = self.state.lock().map_err(|_| STORE_ERROR)?;
        let ids = state.saved.bindings.iter().filter(|binding| binding.scope == scope).map(|binding| binding.account_id.clone()).collect::<Vec<_>>();
        self.pool.invalidate_native_checks(&ids).map_err(|error| error.to_string())
    }

    pub(super) fn signed_out(&self, scope: &str, disconnected: Option<&NativeProviderConnection>) -> Result<(), String> {
        let mut state = self.state.lock().map_err(|_| STORE_ERROR)?;
        let identity = disconnected.and_then(NativeIdentity::opencode)
            .map(|identity| digest(&state.saved.salt, &["identity", "opencode", identity.subject.as_str()]));
        state.candidates.retain(|_, candidate| candidate.scope != scope);
        if disconnected.is_some() && identity.is_none() { return Ok(()); }
        for binding in &state.saved.bindings {
            if binding.scope != scope || identity.as_ref().is_some_and(|identity| identity != &binding.identity) { continue; }
            let Some(account) = self.active(binding)? else { continue; };
            match self.pool.update_native_state(&account.id, account.credential_generation, NativeAccountState {
                client: binding.client, reference_id: binding.reference_id.clone(), authorized: false,
                expires_at: None, available_models: account.available_models,
            }) {
                Ok(_) | Err(AccountError::Revoked) => {},
                Err(error) => return Err(error.to_string()),
            }
        }
        Ok(())
    }
}

fn native_quota(snapshot: &NativeAccountSnapshot) -> Option<QuotaSnapshot> {
    let windows: Vec<_> = snapshot.quotas.iter().filter_map(|quota| {
        let used = quota.used_percent.filter(|value| value.is_finite() && (0.0..=100.0).contains(value))?;
        if quota.reset_at.is_some_and(|reset| reset <= snapshot.checked_at) { return None; }
        Some(QuotaWindow { id: quota.source.clone(), unit: QuotaUnit::Percent, remaining: Some(100.0 - used),
            limit: Some(100.0), resets_at: quota.reset_at })
    }).collect();
    if windows.is_empty() { return None; }
    let valid_until = windows.iter().filter_map(|window| window.resets_at).fold(snapshot.checked_at + 60, i64::min);
    Some(QuotaSnapshot { source: QuotaSource::NativeClient, observed_at: snapshot.checked_at, valid_until, windows })
}

#[derive(Default)]
pub(super) struct NativeOperations {
    epoch: AtomicU64,
    gate: Mutex<Option<(u64, String, Result<NativeAccountSnapshot, String>)>>,
}

impl NativeOperations {
    pub(super) fn observe(&self, refresh: bool, operation: impl FnOnce() -> Result<NativeAccountSnapshot, String>) -> Result<NativeAccountSnapshot, String> {
        self.observe_for("", refresh, operation)
    }

    pub(super) fn observe_for(&self, key: &str, refresh: bool, operation: impl FnOnce() -> Result<NativeAccountSnapshot, String>) -> Result<NativeAccountSnapshot, String> {
        let ticket = self.epoch.load(Ordering::Acquire);
        let mut cache = self.gate.lock().map_err(|_| STORE_ERROR)?;
        if refresh && let Some((epoch, cached_key, result)) = cache.as_ref()
            && *epoch != ticket && cached_key == key { return result.clone(); }
        *cache = None;
        let result = operation();
        if refresh {
            let epoch = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
            *cache = Some((epoch, key.into(), result.clone()));
        }
        result
    }

    pub(super) fn mutate<T>(&self, operation: impl FnOnce() -> Result<T, String>) -> Result<T, String> {
        let mut cache = self.gate.lock().map_err(|_| STORE_ERROR)?;
        *cache = None;
        operation()
    }
}

#[cfg(test)]
mod tests;
