use crate::sessions::{
    SessionCatalog, SessionDetail, SessionIdPair, SessionMessage, SessionMeta, SessionStore,
};

fn store(app: tauri::AppHandle) -> Result<SessionStore, String> {
    crate::platform::require_desktop("本地会话管理")?;
    let paths = crate::app_paths(app)?;
    #[cfg(debug_assertions)]
    let isolated = crate::debug_path_override("XY_SWITCH_TEST_HOME_DIR").is_some();
    #[cfg(not(debug_assertions))]
    let isolated = false;
    // A QA process must not inherit real client roots from the desktop environment.
    let directory = |key| if isolated { None } else { std::env::var_os(key).filter(|v| !v.is_empty()).map(std::path::PathBuf::from) };
    let claude = directory("CLAUDE_CONFIG_DIR");
    let gemini = directory("GEMINI_CLI_HOME");
    SessionStore::with_overrides(
        std::path::Path::new(&paths.home_dir),
        std::path::Path::new(&paths.codex_home),
        claude.as_deref(),
        gemini.as_deref(),
    )
}

#[tauri::command]
pub async fn get_session_catalog(app: tauri::AppHandle) -> Result<SessionCatalog, String> {
    let store = store(app)?;
    tauri::async_runtime::spawn_blocking(move || store.catalog())
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn list_sessions(app: tauri::AppHandle) -> Result<Vec<SessionMeta>, String> {
    Ok(get_session_catalog(app).await?.sessions)
}

#[tauri::command]
pub async fn get_session_detail(
    app: tauri::AppHandle,
    provider_id: String,
    session_id: String,
) -> Result<SessionDetail, String> {
    let store = store(app)?;
    tauri::async_runtime::spawn_blocking(move || store.detail(&provider_id, &session_id))
        .await
        .map_err(|e| e.to_string())?
}

#[tauri::command]
pub async fn get_session_messages(
    app: tauri::AppHandle,
    provider_id: String,
    session_id: String,
) -> Result<Vec<SessionMessage>, String> {
    Ok(get_session_detail(app, provider_id, session_id)
        .await?
        .messages)
}

#[tauri::command]
pub async fn delete_sessions(
    app: tauri::AppHandle,
    ids: Vec<SessionIdPair>,
) -> Result<usize, String> {
    let store = store(app)?;
    tauri::async_runtime::spawn_blocking(move || store.delete(&ids))
        .await
        .map_err(|e| e.to_string())?
}
