pub mod access;
pub mod budget;
pub mod monitor;
pub mod mounts;
pub mod versions;

use crate::database::Database;
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn init(conn: &Connection) -> Result<(), String> {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS access_tokens(id TEXT PRIMARY KEY,label TEXT NOT NULL,digest TEXT NOT NULL UNIQUE,scope TEXT NOT NULL,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL,revoked INTEGER NOT NULL DEFAULT 0);
    CREATE TABLE IF NOT EXISTS budget_ledger(id TEXT PRIMARY KEY,day INTEGER NOT NULL,provider_id TEXT NOT NULL,model TEXT NOT NULL,tokens INTEGER NOT NULL,cost_microusd INTEGER NOT NULL,source TEXT NOT NULL,created_at INTEGER NOT NULL);
    CREATE INDEX IF NOT EXISTS budget_day ON budget_ledger(day);
    CREATE TABLE IF NOT EXISTS governance_events(id INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,detail TEXT NOT NULL,created_at INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS config_versions(id TEXT PRIMARY KEY,label TEXT NOT NULL,content TEXT NOT NULL,hash TEXT NOT NULL,created_at INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS synthetic_runs(id TEXT PRIMARY KEY,provider_id TEXT NOT NULL,model TEXT NOT NULL,mode TEXT NOT NULL,status TEXT NOT NULL,latency_ms INTEGER NOT NULL,detail TEXT NOT NULL,created_at INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS mount_backups(id TEXT PRIMARY KEY,target TEXT NOT NULL,before_content TEXT,after_hash TEXT NOT NULL,created_at INTEGER NOT NULL);")
    .map_err(|e|e.to_string())?;
    let mut columns = conn
        .prepare("PRAGMA table_info(synthetic_runs)")
        .map_err(|e| e.to_string())?;
    let names = columns
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    if !names.iter().any(|s| s == "evidence_json") {
        conn.execute(
            "ALTER TABLE synthetic_runs ADD COLUMN evidence_json TEXT NOT NULL DEFAULT '{}'",
            [],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
}
pub fn load<T: serde::de::DeserializeOwned + Default>(
    db: &Database,
    key: &str,
) -> Result<T, String> {
    db.get_setting(key)?
        .map(|v| serde_json::from_str(&v).map_err(|_| format!("Invalid {key}; operation blocked")))
        .unwrap_or_else(|| Ok(T::default()))
}
pub fn save<T: Serialize>(db: &Database, key: &str, value: &T) -> Result<(), String> {
    db.set_setting(
        key,
        &serde_json::to_string(value).map_err(|e| e.to_string())?,
    )
}
pub fn event(db: &Database, kind: &str, detail: &str) -> Result<(), String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    event_on(&c, kind, detail)
}
pub fn event_on(c: &Connection, kind: &str, detail: &str) -> Result<(), String> {
    c.execute(
        "INSERT INTO governance_events(kind,detail,created_at) VALUES(?1,?2,?3)",
        params![kind, detail, now()],
    ).map_err(|e| e.to_string())?;
    c.execute("DELETE FROM governance_events WHERE id NOT IN (SELECT id FROM governance_events ORDER BY id DESC LIMIT 2000)", [])
        .map_err(|e| e.to_string())?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    kind: String,
    detail: String,
    created_at: i64,
}
#[tauri::command]
pub fn governance_events(db: tauri::State<'_, Database>) -> Result<Vec<Event>, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    let mut s = c
        .prepare("SELECT kind,detail,created_at FROM governance_events ORDER BY id DESC LIMIT 100")
        .map_err(|e| e.to_string())?;
    s.query_map([], |r| {
        Ok(Event {
            kind: r.get(0)?,
            detail: r.get(1)?,
            created_at: r.get(2)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}

#[cfg(test)]
pub(crate) mod test_support;
