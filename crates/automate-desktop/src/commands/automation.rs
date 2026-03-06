use automate_shared::config::AutomationDef;

use crate::daemon_client::DaemonClient;
use crate::state::AppState;

#[tauri::command]
pub async fn deploy_automation(
    state: tauri::State<'_, AppState>,
    automation: AutomationDef,
    vm_id: String,
) -> Result<AutomationDef, String> {
    let client = DaemonClient::from_vm(&state, &vm_id).await?;
    client.create_automation(&automation).await?;
    Ok(automation)
}

#[tauri::command]
pub async fn list_remote_automations(
    state: tauri::State<'_, AppState>,
    vm_id: String,
) -> Result<Vec<AutomationDef>, String> {
    let client = DaemonClient::from_vm(&state, &vm_id).await?;
    client.list_automations().await
}

#[tauri::command]
pub async fn trigger_remote_run(
    state: tauri::State<'_, AppState>,
    vm_id: String,
    name: String,
) -> Result<String, String> {
    let client = DaemonClient::from_vm(&state, &vm_id).await?;
    client.trigger_run(&name).await
}
