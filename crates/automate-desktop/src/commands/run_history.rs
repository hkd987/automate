use automate_shared::models::RunRecord;

#[tauri::command]
pub async fn fetch_run_history() -> Result<Vec<RunRecord>, String> {
    // Placeholder: Would fetch from daemon via SSH tunnel
    Ok(vec![])
}

#[tauri::command]
pub async fn get_run_detail(_run_id: String) -> Result<Option<RunRecord>, String> {
    // Placeholder: Would fetch a specific run from daemon
    Ok(None)
}
