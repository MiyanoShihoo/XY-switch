use crate::database::{Database, Prompt};
use tauri::State;

#[tauri::command]
pub fn list_prompts(database: State<'_, Database>) -> Result<Vec<Prompt>, String> {
    database.list_prompts()
}

#[tauri::command]
pub fn save_prompt(database: State<'_, Database>, prompt: Prompt) -> Result<(), String> {
    database.save_prompt(&prompt)
}

#[tauri::command]
pub fn delete_prompt(database: State<'_, Database>, id: String) -> Result<(), String> {
    database.delete_prompt(&id)
}

#[tauri::command]
pub fn set_prompt_enabled(
    database: State<'_, Database>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    database.set_prompt_enabled(&id, enabled)
}
