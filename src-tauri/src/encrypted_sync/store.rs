use super::{
    crypto::MAX_PLAINTEXT,
    model::{Favorite, Payload, SyncProvider},
};
use crate::{
    database::{Database, Provider},
    model_pricing::PricingInput,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use zeroize::Zeroizing;

fn db_error(_: rusqlite::Error) -> String {
    "Encrypted sync database operation failed".into()
}
fn encoded<T: Serialize>(value: &T) -> Result<Zeroizing<Vec<u8>>, String> {
    let data = Zeroizing::new(serde_json::to_vec(value).map_err(|_| "Invalid sync data")?);
    if data.len() > MAX_PLAINTEXT {
        return Err("Sync snapshot exceeds 8 MiB".into());
    }
    Ok(data)
}
pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS session_favorites(provider_id TEXT NOT NULL,session_id TEXT NOT NULL,PRIMARY KEY(provider_id,session_id));
    CREATE TABLE IF NOT EXISTS sync_provider_ids(provider_id TEXT PRIMARY KEY REFERENCES providers(id) ON DELETE CASCADE,sync_id TEXT NOT NULL UNIQUE);
    CREATE TABLE IF NOT EXISTS sync_backups(id TEXT PRIMARY KEY,content BLOB NOT NULL,before_hash TEXT NOT NULL,after_hash TEXT NOT NULL,created_at INTEGER NOT NULL,used INTEGER NOT NULL DEFAULT 0);").map_err(db_error)
}
fn collect<T>(
    conn: &Connection,
    sql: &str,
    f: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, String> {
    conn.prepare(sql)
        .map_err(db_error)?
        .query_map([], f)
        .map_err(db_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(db_error)
}
fn favorites_on(conn: &Connection) -> Result<Vec<Favorite>, String> {
    collect(
        conn,
        "SELECT provider_id,session_id FROM session_favorites ORDER BY provider_id,session_id",
        |r| {
            Ok(Favorite {
                provider_id: r.get(0)?,
                session_id: r.get(1)?,
            })
        },
    )
}
pub fn load_favorites(db: &Database, legacy: Option<&str>) -> Result<Vec<Favorite>, String> {
    let conn = db.conn();
    let mut conn = conn.lock().map_err(|_| "Sync database lock unavailable")?;
    let tx = conn.transaction().map_err(db_error)?;
    let migrated: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM app_settings WHERE key='sync_favorites_migrated_v1')",
            [],
            |r| r.get(0),
        )
        .map_err(db_error)?;
    if !migrated {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Legacy {
            version: u32,
            keys: Vec<String>,
        }
        let mut records = BTreeSet::new();
        if let Some(raw) = legacy {
            if raw.len() > 2 * 1024 * 1024 {
                return Err("Invalid legacy favorites data".into());
            }
            let data: Legacy =
                serde_json::from_str(raw).map_err(|_| "Invalid legacy favorites data")?;
            if data.version != 1 || data.keys.len() > 10000 {
                return Err("Invalid legacy favorites data".into());
            }
            for key in data.keys {
                let pair: (String, String) =
                    serde_json::from_str(&key).map_err(|_| "Invalid legacy favorites data")?;
                let record = Favorite {
                    provider_id: pair.0,
                    session_id: pair.1,
                };
                record
                    .validate()
                    .map_err(|_| "Invalid legacy favorites data")?;
                if record.key() != key {
                    return Err("Invalid legacy favorites data".into());
                }
                records.insert(record);
            }
            tx.execute(
                "INSERT INTO app_settings(key,value) VALUES('sync_favorites_legacy_backup_v1',?1)",
                [raw],
            )
            .map_err(db_error)?;
        }
        for record in records {
            set_favorite_on(&tx, &record, true)?;
        }
        tx.execute(
            "INSERT INTO app_settings(key,value) VALUES('sync_favorites_migrated_v1','1')",
            [],
        )
        .map_err(db_error)?;
    }
    let result = favorites_on(&tx)?;
    tx.commit().map_err(db_error)?;
    Ok(result)
}
fn set_favorite_on(conn: &Connection, favorite: &Favorite, enabled: bool) -> Result<(), String> {
    favorite.validate()?;
    if enabled {
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM session_favorites WHERE NOT(provider_id=?1 AND session_id=?2)", params![favorite.provider_id, favorite.session_id], |r| r.get(0)).map_err(db_error)?;
        if count >= 10000 {
            return Err("Favorite limit reached".into());
        }
        conn.execute(
            "INSERT OR IGNORE INTO session_favorites(provider_id,session_id) VALUES(?1,?2)",
            params![favorite.provider_id, favorite.session_id],
        )
        .map_err(db_error)?;
    } else {
        conn.execute(
            "DELETE FROM session_favorites WHERE provider_id=?1 AND session_id=?2",
            params![favorite.provider_id, favorite.session_id],
        )
        .map_err(db_error)?;
    }
    Ok(())
}
pub fn set_favorite(db: &Database, favorite: &Favorite, enabled: bool) -> Result<(), String> {
    let conn = db.conn();
    let conn = conn.lock().map_err(|_| "Sync database lock unavailable")?;
    set_favorite_on(&conn, favorite, enabled)
}
#[derive(Clone, Serialize, Deserialize)]
struct StoredPrice {
    pricing: PricingInput,
    source: String,
    updated_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
struct LocalState {
    providers: Vec<Provider>,
    prices: Vec<StoredPrice>,
    favorites: Vec<Favorite>,
    identities: BTreeMap<String, String>,
    current_provider: Option<String>,
}
impl LocalState {
    fn payload(&self) -> Payload {
        let mut providers: Vec<_> = self
            .providers
            .iter()
            .map(|p| SyncProvider::from_local(p, &self.identities[&p.id]))
            .collect();
        providers.sort_by(|a, b| a.id.cmp(&b.id));
        Payload {
            schema: 1,
            providers,
            favorites: self.favorites.clone(),
            prices: self
                .prices
                .iter()
                .filter(|p| p.source == "custom")
                .map(|p| p.pricing.clone())
                .collect(),
        }
    }
    fn hash(&self) -> Result<String, String> {
        Ok(crate::governance::digest(&encoded(self)?))
    }
}
fn state_on(conn: &Connection) -> Result<LocalState, String> {
    let providers = collect(
        conn,
        "SELECT id,name,provider_type,endpoint,model,app_group,enabled,sort_order,advanced_json FROM providers WHERE provider_type <> 'local_usage' ORDER BY id",
        crate::database::provider_from_row,
    )?;
    if providers.len() > 1000 {
        return Err("Too many local providers for sync".into());
    }
    let mut identities = BTreeMap::new();
    for provider in &providers {
        let id: Option<String> = conn
            .query_row(
                "SELECT sync_id FROM sync_provider_ids WHERE provider_id=?1",
                [&provider.id],
                |r| r.get(0),
            )
            .optional()
            .map_err(db_error)?;
        let id = match id {
            Some(id) => id,
            None => {
                let id = uuid::Uuid::new_v4().to_string();
                conn.execute(
                    "INSERT INTO sync_provider_ids(provider_id,sync_id) VALUES(?1,?2)",
                    params![provider.id, id],
                )
                .map_err(db_error)?;
                id
            }
        };
        identities.insert(provider.id.clone(), id);
    }
    let prices = collect(
        conn,
        "SELECT id,input_per_million,output_per_million,cache_read_per_million,cache_creation_per_million,source,updated_at FROM model_pricing ORDER BY id",
        |r| {
            Ok(StoredPrice {
                pricing: PricingInput {
                    id: r.get(0)?,
                    input_per_million: r.get(1)?,
                    output_per_million: r.get(2)?,
                    cache_read_per_million: r.get(3)?,
                    cache_creation_per_million: r.get(4)?,
                },
                source: r.get(5)?,
                updated_at: r.get(6)?,
            })
        },
    )?;
    let favorites = favorites_on(conn)?;
    let current_provider = conn
        .query_row(
            "SELECT value FROM app_settings WHERE key='current_provider'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(db_error)?;
    Ok(LocalState {
        providers,
        prices,
        favorites,
        identities,
        current_provider,
    })
}
pub fn export_snapshot(db: &Database) -> Result<(Payload, String), String> {
    let conn = db.conn();
    let mut conn = conn.lock().map_err(|_| "Sync database lock unavailable")?;
    let tx = conn.transaction().map_err(db_error)?;
    let local = state_on(&tx)?;
    let payload = local.payload();
    payload.validate()?;
    let hash = local.hash()?;
    tx.commit().map_err(db_error)?;
    Ok((payload, hash))
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Change {
    pub key: String,
    pub kind: String,
    pub status: String,
    pub effect: String,
    pub local: Option<Value>,
    pub incoming: Value,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Choice {
    pub key: String,
    pub use_incoming: bool,
}
fn changes(local: &LocalState, incoming: &Payload) -> Result<Vec<Change>, String> {
    let local_payload = local.payload();
    local_payload.validate()?;
    let mut changes = Vec::new();
    let providers: BTreeMap<_, _> = local_payload
        .providers
        .iter()
        .map(|p| (p.id.as_str(), p))
        .collect();
    let prices: BTreeMap<_, _> = local
        .prices
        .iter()
        .map(|p| (p.pricing.id.as_str(), p))
        .collect();
    let favorites: BTreeSet<_> = local.favorites.iter().collect();
    for provider in &incoming.providers {
        let current = providers.get(provider.id.as_str()).copied();
        let status = match current {
            None => "add",
            Some(p) if p == provider => "unchanged",
            _ => "conflict",
        };
        let effect = match current {
            None => "new_disabled_provider",
            Some(p) if provider.target_changed(p) => "keep_old_target_create_disabled_copy",
            _ => "update_allowed_fields_only",
        };
        changes.push(Change {
            key: format!("provider:{}", provider.id),
            kind: "provider".into(),
            status: status.into(),
            effect: effect.into(),
            local: current.map(|p| serde_json::to_value(p).unwrap()),
            incoming: serde_json::to_value(provider).unwrap(),
        });
    }
    for price in &incoming.prices {
        let current = prices.get(price.id.as_str()).copied();
        let status = match current {
            None => "add",
            Some(p) if p.source == "custom" && p.pricing == *price => "unchanged",
            _ => "conflict",
        };
        changes.push(Change {
            key: format!("price:{}", price.id),
            kind: "price".into(),
            status: status.into(),
            effect: "custom_price_only_history_unchanged".into(),
            local: current.map(|p| serde_json::to_value(p).unwrap()),
            incoming: serde_json::to_value(price).unwrap(),
        });
    }
    for favorite in &incoming.favorites {
        let present = favorites.contains(favorite);
        changes.push(Change {
            key: format!("favorite:{}", favorite.key()),
            kind: "favorite".into(),
            status: if present { "unchanged" } else { "add" }.into(),
            effect: "identifier_only_no_session_content".into(),
            local: if present {
                Some(serde_json::to_value(favorite).unwrap())
            } else {
                None
            },
            incoming: serde_json::to_value(favorite).unwrap(),
        });
    }
    Ok(changes)
}
pub fn import_preview(db: &Database, incoming: &Payload) -> Result<(Vec<Change>, String), String> {
    incoming.validate()?;
    let conn = db.conn();
    let mut conn = conn.lock().map_err(|_| "Sync database lock unavailable")?;
    let tx = conn.transaction().map_err(db_error)?;
    let local = state_on(&tx)?;
    let changes = changes(&local, incoming)?;
    let hash = local.hash()?;
    tx.commit().map_err(db_error)?;
    Ok((changes, hash))
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub backup_id: String,
    pub applied: usize,
    pub disabled_copies: usize,
}
pub fn apply(
    db: &Database,
    incoming: &Payload,
    expected: &str,
    choices: &[Choice],
) -> Result<Applied, String> {
    incoming.validate()?;
    let conn = db.conn();
    let mut conn = conn.lock().map_err(|_| "Sync database lock unavailable")?;
    let tx = conn.transaction().map_err(db_error)?;
    let local = state_on(&tx)?;
    if local.hash()? != expected {
        return Err("Local data changed after preview; preview again".into());
    }
    let rows = changes(&local, incoming)?;
    let expected_keys: BTreeSet<_> = rows
        .iter()
        .filter(|r| r.status != "unchanged")
        .map(|r| r.key.clone())
        .collect();
    let selected: BTreeMap<_, _> = choices
        .iter()
        .map(|c| (c.key.clone(), c.use_incoming))
        .collect();
    if selected.len() != choices.len()
        || selected.keys().cloned().collect::<BTreeSet<_>>() != expected_keys
    {
        return Err("Choose keep-local or incoming for every changed item".into());
    }
    let selected = |key: &str| selected.get(key).copied().unwrap_or(false);
    let before = encoded(&local)?;
    let mut applied = 0;
    let mut disabled_copies = 0;
    for incoming in &incoming.providers {
        if !selected(&format!("provider:{}", incoming.id)) {
            continue;
        }
        let current = local
            .identities
            .iter()
            .find(|(_, id)| **id == incoming.id)
            .and_then(|(id, _)| local.providers.iter().find(|p| p.id == *id));
        let target_changed = current
            .is_some_and(|p| incoming.target_changed(&SyncProvider::from_local(p, &incoming.id)));
        let provider = if current.is_none() || target_changed {
            disabled_copies += 1;
            let provider = incoming.new_disabled();
            if let Some(old) = current {
                tx.execute(
                    "DELETE FROM sync_provider_ids WHERE provider_id=?1",
                    [&old.id],
                )
                .map_err(db_error)?;
            }
            crate::database::save_provider_on(&tx, &provider)?;
            tx.execute(
                "INSERT INTO sync_provider_ids(provider_id,sync_id) VALUES(?1,?2)",
                params![provider.id, incoming.id],
            )
            .map_err(db_error)?;
            provider
        } else {
            let mut provider = current.unwrap().clone();
            incoming.apply_fields(&mut provider);
            crate::database::save_provider_on(&tx, &provider)?;
            provider
        };
        let _ = provider;
        applied += 1;
    }
    for price in &incoming.prices {
        if selected(&format!("price:{}", price.id)) {
            crate::model_pricing::save_on(&tx, price)?;
            applied += 1;
        }
    }
    for favorite in &incoming.favorites {
        if selected(&format!("favorite:{}", favorite.key())) {
            set_favorite_on(&tx, favorite, true)?;
            applied += 1;
        }
    }
    if applied == 0 {
        return Err("No incoming changes selected".into());
    }
    let after = state_on(&tx)?;
    let after_hash = after.hash()?;
    let backup_id = uuid::Uuid::new_v4().to_string();
    tx.execute("INSERT INTO sync_backups(id,content,before_hash,after_hash,created_at) VALUES(?1,?2,?3,?4,?5)", params![backup_id, before.as_slice(), expected, after_hash, crate::governance::now()]).map_err(db_error)?;
    tx.execute("DELETE FROM sync_backups WHERE id NOT IN(SELECT id FROM sync_backups ORDER BY created_at DESC,rowid DESC LIMIT 10)", []).map_err(db_error)?;
    crate::governance::event_on(
        &tx,
        "e2ee_import",
        &format!("items={applied}; disabled_copies={disabled_copies}; backup={backup_id}"),
    )?;
    tx.commit().map_err(db_error)?;
    Ok(Applied {
        backup_id,
        applied,
        disabled_copies,
    })
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Backup {
    pub id: String,
    pub created_at: i64,
    pub used: bool,
}
pub fn backups(db: &Database) -> Result<Vec<Backup>, String> {
    let conn = db.conn();
    let conn = conn.lock().map_err(|_| "Sync database lock unavailable")?;
    collect(
        &conn,
        "SELECT id,created_at,used FROM sync_backups ORDER BY created_at DESC,rowid DESC LIMIT 10",
        |r| {
            Ok(Backup {
                id: r.get(0)?,
                created_at: r.get(1)?,
                used: r.get(2)?,
            })
        },
    )
}
fn read_backup(conn: &Connection, id: &str, current: &LocalState) -> Result<LocalState, String> {
    let data: Option<(Vec<u8>, String, String, bool)> = conn
        .query_row(
            "SELECT content,before_hash,after_hash,used FROM sync_backups WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()
        .map_err(db_error)?;
    let (data, before_hash, after_hash, used) = data.ok_or("Sync backup no longer exists")?;
    let data = Zeroizing::new(data);
    if used || current.hash()? != after_hash {
        return Err("Local data changed since import; refusing to overwrite newer data".into());
    }
    if data.len() > MAX_PLAINTEXT || crate::governance::digest(&data) != before_hash {
        return Err("Invalid sync backup".into());
    }
    serde_json::from_slice(&data).map_err(|_| "Invalid sync backup".into())
}
pub fn rollback_preview(db: &Database, id: &str) -> Result<(Value, Value, String), String> {
    let conn = db.conn();
    let mut conn = conn.lock().map_err(|_| "Sync database lock unavailable")?;
    let tx = conn.transaction().map_err(db_error)?;
    let current = state_on(&tx)?;
    let target = read_backup(&tx, id, &current)?;
    let retained: Vec<_> = current
        .providers
        .iter()
        .filter(|p| !target.providers.iter().any(|t| t.id == p.id))
        .map(|p| SyncProvider::from_local(p, &current.identities[&p.id]))
        .collect();
    let before = serde_json::json!({"configuration":current.payload()});
    let after =
        serde_json::json!({"configuration":target.payload(),"retainedDisabledProviders":retained});
    let hash = current.hash()?;
    tx.commit().map_err(db_error)?;
    Ok((before, after, hash))
}
pub fn rollback(db: &Database, id: &str, expected: &str) -> Result<(), String> {
    let conn = db.conn();
    let mut conn = conn.lock().map_err(|_| "Sync database lock unavailable")?;
    let tx = conn.transaction().map_err(db_error)?;
    let current = state_on(&tx)?;
    if current.hash()? != expected {
        return Err("Local data changed after preview; preview again".into());
    }
    let target = read_backup(&tx, id, &current)?;
    for provider in &target.providers {
        crate::database::save_provider_on(&tx, provider)?;
    }
    tx.execute("DELETE FROM sync_provider_ids", [])
        .map_err(db_error)?;
    for (provider, id) in &target.identities {
        tx.execute(
            "INSERT INTO sync_provider_ids(provider_id,sync_id) VALUES(?1,?2)",
            params![provider, id],
        )
        .map_err(db_error)?;
    }
    for provider in &current.providers {
        if !target.providers.iter().any(|p| p.id == provider.id) {
            let mut provider = provider.clone();
            provider.enabled = false;
            crate::database::save_provider_on(&tx, &provider)?;
        }
    }
    tx.execute("DELETE FROM model_pricing", [])
        .map_err(db_error)?;
    for price in &target.prices {
        tx.execute("INSERT INTO model_pricing(id,input_per_million,output_per_million,cache_read_per_million,cache_creation_per_million,source,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)", params![price.pricing.id,price.pricing.input_per_million,price.pricing.output_per_million,price.pricing.cache_read_per_million,price.pricing.cache_creation_per_million,price.source,price.updated_at]).map_err(db_error)?;
    }
    tx.execute("DELETE FROM session_favorites", [])
        .map_err(db_error)?;
    for favorite in &target.favorites {
        set_favorite_on(&tx, favorite, true)?;
    }
    tx.execute("UPDATE sync_backups SET used=1 WHERE id=?1", [id])
        .map_err(db_error)?;
    crate::governance::event_on(&tx, "e2ee_rollback", id)?;
    let _ = state_on(&tx)?;
    tx.commit().map_err(db_error)?;
    Ok(())
}
