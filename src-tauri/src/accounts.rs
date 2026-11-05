pub use crate::cancellation::Cancellation as AccountCancellation;
use crate::database::Database;
use futures_util::FutureExt;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fmt;
use std::future::Future;
use std::io::Read;
use std::panic::AssertUnwindSafe;
use std::path::Path;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::task::Poll;
use std::time::Duration;
use tokio::sync::watch;
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_SECRET_BYTES: usize = 2560;
const MAX_IMPORT_BYTES: u64 = 32 * 1024;
const MAX_ACCOUNTS: i64 = 256;
const IMPORT_LIFETIME: i64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum AccountError {
    #[error("account_authorization_required")]
    AuthorizationRequired,
    #[error("account_invalid_input")]
    InvalidInput,
    #[error("account_not_found")]
    NotFound,
    #[error("account_provider_unavailable")]
    ProviderUnavailable,
    #[error("account_store_unavailable")]
    StoreUnavailable,
    #[error("account_credential_store_unavailable")]
    CredentialStoreUnavailable,
    #[error("account_credential_missing")]
    CredentialMissing,
    #[error("account_destination_authorization_required")]
    DestinationApprovalRequired,
    #[error("account_revision_conflict")]
    RevisionConflict,
    #[error("account_revoked")]
    Revoked,
    #[error("account_cancelled")]
    Cancelled,
    #[error("account_attempt_not_admitted")]
    AttemptNotAdmitted,
    #[error("account_attempt_already_dispatched")]
    AlreadyDispatched,
    #[error("account_selection_stale")]
    StaleSelection,
    #[error("account_no_eligible_account")]
    NoEligibleAccount,
    #[error("account_refresh_not_configured")]
    RefreshNotConfigured,
    #[error("account_refresh_failed")]
    RefreshFailed,
    #[error("account_refresh_rate_limited")]
    RefreshRateLimited,
    #[error("account_login_required")]
    LoginRequired,
    #[error("account_invalid_quota_evidence")]
    InvalidQuotaEvidence,
    #[error("account_import_selection_expired")]
    ImportSelectionExpired,
    #[error("account_import_invalid_file")]
    InvalidImportFile,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccountKind {
    ApiKey,
    #[serde(rename = "oauth")]
    OAuth,
    NativeManaged,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccountClient {
    CodexDesktop,
    CodexCli,
    ClaudeDesktop,
    ClaudeCode,
    OpenCodeDesktop,
    OpenCodeCli,
    GeminiCli,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccountSource {
    Manual,
    SelectedFile,
    NativeClient,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AuthorizationStatus {
    Unverified,
    Authorized,
    LoginRequired,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AccountStatus {
    Available,
    NativeCheckRequired,
    Refreshing,
    RefreshRequired,
    DestinationApprovalRequired,
    LoginRequired,
    QuotaLimited,
    CoolingDown,
    Disabled,
    Revoked,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectionMode {
    Fixed,
    #[default]
    Rotate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AccountPolicy {
    pub mode: SelectionMode,
    pub fixed_account_id: Option<String>,
    pub affinity: bool,
    pub failure_cooldown_seconds: u64,
    pub rate_limit_cooldown_seconds: u64,
    pub max_cooldown_seconds: u64,
}

impl Default for AccountPolicy {
    fn default() -> Self {
        Self {
            mode: SelectionMode::Rotate,
            fixed_account_id: None,
            affinity: true,
            failure_cooldown_seconds: 30,
            rate_limit_cooldown_seconds: 60,
            max_cooldown_seconds: 3600,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QuotaSource {
    Unknown,
    ProviderResponse,
    ProviderAccountApi,
    NativeClient,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum QuotaUnit {
    Requests,
    Tokens,
    Percent,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaWindow {
    pub id: String,
    pub unit: QuotaUnit,
    pub remaining: Option<f64>,
    pub limit: Option<f64>,
    pub resets_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QuotaSnapshot {
    pub source: QuotaSource,
    pub observed_at: i64,
    pub valid_until: i64,
    pub windows: Vec<QuotaWindow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub id: String,
    pub alias: String,
    pub provider_id: String,
    pub kind: AccountKind,
    pub client: Option<AccountClient>,
    pub source: AccountSource,
    pub status: AccountStatus,
    pub authorization_status: AuthorizationStatus,
    pub destination_approved: bool,
    pub enabled: bool,
    pub revision: u64,
    pub credential_generation: u64,
    pub expires_at: Option<i64>,
    pub cooldown_until: Option<i64>,
    pub available_models: Vec<String>,
    pub quota: Option<QuotaSnapshot>,
    pub allow_refresh: bool,
    pub can_refresh: bool,
    pub cleanup_pending: bool,
    pub last_validated_at: Option<i64>,
    pub last_used_at: Option<i64>,
    pub created_at: i64,
    pub updated_at: i64,
}

pub struct AccountSecret(Zeroizing<String>);

impl AccountSecret {
    pub fn new(value: String) -> Result<Self, AccountError> {
        let value = Zeroizing::new(value);
        if value.is_empty()
            || value.len() > MAX_SECRET_BYTES
            || value.trim() != value.as_str()
            || value.chars().any(char::is_control)
        {
            return Err(AccountError::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn expose(&self) -> &str {
        self.0.as_str()
    }
}

impl fmt::Debug for AccountSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountSecret([REDACTED])")
    }
}

impl<'de> Deserialize<'de> for AccountSecret {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?)
            .map_err(|_| serde::de::Error::custom("invalid account secret"))
    }
}

#[derive(Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum AccountCredentialInput {
    ApiKey {
        api_key: AccountSecret,
    },
    #[serde(rename = "oauth")]
    OAuth {
        access_token: AccountSecret,
        refresh_token: Option<AccountSecret>,
        expires_at: Option<i64>,
    },
    NativeManaged {
        client: AccountClient,
    },
}

impl fmt::Debug for AccountCredentialInput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountCredentialInput([REDACTED])")
    }
}

impl AccountCredentialInput {
    fn kind(&self) -> AccountKind {
        match self {
            Self::ApiKey { .. } => AccountKind::ApiKey,
            Self::OAuth { .. } => AccountKind::OAuth,
            Self::NativeManaged { .. } => AccountKind::NativeManaged,
        }
    }
    fn secrets(&self) -> Vec<(&'static str, &AccountSecret)> {
        match self {
            Self::ApiKey { api_key } => vec![("access", api_key)],
            Self::OAuth {
                access_token,
                refresh_token,
                ..
            } => {
                let mut values = vec![("access", access_token)];
                if let Some(refresh) = refresh_token {
                    values.push(("refresh", refresh));
                }
                values
            }
            Self::NativeManaged { .. } => vec![],
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AddAccountRequest {
    pub alias: String,
    pub provider_id: String,
    #[serde(default)]
    pub approved_destination: Option<String>,
    pub credentials: AccountCredentialInput,
    #[serde(default)]
    pub available_models: Vec<String>,
    pub allow_refresh: bool,
    pub confirmed: bool,
}

impl fmt::Debug for AddAccountRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AddAccountRequest([REDACTED])")
    }
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UpdateAccountRequest {
    pub id: String,
    pub expected_revision: u64,
    pub alias: String,
    pub provider_id: String,
    #[serde(default)]
    pub approved_destination: Option<String>,
    pub enabled: bool,
    pub available_models: Vec<String>,
    pub confirmed: bool,
}

impl fmt::Debug for UpdateAccountRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UpdateAccountRequest([REDACTED])")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplaceCredentialRequest {
    pub id: String,
    pub expected_revision: u64,
    #[serde(default)]
    pub approved_destination: Option<String>,
    pub credentials: AccountCredentialInput,
    pub allow_refresh: bool,
    pub confirmed: bool,
}

impl fmt::Debug for ReplaceCredentialRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ReplaceCredentialRequest([REDACTED])")
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectedAccountImport {
    pub source_id: String,
    pub expires_at: i64,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ImportAccountRequest {
    pub source_id: String,
    pub alias: String,
    pub provider_id: String,
    #[serde(default)]
    pub approved_destination: Option<String>,
    pub allow_refresh: bool,
    pub confirmed: bool,
}

impl fmt::Debug for ImportAccountRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ImportAccountRequest([REDACTED])")
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ImportDocument {
    schema_version: u32,
    credentials: AccountCredentialInput,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalRevocation {
    pub account: AccountInfo,
    pub local_reference_revoked: bool,
    pub credentials_removed: bool,
    pub scope: RevocationScope,
}

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RevocationScope {
    LocalOnly,
}

#[derive(Clone)]
pub struct SelectionContext {
    pub provider_id: String,
    pub destination: Option<String>,
    pub http_only: bool,
    pub model: String,
    pub task_id: String,
    pub attempt_id: String,
    pub affinity_key: Option<String>,
    pub cancellation: AccountCancellation,
}

impl fmt::Debug for SelectionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SelectionContext")
            .field("affinity", &self.affinity_key.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SelectionReason {
    Fixed,
    Affinity,
    RoundRobin,
    ExistingAttempt,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSelection {
    account_id: String,
    provider_id: String,
    task_id: String,
    attempt_id: String,
    credential_generation: u64,
    pub reason: SelectionReason,
}

impl AccountSelection {
    pub fn account_id(&self) -> &str {
        &self.account_id
    }
    pub fn provider_id(&self) -> &str {
        &self.provider_id
    }
    pub fn task_id(&self) -> &str {
        &self.task_id
    }
    pub fn credential_generation(&self) -> u64 {
        self.credential_generation
    }
    pub fn attempt_id(&self) -> &str {
        &self.attempt_id
    }
}

pub enum AccountAuthorization {
    ApiKey(AccountSecret),
    OAuth(AccountSecret),
    NativeManaged {
        client: AccountClient,
        reference_id: String,
    },
}

impl AccountAuthorization {
    pub(crate) fn matches(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::ApiKey(left), Self::ApiKey(right)) | (Self::OAuth(left), Self::OAuth(right)) => {
                left.expose() == right.expose()
            }
            (
                Self::NativeManaged {
                    client: left_client,
                    reference_id: left_reference,
                },
                Self::NativeManaged {
                    client: right_client,
                    reference_id: right_reference,
                },
            ) => left_client == right_client && left_reference == right_reference,
            _ => false,
        }
    }
}

impl fmt::Debug for AccountAuthorization {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountAuthorization([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum OutcomeKind {
    Success,
    Unauthorized,
    RateLimited,
    QuotaExhausted,
    TransientFailure,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct AccountOutcome {
    pub kind: OutcomeKind,
    pub retry_after_seconds: Option<u64>,
    pub quota: Option<QuotaSnapshot>,
}

impl AccountOutcome {
    pub fn from_http(status: u16, retry_after_seconds: Option<u64>) -> Self {
        let kind = match status {
            200..=299 => OutcomeKind::Success,
            401 | 403 => OutcomeKind::Unauthorized,
            429 => OutcomeKind::RateLimited,
            _ => OutcomeKind::TransientFailure,
        };
        Self {
            kind,
            retry_after_seconds,
            quota: None,
        }
    }
}

pub struct RefreshRequest {
    pub account_id: String,
    pub provider_id: String,
    pub expected_generation: u64,
    pub refresh_token: AccountSecret,
    pub cancellation: AccountCancellation,
}

impl fmt::Debug for RefreshRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RefreshRequest([REDACTED])")
    }
}

pub struct RefreshedCredential {
    pub access_token: AccountSecret,
    pub refresh_token: Option<AccountSecret>,
    pub expires_at: Option<i64>,
}

impl fmt::Debug for RefreshedCredential {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("RefreshedCredential([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshError {
    Unauthorized,
    RateLimited { retry_after_seconds: u64 },
    Unavailable,
    Cancelled,
}

pub trait AccountRefresher: Send + Sync + 'static {
    fn refresh(
        &self,
        request: RefreshRequest,
    ) -> Pin<Box<dyn Future<Output = Result<RefreshedCredential, RefreshError>> + Send>>;
}

pub struct NativeAccountState {
    pub client: AccountClient,
    pub reference_id: String,
    pub authorized: bool,
    pub expires_at: Option<i64>,
    pub available_models: Vec<String>,
}

impl fmt::Debug for NativeAccountState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("NativeAccountState([REDACTED])")
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AccountRecord {
    id: String,
    alias: String,
    provider_id: String,
    #[serde(default)]
    approved_destination_fingerprint: Option<String>,
    kind: AccountKind,
    client: Option<AccountClient>,
    source: AccountSource,
    authorization_status: AuthorizationStatus,
    enabled: bool,
    revoked: bool,
    #[serde(default)]
    refresh_login_required: bool,
    revision: u64,
    credential_generation: u64,
    has_refresh_token: bool,
    native_reference: Option<String>,
    expires_at: Option<i64>,
    cooldown_until: Option<i64>,
    available_models: Vec<String>,
    quota: Option<QuotaSnapshot>,
    allow_refresh: bool,
    consecutive_failures: u32,
    last_validated_at: Option<i64>,
    last_used_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

trait CredentialStore: Send + Sync {
    fn set(&self, id: &str, value: &str) -> Result<(), AccountError>;
    fn get(&self, id: &str) -> Result<Option<AccountSecret>, AccountError>;
    fn delete(&self, id: &str) -> Result<(), AccountError>;
}

#[cfg(test)]
#[derive(Default)]
struct MemoryCredentialStore {
    values: Mutex<HashMap<String, Zeroizing<String>>>,
}

#[cfg(test)]
impl CredentialStore for MemoryCredentialStore {
    fn set(&self, id: &str, value: &str) -> Result<(), AccountError> {
        self.values
            .lock()
            .map_err(lock_error)?
            .insert(id.to_owned(), Zeroizing::new(value.to_owned()));
        Ok(())
    }

    fn get(&self, id: &str) -> Result<Option<AccountSecret>, AccountError> {
        self.values
            .lock()
            .map_err(lock_error)?
            .get(id)
            .map(|value| AccountSecret::new(value.as_str().to_owned()))
            .transpose()
    }

    fn delete(&self, id: &str) -> Result<(), AccountError> {
        self.values.lock().map_err(lock_error)?.remove(id);
        Ok(())
    }
}

struct NativeCredentialStore;

impl CredentialStore for NativeCredentialStore {
    fn set(&self, id: &str, value: &str) -> Result<(), AccountError> {
        crate::credential_store::set_secret("accounts", id, value)
            .map_err(|_| AccountError::CredentialStoreUnavailable)
    }
    fn get(&self, id: &str) -> Result<Option<AccountSecret>, AccountError> {
        crate::credential_store::get_secret("accounts", id)
            .map_err(|_| AccountError::CredentialStoreUnavailable)?
            .map(AccountSecret::new)
            .transpose()
            .map_err(|_| AccountError::CredentialStoreUnavailable)
    }
    fn delete(&self, id: &str) -> Result<(), AccountError> {
        crate::credential_store::delete_secret("accounts", id)
            .map_err(|_| AccountError::CredentialStoreUnavailable)
    }
}

struct ImportSource {
    file: std::fs::File,
    expires_at: i64,
}
struct RefreshRegistration {
    callback: Arc<dyn AccountRefresher>,
    timeout: Duration,
}
struct RefreshFlight {
    generation: u64,
    completed: watch::Sender<Option<Result<AccountInfo, AccountError>>>,
    cancellation: AccountCancellation,
}

#[derive(Default)]
struct PoolRuntime {
    gate: Mutex<()>,
    native_checks: Mutex<HashMap<String, (u64, std::time::Instant)>>,
    imports: Mutex<HashMap<String, ImportSource>>,
    refreshers: Mutex<HashMap<String, RefreshRegistration>>,
    flights: Mutex<HashMap<String, Arc<RefreshFlight>>>,
}

#[derive(Clone)]
pub struct AccountPool {
    database: Database,
    credentials: Arc<dyn CredentialStore>,
    runtime: Arc<PoolRuntime>,
}

impl fmt::Debug for AccountPool {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AccountPool([LOCAL METADATA ONLY])")
    }
}

fn now() -> i64 {
    crate::governance::now()
}
fn store_error(_: rusqlite::Error) -> AccountError {
    AccountError::StoreUnavailable
}
fn lock_error<T>(_: std::sync::PoisonError<T>) -> AccountError {
    AccountError::StoreUnavailable
}
fn confirmed(value: bool) -> Result<(), AccountError> {
    if value {
        Ok(())
    } else {
        Err(AccountError::AuthorizationRequired)
    }
}
fn identifier(value: &str) -> Result<(), AccountError> {
    if value.is_empty()
        || value.len() > 256
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        Err(AccountError::InvalidInput)
    } else {
        Ok(())
    }
}
fn labels(alias: &str, models: &[String]) -> Result<(), AccountError> {
    if alias.is_empty()
        || alias.trim() != alias
        || alias.chars().count() > 80
        || alias.chars().any(char::is_control)
        || alias.contains('@')
        || models.len() > 128
    {
        return Err(AccountError::InvalidInput);
    }
    for model in models {
        identifier(model)?;
    }
    Ok(())
}
fn secret_id(id: &str, generation: u64, part: &str) -> String {
    format!("{id}.g{generation}.{part}")
}
fn advance(record: &mut AccountRecord) -> Result<(), AccountError> {
    record.revision = record
        .revision
        .checked_add(1)
        .filter(|value| *value <= i64::MAX as u64)
        .ok_or(AccountError::StoreUnavailable)?;
    record.updated_at = now();
    Ok(())
}
fn extend_cooldown(record: &mut AccountRecord, until: i64) {
    record.cooldown_until = Some(
        record
            .cooldown_until
            .map_or(until, |current| current.max(until)),
    );
}
fn next_generation_on(
    connection: &Connection,
    record: &AccountRecord,
) -> Result<u64, AccountError> {
    let reserved: u64 = connection
        .query_row(
            "SELECT COALESCE(MAX(generation),0) FROM account_pool_secret_refs WHERE account_id=?1",
            [&record.id],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    record
        .credential_generation
        .max(reserved)
        .checked_add(1)
        .filter(|generation| *generation <= i64::MAX as u64)
        .ok_or(AccountError::StoreUnavailable)
}
fn load_record(connection: &Connection, id: &str) -> Result<AccountRecord, AccountError> {
    let value: Option<String> = connection
        .query_row(
            "SELECT metadata FROM account_pool_accounts WHERE id=?1",
            [id],
            |row| row.get(0),
        )
        .optional()
        .map_err(store_error)?;
    serde_json::from_str(&value.ok_or(AccountError::NotFound)?)
        .map_err(|_| AccountError::StoreUnavailable)
}
fn save_record(connection: &Connection, record: &AccountRecord) -> Result<(), AccountError> {
    let value = serde_json::to_string(record).map_err(|_| AccountError::StoreUnavailable)?;
    connection.execute("INSERT INTO account_pool_accounts(id,metadata) VALUES(?1,?2) ON CONFLICT(id) DO UPDATE SET metadata=excluded.metadata",
        params![record.id, value]).map_err(store_error)?;
    Ok(())
}
fn policy_on(connection: &Connection, provider_id: &str) -> Result<AccountPolicy, AccountError> {
    let value: Option<String> = connection
        .query_row(
            "SELECT policy FROM account_pool_policies WHERE provider_id=?1",
            [provider_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(store_error)?;
    value
        .map(|value| serde_json::from_str(&value).map_err(|_| AccountError::StoreUnavailable))
        .unwrap_or_else(|| Ok(AccountPolicy::default()))
}
fn provider_on(
    connection: &Connection,
    provider_id: &str,
    enabled: bool,
) -> Result<(), AccountError> {
    identifier(provider_id)?;
    let present: Option<bool> = connection
        .query_row(
            "SELECT enabled FROM providers WHERE id=?1",
            [provider_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(store_error)?;
    match present {
        Some(value) if !enabled || value => Ok(()),
        _ => Err(AccountError::ProviderUnavailable),
    }
}
fn check_record(record: &AccountRecord, revision: u64) -> Result<(), AccountError> {
    if record.revoked {
        return Err(AccountError::Revoked);
    }
    if record.revision != revision {
        return Err(AccountError::RevisionConflict);
    }
    Ok(())
}
fn destination_fingerprint(provider_id: &str, destination: &str) -> Result<String, AccountError> {
    if destination.is_empty()
        || destination.len() > 4096
        || destination.trim() != destination
        || destination
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return Err(AccountError::DestinationApprovalRequired);
    }
    let url =
        reqwest::Url::parse(destination).map_err(|_| AccountError::DestinationApprovalRequired)?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(AccountError::DestinationApprovalRequired);
    }
    let mut hash = Sha256::new();
    for value in ["xy-accounts-destination-v1", provider_id, destination] {
        hash.update((value.len() as u64).to_be_bytes());
        hash.update(value.as_bytes());
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn provider_destination_on(
    connection: &Connection,
    provider_id: &str,
) -> Result<String, AccountError> {
    connection
        .query_row(
            "SELECT endpoint FROM providers WHERE id=?1",
            [provider_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(store_error)?
        .ok_or(AccountError::ProviderUnavailable)
}
fn approved_destination_on(
    connection: &Connection,
    provider_id: &str,
    approved_destination: Option<&str>,
) -> Result<String, AccountError> {
    let destination = provider_destination_on(connection, provider_id)?;
    if approved_destination != Some(destination.as_str()) {
        return Err(AccountError::DestinationApprovalRequired);
    }
    destination_fingerprint(provider_id, &destination)
}
fn require_record_destination_on(
    connection: &Connection,
    record: &AccountRecord,
) -> Result<(), AccountError> {
    let destination = provider_destination_on(connection, &record.provider_id)?;
    let fingerprint = destination_fingerprint(&record.provider_id, &destination)?;
    if record.approved_destination_fingerprint.as_deref() != Some(fingerprint.as_str()) {
        return Err(AccountError::DestinationApprovalRequired);
    }
    Ok(())
}
fn record_destination_approved_on(
    connection: &Connection,
    record: &AccountRecord,
) -> Result<bool, AccountError> {
    match require_record_destination_on(connection, record) {
        Ok(()) => Ok(true),
        Err(AccountError::DestinationApprovalRequired | AccountError::ProviderUnavailable) => {
            Ok(false)
        }
        Err(error) => Err(error),
    }
}
fn require_selection_destination_on(
    connection: &Connection,
    record: &AccountRecord,
    context: &SelectionContext,
) -> Result<(), AccountError> {
    if context.http_only && record.kind == AccountKind::NativeManaged {
        return Err(AccountError::NoEligibleAccount);
    }
    require_record_destination_on(connection, record)?;
    match context.destination.as_deref() {
        Some(destination) => {
            let fingerprint =
                approved_destination_on(connection, &context.provider_id, Some(destination))?;
            if record.approved_destination_fingerprint.as_deref() != Some(fingerprint.as_str()) {
                return Err(AccountError::DestinationApprovalRequired);
            }
        }
        None if record.kind == AccountKind::NativeManaged && !context.http_only => {}
        None => return Err(AccountError::DestinationApprovalRequired),
    }
    Ok(())
}
fn valid_quota(snapshot: &QuotaSnapshot, timestamp: i64) -> Result<(), AccountError> {
    if snapshot.source == QuotaSource::Unknown
        || snapshot.windows.is_empty()
        || snapshot.windows.len() > 16
        || snapshot.observed_at < timestamp - 86400
        || snapshot.observed_at > timestamp + 5
        || snapshot.valid_until <= timestamp
        || snapshot.valid_until <= snapshot.observed_at
        || snapshot.valid_until - snapshot.observed_at > 86400
    {
        return Err(AccountError::InvalidQuotaEvidence);
    }
    for window in &snapshot.windows {
        if window.id.is_empty()
            || window.id.len() > 64
            || !window
                .id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
            || window
                .remaining
                .is_some_and(|value| !value.is_finite() || value < 0.0)
            || window
                .limit
                .is_some_and(|value| !value.is_finite() || value <= 0.0)
            || matches!((window.remaining, window.limit), (Some(remaining), Some(limit)) if remaining > limit)
            || (window.unit == QuotaUnit::Percent
                && (window.remaining.is_some_and(|value| value > 100.0)
                    || window.limit.is_some_and(|value| value > 100.0)))
            || window
                .resets_at
                .is_some_and(|value| value <= snapshot.observed_at)
        {
            return Err(AccountError::InvalidQuotaEvidence);
        }
    }
    Ok(())
}
fn quota_blocks(snapshot: Option<&QuotaSnapshot>, timestamp: i64) -> bool {
    snapshot.is_some_and(|snapshot| {
        snapshot.source != QuotaSource::Unknown
            && snapshot.valid_until > timestamp
            && snapshot.windows.iter().any(|window| {
                window.remaining == Some(0.0)
                    && window.resets_at.is_none_or(|reset| reset > timestamp)
            })
    })
}

impl AccountPool {
    pub fn new(database: Database) -> Result<Self, AccountError> {
        Self::with_store(database, Arc::new(NativeCredentialStore))
    }

    #[cfg(test)]
    pub(crate) fn test_pool(database: Database) -> Result<Self, AccountError> {
        struct TestStore {
            connection: Weak<Mutex<Connection>>,
            credentials: Arc<MemoryCredentialStore>,
        }
        static STORES: OnceLock<Mutex<HashMap<usize, TestStore>>> = OnceLock::new();
        let connection = database.conn();
        let key = Arc::as_ptr(&connection) as usize;
        let credentials = {
            let mut stores = STORES
                .get_or_init(Mutex::default)
                .lock()
                .map_err(lock_error)?;
            stores.retain(|_, store| store.connection.strong_count() > 0);
            stores
                .entry(key)
                .or_insert_with(|| TestStore {
                    connection: Arc::downgrade(&connection),
                    credentials: Arc::new(MemoryCredentialStore::default()),
                })
                .credentials
                .clone()
        };
        Self::with_store(database, credentials)
    }

    fn with_store(
        database: Database,
        credentials: Arc<dyn CredentialStore>,
    ) -> Result<Self, AccountError> {
        static RUNTIMES: OnceLock<Mutex<HashMap<usize, Weak<PoolRuntime>>>> = OnceLock::new();
        let connection = database.conn();
        let key = Arc::as_ptr(&connection) as usize;
        let runtime = {
            let mut runtimes = RUNTIMES
                .get_or_init(Mutex::default)
                .lock()
                .map_err(lock_error)?;
            runtimes.retain(|_, runtime| runtime.strong_count() > 0);
            if let Some(runtime) = runtimes.get(&key).and_then(Weak::upgrade) {
                runtime
            } else {
                let runtime = Arc::new(PoolRuntime::default());
                runtimes.insert(key, Arc::downgrade(&runtime));
                runtime
            }
        };
        {
            let _guard = runtime.gate.lock().map_err(lock_error)?;
            let connection = connection.lock().map_err(lock_error)?;
            connection.execute_batch("CREATE TABLE IF NOT EXISTS account_pool_accounts(id TEXT PRIMARY KEY,metadata TEXT NOT NULL);
                CREATE TABLE IF NOT EXISTS account_pool_policies(provider_id TEXT PRIMARY KEY,policy TEXT NOT NULL,cursor TEXT);
                CREATE TABLE IF NOT EXISTS account_pool_affinity(provider_id TEXT NOT NULL,affinity_hash TEXT NOT NULL,account_id TEXT NOT NULL,updated_at INTEGER NOT NULL,PRIMARY KEY(provider_id,affinity_hash));
                CREATE TABLE IF NOT EXISTS account_pool_attempts(attempt_id TEXT PRIMARY KEY,selection TEXT NOT NULL,dispatched INTEGER NOT NULL DEFAULT 0,outcome TEXT,created_at INTEGER NOT NULL);
                CREATE TABLE IF NOT EXISTS account_pool_secret_refs(item_id TEXT PRIMARY KEY,account_id TEXT NOT NULL,generation INTEGER NOT NULL);
                CREATE TABLE IF NOT EXISTS account_pool_meta(id TEXT PRIMARY KEY,value TEXT NOT NULL);")
                .map_err(store_error)?;
            connection
                .execute(
                    "INSERT OR IGNORE INTO account_pool_meta(id,value) VALUES('affinity_salt',?1)",
                    [Uuid::new_v4().to_string()],
                )
                .map_err(store_error)?;
        }
        Ok(Self {
            database,
            credentials,
            runtime,
        })
    }

    fn info_on(
        &self,
        connection: &Connection,
        record: &AccountRecord,
    ) -> Result<AccountInfo, AccountError> {
        let timestamp = now();
        let destination_approved = record_destination_approved_on(connection, record)?;
        let can_refresh = record.kind == AccountKind::OAuth
            && destination_approved
            && record.allow_refresh
            && record.has_refresh_token
            && !record.revoked
            && !record.refresh_login_required
            && self
                .runtime
                .refreshers
                .lock()
                .map_err(lock_error)?
                .contains_key(&record.id);
        let (refreshing, refresh_login_required) = self
            .runtime
            .flights
            .lock()
            .map_err(lock_error)?
            .get(&record.id)
            .filter(|flight| {
                flight.generation == record.credential_generation
                    && !flight.cancellation.is_cancelled()
            })
            .map(|flight| {
                let completed = flight.completed.borrow();
                (
                    completed.is_none(),
                    matches!(*completed, Some(Err(AccountError::LoginRequired))),
                )
            })
            .unwrap_or((false, false));
        let status = if record.revoked {
            AccountStatus::Revoked
        } else if !record.enabled {
            AccountStatus::Disabled
        } else if !destination_approved {
            AccountStatus::DestinationApprovalRequired
        } else if refreshing {
            AccountStatus::Refreshing
        } else if record.refresh_login_required || refresh_login_required {
            AccountStatus::LoginRequired
        } else if record.cooldown_until.is_some_and(|until| until > timestamp) {
            AccountStatus::CoolingDown
        } else if record.authorization_status == AuthorizationStatus::LoginRequired
            || record
                .expires_at
                .is_some_and(|expires| expires <= timestamp)
        {
            if can_refresh {
                AccountStatus::RefreshRequired
            } else {
                AccountStatus::LoginRequired
            }
        } else if record.kind == AccountKind::NativeManaged
            && !self.runtime.native_checks.lock().map_err(lock_error)?.get(&record.id)
                .is_some_and(|(generation, checked)| *generation == record.credential_generation
                    && checked.elapsed() < Duration::from_secs(300))
        {
            AccountStatus::NativeCheckRequired
        } else if quota_blocks(record.quota.as_ref(), timestamp) {
            AccountStatus::QuotaLimited
        } else {
            AccountStatus::Available
        };
        let cleanup_pending = connection.query_row("SELECT COUNT(*) FROM account_pool_secret_refs WHERE account_id=?1 AND (?2 OR generation!=?3)",
            params![record.id, record.revoked, record.credential_generation], |row| row.get::<_, i64>(0)).map_err(store_error)? > 0;
        Ok(AccountInfo {
            id: record.id.clone(),
            alias: record.alias.clone(),
            provider_id: record.provider_id.clone(),
            kind: record.kind,
            client: record.client,
            source: record.source,
            status,
            authorization_status: record.authorization_status,
            destination_approved,
            enabled: record.enabled,
            revision: record.revision,
            credential_generation: record.credential_generation,
            expires_at: record.expires_at,
            cooldown_until: record.cooldown_until.filter(|until| *until > timestamp),
            available_models: record.available_models.clone(),
            quota: record
                .quota
                .clone()
                .filter(|quota| quota.valid_until > timestamp),
            allow_refresh: record.allow_refresh,
            can_refresh,
            cleanup_pending,
            last_validated_at: record.last_validated_at,
            last_used_at: record.last_used_at,
            created_at: record.created_at,
            updated_at: record.updated_at,
        })
    }

    pub fn is_configured(&self, provider_id: &str) -> Result<bool, AccountError> {
        self.manages_provider(provider_id)
    }

    pub fn manages_provider(&self, provider_id: &str) -> Result<bool, AccountError> {
        identifier(provider_id)?;
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        provider_on(&connection, provider_id, false)?;
        connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM account_pool_policies WHERE provider_id=?1) OR EXISTS(SELECT 1 FROM account_pool_accounts WHERE json_extract(metadata,'$.providerId')=?1)",
                [provider_id],
                |row| row.get(0),
            )
            .map_err(store_error)
    }

    pub fn list(&self) -> Result<Vec<AccountInfo>, AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let mut query = connection
            .prepare("SELECT metadata FROM account_pool_accounts ORDER BY id")
            .map_err(store_error)?;
        let values = query
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(store_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(store_error)?;
        values
            .into_iter()
            .map(|value| {
                let record =
                    serde_json::from_str(&value).map_err(|_| AccountError::StoreUnavailable)?;
                self.info_on(&connection, &record)
            })
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<AccountInfo, AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        self.info_on(&connection, &load_record(&connection, id)?)
    }

    fn validate_credentials(
        &self,
        input: &AccountCredentialInput,
        alias: &str,
        provider_id: &str,
        models: &[String],
    ) -> Result<(), AccountError> {
        labels(alias, models)?;
        identifier(provider_id)?;
        for (_, secret) in input.secrets() {
            if alias.contains(secret.expose())
                || provider_id.contains(secret.expose())
                || models.iter().any(|model| model.contains(secret.expose()))
            {
                return Err(AccountError::InvalidInput);
            }
        }
        if let AccountCredentialInput::OAuth {
            expires_at: Some(expires),
            ..
        } = input
        {
            if *expires < 0 || *expires > now().saturating_add(366 * 86400) {
                return Err(AccountError::InvalidInput);
            }
        }
        Ok(())
    }

    fn stage_on(
        &self,
        connection: &Connection,
        id: &str,
        generation: u64,
        input: &AccountCredentialInput,
    ) -> Result<(), AccountError> {
        for (part, _) in input.secrets() {
            connection.execute("INSERT INTO account_pool_secret_refs(item_id,account_id,generation) VALUES(?1,?2,?3)",
                params![secret_id(id, generation, part), id, generation]).map_err(store_error)?;
        }
        for (part, secret) in input.secrets() {
            if let Err(error) = self
                .credentials
                .set(&secret_id(id, generation, part), secret.expose())
            {
                self.cleanup_on(connection, Some(id))?;
                return Err(error);
            }
        }
        Ok(())
    }

    fn cleanup_on(
        &self,
        connection: &Connection,
        account_id: Option<&str>,
    ) -> Result<bool, AccountError> {
        let mut query = connection.prepare("SELECT item_id,account_id,generation FROM account_pool_secret_refs WHERE (?1 IS NULL OR account_id=?1)").map_err(store_error)?;
        let references = query
            .query_map([account_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, u64>(2)?,
                ))
            })
            .map_err(store_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(store_error)?;
        let mut complete = true;
        for (item_id, id, generation) in references {
            match load_record(connection, &id) {
                Ok(record) if !record.revoked && record.credential_generation == generation => {
                    continue;
                }
                Ok(_) | Err(AccountError::NotFound) => {}
                Err(error) => return Err(error),
            }
            if self.credentials.delete(&item_id).is_err() {
                complete = false;
                continue;
            }
            connection
                .execute(
                    "DELETE FROM account_pool_secret_refs WHERE item_id=?1",
                    [item_id],
                )
                .map_err(store_error)?;
        }
        Ok(complete)
    }

    pub fn cleanup_secrets(&self) -> Result<bool, AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        self.cleanup_on(&connection, None)
    }

    pub fn add(&self, request: AddAccountRequest) -> Result<AccountInfo, AccountError> {
        self.add_from(request, AccountSource::Manual)
    }

    fn add_from(
        &self,
        request: AddAccountRequest,
        source: AccountSource,
    ) -> Result<AccountInfo, AccountError> {
        confirmed(request.confirmed)?;
        self.validate_credentials(
            &request.credentials,
            &request.alias,
            &request.provider_id,
            &request.available_models,
        )?;
        if request.allow_refresh
            && !matches!(
                &request.credentials,
                AccountCredentialInput::OAuth {
                    refresh_token: Some(_),
                    ..
                }
            )
        {
            return Err(AccountError::InvalidInput);
        }
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        provider_on(&connection, &request.provider_id, false)?;
        let approved_destination_fingerprint = approved_destination_on(
            &connection,
            &request.provider_id,
            request.approved_destination.as_deref(),
        )?;
        let count: i64 = connection.query_row("SELECT COUNT(*) FROM account_pool_accounts WHERE json_extract(metadata,'$.revoked')=0", [], |row| row.get(0)).map_err(store_error)?;
        if count >= MAX_ACCOUNTS {
            return Err(AccountError::InvalidInput);
        }
        let timestamp = now();
        let (client, expires_at, has_refresh_token) = match &request.credentials {
            AccountCredentialInput::ApiKey { .. } => (None, None, false),
            AccountCredentialInput::OAuth {
                refresh_token,
                expires_at,
                ..
            } => (None, *expires_at, refresh_token.is_some()),
            AccountCredentialInput::NativeManaged { client } => (Some(*client), None, false),
        };
        let kind = request.credentials.kind();
        let record = AccountRecord {
            id: Uuid::new_v4().to_string(),
            alias: request.alias,
            provider_id: request.provider_id,
            approved_destination_fingerprint: Some(approved_destination_fingerprint),
            kind,
            client,
            source: if kind == AccountKind::NativeManaged {
                AccountSource::NativeClient
            } else {
                source
            },
            authorization_status: if kind == AccountKind::NativeManaged {
                AuthorizationStatus::LoginRequired
            } else {
                AuthorizationStatus::Unverified
            },
            enabled: true,
            revoked: false,
            refresh_login_required: false,
            revision: 1,
            credential_generation: 1,
            has_refresh_token,
            native_reference: None,
            expires_at,
            cooldown_until: None,
            available_models: request.available_models,
            quota: None,
            allow_refresh: request.allow_refresh,
            consecutive_failures: 0,
            last_validated_at: None,
            last_used_at: None,
            created_at: timestamp,
            updated_at: timestamp,
        };
        self.stage_on(
            &connection,
            &record.id,
            record.credential_generation,
            &request.credentials,
        )?;
        if let Err(error) = save_record(&connection, &record) {
            self.cleanup_on(&connection, Some(&record.id))?;
            return Err(error);
        }
        self.info_on(&connection, &record)
    }

    pub fn update(&self, request: UpdateAccountRequest) -> Result<AccountInfo, AccountError> {
        confirmed(request.confirmed)?;
        labels(&request.alias, &request.available_models)?;
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let mut record = load_record(&connection, &request.id)?;
        check_record(&record, request.expected_revision)?;
        provider_on(&connection, &request.provider_id, false)?;
        if record.provider_id != request.provider_id && record.kind != AccountKind::ApiKey {
            return Err(AccountError::InvalidInput);
        }
        let provider_changed = record.provider_id != request.provider_id;
        let approved_destination_fingerprint = match request.approved_destination.as_deref() {
            Some(destination) => Some(approved_destination_on(
                &connection,
                &request.provider_id,
                Some(destination),
            )?),
            None if !provider_changed => record.approved_destination_fingerprint.clone(),
            None => return Err(AccountError::DestinationApprovalRequired),
        };
        let destination_changed =
            record.approved_destination_fingerprint != approved_destination_fingerprint;
        if record.kind != AccountKind::NativeManaged {
            let input = self.read_input(&record)?;
            self.validate_credentials(
                &input,
                &request.alias,
                &request.provider_id,
                &request.available_models,
            )?;
        }
        advance(&mut record)?;
        if provider_changed || destination_changed {
            let credentials = self.read_input(&record)?;
            record.credential_generation = next_generation_on(&connection, &record)?;
            self.stage_on(
                &connection,
                &record.id,
                record.credential_generation,
                &credentials,
            )?;
            record.quota = None;
            record.last_validated_at = None;
            if provider_changed {
                record.cooldown_until = None;
                record.consecutive_failures = 0;
                record.authorization_status = AuthorizationStatus::Unverified;
            } else if record.kind != AccountKind::NativeManaged
                && record.authorization_status != AuthorizationStatus::LoginRequired
            {
                record.authorization_status = AuthorizationStatus::Unverified;
            }
        }
        record.alias = request.alias;
        record.provider_id = request.provider_id;
        record.approved_destination_fingerprint = approved_destination_fingerprint;
        record.enabled = request.enabled;
        record.available_models = request.available_models;
        if let Err(error) = save_record(&connection, &record) {
            self.cleanup_on(&connection, Some(&record.id))?;
            return Err(error);
        }
        if !record.enabled || destination_changed {
            self.cancel_refresh(&record.id)?;
        }
        if destination_changed {
            self.runtime
                .refreshers
                .lock()
                .map_err(lock_error)?
                .remove(&record.id);
        }
        self.cleanup_on(&connection, Some(&record.id))?;
        connection
            .execute(
                "DELETE FROM account_pool_affinity WHERE account_id=?1",
                [&record.id],
            )
            .map_err(store_error)?;
        self.info_on(&connection, &record)
    }

    pub fn reconsent_destination(
        &self,
        id: &str,
        expected_revision: u64,
        destination: &str,
        consent: bool,
    ) -> Result<AccountInfo, AccountError> {
        confirmed(consent)?;
        let account = self.get(id)?;
        self.update(UpdateAccountRequest {
            id: id.to_owned(),
            expected_revision,
            alias: account.alias,
            provider_id: account.provider_id,
            approved_destination: Some(destination.to_owned()),
            enabled: account.enabled,
            available_models: account.available_models,
            confirmed: consent,
        })
    }

    fn read_input(&self, record: &AccountRecord) -> Result<AccountCredentialInput, AccountError> {
        if record.kind == AccountKind::NativeManaged {
            return Ok(AccountCredentialInput::NativeManaged {
                client: record.client.ok_or(AccountError::StoreUnavailable)?,
            });
        }
        let access = self
            .credentials
            .get(&secret_id(
                &record.id,
                record.credential_generation,
                "access",
            ))?
            .ok_or(AccountError::CredentialMissing)?;
        match record.kind {
            AccountKind::ApiKey => Ok(AccountCredentialInput::ApiKey { api_key: access }),
            AccountKind::OAuth => Ok(AccountCredentialInput::OAuth {
                access_token: access,
                refresh_token: if record.has_refresh_token {
                    Some(
                        self.credentials
                            .get(&secret_id(
                                &record.id,
                                record.credential_generation,
                                "refresh",
                            ))?
                            .ok_or(AccountError::CredentialMissing)?,
                    )
                } else {
                    None
                },
                expires_at: record.expires_at,
            }),
            AccountKind::NativeManaged => Err(AccountError::StoreUnavailable),
        }
    }

    fn validate_quota_secrets(
        &self,
        record: &AccountRecord,
        quota: &QuotaSnapshot,
    ) -> Result<(), AccountError> {
        if quota.source == QuotaSource::NativeClient && record.kind != AccountKind::NativeManaged {
            return Err(AccountError::InvalidQuotaEvidence);
        }
        if record.kind != AccountKind::NativeManaged {
            let credentials = self.read_input(record)?;
            for (_, secret) in credentials.secrets() {
                if quota
                    .windows
                    .iter()
                    .any(|window| window.id.contains(secret.expose()))
                {
                    return Err(AccountError::InvalidQuotaEvidence);
                }
            }
        }
        Ok(())
    }

    pub fn replace_credentials(
        &self,
        request: ReplaceCredentialRequest,
    ) -> Result<AccountInfo, AccountError> {
        confirmed(request.confirmed)?;
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let mut record = load_record(&connection, &request.id)?;
        check_record(&record, request.expected_revision)?;
        if request.credentials.kind() != record.kind || record.kind == AccountKind::NativeManaged {
            return Err(AccountError::InvalidInput);
        }
        let approved_destination_fingerprint = match request.approved_destination.as_deref() {
            Some(destination) => {
                approved_destination_on(&connection, &record.provider_id, Some(destination))?
            }
            None => {
                require_record_destination_on(&connection, &record)?;
                record
                    .approved_destination_fingerprint
                    .clone()
                    .ok_or(AccountError::DestinationApprovalRequired)?
            }
        };
        let destination_changed = record.approved_destination_fingerprint.as_deref()
            != Some(approved_destination_fingerprint.as_str());
        self.validate_credentials(
            &request.credentials,
            &record.alias,
            &record.provider_id,
            &record.available_models,
        )?;
        if request.allow_refresh
            && !matches!(
                &request.credentials,
                AccountCredentialInput::OAuth {
                    refresh_token: Some(_),
                    ..
                }
            )
        {
            return Err(AccountError::InvalidInput);
        }
        record.credential_generation = next_generation_on(&connection, &record)?;
        advance(&mut record)?;
        self.stage_on(
            &connection,
            &record.id,
            record.credential_generation,
            &request.credentials,
        )?;
        if let AccountCredentialInput::OAuth {
            refresh_token,
            expires_at,
            ..
        } = &request.credentials
        {
            record.has_refresh_token = refresh_token.is_some();
            record.expires_at = *expires_at;
        }
        record.allow_refresh = request.allow_refresh;
        record.approved_destination_fingerprint = Some(approved_destination_fingerprint);
        record.refresh_login_required = false;
        record.authorization_status = AuthorizationStatus::Unverified;
        record.last_validated_at = None;
        record.quota = None;
        record.cooldown_until = None;
        record.consecutive_failures = 0;
        if let Err(error) = save_record(&connection, &record) {
            self.cleanup_on(&connection, Some(&record.id))?;
            return Err(error);
        }
        self.cancel_refresh(&record.id)?;
        if destination_changed {
            self.runtime
                .refreshers
                .lock()
                .map_err(lock_error)?
                .remove(&record.id);
        }
        self.cleanup_on(&connection, Some(&record.id))?;
        self.info_on(&connection, &record)
    }

    fn cancel_refresh(&self, id: &str) -> Result<(), AccountError> {
        let flight = self
            .runtime
            .flights
            .lock()
            .map_err(lock_error)?
            .get(id)
            .cloned();
        if let Some(flight) = flight {
            flight.cancellation.cancel();
        }
        Ok(())
    }

    pub fn revoke(&self, id: &str, consent: bool) -> Result<LocalRevocation, AccountError> {
        confirmed(consent)?;
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let mut record = load_record(&connection, id)?;
        if !record.revoked {
            record.revoked = true;
            record.enabled = false;
            record.native_reference = None;
            record.has_refresh_token = false;
            record.allow_refresh = false;
            record.quota = None;
            advance(&mut record)?;
            save_record(&connection, &record)?;
        }
        self.cancel_refresh(id)?;
        self.runtime
            .refreshers
            .lock()
            .map_err(lock_error)?
            .remove(id);
        connection
            .execute(
                "DELETE FROM account_pool_affinity WHERE account_id=?1",
                [id],
            )
            .map_err(store_error)?;
        let credentials_removed = self.cleanup_on(&connection, Some(id))?;
        Ok(LocalRevocation {
            account: self.info_on(&connection, &record)?,
            local_reference_revoked: true,
            credentials_removed,
            scope: RevocationScope::LocalOnly,
        })
    }

    pub fn policy(&self, provider_id: &str) -> Result<AccountPolicy, AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        provider_on(&connection, provider_id, false)?;
        policy_on(&connection, provider_id)
    }

    pub fn save_policy(
        &self,
        provider_id: &str,
        policy: AccountPolicy,
        consent: bool,
    ) -> Result<AccountPolicy, AccountError> {
        confirmed(consent)?;
        if policy.failure_cooldown_seconds == 0
            || policy.rate_limit_cooldown_seconds == 0
            || policy.max_cooldown_seconds > 86400
            || policy.max_cooldown_seconds < policy.failure_cooldown_seconds
            || policy.max_cooldown_seconds < policy.rate_limit_cooldown_seconds
            || (policy.mode == SelectionMode::Fixed && policy.fixed_account_id.is_none())
            || (policy.mode == SelectionMode::Rotate && policy.fixed_account_id.is_some())
        {
            return Err(AccountError::InvalidInput);
        }
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        provider_on(&connection, provider_id, false)?;
        if let Some(id) = &policy.fixed_account_id {
            let record = load_record(&connection, id)?;
            if record.revoked || record.provider_id != provider_id {
                return Err(AccountError::InvalidInput);
            }
        }
        let value = serde_json::to_string(&policy).map_err(|_| AccountError::StoreUnavailable)?;
        connection.execute("INSERT INTO account_pool_policies(provider_id,policy) VALUES(?1,?2) ON CONFLICT(provider_id) DO UPDATE SET policy=excluded.policy",
            params![provider_id, value]).map_err(store_error)?;
        connection
            .execute(
                "DELETE FROM account_pool_affinity WHERE provider_id=?1",
                [provider_id],
            )
            .map_err(store_error)?;
        Ok(policy)
    }

    pub fn select_import_source(
        &self,
        selected_path: &Path,
    ) -> Result<SelectedAccountImport, AccountError> {
        let metadata = std::fs::symlink_metadata(selected_path)
            .map_err(|_| AccountError::InvalidImportFile)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_IMPORT_BYTES
        {
            return Err(AccountError::InvalidImportFile);
        }
        let mut options = std::fs::OpenOptions::new();
        options.read(true);
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;
            options.custom_flags(
                windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT,
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        let file = options
            .open(selected_path)
            .map_err(|_| AccountError::InvalidImportFile)?;
        let opened = file
            .metadata()
            .map_err(|_| AccountError::InvalidImportFile)?;
        if !opened.is_file() || opened.file_type().is_symlink() || opened.len() > MAX_IMPORT_BYTES {
            return Err(AccountError::InvalidImportFile);
        }
        let source_id = Uuid::new_v4().to_string();
        let expires_at = now().saturating_add(IMPORT_LIFETIME);
        let mut imports = self.runtime.imports.lock().map_err(lock_error)?;
        imports.retain(|_, source| source.expires_at > now());
        if imports.len() >= 32 {
            return Err(AccountError::InvalidInput);
        }
        imports.insert(source_id.clone(), ImportSource { file, expires_at });
        Ok(SelectedAccountImport {
            source_id,
            expires_at,
        })
    }

    pub fn import(&self, request: ImportAccountRequest) -> Result<AccountInfo, AccountError> {
        confirmed(request.confirmed)?;
        let source = self
            .runtime
            .imports
            .lock()
            .map_err(lock_error)?
            .remove(&request.source_id)
            .ok_or(AccountError::ImportSelectionExpired)?;
        if source.expires_at <= now() {
            return Err(AccountError::ImportSelectionExpired);
        }
        let metadata = source
            .file
            .metadata()
            .map_err(|_| AccountError::InvalidImportFile)?;
        if !metadata.is_file()
            || metadata.file_type().is_symlink()
            || metadata.len() > MAX_IMPORT_BYTES
        {
            return Err(AccountError::InvalidImportFile);
        }
        let mut content = Zeroizing::new(Vec::new());
        source
            .file
            .take(MAX_IMPORT_BYTES + 1)
            .read_to_end(&mut content)
            .map_err(|_| AccountError::InvalidImportFile)?;
        if content.len() as u64 > MAX_IMPORT_BYTES {
            return Err(AccountError::InvalidImportFile);
        }
        let document: ImportDocument =
            serde_json::from_slice(&content).map_err(|_| AccountError::InvalidImportFile)?;
        if document.schema_version != 1 || document.credentials.kind() == AccountKind::NativeManaged
        {
            return Err(AccountError::InvalidImportFile);
        }
        self.add_from(
            AddAccountRequest {
                alias: request.alias,
                provider_id: request.provider_id,
                approved_destination: request.approved_destination,
                credentials: document.credentials,
                available_models: vec![],
                allow_refresh: request.allow_refresh,
                confirmed: request.confirmed,
            },
            AccountSource::SelectedFile,
        )
    }

    pub fn update_native_state(
        &self,
        id: &str,
        expected_generation: u64,
        state: NativeAccountState,
    ) -> Result<AccountInfo, AccountError> {
        if Uuid::parse_str(&state.reference_id).is_err() {
            return Err(AccountError::InvalidInput);
        }
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let mut record = load_record(&connection, id)?;
        if record.revoked {
            return Err(AccountError::Revoked);
        }
        if record.credential_generation != expected_generation {
            return Err(AccountError::StaleSelection);
        }
        if record.kind != AccountKind::NativeManaged || record.client != Some(state.client) {
            return Err(AccountError::InvalidInput);
        }
        labels(&record.alias, &state.available_models)?;
        if state.expires_at.is_some_and(|expires| expires <= now()) && state.authorized {
            return Err(AccountError::InvalidInput);
        }
        let reference = state.authorized.then_some(state.reference_id);
        let reference_changed = record.native_reference != reference;
        if !state.authorized || reference_changed {
            record.quota = None;
        }
        if !state.authorized {
            record.authorization_status = AuthorizationStatus::LoginRequired;
        } else if reference_changed {
            record.authorization_status = AuthorizationStatus::Authorized;
        }
        record.native_reference = reference;
        record.expires_at = state.expires_at;
        record.available_models = state.available_models;
        record.last_validated_at =
            (record.authorization_status == AuthorizationStatus::Authorized).then(now);
        record.cooldown_until = record.cooldown_until.filter(|until| *until > now());
        if reference_changed {
            record.credential_generation = record
                .credential_generation
                .checked_add(1)
                .filter(|generation| *generation <= i64::MAX as u64)
                .ok_or(AccountError::StoreUnavailable)?;
            if record.cooldown_until.is_none() {
                record.consecutive_failures = 0;
            }
        }
        advance(&mut record)?;
        save_record(&connection, &record)?;
        let mut checks = self.runtime.native_checks.lock().map_err(lock_error)?;
        if state.authorized {
            checks.insert(record.id.clone(), (record.credential_generation, std::time::Instant::now()));
        } else {
            checks.remove(&record.id);
        }
        drop(checks);
        self.info_on(&connection, &record)
    }

    pub(crate) fn invalidate_native_checks(&self, ids: &[String]) -> Result<(), AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let mut checks = self.runtime.native_checks.lock().map_err(lock_error)?;
        for id in ids { checks.remove(id); }
        Ok(())
    }
}

fn admitted_on(connection: &Connection, context: &SelectionContext) -> Result<(), AccountError> {
    if context.cancellation.is_cancelled() {
        return Err(AccountError::Cancelled);
    }
    for value in [
        &context.provider_id,
        &context.task_id,
        &context.attempt_id,
        &context.model,
    ] {
        identifier(value)?;
    }
    let attempt = connection.query_row("SELECT a.task_id,a.provider_id,a.model,a.status,t.cancelled FROM proxy_attempts a JOIN proxy_tasks t ON t.id=a.task_id WHERE a.id=?1",
        [&context.attempt_id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?, row.get::<_, String>(3)?, row.get::<_, bool>(4)?)))
        .optional().map_err(store_error)?.ok_or(AccountError::AttemptNotAdmitted)?;
    if attempt.4 || attempt.3 == "cancelled" {
        return Err(AccountError::Cancelled);
    }
    if attempt.0 != context.task_id
        || attempt.1 != context.provider_id
        || attempt.2 != context.model
        || attempt.3 != "sending"
    {
        return Err(AccountError::AttemptNotAdmitted);
    }
    provider_on(connection, &context.provider_id, true)
}

fn selection_on(
    connection: &Connection,
    attempt_id: &str,
) -> Result<Option<(AccountSelection, bool, Option<String>)>, AccountError> {
    let value = connection
        .query_row(
            "SELECT selection,dispatched,outcome FROM account_pool_attempts WHERE attempt_id=?1",
            [attempt_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, bool>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(store_error)?;
    value
        .map(|(selection, dispatched, outcome)| {
            Ok((
                serde_json::from_str(&selection).map_err(|_| AccountError::StoreUnavailable)?,
                dispatched,
                outcome,
            ))
        })
        .transpose()
}

enum DispatchStart<F: Future> {
    Complete(F::Output),
    Pending(Pin<Box<F>>),
}

impl AccountPool {
    fn eligible_on(
        &self,
        connection: &Connection,
        context: &SelectionContext,
    ) -> Result<Vec<AccountInfo>, AccountError> {
        self.candidates_on(connection, context, false)
    }

    fn candidates_on(
        &self,
        connection: &Connection,
        context: &SelectionContext,
        refresh: bool,
    ) -> Result<Vec<AccountInfo>, AccountError> {
        admitted_on(connection, context)?;
        let policy = policy_on(connection, &context.provider_id)?;
        let mut query = connection.prepare("SELECT metadata FROM account_pool_accounts WHERE json_extract(metadata,'$.providerId')=?1 ORDER BY id").map_err(store_error)?;
        let values = query
            .query_map([&context.provider_id], |row| row.get::<_, String>(0))
            .map_err(store_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(store_error)?;
        let mut result = Vec::new();
        for value in values {
            let record: AccountRecord =
                serde_json::from_str(&value).map_err(|_| AccountError::StoreUnavailable)?;
            match require_selection_destination_on(connection, &record, context) {
                Ok(()) => {}
                Err(
                    AccountError::DestinationApprovalRequired | AccountError::NoEligibleAccount,
                ) => continue,
                Err(error) => return Err(error),
            }
            let info = self.info_on(connection, &record)?;
            if info.status
                == if refresh {
                    AccountStatus::RefreshRequired
                } else {
                    AccountStatus::Available
                }
                && (policy.mode != SelectionMode::Fixed
                    || policy.fixed_account_id.as_deref() == Some(&record.id))
                && (record.available_models.is_empty()
                    || record.available_models.contains(&context.model))
            {
                result.push(info);
            }
        }
        Ok(result)
    }

    pub fn eligible(&self, context: &SelectionContext) -> Result<Vec<AccountInfo>, AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        self.eligible_on(&connection, context)
    }

    pub fn refresh_candidates(
        &self,
        context: &SelectionContext,
    ) -> Result<Vec<AccountInfo>, AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        self.candidates_on(&connection, context, true)
    }

    fn affinity_hash(
        &self,
        connection: &Connection,
        context: &SelectionContext,
    ) -> Result<Option<String>, AccountError> {
        context
            .affinity_key
            .as_ref()
            .map(|key| {
                if key.is_empty() || key.len() > 1024 {
                    return Err(AccountError::InvalidInput);
                }
                let salt: String = connection
                    .query_row(
                        "SELECT value FROM account_pool_meta WHERE id='affinity_salt'",
                        [],
                        |row| row.get(0),
                    )
                    .map_err(store_error)?;
                let mut hash = Sha256::new();
                for value in ["xy-accounts-affinity-v1", &salt, &context.provider_id, key] {
                    hash.update((value.len() as u64).to_be_bytes());
                    hash.update(value.as_bytes());
                }
                Ok(format!("{:x}", hash.finalize()))
            })
            .transpose()
    }

    pub fn select(&self, context: &SelectionContext) -> Result<AccountSelection, AccountError> {
        let _cancel = context.cancellation.0.gate.lock().map_err(lock_error)?;
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let mut connection = connection.lock().map_err(lock_error)?;
        let transaction = connection.transaction().map_err(store_error)?;
        let eligible = self.eligible_on(&transaction, context)?;
        if let Some((mut existing, dispatched, outcome)) =
            selection_on(&transaction, &context.attempt_id)?
        {
            if dispatched || outcome.is_some() {
                return Err(AccountError::AlreadyDispatched);
            }
            if existing.task_id != context.task_id || existing.provider_id != context.provider_id {
                return Err(AccountError::AttemptNotAdmitted);
            }
            if let Some(account) = eligible
                .iter()
                .find(|account| account.id == existing.account_id)
            {
                existing.credential_generation = account.credential_generation;
                existing.reason = SelectionReason::ExistingAttempt;
                transaction.execute("UPDATE account_pool_attempts SET selection=?2 WHERE attempt_id=?1 AND dispatched=0 AND outcome IS NULL",
                    params![context.attempt_id, serde_json::to_string(&existing).map_err(|_| AccountError::StoreUnavailable)?]).map_err(store_error)?;
                transaction.commit().map_err(store_error)?;
                return Ok(existing);
            }
        }
        if eligible.is_empty() {
            return Err(AccountError::NoEligibleAccount);
        }
        let policy = policy_on(&transaction, &context.provider_id)?;
        let affinity = if policy.affinity {
            self.affinity_hash(&transaction, context)?
        } else {
            None
        };
        let preferred: Option<String> = if let Some(hash) = &affinity {
            transaction.query_row("SELECT account_id FROM account_pool_affinity WHERE provider_id=?1 AND affinity_hash=?2",
                params![context.provider_id, hash], |row| row.get(0)).optional().map_err(store_error)?
        } else {
            None
        };
        let cursor: Option<String> = transaction
            .query_row(
                "SELECT cursor FROM account_pool_policies WHERE provider_id=?1",
                [&context.provider_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?
            .flatten();
        let (account, reason) = if policy.mode == SelectionMode::Fixed {
            (&eligible[0], SelectionReason::Fixed)
        } else if let Some(account) = preferred
            .as_ref()
            .and_then(|id| eligible.iter().find(|account| &account.id == id))
        {
            (account, SelectionReason::Affinity)
        } else {
            (
                cursor
                    .as_ref()
                    .and_then(|cursor| eligible.iter().find(|account| account.id > *cursor))
                    .unwrap_or(&eligible[0]),
                SelectionReason::RoundRobin,
            )
        };
        let selection = AccountSelection {
            account_id: account.id.clone(),
            provider_id: context.provider_id.clone(),
            task_id: context.task_id.clone(),
            attempt_id: context.attempt_id.clone(),
            credential_generation: account.credential_generation,
            reason,
        };
        let selected = transaction.execute("INSERT INTO account_pool_attempts(attempt_id,selection,created_at) VALUES(?1,?2,?3)
                ON CONFLICT(attempt_id) DO UPDATE SET selection=excluded.selection WHERE account_pool_attempts.dispatched=0 AND account_pool_attempts.outcome IS NULL",
            params![context.attempt_id, serde_json::to_string(&selection).map_err(|_| AccountError::StoreUnavailable)?, now()]).map_err(store_error)?;
        if selected != 1 {
            return Err(AccountError::AlreadyDispatched);
        }
        transaction.execute("INSERT INTO account_pool_policies(provider_id,policy,cursor) VALUES(?1,?2,?3) ON CONFLICT(provider_id) DO UPDATE SET cursor=excluded.cursor",
            params![context.provider_id, serde_json::to_string(&policy).map_err(|_| AccountError::StoreUnavailable)?, account.id]).map_err(store_error)?;
        if let Some(hash) = affinity {
            transaction.execute("INSERT INTO account_pool_affinity(provider_id,affinity_hash,account_id,updated_at) VALUES(?1,?2,?3,?4)
                ON CONFLICT(provider_id,affinity_hash) DO UPDATE SET account_id=excluded.account_id,updated_at=excluded.updated_at",
                params![context.provider_id, hash, account.id, now()]).map_err(store_error)?;
        }
        transaction.commit().map_err(store_error)?;
        Ok(selection)
    }

    fn validate_selection_on(
        &self,
        connection: &Connection,
        selection: &AccountSelection,
        context: &SelectionContext,
    ) -> Result<AccountRecord, AccountError> {
        admitted_on(connection, context)?;
        let (stored, dispatched, outcome) = selection_on(connection, &context.attempt_id)?
            .ok_or(AccountError::AttemptNotAdmitted)?;
        if dispatched || outcome.is_some() {
            return Err(AccountError::AlreadyDispatched);
        }
        if selection.task_id != context.task_id
            || selection.provider_id != context.provider_id
            || selection.attempt_id != context.attempt_id
            || stored.account_id != selection.account_id
            || stored.credential_generation != selection.credential_generation
        {
            return Err(AccountError::StaleSelection);
        }
        let record = load_record(connection, &selection.account_id)?;
        if record.revoked {
            return Err(AccountError::Revoked);
        }
        if record.provider_id != context.provider_id
            || record.credential_generation != selection.credential_generation
        {
            return Err(AccountError::StaleSelection);
        }
        require_selection_destination_on(connection, &record, context)?;
        if !self
            .eligible_on(connection, context)?
            .iter()
            .any(|account| account.id == record.id)
        {
            return Err(AccountError::NoEligibleAccount);
        }
        Ok(record)
    }

    fn authorization_on(
        &self,
        record: &AccountRecord,
    ) -> Result<AccountAuthorization, AccountError> {
        match record.kind {
            AccountKind::ApiKey | AccountKind::OAuth => {
                let secret = self
                    .credentials
                    .get(&secret_id(
                        &record.id,
                        record.credential_generation,
                        "access",
                    ))?
                    .ok_or(AccountError::CredentialMissing)?;
                Ok(if record.kind == AccountKind::ApiKey {
                    AccountAuthorization::ApiKey(secret)
                } else {
                    AccountAuthorization::OAuth(secret)
                })
            }
            AccountKind::NativeManaged => Ok(AccountAuthorization::NativeManaged {
                client: record.client.ok_or(AccountError::StoreUnavailable)?,
                reference_id: record
                    .native_reference
                    .clone()
                    .ok_or(AccountError::LoginRequired)?,
            }),
        }
    }

    pub(crate) fn prepare_authorization(
        &self,
        selection: &AccountSelection,
        context: &SelectionContext,
    ) -> Result<AccountAuthorization, AccountError> {
        let _cancel = context.cancellation.0.gate.lock().map_err(lock_error)?;
        if context.cancellation.is_cancelled() {
            return Err(AccountError::Cancelled);
        }
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let record = self.validate_selection_on(&connection, selection, context)?;
        self.authorization_on(&record)
    }

    pub async fn dispatch<F, SendFuture>(
        &self,
        selection: &AccountSelection,
        context: &SelectionContext,
        send: F,
    ) -> Result<SendFuture::Output, AccountError>
    where
        F: FnOnce(AccountAuthorization) -> SendFuture,
        SendFuture: Future,
    {
        self.dispatch_inner(selection, context, None, |_, authorization| {
            send(authorization)
        })
        .await
    }

    pub async fn dispatch_with_connection<F, SendFuture>(
        &self,
        selection: &AccountSelection,
        context: &SelectionContext,
        connection_cancellation: &AccountCancellation,
        send: F,
    ) -> Result<SendFuture::Output, AccountError>
    where
        F: FnOnce(AccountAuthorization) -> SendFuture,
        SendFuture: Future,
    {
        self.dispatch_inner(
            selection,
            context,
            Some(connection_cancellation),
            |_, authorization| send(authorization),
        )
        .await
    }

    pub(crate) async fn dispatch_with_connection_on<F, SendFuture>(
        &self,
        selection: &AccountSelection,
        context: &SelectionContext,
        connection_cancellation: &AccountCancellation,
        send: F,
    ) -> Result<SendFuture::Output, AccountError>
    where
        F: FnOnce(&Connection, AccountAuthorization) -> SendFuture,
        SendFuture: Future,
    {
        self.dispatch_inner(selection, context, Some(connection_cancellation), send)
            .await
    }

    async fn dispatch_inner<F, SendFuture>(
        &self,
        selection: &AccountSelection,
        context: &SelectionContext,
        connection_cancellation: Option<&AccountCancellation>,
        send: F,
    ) -> Result<SendFuture::Output, AccountError>
    where
        F: FnOnce(&Connection, AccountAuthorization) -> SendFuture,
        SendFuture: Future,
    {
        let mut send = Some(send);
        let start = std::future::poll_fn(|task| {
            let result = (|| {
                let _connection_cancel = connection_cancellation
                    .filter(|signal| !Arc::ptr_eq(&signal.0, &context.cancellation.0))
                    .map(|signal| signal.0.gate.lock().map_err(lock_error))
                    .transpose()?;
                let _cancel = context.cancellation.0.gate.lock().map_err(lock_error)?;
                if context.cancellation.is_cancelled()
                    || connection_cancellation.is_some_and(AccountCancellation::is_cancelled)
                {
                    return Err(AccountError::Cancelled);
                }
                let _guard = self.runtime.gate.lock().map_err(lock_error)?;
                let connection = self.database.conn();
                let connection = connection.lock().map_err(lock_error)?;
                let record = self.validate_selection_on(&connection, selection, context)?;
                let authorization = self.authorization_on(&record)?;
                let claimed = connection.execute("UPDATE account_pool_attempts SET dispatched=1 WHERE attempt_id=?1 AND dispatched=0 AND outcome IS NULL", [&context.attempt_id]).map_err(store_error)?;
                if claimed != 1 { return Err(AccountError::AlreadyDispatched); }
                let mut future = Box::pin(send.take().ok_or(AccountError::AlreadyDispatched)?(&connection, authorization));
                Ok(match future.as_mut().poll(task) { Poll::Ready(value) => DispatchStart::Complete(value), Poll::Pending => DispatchStart::Pending(future) })
            })();
            Poll::Ready(result)
        }).await?;
        match start {
            DispatchStart::Complete(value) => Ok(value),
            DispatchStart::Pending(future) => tokio::select! {
                biased;
                _ = context.cancellation.cancelled() => Err(AccountError::Cancelled),
                _ = async {
                    match connection_cancellation {
                        Some(signal) => signal.cancelled().await,
                        None => std::future::pending::<()>().await,
                    }
                } => Err(AccountError::Cancelled),
                value = future => Ok(value),
            },
        }
    }

    pub fn record_outcome(
        &self,
        selection: &AccountSelection,
        outcome: AccountOutcome,
    ) -> Result<AccountInfo, AccountError> {
        if let Some(quota) = &outcome.quota {
            valid_quota(quota, now())?;
        }
        if outcome.kind == OutcomeKind::QuotaExhausted
            && !quota_blocks(outcome.quota.as_ref(), now())
        {
            return Err(AccountError::InvalidQuotaEvidence);
        }
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let mut connection = connection.lock().map_err(lock_error)?;
        let transaction = connection.transaction().map_err(store_error)?;
        let (stored, _, previous) = selection_on(&transaction, &selection.attempt_id)?
            .ok_or(AccountError::AttemptNotAdmitted)?;
        if stored.account_id != selection.account_id
            || stored.task_id != selection.task_id
            || stored.provider_id != selection.provider_id
            || stored.credential_generation != selection.credential_generation
        {
            return Err(AccountError::StaleSelection);
        }
        let mut record = load_record(&transaction, &selection.account_id)?;
        if previous.is_some() {
            return self.info_on(&transaction, &record);
        }
        transaction.execute("UPDATE account_pool_attempts SET outcome=?2 WHERE attempt_id=?1 AND outcome IS NULL",
            params![selection.attempt_id, serde_json::to_string(&outcome.kind).map_err(|_| AccountError::StoreUnavailable)?]).map_err(store_error)?;
        if record.revoked
            || record.provider_id != selection.provider_id
            || record.credential_generation != selection.credential_generation
        {
            transaction.commit().map_err(store_error)?;
            return self.info_on(&connection, &record);
        }
        let policy = policy_on(&transaction, &record.provider_id)?;
        let timestamp = now();
        if outcome.kind != OutcomeKind::Cancelled {
            if let Some(quota) = outcome.quota {
                self.validate_quota_secrets(&record, &quota)?;
                if record
                    .quota
                    .as_ref()
                    .is_none_or(|old| quota.observed_at >= old.observed_at)
                {
                    record.quota = Some(quota);
                }
            }
            record.last_used_at = Some(timestamp);
            match outcome.kind {
                OutcomeKind::Success => {
                    if record.authorization_status != AuthorizationStatus::LoginRequired {
                        record.authorization_status = AuthorizationStatus::Authorized;
                        record.last_validated_at = Some(timestamp);
                    }
                    if record.cooldown_until.is_none_or(|until| until <= timestamp) {
                        record.consecutive_failures = 0;
                        record.cooldown_until = None;
                    }
                }
                OutcomeKind::Unauthorized => {
                    record.authorization_status = AuthorizationStatus::LoginRequired;
                    record.last_validated_at = None;
                }
                OutcomeKind::RateLimited | OutcomeKind::TransientFailure => {
                    record.consecutive_failures = record.consecutive_failures.saturating_add(1);
                    let base = if outcome.kind == OutcomeKind::RateLimited {
                        policy.rate_limit_cooldown_seconds
                    } else {
                        policy.failure_cooldown_seconds
                    };
                    let wait = outcome
                        .retry_after_seconds
                        .unwrap_or(base.saturating_mul(
                            1u64 << record.consecutive_failures.saturating_sub(1).min(10),
                        ))
                        .max(1)
                        .min(policy.max_cooldown_seconds);
                    extend_cooldown(&mut record, timestamp.saturating_add(wait as i64));
                }
                OutcomeKind::QuotaExhausted | OutcomeKind::Cancelled => {}
            }
            if outcome.kind != OutcomeKind::Success {
                transaction
                    .execute(
                        "DELETE FROM account_pool_affinity WHERE account_id=?1",
                        [&record.id],
                    )
                    .map_err(store_error)?;
            }
            advance(&mut record)?;
            save_record(&transaction, &record)?;
        }
        transaction.commit().map_err(store_error)?;
        self.info_on(&connection, &record)
    }

    pub fn update_quota(
        &self,
        id: &str,
        expected_generation: u64,
        quota: QuotaSnapshot,
    ) -> Result<AccountInfo, AccountError> {
        valid_quota(&quota, now())?;
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let mut record = load_record(&connection, id)?;
        if record.revoked {
            return Err(AccountError::Revoked);
        }
        if record.credential_generation != expected_generation {
            return Err(AccountError::StaleSelection);
        }
        self.validate_quota_secrets(&record, &quota)?;
        if record
            .quota
            .as_ref()
            .is_some_and(|old| quota.observed_at < old.observed_at)
        {
            return Err(AccountError::InvalidQuotaEvidence);
        }
        record.quota = Some(quota);
        advance(&mut record)?;
        save_record(&connection, &record)?;
        self.info_on(&connection, &record)
    }

    pub fn register_refresher(
        &self,
        id: &str,
        callback: Arc<dyn AccountRefresher>,
        timeout: Duration,
    ) -> Result<(), AccountError> {
        if timeout.is_zero() || timeout > Duration::from_secs(300) {
            return Err(AccountError::InvalidInput);
        }
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let record = load_record(&connection, id)?;
        if record.revoked {
            return Err(AccountError::Revoked);
        }
        if record.kind != AccountKind::OAuth {
            return Err(AccountError::InvalidInput);
        }
        require_record_destination_on(&connection, &record)?;
        self.cancel_refresh(id)?;
        self.runtime
            .refreshers
            .lock()
            .map_err(lock_error)?
            .insert(id.to_owned(), RefreshRegistration { callback, timeout });
        Ok(())
    }

    pub fn unregister_refresher(&self, id: &str) -> Result<(), AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        self.runtime
            .refreshers
            .lock()
            .map_err(lock_error)?
            .remove(id);
        self.cancel_refresh(id)
    }

    pub async fn refresh(
        &self,
        id: &str,
        waiter: &AccountCancellation,
    ) -> Result<AccountInfo, AccountError> {
        self.refresh_inner(id, waiter, true).await
    }

    pub async fn refresh_if_needed(
        &self,
        id: &str,
        waiter: &AccountCancellation,
    ) -> Result<AccountInfo, AccountError> {
        self.refresh_inner(id, waiter, false).await
    }

    async fn refresh_inner(
        &self,
        id: &str,
        waiter: &AccountCancellation,
        force: bool,
    ) -> Result<AccountInfo, AccountError> {
        if waiter.is_cancelled() {
            return Err(AccountError::Cancelled);
        }
        let flight = {
            let _cancel = waiter.0.gate.lock().map_err(lock_error)?;
            let _guard = self.runtime.gate.lock().map_err(lock_error)?;
            if waiter.is_cancelled() {
                return Err(AccountError::Cancelled);
            }
            let connection = self.database.conn();
            let connection = connection.lock().map_err(lock_error)?;
            let record = load_record(&connection, id)?;
            if record.revoked {
                return Err(AccountError::Revoked);
            }
            require_record_destination_on(&connection, &record)?;
            if !record.enabled {
                return Err(AccountError::NoEligibleAccount);
            }
            if record.refresh_login_required {
                return Err(AccountError::LoginRequired);
            }
            if !force
                && record.authorization_status != AuthorizationStatus::LoginRequired
                && record
                    .expires_at
                    .is_none_or(|expires| expires > now().saturating_add(30))
            {
                return self.info_on(&connection, &record);
            }
            if record.kind != AccountKind::OAuth
                || !record.allow_refresh
                || !record.has_refresh_token
            {
                return Err(AccountError::AuthorizationRequired);
            }
            let mut flights = self.runtime.flights.lock().map_err(lock_error)?;
            let existing = flights
                .get(id)
                .filter(|flight| flight.generation == record.credential_generation)
                .filter(|flight| !flight.cancellation.is_cancelled())
                .filter(|flight| {
                    flight.completed.borrow().is_none()
                        || record.cooldown_until.is_some_and(|until| until > now())
                        || matches!(
                            *flight.completed.borrow(),
                            Some(Err(AccountError::LoginRequired))
                        )
                })
                .cloned();
            if let Some(existing) = existing {
                existing
            } else {
                if record.cooldown_until.is_some_and(|until| until > now()) {
                    return Err(AccountError::RefreshRateLimited);
                }
                let refreshers = self.runtime.refreshers.lock().map_err(lock_error)?;
                let registration = refreshers
                    .get(id)
                    .ok_or(AccountError::RefreshNotConfigured)?;
                let refresh_token = self
                    .credentials
                    .get(&secret_id(id, record.credential_generation, "refresh"))?
                    .ok_or(AccountError::CredentialMissing)?;
                let flight = Arc::new(RefreshFlight {
                    generation: record.credential_generation,
                    completed: watch::channel(None).0,
                    cancellation: AccountCancellation::default(),
                });
                let request = RefreshRequest {
                    account_id: id.to_owned(),
                    provider_id: record.provider_id,
                    expected_generation: record.credential_generation,
                    refresh_token,
                    cancellation: flight.cancellation.clone(),
                };
                let pool = self.clone();
                let shared = flight.clone();
                let callback = registration.callback.clone();
                let timeout = registration.timeout;
                let account_id = id.to_owned();
                flights.insert(id.to_owned(), flight.clone());
                tokio::spawn(async move {
                    let mut completion = RefreshCompletion {
                        flight: shared.clone(),
                        finished: false,
                    };
                    let result = match pool
                        .invoke_refresher(&shared, &account_id, callback, request, timeout)
                        .await
                    {
                        Ok(result) => pool.finish_refresh(&completion.flight, &account_id, result),
                        Err(error) => Err(error),
                    };
                    completion.finish(result);
                });
                flight
            }
        };
        let mut receiver = flight.completed.subscribe();
        loop {
            if waiter.is_cancelled() {
                return Err(AccountError::Cancelled);
            }
            if let Some(result) = receiver.borrow_and_update().clone() {
                return result;
            }
            tokio::select! {
                biased;
                _ = waiter.cancelled() => return Err(AccountError::Cancelled),
                result = receiver.changed() => if result.is_err() { return Err(AccountError::RefreshFailed); },
            }
        }
    }

    async fn invoke_refresher(
        &self,
        flight: &RefreshFlight,
        id: &str,
        callback: Arc<dyn AccountRefresher>,
        request: RefreshRequest,
        timeout: Duration,
    ) -> Result<Result<RefreshedCredential, RefreshError>, AccountError> {
        let deadline = tokio::time::Instant::now() + timeout;
        let mut request = Some(request);
        let start = std::future::poll_fn(|task| {
            let result = (|| {
                let _guard = self.runtime.gate.lock().map_err(lock_error)?;
                let connection = self.database.conn();
                let connection = connection.lock().map_err(lock_error)?;
                let _cancel = flight.cancellation.0.gate.lock().map_err(lock_error)?;
                let record = load_record(&connection, id)?;
                if record.revoked {
                    return Err(AccountError::Revoked);
                }
                if record.credential_generation != flight.generation {
                    return Err(AccountError::StaleSelection);
                }
                require_record_destination_on(&connection, &record)?;
                if flight.cancellation.is_cancelled() || !record.enabled || !record.allow_refresh {
                    return Err(AccountError::Cancelled);
                }
                if record.kind != AccountKind::OAuth || !record.has_refresh_token {
                    return Err(AccountError::AuthorizationRequired);
                }
                let request = request.take().ok_or(AccountError::RefreshFailed)?;
                if request.account_id != record.id
                    || request.provider_id != record.provider_id
                    || request.expected_generation != record.credential_generation
                    || !Arc::ptr_eq(&request.cancellation.0, &flight.cancellation.0)
                {
                    return Err(AccountError::StaleSelection);
                }
                if tokio::time::Instant::now() >= deadline {
                    return Ok(DispatchStart::Complete(Err(RefreshError::Unavailable)));
                }
                Ok(std::panic::catch_unwind(AssertUnwindSafe(|| {
                    let mut future = Box::pin(callback.refresh(request));
                    match future.as_mut().poll(task) {
                        Poll::Ready(result) => DispatchStart::Complete(result),
                        Poll::Pending => DispatchStart::Pending(future),
                    }
                }))
                .unwrap_or(DispatchStart::Complete(Err(RefreshError::Unavailable))))
            })();
            Poll::Ready(result)
        })
        .await?;
        Ok(match start {
            DispatchStart::Complete(result) => result,
            DispatchStart::Pending(future) => tokio::select! {
                biased;
                _ = flight.cancellation.cancelled() => Err(RefreshError::Cancelled),
                result = tokio::time::timeout_at(deadline, AssertUnwindSafe(future).catch_unwind()) => {
                    result.unwrap_or(Ok(Err(RefreshError::Unavailable)))
                        .unwrap_or(Err(RefreshError::Unavailable))
                },
            },
        })
    }

    fn finish_refresh(
        &self,
        flight: &RefreshFlight,
        id: &str,
        result: Result<RefreshedCredential, RefreshError>,
    ) -> Result<AccountInfo, AccountError> {
        let _guard = self.runtime.gate.lock().map_err(lock_error)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(lock_error)?;
        let _cancel = flight.cancellation.0.gate.lock().map_err(lock_error)?;
        let mut record = load_record(&connection, id)?;
        if record.revoked {
            return Err(AccountError::Revoked);
        }
        if record.credential_generation != flight.generation {
            return Err(AccountError::StaleSelection);
        }
        require_record_destination_on(&connection, &record)?;
        if flight.cancellation.is_cancelled() || !record.enabled || !record.allow_refresh {
            return Err(AccountError::Cancelled);
        }
        let result = match result {
            Ok(result) if result.expires_at.is_some_and(|expires| expires <= now()) => {
                Err(RefreshError::Unauthorized)
            }
            result => result,
        };
        match result {
            Ok(result) => {
                let refresh_token = match result.refresh_token {
                    Some(token) => Some(token),
                    None => self.credentials.get(&secret_id(
                        id,
                        record.credential_generation,
                        "refresh",
                    ))?,
                };
                let credentials = AccountCredentialInput::OAuth {
                    access_token: result.access_token,
                    refresh_token,
                    expires_at: result.expires_at,
                };
                self.validate_credentials(
                    &credentials,
                    &record.alias,
                    &record.provider_id,
                    &record.available_models,
                )?;
                if record.quota.as_ref().is_some_and(|quota| {
                    credentials.secrets().iter().any(|(_, secret)| {
                        quota
                            .windows
                            .iter()
                            .any(|window| window.id.contains(secret.expose()))
                    })
                }) {
                    record.quota = None;
                }
                record.credential_generation = next_generation_on(&connection, &record)?;
                advance(&mut record)?;
                self.stage_on(&connection, id, record.credential_generation, &credentials)?;
                record.expires_at = result.expires_at;
                record.has_refresh_token = matches!(
                    credentials,
                    AccountCredentialInput::OAuth {
                        refresh_token: Some(_),
                        ..
                    }
                );
                record.refresh_login_required = false;
                record.authorization_status = AuthorizationStatus::Authorized;
                record.last_validated_at = Some(now());
                record.cooldown_until = record.cooldown_until.filter(|until| *until > now());
                if record.cooldown_until.is_none() {
                    record.consecutive_failures = 0;
                }
                if let Err(error) = save_record(&connection, &record) {
                    self.cleanup_on(&connection, Some(id))?;
                    return Err(error);
                }
                self.cleanup_on(&connection, Some(id))?;
                self.info_on(&connection, &record)
            }
            Err(error) => {
                let policy = policy_on(&connection, &record.provider_id)?;
                let result = match error {
                    RefreshError::Unauthorized => {
                        record.authorization_status = AuthorizationStatus::LoginRequired;
                        record.refresh_login_required = true;
                        record.last_validated_at = None;
                        AccountError::LoginRequired
                    }
                    RefreshError::RateLimited {
                        retry_after_seconds,
                    } => {
                        extend_cooldown(
                            &mut record,
                            now().saturating_add(
                                retry_after_seconds.max(1).min(policy.max_cooldown_seconds) as i64,
                            ),
                        );
                        AccountError::RefreshRateLimited
                    }
                    RefreshError::Unavailable => {
                        extend_cooldown(
                            &mut record,
                            now().saturating_add(policy.failure_cooldown_seconds as i64),
                        );
                        AccountError::RefreshFailed
                    }
                    RefreshError::Cancelled => return Err(AccountError::Cancelled),
                };
                advance(&mut record)?;
                save_record(&connection, &record)?;
                Err(result)
            }
        }
    }
}

struct RefreshCompletion {
    flight: Arc<RefreshFlight>,
    finished: bool,
}

impl RefreshCompletion {
    fn finish(&mut self, result: Result<AccountInfo, AccountError>) {
        self.flight.completed.send_replace(Some(result));
        self.finished = true;
    }
}

impl Drop for RefreshCompletion {
    fn drop(&mut self) {
        if !self.finished {
            self.flight
                .completed
                .send_replace(Some(Err(AccountError::RefreshFailed)));
        }
    }
}

#[tauri::command]
pub fn list_accounts(pool: tauri::State<'_, AccountPool>) -> Result<Vec<AccountInfo>, String> {
    pool.list().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn add_account(
    request: AddAccountRequest,
    pool: tauri::State<'_, AccountPool>,
) -> Result<AccountInfo, String> {
    crate::platform::require_desktop("accounts")?;
    pool.add(request).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn update_account(
    request: UpdateAccountRequest,
    pool: tauri::State<'_, AccountPool>,
) -> Result<AccountInfo, String> {
    crate::platform::require_desktop("accounts")?;
    pool.update(request).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn replace_account_credentials(
    request: ReplaceCredentialRequest,
    pool: tauri::State<'_, AccountPool>,
) -> Result<AccountInfo, String> {
    crate::platform::require_desktop("accounts")?;
    pool.replace_credentials(request)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_account_policy(
    provider_id: String,
    pool: tauri::State<'_, AccountPool>,
) -> Result<AccountPolicy, String> {
    pool.policy(&provider_id).map_err(|error| error.to_string())
}

#[tauri::command]
pub fn save_account_policy(
    provider_id: String,
    policy: AccountPolicy,
    confirmed: bool,
    pool: tauri::State<'_, AccountPool>,
) -> Result<AccountPolicy, String> {
    crate::platform::require_desktop("accounts")?;
    pool.save_policy(&provider_id, policy, confirmed)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn revoke_account(
    id: String,
    confirmed: bool,
    pool: tauri::State<'_, AccountPool>,
) -> Result<LocalRevocation, String> {
    crate::platform::require_desktop("accounts")?;
    pool.revoke(&id, confirmed)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn refresh_account(
    id: String,
    confirmed: bool,
    pool: tauri::State<'_, AccountPool>,
) -> Result<AccountInfo, String> {
    if !confirmed {
        return Err(AccountError::AuthorizationRequired.to_string());
    }
    crate::platform::require_desktop("accounts")?;
    pool.refresh(&id, &AccountCancellation::default())
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn choose_account_import(
    app: tauri::AppHandle,
    pool: tauri::State<'_, AccountPool>,
) -> Result<Option<SelectedAccountImport>, String> {
    crate::platform::require_desktop("accounts")?;
    use tauri_plugin_dialog::DialogExt;
    let selected = tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .add_filter("JSON", &["json"])
            .blocking_pick_file()
    })
    .await
    .map_err(|_| AccountError::InvalidImportFile.to_string())?;
    selected
        .map(|file| {
            let path = file
                .into_path()
                .map_err(|_| AccountError::InvalidImportFile.to_string())?;
            pool.select_import_source(&path)
                .map_err(|error| error.to_string())
        })
        .transpose()
}

#[tauri::command]
pub fn import_account(
    request: ImportAccountRequest,
    pool: tauri::State<'_, AccountPool>,
) -> Result<AccountInfo, String> {
    crate::platform::require_desktop("accounts")?;
    pool.import(request).map_err(|error| error.to_string())
}

#[cfg(test)]
#[path = "accounts_tests.rs"]
mod tests;
