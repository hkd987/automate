use chrono::Utc;
use uuid::Uuid;

use crate::db;
use crate::state::AppState;
use automate_shared::models::VmProfile;

#[tauri::command]
pub async fn add_vm(
    state: tauri::State<'_, AppState>,
    name: String,
    host: String,
    port: u16,
    user: String,
    key_path: String,
) -> Result<VmProfile, String> {
    let now = Utc::now();
    let profile = VmProfile {
        id: Uuid::new_v4(),
        name,
        host,
        port,
        user,
        key_path,
        arch: None,
        created_at: now,
        updated_at: now,
    };
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::insert_vm(&conn, &profile).map_err(|e| e.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn update_vm(
    state: tauri::State<'_, AppState>,
    id: String,
    name: String,
    host: String,
    port: u16,
    user: String,
    key_path: String,
) -> Result<VmProfile, String> {
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let existing = db::get_vm(&conn, &uuid)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "VM not found".to_string())?;
    let profile = VmProfile {
        id: uuid,
        name,
        host,
        port,
        user,
        key_path,
        arch: existing.arch,
        created_at: existing.created_at,
        updated_at: Utc::now(),
    };
    db::update_vm(&conn, &profile).map_err(|e| e.to_string())?;
    Ok(profile)
}

#[tauri::command]
pub async fn delete_vm(state: tauri::State<'_, AppState>, id: String) -> Result<(), String> {
    let uuid: Uuid = id.parse().map_err(|e: uuid::Error| e.to_string())?;
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::delete_vm(&conn, &uuid).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn list_vms(state: tauri::State<'_, AppState>) -> Result<Vec<VmProfile>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    db::list_vms(&conn).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_connection(
    host: String,
    port: u16,
    user: String,
    key_path: String,
) -> Result<String, String> {
    let ssh = crate::ssh::SshConnection::new(host, port, user, std::path::PathBuf::from(key_path));
    ssh.connect().await.map_err(|e| e.to_string())?;
    let arch = ssh.detect_arch().await.map_err(|e| e.to_string())?;
    Ok(arch)
}
