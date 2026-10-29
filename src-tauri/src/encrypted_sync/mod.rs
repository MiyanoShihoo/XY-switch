mod crypto;
mod files;
mod model;
pub mod store;

use crate::database::Database;
use model::{Favorite, Payload};
use serde::Serialize;
use serde_json::Value;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::State;
#[cfg(desktop)]
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

const TICKET_TTL: Duration = Duration::from_secs(300);
const MAX_TICKETS: usize = 4;
#[derive(Clone, Copy, PartialEq, Eq)]
enum Action {
    Export,
    Import,
    Rollback,
}
struct Ticket {
    action: Action,
    hash: String,
    data: Zeroizing<Vec<u8>>,
    deadline: Instant,
}
pub struct SyncManager {
    pending: Mutex<HashMap<String, Ticket>>,
    crypto_gate: Arc<tokio::sync::Semaphore>,
}
impl Default for SyncManager {
    fn default() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
            crypto_gate: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview<T> {
    ticket_id: String,
    expires_at: i64,
    content: T,
}
impl SyncManager {
    fn issue<T: Serialize>(
        &self,
        action: Action,
        hash: String,
        data: Zeroizing<Vec<u8>>,
        content: T,
    ) -> Result<Preview<T>, String> {
        if data.len() > crypto::MAX_PLAINTEXT {
            return Err("Sync preview exceeds the size limit".into());
        }
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Sync preview lock unavailable")?;
        pending.retain(|_, ticket| ticket.deadline > Instant::now());
        if pending.len() >= MAX_TICKETS {
            return Err("Close an existing sync preview before opening another".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        pending.insert(
            id.clone(),
            Ticket {
                action,
                hash,
                data,
                deadline: Instant::now() + TICKET_TTL,
            },
        );
        Ok(Preview {
            ticket_id: id,
            expires_at: crate::governance::now() + 300,
            content,
        })
    }
    fn take(&self, id: &str, action: Action) -> Result<Ticket, String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Sync preview lock unavailable")?;
        pending.retain(|_, ticket| ticket.deadline > Instant::now());
        let ticket = pending
            .remove(id)
            .ok_or("Sync preview expired or was already used; preview again")?;
        if ticket.action != action {
            return Err("Wrong sync preview action".into());
        }
        Ok(ticket)
    }
    fn cancel(&self, id: &str) -> Result<(), String> {
        let mut pending = self
            .pending
            .lock()
            .map_err(|_| "Sync preview lock unavailable")?;
        pending.remove(id);
        pending.retain(|_, ticket| ticket.deadline > Instant::now());
        Ok(())
    }
}
fn encode<T: Serialize>(value: &T) -> Result<Zeroizing<Vec<u8>>, String> {
    let data = Zeroizing::new(serde_json::to_vec(value).map_err(|_| "Invalid sync payload")?);
    if data.len() > crypto::MAX_PLAINTEXT {
        return Err("Sync payload exceeds the size limit".into());
    }
    Ok(data)
}
fn parse(data: &[u8]) -> Result<Payload, String> {
    let payload: Payload =
        serde_json::from_slice(data).map_err(|_| "Invalid or unsupported sync payload")?;
    payload.validate()?;
    Ok(payload)
}
fn confirm(actual: &str, expected: &str) -> Result<(), String> {
    if actual != expected {
        return Err(format!("Type {expected} to confirm"));
    }
    Ok(())
}
fn favorites_ready(db: &Database) -> Result<(), String> {
    if db.get_setting("sync_favorites_migrated_v1")?.as_deref() != Some("1") {
        return Err("Load and migrate local favorites before using encrypted sync".into());
    }
    Ok(())
}
async fn pick(app: tauri::AppHandle, save: bool) -> Result<Option<PathBuf>, String> {
    #[cfg(desktop)]
    {
        tauri::async_runtime::spawn_blocking(move || {
            let dialog = app
                .dialog()
                .file()
                .add_filter("XY Switch encrypted configuration", &["xysync"]);
            let file = if save {
                dialog
                    .set_file_name("xy-switch-config.xysync")
                    .blocking_save_file()
            } else {
                dialog.blocking_pick_file()
            };
            file.map(|file| {
                file.into_path()
                    .map_err(|_| "Only local sync files are supported".into())
            })
            .transpose()
        })
        .await
        .map_err(|_| "Sync file picker failed")?
    }
    #[cfg(not(desktop))]
    {
        let _ = (app, save);
        Err("Encrypted file exchange requires a desktop application".into())
    }
}
#[tauri::command]
pub fn load_session_favorites(
    legacy: Option<String>,
    legacy_available: bool,
    db: State<'_, Database>,
) -> Result<Vec<Favorite>, String> {
    crate::platform::require_desktop("Native session favorites")?;
    if !legacy_available {
        favorites_ready(&db)?;
    }
    store::load_favorites(&db, legacy.as_deref())
}
#[tauri::command]
pub fn set_session_favorite(
    favorite: Favorite,
    enabled: bool,
    db: State<'_, Database>,
) -> Result<(), String> {
    crate::platform::require_desktop("Native session favorites")?;
    favorites_ready(&db)?;
    store::set_favorite(&db, &favorite, enabled)
}
#[tauri::command]
pub fn preview_sync_export(
    db: State<'_, Database>,
    manager: State<'_, SyncManager>,
) -> Result<Preview<Payload>, String> {
    crate::platform::require_desktop("Encrypted sync")?;
    favorites_ready(&db)?;
    let (payload, hash) = store::export_snapshot(&db)?;
    manager.issue(Action::Export, hash, encode(&payload)?, payload)
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportReceipt {
    saved: bool,
    audit_recorded: bool,
}
#[tauri::command]
pub async fn save_sync_export(
    ticket_id: String,
    passphrase: String,
    confirmation: String,
    app: tauri::AppHandle,
    db: State<'_, Database>,
    manager: State<'_, SyncManager>,
) -> Result<ExportReceipt, String> {
    let passphrase = Zeroizing::new(passphrase);
    crate::platform::require_desktop("Encrypted sync")?;
    confirm(&confirmation, "EXPORT")?;
    crypto::validate_passphrase(&passphrase)?;
    let permit = manager
        .crypto_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| "Another encryption operation is in progress")?;
    let ticket = manager.take(&ticket_id, Action::Export)?;
    let Some(path) = pick(app, true).await? else {
        return Ok(ExportReceipt {
            saved: false,
            audit_recorded: false,
        });
    };
    if Instant::now() >= ticket.deadline || store::export_snapshot(&db)?.1 != ticket.hash {
        return Err("Local data changed or preview expired; preview again".into());
    }
    crate::governance::event(&db, "e2ee_export_started", "encrypted_file")?;
    tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        let data = crypto::encrypt(&ticket.data, &passphrase)?;
        files::write_new(&path, &data)
    })
    .await
    .map_err(|_| "Encryption task failed")??;
    Ok(ExportReceipt {
        saved: true,
        audit_recorded: crate::governance::event(&db, "e2ee_export", "encrypted_file").is_ok(),
    })
}
#[tauri::command]
pub async fn preview_sync_import(
    passphrase: String,
    app: tauri::AppHandle,
    db: State<'_, Database>,
    manager: State<'_, SyncManager>,
) -> Result<Option<Preview<Vec<store::Change>>>, String> {
    let passphrase = Zeroizing::new(passphrase);
    crate::platform::require_desktop("Encrypted sync")?;
    favorites_ready(&db)?;
    crypto::validate_passphrase(&passphrase)?;
    let permit = manager
        .crypto_gate
        .clone()
        .try_acquire_owned()
        .map_err(|_| "Another encryption operation is in progress")?;
    let Some(path) = pick(app, false).await? else {
        return Ok(None);
    };
    let payload = tauri::async_runtime::spawn_blocking(move || {
        let _permit = permit;
        parse(&crypto::decrypt(&files::read(&path)?, &passphrase)?)
    })
    .await
    .map_err(|_| "Decryption task failed")??;
    let (changes, hash) = store::import_preview(&db, &payload)?;
    manager
        .issue(Action::Import, hash, encode(&payload)?, changes)
        .map(Some)
}
#[tauri::command]
pub async fn apply_sync_import(
    ticket_id: String,
    choices: Vec<store::Choice>,
    confirmation: String,
    db: State<'_, Database>,
    manager: State<'_, SyncManager>,
    proxy: State<'_, crate::proxy_manager::ProxyManager>,
    takeover: State<'_, crate::takeover::AppTakeoverManager>,
) -> Result<store::Applied, String> {
    crate::platform::require_desktop("Encrypted sync")?;
    confirm(&confirmation, "IMPORT")?;
    let _proxy_guard = proxy.configuration_guard().await;
    if proxy.status().await?.running {
        return Err("Stop proxy and restore takeovers before importing".into());
    }
    let _takeover_guard = takeover.inactive_guard()?;
    let ticket = manager.take(&ticket_id, Action::Import)?;
    store::apply(&db, &parse(&ticket.data)?, &ticket.hash, &choices)
}
#[tauri::command]
pub fn cancel_sync_preview(
    ticket_id: String,
    manager: State<'_, SyncManager>,
) -> Result<(), String> {
    manager.cancel(&ticket_id)
}
#[tauri::command]
pub fn list_sync_backups(db: State<'_, Database>) -> Result<Vec<store::Backup>, String> {
    store::backups(&db)
}
#[tauri::command]
pub fn preview_sync_rollback(
    id: String,
    db: State<'_, Database>,
    manager: State<'_, SyncManager>,
) -> Result<Preview<Value>, String> {
    crate::platform::require_desktop("Encrypted sync")?;
    favorites_ready(&db)?;
    let (before, after, hash) = store::rollback_preview(&db, &id)?;
    manager.issue(
        Action::Rollback,
        hash,
        Zeroizing::new(id.into_bytes()),
        serde_json::json!({"before":before,"after":after}),
    )
}
#[tauri::command]
pub async fn apply_sync_rollback(
    ticket_id: String,
    confirmation: String,
    db: State<'_, Database>,
    manager: State<'_, SyncManager>,
    proxy: State<'_, crate::proxy_manager::ProxyManager>,
    takeover: State<'_, crate::takeover::AppTakeoverManager>,
) -> Result<(), String> {
    crate::platform::require_desktop("Encrypted sync")?;
    confirm(&confirmation, "ROLLBACK SYNC")?;
    let _proxy_guard = proxy.configuration_guard().await;
    if proxy.status().await?.running {
        return Err("Stop proxy and restore takeovers before rollback".into());
    }
    let _takeover_guard = takeover.inactive_guard()?;
    let ticket = manager.take(&ticket_id, Action::Rollback)?;
    let id = std::str::from_utf8(&ticket.data).map_err(|_| "Invalid backup identity")?;
    store::rollback(&db, id, &ticket.hash)
}
#[cfg(test)]
mod tests;
