use serde::{Deserialize, Serialize};

use crate::{
    database::Database,
    skills_local::{self, InstalledSkill, SkillCatalog, SkillImportResult, SkillImportSettings},
};
use std::path::Path;
use tauri::State;

/// 可发现（尚未安装）的技能。
///
/// 注意主键是 `key` 而不是 `id`：前端 `installSkill()` 把整个
/// `DiscoverableSkill` 原样回传给 `install_skill_unified`，字段对不上
/// 会直接反序列化失败。
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscoverableSkill {
    pub key: String,
    pub name: String,
    pub description: String,
    pub directory: String,
    pub readme_url: Option<String>,
    pub repo_owner: String,
    pub repo_name: String,
    pub repo_branch: String,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillUpdateInfo {
    pub id: String,
    pub name: String,
    pub current_hash: Option<String>,
    pub remote_hash: String,
}

// Remote installation and client mounts remain intentionally unavailable.
const NOT_IMPLEMENTED: &str = "当前只支持本地技能导入，不支持远程安装或修改平台挂载";

#[tauri::command]
pub async fn get_installed_skills(
    app: tauri::AppHandle,
    database: State<'_, Database>,
) -> Result<Vec<InstalledSkill>, String> {
    Ok(get_skill_catalog(app, database).await?.skills)
}

#[tauri::command]
pub async fn get_skill_catalog(
    app: tauri::AppHandle,
    database: State<'_, Database>,
) -> Result<SkillCatalog, String> {
    let paths = crate::app_paths(app)?;
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        skills_local::catalog(
            &database,
            Path::new(&paths.home_dir),
            Path::new(&paths.codex_home),
        )
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_skill_import_settings(
    database: State<'_, Database>,
) -> Result<SkillImportSettings, String> {
    skills_local::settings(database.inner())
}
#[tauri::command]
pub fn save_skill_import_settings(
    database: State<'_, Database>,
    settings: SkillImportSettings,
) -> Result<(), String> {
    skills_local::save_settings(database.inner(), &settings)
}

#[tauri::command]
pub async fn install_skill_unified(_skill: DiscoverableSkill) -> Result<InstalledSkill, String> {
    Err(NOT_IMPLEMENTED.to_string())
}

#[tauri::command]
pub async fn uninstall_skill_unified(
    database: State<'_, Database>,
    id: String,
) -> Result<(), String> {
    skills_local::remove_reference(database.inner(), &id)
}

#[tauri::command]
pub async fn toggle_skill_app(_id: String, _app: String, _enabled: bool) -> Result<(), String> {
    Err(NOT_IMPLEMENTED.to_string())
}

#[tauri::command]
pub async fn discover_available_skills() -> Result<Vec<DiscoverableSkill>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn check_skill_updates() -> Result<Vec<SkillUpdateInfo>, String> {
    Ok(vec![])
}

#[tauri::command]
pub async fn update_skill(_id: String) -> Result<InstalledSkill, String> {
    Err(NOT_IMPLEMENTED.to_string())
}

#[tauri::command]
pub async fn import_skill_folder(
    database: State<'_, Database>,
    path: String,
) -> Result<SkillImportResult, String> {
    let database = database.inner().clone();
    tauri::async_runtime::spawn_blocking(move || skills_local::import_folder(&database, &path))
        .await
        .map_err(|e| e.to_string())?
}

#[cfg(desktop)]
#[tauri::command]
pub async fn pick_and_import_skill_folder(
    app: tauri::AppHandle,
    database: State<'_, Database>,
) -> Result<Option<SkillImportResult>, String> {
    use tauri_plugin_dialog::DialogExt;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |folder| {
        let _ = tx.send(folder);
    });
    let folder = rx.await.map_err(|e| e.to_string())?;
    if let Some(path) = folder {
        let path_str = path.to_string();
        let database = database.inner().clone();
        tauri::async_runtime::spawn_blocking(move || {
            skills_local::import_folder(&database, &path_str)
        })
        .await
        .map_err(|e| e.to_string())?
        .map(Some)
    } else {
        Ok(None)
    }
}

#[cfg(mobile)]
#[tauri::command]
pub async fn pick_and_import_skill_folder(
    _app: tauri::AppHandle,
    _database: State<'_, Database>,
) -> Result<Option<SkillImportResult>, String> {
    Err("移动端不支持扫描桌面 Skills 目录，请在 Windows、macOS 或 Linux 端导入".to_string())
}
