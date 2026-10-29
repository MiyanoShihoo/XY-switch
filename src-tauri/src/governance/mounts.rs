use super::*;
use crate::takeover::atomic_write;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};
#[cfg(desktop)]
use tauri_plugin_dialog::DialogExt;
const BEGIN: &str = "<!-- XY Switch managed begin -->";
const END: &str = "<!-- XY Switch managed end -->";
const MAX_BYTES: usize = 512 * 1024;
#[derive(Default)]
pub struct MountManager {
    roots: Mutex<HashMap<String, PathBuf>>,
    pending: Mutex<HashMap<String, Pending>>,
}
struct Pending {
    root: PathBuf,
    target: PathBuf,
    before: Option<String>,
    after: Option<String>,
    expires: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RootGrant {
    pub id: String,
    pub path: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MountPreview {
    pub id: String,
    pub target: String,
    pub before: String,
    pub after: String,
    pub creates_file: bool,
    pub removes_file: bool,
    pub expires_at: i64,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupInfo {
    pub id: String,
    pub target: String,
    pub created_at: i64,
}
fn checked_root(root: &Path) -> Result<PathBuf, String> {
    let meta =
        fs::symlink_metadata(root).map_err(|_| "Selected project directory is unavailable")?;
    if !meta.is_dir() || crate::skills_local::is_link(&meta) {
        return Err("Project root must be a real directory, not a link/reparse point".into());
    }
    fs::canonicalize(root).map_err(|_| "Cannot resolve project root".into())
}
fn read_target(path: &Path) -> Result<Option<String>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.is_file()
                || crate::skills_local::is_link(&meta)
                || meta.len() > MAX_BYTES as u64
            {
                return Err("Target must be a regular UTF-8 file no larger than 512 KiB".into());
            }
            fs::read_to_string(path)
                .map(Some)
                .map_err(|_| "Cannot read target as UTF-8".into())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("Cannot inspect mount target".into()),
    }
}
fn merge(before: &str, content: &str) -> Result<String, String> {
    if content.contains(BEGIN) || content.contains(END) {
        return Err("Source contains reserved managed-block markers".into());
    }
    let block = format!("{BEGIN}\n{content}\n{END}");
    let b = before.matches(BEGIN).count();
    let e = before.matches(END).count();
    let result = if b == 0 && e == 0 {
        format!(
            "{before}{}{block}\n",
            if before.is_empty() || before.ends_with('\n') {
                ""
            } else {
                "\n"
            }
        )
    } else if b == 1 && e == 1 {
        let start = before.find(BEGIN).unwrap();
        let end = before.find(END).unwrap() + END.len();
        if before.find(END).unwrap() < start {
            return Err("Malformed managed block".into());
        }
        format!("{}{block}{}", &before[..start], &before[end..])
    } else {
        return Err("Ambiguous managed blocks; resolve manually first".into());
    };
    if result.len() > MAX_BYTES {
        return Err("Mounted document exceeds 512 KiB".into());
    }
    Ok(result)
}
impl MountManager {
    fn grant(&self, path: PathBuf) -> Result<RootGrant, String> {
        let path = checked_root(&path)?;
        let id = uuid::Uuid::new_v4().to_string();
        let grant = RootGrant {
            id: id.clone(),
            path: path.display().to_string(),
        };
        let mut roots = self.roots.lock().map_err(|_| "mount state lock")?;
        if roots.len() >= 20 {
            roots.clear()
        }
        roots.insert(id, path);
        Ok(grant)
    }
    fn root(&self, id: &str) -> Result<PathBuf, String> {
        let root = self
            .roots
            .lock()
            .map_err(|_| "mount state lock")?
            .get(id)
            .cloned()
            .ok_or("Choose a project directory first")?;
        if checked_root(&root)? != root {
            return Err("Project root changed; choose it again".into());
        }
        Ok(root)
    }
    fn remember(
        &self,
        root: PathBuf,
        target: PathBuf,
        before: Option<String>,
        after: Option<String>,
    ) -> Result<MountPreview, String> {
        if before == after {
            return Err("No content changes".into());
        }
        let id = uuid::Uuid::new_v4().to_string();
        let expires = now() + 300;
        let preview = MountPreview {
            id: id.clone(),
            target: target.display().to_string(),
            before: before.clone().unwrap_or_default(),
            after: after.clone().unwrap_or_default(),
            creates_file: before.is_none(),
            removes_file: after.is_none(),
            expires_at: expires,
        };
        let mut pending = self.pending.lock().map_err(|_| "mount state lock")?;
        pending.retain(|_, p| p.expires > now());
        if pending.len() >= 20 {
            return Err("Too many pending previews".into());
        }
        pending.insert(
            id,
            Pending {
                root,
                target,
                before,
                after,
                expires,
            },
        );
        Ok(preview)
    }
    pub fn preview(
        &self,
        root_id: &str,
        client: &str,
        content: &str,
    ) -> Result<MountPreview, String> {
        let name = match client {
            "claude" => "CLAUDE.md",
            "codex" => "AGENTS.md",
            "gemini" => "GEMINI.md",
            _ => return Err("Unsupported mount client".into()),
        };
        let root = self.root(root_id)?;
        let target = root.join(name);
        let before = read_target(&target)?;
        let after = merge(before.as_deref().unwrap_or(""), content)?;
        self.remember(root, target, before, Some(after))
    }
    pub fn rollback_preview(
        &self,
        db: &Database,
        root_id: &str,
        backup_id: &str,
    ) -> Result<MountPreview, String> {
        let root = self.root(root_id)?;
        let (target, old, hash): (String, Option<String>, String) = {
            let c = db.conn();
            let c = c.lock().map_err(|_| "database lock")?;
            c.query_row(
                "SELECT target,before_content,after_hash FROM mount_backups WHERE id=?1",
                [backup_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .map_err(|_| "Backup not found")?
        };
        let target = PathBuf::from(target);
        if target.parent() != Some(root.as_path())
            || !matches!(
                target.file_name().and_then(|s| s.to_str()),
                Some("CLAUDE.md" | "AGENTS.md" | "GEMINI.md")
            )
        {
            return Err("Choose the backup's original project directory".into());
        }
        let current = read_target(&target)?;
        if optional_hash(&current) != hash {
            return Err(
                "File changed since this backup; rollback would overwrite external edits".into(),
            );
        }
        self.remember(root, target, current, old)
    }
    pub fn apply(&self, db: &Database, id: &str, confirmation: &str) -> Result<BackupInfo, String> {
        if confirmation != "WRITE FILE" {
            return Err("Type WRITE FILE after reviewing the diff".into());
        }
        // Keep the operation serialized through compare, backup and replace. One-use previews.
        let mut pending = self.pending.lock().map_err(|_| "mount state lock")?;
        let p = pending
            .remove(id)
            .ok_or("Preview expired or already used")?;
        if p.expires <= now()
            || checked_root(&p.root)? != p.root
            || read_target(&p.target)? != p.before
        {
            return Err("File or directory changed after preview; preview again".into());
        }
        let info = BackupInfo {
            id: uuid::Uuid::new_v4().to_string(),
            target: p.target.display().to_string(),
            created_at: now(),
        };
        {
            let c = db.conn();
            let c = c.lock().map_err(|_| "database lock")?;
            let count: i64 = c
                .query_row("SELECT COUNT(*) FROM mount_backups", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            if count >= 1000 {
                return Err("Mount backup storage is full; no file was changed".into());
            }
            c.execute("INSERT INTO mount_backups(id,target,before_content,after_hash,created_at) VALUES(?1,?2,?3,?4,?5)",params![info.id,info.target,p.before,optional_hash(&p.after),info.created_at]).map_err(|e|e.to_string())?;
        }
        match p.after {
            Some(content) => atomic_write(&p.target, content.as_bytes())?,
            None => fs::remove_file(&p.target).map_err(|e| e.to_string())?,
        }
        event(db, "explicit_mount_write", &info.id)?;
        Ok(info)
    }
}
fn optional_hash(content: &Option<String>) -> String {
    match content {
        Some(s) => format!("file:{}", digest(s.as_bytes())),
        None => "absent".into(),
    }
}
#[tauri::command]
pub async fn choose_mount_root(
    app: tauri::AppHandle,
    mounts: tauri::State<'_, MountManager>,
) -> Result<Option<RootGrant>, String> {
    crate::platform::require_desktop("Explicit rule mounts")?;
    #[cfg(desktop)]
    {
        let selection =
            tauri::async_runtime::spawn_blocking(move || app.dialog().file().blocking_pick_folder())
                .await
                .map_err(|e| e.to_string())?;
        selection
            .map(|p| {
                p.into_path()
                    .map_err(|e| e.to_string())
                    .and_then(|p| mounts.grant(p))
            })
            .transpose()
    }
    #[cfg(mobile)]
    {
        let _ = (app, mounts);
        Err("Explicit rule mounts require a desktop application".into())
    }
}
#[tauri::command]
pub async fn preview_explicit_mount(
    root_id: String,
    client: String,
    prompt_ids: Vec<String>,
    skill_ids: Vec<String>,
    app: tauri::AppHandle,
    db: tauri::State<'_, Database>,
    mounts: tauri::State<'_, MountManager>,
) -> Result<MountPreview, String> {
    crate::platform::require_desktop("Explicit rule mounts")?;
    if prompt_ids.len() + skill_ids.len() == 0 || prompt_ids.len() + skill_ids.len() > 20 {
        return Err("Select 1 to 20 saved prompts/skills".into());
    }
    let mut parts = Vec::new();
    let prompts = db.list_prompts()?;
    for id in prompt_ids {
        let p = prompts
            .iter()
            .find(|p| p.id == id)
            .ok_or("Selected prompt no longer exists")?;
        parts.push(format!("## Prompt: {}\n{}", p.name, p.content));
    }
    if !skill_ids.is_empty() {
        let paths = crate::app_paths(app)?;
        let catalog = crate::skills_local::catalog(
            &db,
            Path::new(&paths.home_dir),
            Path::new(&paths.codex_home),
        )?;
        for id in skill_ids {
            let skill = catalog
                .skills
                .iter()
                .find(|s| s.id == id)
                .ok_or("Selected skill is not in the enabled local catalog")?;
            let directory = checked_root(Path::new(&skill.directory))?;
            let text = read_target(&directory.join("SKILL.md"))?.ok_or("SKILL.md is missing")?;
            parts.push(format!("## Skill: {}\n{}", skill.name, text));
        }
    }
    mounts.preview(&root_id, &client, &parts.join("\n\n"))
}
#[tauri::command]
pub fn preview_mount_rollback(
    root_id: String,
    backup_id: String,
    db: tauri::State<'_, Database>,
    mounts: tauri::State<'_, MountManager>,
) -> Result<MountPreview, String> {
    crate::platform::require_desktop("Governance changes")?;
    mounts.rollback_preview(&db, &root_id, &backup_id)
}
#[tauri::command]
pub fn apply_explicit_mount(
    id: String,
    confirmation: String,
    db: tauri::State<'_, Database>,
    mounts: tauri::State<'_, MountManager>,
) -> Result<BackupInfo, String> {
    crate::platform::require_desktop("Explicit rule mounts")?;
    mounts.apply(&db, &id, &confirmation)
}
#[tauri::command]
pub fn list_mount_backups(db: tauri::State<'_, Database>) -> Result<Vec<BackupInfo>, String> {
    let c = db.conn();
    let c = c.lock().map_err(|_| "database lock")?;
    let mut s=c.prepare("SELECT id,target,created_at FROM mount_backups ORDER BY created_at DESC,rowid DESC LIMIT 1000").map_err(|e|e.to_string())?;
    s.query_map([], |r| {
        Ok(BackupInfo {
            id: r.get(0)?,
            target: r.get(1)?,
            created_at: r.get(2)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())
}
#[cfg(test)]
#[path = "mount_tests.rs"]
mod tests;

#[tauri::command]
pub fn delete_mount_backup(id: String, confirmation: String, db: tauri::State<'_, Database>) -> Result<(), String> {
    crate::platform::require_desktop("Explicit rule mounts")?;
    if confirmation != "DELETE BACKUP" { return Err("Type DELETE BACKUP to confirm permanent deletion".into()); }
    let c = db.conn();
    let mut c = c.lock().map_err(|_| "database lock")?;
    let tx = c.transaction().map_err(|e| e.to_string())?;
    if tx.execute("DELETE FROM mount_backups WHERE id=?1", [&id]).map_err(|e| e.to_string())? != 1 {
        return Err("Backup no longer exists".into());
    }
    event_on(&tx, "mount_backup_deleted", &id)?;
    tx.commit().map_err(|e| e.to_string())
}
