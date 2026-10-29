//! Interface preferences are independent from provider and request configuration.
use crate::database::Database;
use rusqlite::params;
use tauri::{AppHandle, State};

const PREFERENCE_KEY: &str = "interface_language";
const RESOLVED_KEY: &str = "interface_language_resolved";

fn valid_preference(value: &str) -> bool {
    matches!(value, "system" | "zh-CN" | "en")
}
pub fn resolved(database: &Database) -> String {
    match database.get_setting(PREFERENCE_KEY).ok().flatten().as_deref() {
        Some("en") => "en".into(),
        Some("zh-CN") => "zh-CN".into(),
        _ => database.get_setting(RESOLVED_KEY).ok().flatten()
            .filter(|value| matches!(value.as_str(), "zh-CN" | "en"))
            .unwrap_or_else(|| "zh-CN".into()),
    }
}
pub fn label<'a>(language: &str, chinese: &'a str, english: &'a str) -> &'a str {
    if language == "en" { english } else { chinese }
}
fn save(database: &Database, preference: &str, resolved_language: &str) -> Result<(), String> {
    if !valid_preference(preference)
        || !matches!(resolved_language, "zh-CN" | "en")
        || (preference != "system" && preference != resolved_language)
    {
        return Err("Unsupported interface language".into());
    }
    let connection = database.conn();
    let mut connection = connection.lock().map_err(|_| "database lock poisoned".to_string())?;
    let transaction = connection.transaction().map_err(|error| error.to_string())?;
    for (key, value) in [(RESOLVED_KEY, resolved_language), (PREFERENCE_KEY, preference)] {
        transaction.execute(
            "INSERT INTO app_settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value,updated_at=CURRENT_TIMESTAMP",
            params![key, value],
        ).map_err(|error| error.to_string())?;
    }
    transaction.commit().map_err(|error| error.to_string())
}
#[tauri::command]
pub fn get_interface_language(db: State<'_, Database>) -> Result<Option<String>, String> {
    Ok(db.get_setting(PREFERENCE_KEY)?.filter(|value| valid_preference(value)))
}
#[tauri::command]
pub fn set_interface_language(app: AppHandle, db: State<'_, Database>, preference: String, resolved_language: String) -> Result<(), String> {
    save(&db, &preference, &resolved_language)?;
    crate::tray::refresh(&app, &db);
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn database() -> Database { Database::open(Path::new(":memory:")).unwrap() }
    #[test]
    fn accepts_supported_preferences_and_localizes_only_tray_labels() {
        for value in ["system", "zh-CN", "en"] { assert!(valid_preference(value)); }
        for value in ["", "en-US", "fr", "../en"] { assert!(!valid_preference(value)); }
        assert_eq!(label("en", "供应商", "Providers"), "Providers");
        assert_eq!(label("zh-CN", "供应商", "Providers"), "供应商");
    }
    #[test]
    fn persists_selection_and_system_resolution_without_touching_other_settings() {
        let db = database();
        db.set_setting("user-data", "用户内容").unwrap();
        for (preference, language) in [("en", "en"), ("zh-CN", "zh-CN"), ("system", "en"), ("system", "zh-CN")] {
            save(&db, preference, language).unwrap();
            assert_eq!(db.get_setting(PREFERENCE_KEY).unwrap().as_deref(), Some(preference));
            assert_eq!(resolved(&db), language);
        }
        assert_eq!(db.get_setting("user-data").unwrap().as_deref(), Some("用户内容"));
        for (preference, language) in [("en", "zh-CN"), ("fr", "en"), ("system", "fr")] {
            assert!(save(&db, preference, language).is_err());
            assert_eq!(resolved(&db), "zh-CN");
        }
    }
    #[test]
    fn rolls_back_both_keys_when_one_write_fails() {
        let db = database();
        save(&db, "en", "en").unwrap();
        db.conn().lock().unwrap().execute_batch(
            "CREATE TRIGGER fail_language BEFORE INSERT ON app_settings WHEN NEW.key='interface_language' BEGIN SELECT RAISE(ABORT, 'synthetic save failure'); END;"
        ).unwrap();
        assert!(save(&db, "system", "zh-CN").is_err());
        assert_eq!(db.get_setting(RESOLVED_KEY).unwrap().as_deref(), Some("en"));
        assert_eq!(db.get_setting(PREFERENCE_KEY).unwrap().as_deref(), Some("en"));
    }
}
