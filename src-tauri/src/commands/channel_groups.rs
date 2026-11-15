use crate::{accounts::AccountPool, channel_groups::{self, types::*}, database::Database};
use tauri::State;

fn metadata(pool: &AccountPool) -> Result<Vec<crate::accounts::AccountInfo>, String> {
    pool.list().map_err(|_| "channel_group_account_metadata_unavailable".into())
}

#[tauri::command]
pub fn channel_groups_list(database: State<'_, Database>, pool: State<'_, AccountPool>) -> Result<Snapshot, String> {
    channel_groups::list(&database, &metadata(&pool)?)
}

#[tauri::command]
pub fn channel_groups_save(database: State<'_, Database>, pool: State<'_, AccountPool>, input: SaveGroup) -> Result<Snapshot, String> {
    channel_groups::save(&database, &metadata(&pool)?, input)
}

#[tauri::command]
pub fn channel_groups_set_enabled(database: State<'_, Database>, pool: State<'_, AccountPool>, group_id: String, enabled: bool, expected_revision: u64) -> Result<Snapshot, String> {
    channel_groups::set_enabled(&database, &metadata(&pool)?, &group_id, enabled, expected_revision)
}

#[tauri::command]
pub fn channel_groups_recorded_events(database: State<'_, Database>, group_id: String, limit: usize) -> Result<Vec<RecordedEvent>, String> {
    channel_groups::recorded_events(&database, &group_id, limit)
}

#[tauri::command]
pub fn channel_groups_simulate(database: State<'_, Database>, pool: State<'_, AccountPool>, input: TrialInput) -> Result<TrialResult, String> {
    channel_groups::simulate(&database, &metadata(&pool)?, input)
}
