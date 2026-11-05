use super::{store::PackageStore, types::*, validation::*};
use crate::database::Database;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

const PREVIEW_TTL: Duration = Duration::from_secs(300);
const MAX_INSTALL_PREVIEWS: usize = 8;
const MAX_CACHED_VERSIONS: i64 = 256;

fn error(code: &'static str) -> PluginError {
    PluginError::new(code)
}
pub(crate) fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .min(i64::MAX as u128) as i64
}
fn database_error(_: rusqlite::Error) -> PluginError {
    error("plugin_database")
}

#[derive(Clone)]
pub struct PluginManager {
    pub(crate) database: Database,
    pub(crate) store: Arc<PackageStore>,
    pub(crate) state: Arc<Mutex<HostState>>,
}

#[derive(Default)]
pub(crate) struct HostState {
    install_previews: BTreeMap<String, PendingInstall>,
    pub tool_previews: BTreeMap<String, super::runtime::PendingToolCall>,
    pub in_flight: usize,
}

impl HostState {
    fn clear_previews(&mut self) {
        self.install_previews.clear();
        self.tool_previews.clear();
    }
}

struct PendingInstall {
    published: PublishedPlugin,
    revision: i64,
    deadline: Instant,
    rollback: bool,
}

#[derive(Clone)]
pub(crate) struct VersionRecord {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub signing_key_id: String,
    pub signature: String,
    pub installed_at: i64,
}

pub(crate) struct VerifiedPackage {
    pub published: PublishedPlugin,
    pub validated: ValidatedPackage,
}

pub(crate) fn revision(connection: &Connection) -> PluginResult<i64> {
    connection
        .query_row(
            "SELECT revision FROM xy_plugin_host WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(database_error)
}

fn bump_revision(connection: &Connection) -> PluginResult<()> {
    connection
        .execute(
            "UPDATE xy_plugin_host SET revision=revision+1 WHERE singleton=1",
            [],
        )
        .map_err(database_error)?;
    Ok(())
}

pub(crate) fn load_key(connection: &Connection, id: &str) -> PluginResult<TrustedSigningKey> {
    let key: TrustedSigningKey = connection.query_row(
        "SELECT id,label,public_key,fingerprint,enabled FROM xy_plugin_trusted_keys WHERE id=?1 AND enabled=1", [id],
        |row| Ok(TrustedSigningKey { id: row.get(0)?, label: row.get(1)?, public_key: row.get(2)?, fingerprint: row.get(3)?, enabled: row.get(4)? })
    ).optional().map_err(database_error)?.ok_or_else(|| error("plugin_untrusted_key"))?;
    if validate_trust_key(&TrustedKeyInput {
        id: key.id.clone(),
        label: key.label.clone(),
        public_key: key.public_key.clone(),
    })? != key.fingerprint
    {
        return Err(error("plugin_untrusted_key"));
    }
    Ok(key)
}

pub(crate) fn version_record(
    connection: &Connection,
    id: &str,
    version: &str,
) -> PluginResult<Option<VersionRecord>> {
    connection.query_row("SELECT plugin_id,version,digest,signing_key_id,signature,installed_at FROM xy_plugin_versions WHERE plugin_id=?1 AND version=?2", params![id,version],
        |row| Ok(VersionRecord { id: row.get(0)?, version: row.get(1)?, digest: row.get(2)?, signing_key_id: row.get(3)?, signature: row.get(4)?, installed_at: row.get(5)? }))
        .optional().map_err(database_error)
}

pub(crate) fn active_version(connection: &Connection, id: &str) -> PluginResult<Option<String>> {
    connection
        .query_row(
            "SELECT active_version FROM xy_plugin_installations WHERE plugin_id=?1 AND installed=1",
            [id],
            |row| row.get::<_, Option<String>>(0),
        )
        .optional()
        .map(|value| value.flatten())
        .map_err(database_error)
}

fn withdrawn(connection: &Connection, record: &VersionRecord) -> PluginResult<bool> {
    connection.query_row("SELECT EXISTS(SELECT 1 FROM xy_plugin_withdrawals WHERE plugin_id=?1 AND version=?2 AND digest=?3)",
        params![record.id,record.version,record.digest], |row| row.get(0)).map_err(database_error)
}

pub(crate) fn approved_for_client(
    connection: &Connection,
    verified: &VerifiedPackage,
    client: &str,
) -> PluginResult<bool> {
    let manifest = &verified.published.package.manifest;
    if !manifest.clients.iter().any(|allowed| allowed == client) {
        return Ok(false);
    }
    let grant: Option<(String, String, bool)> = connection.query_row(
        "SELECT digest,permissions_json,enabled FROM xy_plugin_grants WHERE plugin_id=?1 AND version=?2 AND client=?3",
        params![manifest.id,manifest.version,client], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(database_error)?;
    let Some((digest, permissions, enabled)) = grant else {
        return Ok(false);
    };
    let permissions: PluginPermissions =
        serde_json::from_str(&permissions).map_err(|_| error("plugin_confirmation"))?;
    validate_permissions(&permissions)?;
    Ok(enabled && digest == verified.published.digest && permissions == manifest.permissions)
}

pub(crate) fn load_verified(
    connection: &Connection,
    store: &PackageStore,
    record: &VersionRecord,
) -> PluginResult<VerifiedPackage> {
    if withdrawn(connection, record)? {
        return Err(error("plugin_withdrawn"));
    }
    let published = store.read(&record.digest)?;
    let manifest = &published.package.manifest;
    if manifest.id != record.id
        || manifest.version != record.version
        || published.digest != record.digest
        || published.signing_key_id != record.signing_key_id
        || published.signature != record.signature
    {
        return Err(error("plugin_immutable_version"));
    }
    let key = load_key(connection, &record.signing_key_id)?;
    let validated = verify_published(&published, &key)?;
    Ok(VerifiedPackage {
        published,
        validated,
    })
}

pub(crate) fn require_dependencies(
    connection: &Connection,
    store: &PackageStore,
    manifest: &PluginManifest,
    client: Option<&str>,
) -> PluginResult<()> {
    fn visit(
        connection: &Connection,
        store: &PackageStore,
        manifest: &PluginManifest,
        client: Option<&str>,
        visiting: &mut BTreeSet<String>,
        complete: &mut BTreeSet<String>,
    ) -> PluginResult<()> {
        if visiting.len() >= 32 || complete.len() >= 128 || !visiting.insert(manifest.id.clone()) {
            return Err(error("plugin_dependency"));
        }
        for dependency in &manifest.dependencies {
            let (id, version) = parse_dependency(dependency)?;
            if visiting.contains(id) {
                return Err(error("plugin_dependency"));
            }
            if active_version(connection, id)?.as_deref() != Some(version) {
                return Err(error("plugin_dependency"));
            }
            let record = version_record(connection, id, version)?
                .ok_or_else(|| error("plugin_dependency"))?;
            let verified = load_verified(connection, store, &record)
                .map_err(|_| error("plugin_dependency"))?;
            if let Some(client) = client {
                require_supported_runtime(&verified.published.package.manifest.permissions)
                    .map_err(|_| error("plugin_dependency"))?;
                if !approved_for_client(connection, &verified, client)? {
                    return Err(error("plugin_dependency"));
                }
            }
            if !complete.contains(id) {
                visit(
                    connection,
                    store,
                    &verified.published.package.manifest,
                    client,
                    visiting,
                    complete,
                )?;
            }
        }
        visiting.remove(&manifest.id);
        complete.insert(manifest.id.clone());
        Ok(())
    }
    visit(
        connection,
        store,
        manifest,
        client,
        &mut BTreeSet::new(),
        &mut BTreeSet::new(),
    )
}

pub(crate) fn active_verified(
    connection: &Connection,
    store: &PackageStore,
    id: &str,
    client: &str,
) -> PluginResult<VerifiedPackage> {
    validate_id(id)?;
    validate_client(client)?;
    let version = active_version(connection, id)?.ok_or_else(|| error("plugin_disabled"))?;
    let record =
        version_record(connection, id, &version)?.ok_or_else(|| error("plugin_not_found"))?;
    let verified = load_verified(connection, store, &record)?;
    require_supported_runtime(&verified.published.package.manifest.permissions)?;
    if !approved_for_client(connection, &verified, client)? {
        return Err(error("plugin_disabled"));
    }
    require_dependencies(
        connection,
        store,
        &verified.published.package.manifest,
        Some(client),
    )?;
    Ok(verified)
}

fn same_version(record: &VersionRecord, published: &PublishedPlugin) -> bool {
    record.digest == published.digest
        && record.signature == published.signature
        && record.signing_key_id == published.signing_key_id
}

fn check_install(
    connection: &Connection,
    store: &PackageStore,
    published: &PublishedPlugin,
    rollback: bool,
) -> PluginResult<(ValidatedPackage, TrustedSigningKey, Option<String>)> {
    let key = load_key(connection, &published.signing_key_id)?;
    let validated = verify_published(published, &key)?;
    let manifest = &published.package.manifest;
    if let Some(record) = version_record(connection, &manifest.id, &manifest.version)? {
        if !same_version(&record, published) {
            return Err(error("plugin_immutable_version"));
        }
        load_verified(connection, store, &record)?;
    } else if rollback {
        return Err(error("plugin_not_found"));
    }
    let previous = active_version(connection, &manifest.id)?;
    if let Some(previous) = &previous {
        if previous != &manifest.version {
            let ordering = parse_version(&manifest.version)?.cmp(&parse_version(previous)?);
            if !rollback && ordering != std::cmp::Ordering::Greater
                || rollback && ordering != std::cmp::Ordering::Less
            {
                return Err(error("plugin_immutable_version"));
            }
        } else if rollback {
            return Err(error("plugin_confirmation"));
        }
    }
    let probe = VersionRecord {
        id: manifest.id.clone(),
        version: manifest.version.clone(),
        digest: published.digest.clone(),
        signing_key_id: published.signing_key_id.clone(),
        signature: published.signature.clone(),
        installed_at: 0,
    };
    if withdrawn(connection, &probe)? {
        return Err(error("plugin_withdrawn"));
    }
    require_dependencies(connection, store, manifest, None)?;
    Ok((validated, key, previous))
}

impl PluginManager {
    pub fn new(database: Database, root: PathBuf) -> PluginResult<Self> {
        let store = Arc::new(PackageStore::open(root)?);
        let store_identity = store.identity()?;
        let connection = database.conn();
        let mut connection = connection.lock().map_err(|_| error("plugin_database"))?;
        let transaction = connection.transaction().map_err(database_error)?;
        transaction.execute_batch(
            "CREATE TABLE IF NOT EXISTS xy_plugin_host(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema_version INTEGER NOT NULL,root_identity TEXT NOT NULL,revision INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS xy_plugin_trusted_keys(id TEXT PRIMARY KEY,label TEXT NOT NULL,public_key TEXT NOT NULL,fingerprint TEXT NOT NULL,enabled INTEGER NOT NULL,configured_at INTEGER NOT NULL);
             CREATE TABLE IF NOT EXISTS xy_plugin_versions(plugin_id TEXT NOT NULL,version TEXT NOT NULL,digest TEXT NOT NULL,signing_key_id TEXT NOT NULL,signature TEXT NOT NULL,installed_at INTEGER NOT NULL,PRIMARY KEY(plugin_id,version));
             CREATE TRIGGER IF NOT EXISTS xy_plugin_versions_immutable BEFORE UPDATE ON xy_plugin_versions BEGIN SELECT RAISE(ABORT,'immutable plugin version'); END;
             CREATE TABLE IF NOT EXISTS xy_plugin_installations(plugin_id TEXT PRIMARY KEY,active_version TEXT,installed INTEGER NOT NULL DEFAULT 0);
             CREATE TABLE IF NOT EXISTS xy_plugin_grants(plugin_id TEXT NOT NULL,version TEXT NOT NULL,client TEXT NOT NULL,digest TEXT NOT NULL,permissions_json TEXT NOT NULL,enabled INTEGER NOT NULL DEFAULT 0,approved_at INTEGER NOT NULL,PRIMARY KEY(plugin_id,version,client));
             CREATE TABLE IF NOT EXISTS xy_plugin_withdrawals(plugin_id TEXT NOT NULL,version TEXT NOT NULL,digest TEXT NOT NULL,reason TEXT NOT NULL,signing_key_id TEXT NOT NULL,signature TEXT NOT NULL,created_at INTEGER NOT NULL,PRIMARY KEY(plugin_id,version,digest));"
        ).map_err(database_error)?;
        transaction.execute("INSERT OR IGNORE INTO xy_plugin_host(singleton,schema_version,root_identity,revision) VALUES(1,1,?1,0)", [&store_identity]).map_err(database_error)?;
        let (schema, identity): (i64, String) = transaction
            .query_row(
                "SELECT schema_version,root_identity FROM xy_plugin_host WHERE singleton=1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(database_error)?;
        if schema != 1 || identity != store_identity {
            return Err(error("plugin_storage_boundary"));
        }
        transaction.commit().map_err(database_error)?;
        drop(connection);
        Ok(Self {
            database,
            store,
            state: Arc::new(Mutex::new(HostState::default())),
        })
    }

    pub fn validate_local_package(&self, package_json: &str) -> PluginResult<PackageValidation> {
        let input = strict_json(package_json, MAX_PACKAGE_BYTES + 2048)?;
        let package = if input.get("package").is_some() {
            let published: PublishedPlugin =
                serde_json::from_value(input).map_err(|_| error("plugin_schema"))?;
            validate_published_metadata(&published)?;
            if package_digest(&published.package)? != published.digest {
                return Err(error("plugin_digest"));
            }
            published.package
        } else {
            serde_json::from_value(input).map_err(|_| error("plugin_schema"))?
        };
        let validated = validate_package(&package)?;
        Ok(validation_summary(&package, &validated))
    }

    pub fn preview_trusted_key(&self, input: &TrustedKeyInput) -> PluginResult<TrustedSigningKey> {
        let fingerprint = validate_trust_key(input)?;
        Ok(TrustedSigningKey {
            id: input.id.clone(),
            label: input.label.clone(),
            public_key: input.public_key.clone(),
            fingerprint,
            enabled: false,
        })
    }

    pub fn preview_install_json(&self, published_json: &str) -> PluginResult<InstallPreview> {
        self.preview_install(parse_published_json(published_json)?)
    }

    pub fn preview_install(&self, published: PublishedPlugin) -> PluginResult<InstallPreview> {
        self.create_install_preview(published, false)
    }

    pub fn preview_installed(&self, id: &str) -> PluginResult<InstallPreview> {
        validate_id(id)?;
        let published = {
            let connection = self.database.conn();
            let connection = connection.lock().map_err(|_| error("plugin_database"))?;
            let version =
                active_version(&connection, id)?.ok_or_else(|| error("plugin_not_found"))?;
            let record = version_record(&connection, id, &version)?
                .ok_or_else(|| error("plugin_not_found"))?;
            load_verified(&connection, &self.store, &record)?.published
        };
        self.create_install_preview(published, false)
    }

    pub fn preview_rollback(&self, id: &str, version: &str) -> PluginResult<InstallPreview> {
        validate_id(id)?;
        parse_version(version)?;
        let published = {
            let connection = self.database.conn();
            let connection = connection.lock().map_err(|_| error("plugin_database"))?;
            let record = version_record(&connection, id, version)?
                .ok_or_else(|| error("plugin_not_found"))?;
            load_verified(&connection, &self.store, &record)?.published
        };
        self.create_install_preview(published, true)
    }

    fn create_install_preview(
        &self,
        published: PublishedPlugin,
        rollback: bool,
    ) -> PluginResult<InstallPreview> {
        let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(|_| error("plugin_database"))?;
        let current_revision = revision(&connection)?;
        state.install_previews.retain(|_, preview| {
            preview.deadline > Instant::now() && preview.revision == current_revision
        });
        if state.install_previews.len() >= MAX_INSTALL_PREVIEWS {
            return Err(error("plugin_limit"));
        }
        let (validated, key, previous) =
            check_install(&connection, &self.store, &published, rollback)?;
        let previous_verified = previous
            .as_ref()
            .and_then(|version| {
                version_record(&connection, &published.package.manifest.id, version)
                    .ok()
                    .flatten()
            })
            .and_then(|record| load_verified(&connection, &self.store, &record).ok());
        let mut changes = Vec::new();
        for client in &published.package.manifest.clients {
            let baseline = if let Some(previous) = &previous_verified {
                if approved_for_client(&connection, previous, client)? {
                    previous.published.package.manifest.permissions.clone()
                } else {
                    PluginPermissions::default()
                }
            } else {
                PluginPermissions::default()
            };
            changes.push(ClientPermissionChange {
                client: client.clone(),
                added: published.package.manifest.permissions.added_to(&baseline),
            });
        }
        let preview_id = uuid::Uuid::new_v4().to_string();
        let preview = InstallPreview {
            preview_id: preview_id.clone(),
            expires_at: now() + PREVIEW_TTL.as_millis() as i64,
            validation: validation_summary(&published.package, &validated),
            signing_key_id: key.id,
            signing_key_fingerprint: key.fingerprint,
            previous_version: previous,
            permission_changes: changes,
        };
        state.install_previews.insert(
            preview_id,
            PendingInstall {
                published,
                revision: current_revision,
                deadline: Instant::now() + PREVIEW_TTL,
                rollback,
            },
        );
        Ok(preview)
    }

    pub fn confirm_install_json(&self, confirmation_json: &str) -> PluginResult<InstalledPlugin> {
        let confirmation = serde_json::from_value(strict_json(confirmation_json, 8192)?)
            .map_err(|_| error("plugin_confirmation"))?;
        self.confirm_install(confirmation)
    }

    pub fn confirm_install(
        &self,
        confirmation: InstallConfirmation,
    ) -> PluginResult<InstalledPlugin> {
        let id = {
            let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
            let pending = state
                .install_previews
                .remove(&confirmation.preview_id)
                .ok_or_else(|| error("plugin_preview_expired"))?;
            let connection = self.database.conn();
            let mut connection = connection.lock().map_err(|_| error("plugin_database"))?;
            let transaction = connection.transaction().map_err(database_error)?;
            if pending.deadline <= Instant::now() || pending.revision != revision(&transaction)? {
                return Err(error("plugin_preview_expired"));
            }
            let manifest = &pending.published.package.manifest;
            if !confirmation.confirmed
                || confirmation.digest != pending.published.digest
                || confirmation.accepted_permissions != manifest.permissions
                || confirmation.clients.is_empty()
                || confirmation.clients.len() > manifest.clients.len()
            {
                return Err(error("plugin_confirmation"));
            }
            let mut clients = BTreeSet::new();
            for client in &confirmation.clients {
                validate_client(client)?;
                if !manifest.clients.contains(client) || !clients.insert(client) {
                    return Err(error("plugin_confirmation"));
                }
            }
            check_install(
                &transaction,
                &self.store,
                &pending.published,
                pending.rollback,
            )?;
            if confirmation.enable {
                require_supported_runtime(&manifest.permissions)?;
                for client in &confirmation.clients {
                    require_dependencies(&transaction, &self.store, manifest, Some(client))?;
                }
            }
            let exists = version_record(&transaction, &manifest.id, &manifest.version)?.is_some();
            let count: i64 = transaction
                .query_row("SELECT COUNT(*) FROM xy_plugin_versions", [], |row| {
                    row.get(0)
                })
                .map_err(database_error)?;
            if !exists && count >= MAX_CACHED_VERSIONS {
                return Err(error("plugin_limit"));
            }
            self.store.put(&pending.published)?;
            transaction.execute("INSERT OR IGNORE INTO xy_plugin_versions(plugin_id,version,digest,signing_key_id,signature,installed_at) VALUES(?1,?2,?3,?4,?5,?6)",
                params![manifest.id,manifest.version,pending.published.digest,pending.published.signing_key_id,pending.published.signature,now()]).map_err(database_error)?;
            transaction.execute("INSERT INTO xy_plugin_installations(plugin_id,active_version,installed) VALUES(?1,?2,1) ON CONFLICT(plugin_id) DO UPDATE SET active_version=excluded.active_version,installed=1",
                params![manifest.id,manifest.version]).map_err(database_error)?;
            transaction
                .execute(
                    "UPDATE xy_plugin_grants SET enabled=0 WHERE plugin_id=?1",
                    [&manifest.id],
                )
                .map_err(database_error)?;
            let permissions = serde_json::to_string(&manifest.permissions)
                .map_err(|_| error("plugin_permissions"))?;
            for client in &confirmation.clients {
                transaction.execute("INSERT INTO xy_plugin_grants(plugin_id,version,client,digest,permissions_json,enabled,approved_at) VALUES(?1,?2,?3,?4,?5,?6,?7)
                    ON CONFLICT(plugin_id,version,client) DO UPDATE SET digest=excluded.digest,permissions_json=excluded.permissions_json,enabled=excluded.enabled,approved_at=excluded.approved_at",
                    params![manifest.id,manifest.version,client,pending.published.digest,permissions,confirmation.enable,now()]).map_err(database_error)?;
            }
            bump_revision(&transaction)?;
            transaction.commit().map_err(database_error)?;
            state.clear_previews();
            manifest.id.clone()
        };
        self.installed(&id)
    }

    pub fn disable_plugin(&self, id: &str, client: Option<&str>) -> PluginResult<InstalledPlugin> {
        validate_id(id)?;
        if let Some(client) = client {
            validate_client(client)?;
        }
        {
            let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
            let connection = self.database.conn();
            let mut connection = connection.lock().map_err(|_| error("plugin_database"))?;
            let transaction = connection.transaction().map_err(database_error)?;
            if active_version(&transaction, id)?.is_none() {
                return Err(error("plugin_not_found"));
            }
            transaction.execute("UPDATE xy_plugin_grants SET enabled=0 WHERE plugin_id=?1 AND (?2 IS NULL OR client=?2)", params![id,client]).map_err(database_error)?;
            bump_revision(&transaction)?;
            transaction.commit().map_err(database_error)?;
            state.clear_previews();
        }
        self.installed(id)
    }

    pub fn enable_plugin_client(
        &self,
        id: &str,
        client: &str,
        digest: &str,
        confirmed: bool,
    ) -> PluginResult<InstalledPlugin> {
        validate_id(id)?;
        validate_client(client)?;
        validate_hex(digest, 32).map_err(|_| error("plugin_digest"))?;
        if !confirmed {
            return Err(error("plugin_confirmation"));
        }
        {
            let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
            let connection = self.database.conn();
            let mut connection = connection.lock().map_err(|_| error("plugin_database"))?;
            let transaction = connection.transaction().map_err(database_error)?;
            let version =
                active_version(&transaction, id)?.ok_or_else(|| error("plugin_not_found"))?;
            let record = version_record(&transaction, id, &version)?
                .ok_or_else(|| error("plugin_not_found"))?;
            let verified = load_verified(&transaction, &self.store, &record)?;
            let manifest = &verified.published.package.manifest;
            if digest != verified.published.digest
                || !manifest.clients.iter().any(|allowed| allowed == client)
            {
                return Err(error("plugin_confirmation"));
            }
            require_supported_runtime(&manifest.permissions)?;
            require_dependencies(&transaction, &self.store, manifest, Some(client))?;
            let grant: Option<(String, String)> = transaction.query_row("SELECT digest,permissions_json FROM xy_plugin_grants WHERE plugin_id=?1 AND version=?2 AND client=?3",
                params![id,version,client], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(database_error)?;
            let (approved_digest, permissions) =
                grant.ok_or_else(|| error("plugin_confirmation"))?;
            let permissions: PluginPermissions =
                serde_json::from_str(&permissions).map_err(|_| error("plugin_confirmation"))?;
            validate_permissions(&permissions)?;
            if approved_digest != digest || permissions != manifest.permissions {
                return Err(error("plugin_confirmation"));
            }
            transaction.execute("UPDATE xy_plugin_grants SET enabled=1 WHERE plugin_id=?1 AND version=?2 AND client=?3", params![id,version,client]).map_err(database_error)?;
            bump_revision(&transaction)?;
            transaction.commit().map_err(database_error)?;
            state.clear_previews();
        }
        self.installed(id)
    }

    pub fn uninstall_plugin(&self, id: &str, confirmed: bool) -> PluginResult<InstalledPlugin> {
        validate_id(id)?;
        if !confirmed {
            return Err(error("plugin_confirmation"));
        }
        {
            let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
            let connection = self.database.conn();
            let mut connection = connection.lock().map_err(|_| error("plugin_database"))?;
            let transaction = connection.transaction().map_err(database_error)?;
            if active_version(&transaction, id)?.is_none() {
                return Err(error("plugin_not_found"));
            }
            transaction
                .execute("DELETE FROM xy_plugin_grants WHERE plugin_id=?1", [id])
                .map_err(database_error)?;
            transaction.execute("UPDATE xy_plugin_installations SET installed=0,active_version=NULL WHERE plugin_id=?1", [id]).map_err(database_error)?;
            bump_revision(&transaction)?;
            transaction.commit().map_err(database_error)?;
            state.clear_previews();
        }
        self.installed(id)
    }

    pub fn apply_withdrawal_json(&self, notice_json: &str) -> PluginResult<InstalledPlugin> {
        let notice = serde_json::from_value(strict_json(notice_json, 4096)?)
            .map_err(|_| error("plugin_schema"))?;
        self.apply_withdrawal(notice)
    }

    pub fn apply_withdrawal(&self, notice: PluginWithdrawal) -> PluginResult<InstalledPlugin> {
        let payload = withdrawal_payload(&notice)?;
        validate_id(&notice.signing_key_id)?;
        let signature = decode_hex(&notice.signature, 64)?;
        {
            let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
            let connection = self.database.conn();
            let mut connection = connection.lock().map_err(|_| error("plugin_database"))?;
            let transaction = connection.transaction().map_err(database_error)?;
            let record = version_record(&transaction, &notice.id, &notice.version)?
                .ok_or_else(|| error("plugin_not_found"))?;
            if record.digest != notice.digest || record.signing_key_id != notice.signing_key_id {
                return Err(error("plugin_signature"));
            }
            let key = load_key(&transaction, &notice.signing_key_id)?;
            ring::signature::UnparsedPublicKey::new(
                &ring::signature::ED25519,
                decode_hex(&key.public_key, 32)?,
            )
            .verify(&payload, &signature)
            .map_err(|_| error("plugin_signature"))?;
            transaction.execute("INSERT OR IGNORE INTO xy_plugin_withdrawals(plugin_id,version,digest,reason,signing_key_id,signature,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",
                params![notice.id,notice.version,notice.digest,notice.reason.as_str(),notice.signing_key_id,notice.signature,now()]).map_err(database_error)?;
            transaction
                .execute(
                    "UPDATE xy_plugin_grants SET enabled=0 WHERE plugin_id=?1 AND version=?2",
                    params![notice.id, notice.version],
                )
                .map_err(database_error)?;
            bump_revision(&transaction)?;
            transaction.commit().map_err(database_error)?;
            state.clear_previews();
        }
        self.installed(&notice.id)
    }

    pub fn list_installed(&self) -> PluginResult<Vec<InstalledPlugin>> {
        let connection = self.database.conn();
        let connection = connection.lock().map_err(|_| error("plugin_database"))?;
        let mut statement = connection
            .prepare("SELECT plugin_id FROM xy_plugin_installations ORDER BY plugin_id")
            .map_err(database_error)?;
        let ids = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        ids.iter()
            .map(|id| self.read_installed(&connection, id))
            .collect()
    }

    pub fn installed(&self, id: &str) -> PluginResult<InstalledPlugin> {
        validate_id(id)?;
        let connection = self.database.conn();
        let connection = connection.lock().map_err(|_| error("plugin_database"))?;
        self.read_installed(&connection, id)
    }

    fn read_installed(&self, connection: &Connection, id: &str) -> PluginResult<InstalledPlugin> {
        validate_id(id)?;
        let (active, installed): (Option<String>, bool) = connection
            .query_row(
                "SELECT active_version,installed FROM xy_plugin_installations WHERE plugin_id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(database_error)?
            .ok_or_else(|| error("plugin_not_found"))?;
        let mut statement = connection.prepare("SELECT plugin_id,version,digest,signing_key_id,signature,installed_at FROM xy_plugin_versions WHERE plugin_id=?1 ORDER BY installed_at DESC,version DESC").map_err(database_error)?;
        let records = statement
            .query_map([id], |row| {
                Ok(VersionRecord {
                    id: row.get(0)?,
                    version: row.get(1)?,
                    digest: row.get(2)?,
                    signing_key_id: row.get(3)?,
                    signature: row.get(4)?,
                    installed_at: row.get(5)?,
                })
            })
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        let mut result = InstalledPlugin {
            id: id.to_owned(),
            manifest: None,
            active_version: active.clone(),
            digest: None,
            installed,
            enabled_clients: Vec::new(),
            versions: Vec::new(),
            verification: if installed {
                "plugin_not_found"
            } else {
                "uninstalled"
            }
            .to_owned(),
            warnings: Vec::new(),
        };
        for record in records {
            parse_version(&record.version)?;
            validate_hex(&record.digest, 32).map_err(|_| error("plugin_digest"))?;
            validate_id(&record.signing_key_id)?;
            let is_withdrawn = withdrawn(connection, &record)?;
            let checked = load_verified(connection, &self.store, &record);
            let verification = checked
                .as_ref()
                .map(|_| "verified")
                .unwrap_or_else(|failure| failure.code)
                .to_owned();
            result.versions.push(PluginVersionInfo {
                version: record.version.clone(),
                digest: record.digest.clone(),
                installed_at: record.installed_at,
                signing_key_id: record.signing_key_id.clone(),
                verification: verification.clone(),
                withdrawn: is_withdrawn,
            });
            if installed && active.as_deref() == Some(&record.version) {
                result.digest = Some(record.digest);
                result.verification = verification;
                match checked {
                    Ok(verified) => {
                        let manifest = &verified.published.package.manifest;
                        if let Err(failure) = require_supported_runtime(&manifest.permissions) {
                            result.warnings.push(failure.code.to_owned());
                        }
                        for client in &manifest.clients {
                            if approved_for_client(connection, &verified, client)? {
                                match require_supported_runtime(&manifest.permissions).and_then(
                                    |_| {
                                        require_dependencies(
                                            connection,
                                            &self.store,
                                            manifest,
                                            Some(client),
                                        )
                                    },
                                ) {
                                    Ok(()) => result.enabled_clients.push(client.clone()),
                                    Err(failure) => {
                                        if !result
                                            .warnings
                                            .iter()
                                            .any(|warning| warning == failure.code)
                                        {
                                            result.warnings.push(failure.code.to_owned());
                                        }
                                    }
                                }
                            }
                        }
                        result.manifest = Some(manifest.clone());
                    }
                    Err(failure) => result.warnings.push(failure.code.to_owned()),
                }
            }
        }
        Ok(result)
    }

    pub fn list_trusted_keys(&self) -> PluginResult<Vec<TrustedSigningKey>> {
        let connection = self.database.conn();
        let connection = connection.lock().map_err(|_| error("plugin_database"))?;
        let mut statement = connection.prepare("SELECT id,label,public_key,fingerprint,enabled FROM xy_plugin_trusted_keys ORDER BY id").map_err(database_error)?;
        let keys = statement
            .query_map([], |row| {
                Ok(TrustedSigningKey {
                    id: row.get(0)?,
                    label: row.get(1)?,
                    public_key: row.get(2)?,
                    fingerprint: row.get(3)?,
                    enabled: row.get(4)?,
                })
            })
            .map_err(database_error)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(database_error)?;
        for key in &keys {
            if validate_trust_key(&TrustedKeyInput {
                id: key.id.clone(),
                label: key.label.clone(),
                public_key: key.public_key.clone(),
            })? != key.fingerprint
            {
                return Err(error("plugin_untrusted_key"));
            }
        }
        Ok(keys)
    }

    pub fn configure_trusted_key(
        &self,
        input: TrustedKeyInput,
        fingerprint: &str,
        confirmed: bool,
    ) -> PluginResult<TrustedSigningKey> {
        let expected = validate_trust_key(&input)?;
        if !confirmed || fingerprint != expected {
            return Err(error("plugin_confirmation"));
        }
        let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
        let connection = self.database.conn();
        let mut connection = connection.lock().map_err(|_| error("plugin_database"))?;
        let transaction = connection.transaction().map_err(database_error)?;
        let existing: Option<String> = transaction
            .query_row(
                "SELECT public_key FROM xy_plugin_trusted_keys WHERE id=?1",
                [&input.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(database_error)?;
        if existing
            .as_ref()
            .is_some_and(|key| key != &input.public_key)
        {
            return Err(error("plugin_trust_conflict"));
        }
        let count: i64 = transaction
            .query_row("SELECT COUNT(*) FROM xy_plugin_trusted_keys", [], |row| {
                row.get(0)
            })
            .map_err(database_error)?;
        if existing.is_none() && count >= 64 {
            return Err(error("plugin_limit"));
        }
        transaction.execute("INSERT INTO xy_plugin_trusted_keys(id,label,public_key,fingerprint,enabled,configured_at) VALUES(?1,?2,?3,?4,1,?5)
            ON CONFLICT(id) DO UPDATE SET label=excluded.label,enabled=1,configured_at=excluded.configured_at", params![input.id,input.label,input.public_key,expected,now()]).map_err(database_error)?;
        bump_revision(&transaction)?;
        transaction.commit().map_err(database_error)?;
        state.clear_previews();
        Ok(TrustedSigningKey {
            id: input.id,
            label: input.label,
            public_key: input.public_key,
            fingerprint: expected,
            enabled: true,
        })
    }

    pub fn revoke_trusted_key(&self, id: &str) -> PluginResult<()> {
        validate_id(id)?;
        let mut state = self.state.lock().map_err(|_| error("plugin_database"))?;
        let connection = self.database.conn();
        let mut connection = connection.lock().map_err(|_| error("plugin_database"))?;
        let transaction = connection.transaction().map_err(database_error)?;
        transaction
            .execute(
                "UPDATE xy_plugin_trusted_keys SET enabled=0 WHERE id=?1",
                [id],
            )
            .map_err(database_error)?;
        transaction.execute("UPDATE xy_plugin_grants SET enabled=0 WHERE EXISTS(SELECT 1 FROM xy_plugin_versions AS version WHERE version.plugin_id=xy_plugin_grants.plugin_id AND version.version=xy_plugin_grants.version AND version.signing_key_id=?1)", [id]).map_err(database_error)?;
        bump_revision(&transaction)?;
        transaction.commit().map_err(database_error)?;
        state.clear_previews();
        Ok(())
    }
}
