use automate_shared::models::RunRecord;

use crate::daemon_client::DaemonClient;
use crate::state::AppState;

#[tauri::command]
pub async fn daemon_api_get(
    state: tauri::State<'_, AppState>,
    vm_id: String,
    path: String,
) -> Result<String, String> {
    let client = DaemonClient::from_vm(&state, &vm_id).await?;
    client.get_raw(&path).await
}

#[tauri::command]
pub async fn fetch_run_history(
    state: tauri::State<'_, AppState>,
    vm_id: String,
) -> Result<Vec<RunRecord>, String> {
    let client = DaemonClient::from_vm(&state, &vm_id).await?;
    client.list_runs(Some(100)).await
}

#[tauri::command]
pub async fn get_run_detail(
    state: tauri::State<'_, AppState>,
    vm_id: String,
    run_id: String,
) -> Result<Option<RunRecord>, String> {
    let client = DaemonClient::from_vm(&state, &vm_id).await?;
    client.get_run(&run_id).await
}
