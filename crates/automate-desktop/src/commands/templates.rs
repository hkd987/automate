use automate_shared::templates::{built_in_templates, Template};

#[tauri::command]
pub async fn list_templates() -> Result<Vec<Template>, String> {
    Ok(built_in_templates())
}
