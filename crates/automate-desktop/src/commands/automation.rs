use automate_shared::config::AutomationDef;

#[tauri::command]
pub async fn deploy_automation(automation: AutomationDef) -> Result<AutomationDef, String> {
    // Placeholder: In a full implementation, this would push the automation config
    // to the daemon via HTTP through an SSH tunnel.
    Ok(automation)
}

#[tauri::command]
pub async fn list_remote_automations() -> Result<Vec<AutomationDef>, String> {
    // Placeholder: Would fetch from daemon via SSH tunnel
    Ok(vec![])
}

#[tauri::command]
pub async fn trigger_remote_run(name: String) -> Result<String, String> {
    // Placeholder: Would POST to daemon's run endpoint via SSH tunnel
    Ok(format!("Run triggered for '{}' (placeholder)", name))
}
