pub mod commands;
pub mod db;
pub mod ssh;
pub mod state;

use std::path::PathBuf;

use commands::{auth, automation, deploy, info, run_history, templates, vm};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let db_path = resolve_db_path();
    let conn = db::init_db(db_path.to_str().unwrap()).expect("failed to initialize database");
    let app_state = state::init_state(conn);

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            vm::add_vm,
            vm::update_vm,
            vm::delete_vm,
            vm::list_vms,
            vm::test_connection,
            deploy::deploy_daemon,
            automation::deploy_automation,
            automation::list_remote_automations,
            automation::trigger_remote_run,
            run_history::fetch_run_history,
            run_history::get_run_detail,
            auth::list_auth_profiles,
            auth::save_auth_profile,
            auth::delete_auth_profile,
            auth::push_credentials,
            auth::configure_slack,
            auth::configure_whatsapp,
            info::get_app_info,
            templates::list_templates,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn resolve_db_path() -> PathBuf {
    if let Some(data_dir) = dirs::data_dir() {
        let app_dir = data_dir.join("automate");
        if std::fs::create_dir_all(&app_dir).is_ok() {
            return app_dir.join("automate_desktop.db");
        }
    }
    std::env::temp_dir().join("automate_desktop.db")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_db_path_returns_valid_path() {
        let path = resolve_db_path();
        assert!(path.to_str().is_some());
        assert!(path.ends_with("automate_desktop.db"));
    }

    #[test]
    fn test_resolve_db_path_prefers_data_dir() {
        let path = resolve_db_path();
        if let Some(data_dir) = dirs::data_dir() {
            let expected = data_dir.join("automate").join("automate_desktop.db");
            assert_eq!(path, expected);
        }
    }

    #[test]
    fn test_resolve_db_path_creates_parent_dir() {
        let path = resolve_db_path();
        let parent = path.parent().unwrap();
        assert!(parent.exists());
    }
}
