use super::*;
use crate::database::{Prompt, Provider};
use crate::model_pricing::PricingInput;
use rusqlite::OptionalExtension;
use std::collections::BTreeMap;
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Snapshot {
    schema: u32,
    providers: Vec<Provider>,
    prompts: Vec<Prompt>,
    prices: Vec<PricingInput>,
    settings: BTreeMap<String, String>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionInfo {
    pub id: String,
    pub label: String,
    pub hash: String,
    pub created_at: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VersionPreview {
    pub version_id: String,
    pub expected_current_hash: String,
    pub target_hash: String,
    pub before: String,
    pub after: String,
}
fn collect<T>(
    s: &mut rusqlite::Statement<'_>,
    f: impl FnMut(&rusqlite::Row<'_>) -> rusqlite::Result<T>,
) -> Result<Vec<T>, String> {
    s.query_map([], f)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())
}
fn snapshot_on(c: &Connection) -> Result<Snapshot, String> {
    let providers=collect(&mut c.prepare("SELECT id,name,provider_type,endpoint,model,app_group,enabled,sort_order,advanced_json FROM providers WHERE provider_type <> 'local_usage' ORDER BY id").map_err(|e|e.to_string())?,crate::database::provider_from_row)?;
    let prompts = collect(
        &mut c
            .prepare("SELECT id,name,content,description,enabled FROM prompts ORDER BY id")
            .map_err(|e| e.to_string())?,
        |r| {
            Ok(Prompt {
                id: r.get(0)?,
                name: r.get(1)?,
                content: r.get(2)?,
                description: r.get(3)?,
                enabled: r.get(4)?,
            })
        },
    )?;
    let prices=collect(&mut c.prepare("SELECT id,input_per_million,output_per_million,cache_read_per_million,cache_creation_per_million FROM model_pricing WHERE source='custom' ORDER BY id").map_err(|e|e.to_string())?,|r|Ok(PricingInput{id:r.get(0)?,input_per_million:r.get(1)?,output_per_million:r.get(2)?,cache_read_per_million:r.get(3)?,cache_creation_per_million:r.get(4)?}))?;
    let mut settings = BTreeMap::new();
    for key in ["current_provider", "proxy_settings"] {
        let v: Option<String> = c
            .query_row("SELECT value FROM app_settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(v) = v {
            settings.insert(key.into(), v);
        }
    }
    Ok(Snapshot {
        schema: 1,
        providers,
        prompts,
        prices,
        settings,
    })
}
fn encoded(s: &Snapshot) -> Result<String, String> {
    let v = serde_json::to_string_pretty(s).map_err(|e| e.to_string())?;
    if v.len() > 8 * 1024 * 1024 {
        return Err("Version exceeds 8 MiB".into());
    }
    Ok(v)
}
fn record(c: &Connection, label: &str, content: &str) -> Result<VersionInfo, String> {
    if label.trim().is_empty() || label.len() > 160 || label.chars().any(char::is_control) {
        return Err("Invalid version label".into());
    }
    let count: i64 = c
        .query_row("SELECT COUNT(*) FROM config_versions", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if count >= 200 {
        return Err(
            "Version storage is full (200); delete an old version before continuing".into(),
        );
    }
    let v = VersionInfo {
        id: uuid::Uuid::new_v4().to_string(),
        label: label.trim().into(),
        hash: digest(content.as_bytes()),
        created_at: now(),
    };
    c.execute(
        "INSERT INTO config_versions(id,label,content,hash,created_at) VALUES(?1,?2,?3,?4,?5)",
        params![v.id, v.label, content, v.hash, v.created_at],
    )
    .map_err(|e| e.to_string())?;
    Ok(v)
}
pub fn capture(db: &Database, label: &str) -> Result<VersionInfo, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    record(&c, label, &encoded(&snapshot_on(&c)?)?)
}
fn target(c: &Connection, id: &str, current: &Snapshot) -> Result<Snapshot, String> {
    let (text, hash): (String, String) = c
        .query_row(
            "SELECT content,hash FROM config_versions WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|_| "Version not found")?;
    if digest(text.as_bytes()) != hash {
        return Err("Version integrity check failed".into());
    }
    let mut s: Snapshot = serde_json::from_str(&text).map_err(|_| "Invalid version")?;
    if s.schema != 1 {
        return Err("Unsupported version schema".into());
    }
    crate::database::validate_config_records(&s.providers, &s.prompts)?;
    for price in &s.prices {
        price.validate()?;
    }
    // Never delete providers: their request history and external credentials must survive rollback.
    for provider in &current.providers {
        if !s.providers.iter().any(|p| p.id == provider.id) {
            let mut p = provider.clone();
            p.enabled = false;
            s.providers.push(p);
        }
    }
    s.providers.sort_by(|a, b| a.id.cmp(&b.id));
    if let Some(raw) = s.settings.get_mut("proxy_settings") {
        let mut proxy: crate::proxy_manager::ProxySettings =
            serde_json::from_str(raw).map_err(|_| "Invalid version routing settings")?;
        proxy.enabled = false;
        proxy.validate()?;
        *raw = serde_json::to_string(&proxy).map_err(|e| e.to_string())?;
    }
    Ok(s)
}
pub fn preview(db: &Database, id: &str) -> Result<VersionPreview, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    let current = snapshot_on(&c)?;
    let before = encoded(&current)?;
    let after = encoded(&target(&c, id, &current)?)?;
    Ok(VersionPreview {
        version_id: id.into(),
        expected_current_hash: digest(before.as_bytes()),
        target_hash: digest(after.as_bytes()),
        before,
        after,
    })
}
pub fn restore(
    db: &Database,
    id: &str,
    expected: &str,
    expected_target: &str,
) -> Result<VersionInfo, String> {
    let c = db.conn();
    let mut c = c.lock().map_err(|_| "database lock")?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    let current = snapshot_on(&tx)?;
    let before = encoded(&current)?;
    if digest(before.as_bytes()) != expected {
        return Err("Configuration changed after preview; preview again".into());
    }
    let target = target(&tx, id, &current)?;
    if digest(encoded(&target)?.as_bytes()) != expected_target {
        return Err("Rollback target changed; preview again".into());
    }
    let backup = record(&tx, "Automatic backup before rollback", &before)?;
    for p in &target.providers {
        crate::database::save_provider_on(&tx, p)?;
    }
    tx.execute("DELETE FROM prompts", [])
        .map_err(|e| e.to_string())?;
    for p in &target.prompts {
        crate::database::save_prompt_on(&tx, p)?;
    }
    tx.execute("DELETE FROM model_pricing WHERE source='custom'", [])
        .map_err(|e| e.to_string())?;
    for price in &target.prices {
        crate::model_pricing::save_on(&tx, price)?;
    }
    for key in ["current_provider", "proxy_settings"] {
        tx.execute("DELETE FROM app_settings WHERE key=?1", [key])
            .map_err(|e| e.to_string())?;
        if let Some(value) = target.settings.get(key) {
            tx.execute(
                "INSERT INTO app_settings(key,value) VALUES(?1,?2)",
                params![key, value],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    event_on(&tx, "config_rollback", id)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(backup)
}
#[tauri::command]
pub fn create_config_version(
    label: String,
    db: tauri::State<'_, Database>,
) -> Result<VersionInfo, String> {
    crate::platform::require_desktop("Governance changes")?;
    capture(&db, &label)
}
#[tauri::command]
pub fn list_config_versions(db: tauri::State<'_, Database>) -> Result<Vec<VersionInfo>, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    collect(&mut c.prepare("SELECT id,label,hash,created_at FROM config_versions ORDER BY created_at DESC,rowid DESC LIMIT 200").map_err(|e|e.to_string())?,|r|Ok(VersionInfo{id:r.get(0)?,label:r.get(1)?,hash:r.get(2)?,created_at:r.get(3)?}))
}
#[tauri::command]
pub fn preview_config_rollback(
    id: String,
    db: tauri::State<'_, Database>,
) -> Result<VersionPreview, String> {
    preview(&db, &id)
}
#[tauri::command]
pub async fn apply_config_rollback(
    id: String,
    expected_current_hash: String,
    target_hash: String,
    confirmation: String,
    db: tauri::State<'_, Database>,
    proxy: tauri::State<'_, crate::proxy_manager::ProxyManager>,
    takeover: tauri::State<'_, crate::takeover::AppTakeoverManager>,
) -> Result<VersionInfo, String> {
    crate::platform::require_desktop("Governance changes")?;
    if confirmation != "ROLLBACK" {
        return Err("Type ROLLBACK to confirm".into());
    }
    let _proxy_guard = proxy.configuration_guard().await;
    if proxy.status().await?.running {
        return Err("Stop proxy and restore takeovers before rollback".into());
    }
    let _takeover_guard = takeover.inactive_guard()?;
    restore(&db, &id, &expected_current_hash, &target_hash)
}

#[tauri::command]
pub fn delete_config_version(id: String, confirmation: String, db: tauri::State<'_, Database>) -> Result<(), String> {
    crate::platform::require_desktop("Governance changes")?;
    if confirmation != "DELETE VERSION" { return Err("Type DELETE VERSION to confirm permanent deletion".into()); }
    let c = db.conn();
    let mut c = c.lock().map_err(|_| "database lock")?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    if tx.execute("DELETE FROM config_versions WHERE id=?1", [&id]).map_err(|e| e.to_string())? != 1 {
        return Err("Version no longer exists".into());
    }
    event_on(&tx, "config_version_deleted", &id)?;
    tx.commit().map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "version_tests.rs"]
mod tests;
