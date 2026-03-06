use automate_shared::auth::AuthProfileDef;
use automate_shared::config::ProfilesConfig;
use std::collections::HashMap;

fn profiles_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    std::path::Path::new(&home)
        .join(".automate")
        .join("profiles.yml")
}

fn read_profiles() -> Result<ProfilesConfig, String> {
    let path = profiles_path();
    if !path.exists() {
        return Ok(ProfilesConfig {
            auth_profiles: HashMap::new(),
        });
    }
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read profiles.yml: {}", e))?;
    serde_yaml::from_str(&contents).map_err(|e| format!("Failed to parse profiles.yml: {}", e))
}

fn write_profiles(config: &ProfilesConfig) -> Result<(), String> {
    let path = profiles_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create config directory: {}", e))?;
    }
    let yaml = serde_yaml::to_string(config)
        .map_err(|e| format!("Failed to serialize profiles: {}", e))?;
    std::fs::write(&path, yaml).map_err(|e| format!("Failed to write profiles.yml: {}", e))
}

#[tauri::command]
pub async fn list_auth_profiles() -> Result<HashMap<String, AuthProfileDef>, String> {
    let config = read_profiles()?;
    Ok(config.auth_profiles)
}

#[tauri::command]
pub async fn save_auth_profile(name: String, profile: AuthProfileDef) -> Result<(), String> {
    let mut config = read_profiles()?;
    config.auth_profiles.insert(name, profile);
    write_profiles(&config)
}

#[tauri::command]
pub async fn delete_auth_profile(name: String) -> Result<(), String> {
    let mut config = read_profiles()?;
    if config.auth_profiles.remove(&name).is_none() {
        return Err(format!("Profile '{}' not found", name));
    }
    write_profiles(&config)
}

#[tauri::command]
pub async fn push_credentials(
    _daemon_url: String,
    _key: String,
    _value: String,
) -> Result<(), String> {
    // Placeholder: In production this would push credentials to the daemon
    // via an SSH tunnel. For now we just validate the inputs.
    if _key.is_empty() {
        return Err("Credential key cannot be empty".to_string());
    }
    if _value.is_empty() {
        return Err("Credential value cannot be empty".to_string());
    }
    // TODO: POST to daemon_url/credentials once SSH tunnel is wired up
    Ok(())
}

#[tauri::command]
pub async fn configure_slack(
    bot_token: String,
    app_token: String,
    allowed_user_ids: Vec<String>,
    enabled: bool,
) -> Result<(), String> {
    if enabled && bot_token.is_empty() {
        return Err("Bot token is required when Slack is enabled".to_string());
    }
    if enabled && app_token.is_empty() {
        return Err("App token is required for socket mode".to_string());
    }
    // TODO: Push config to daemon via API
    let _ = (bot_token, app_token, allowed_user_ids, enabled);
    Ok(())
}

#[tauri::command]
pub async fn configure_whatsapp(allowed_numbers: Vec<String>, enabled: bool) -> Result<(), String> {
    // TODO: Push config to daemon via API
    let _ = (allowed_numbers, enabled);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use automate_shared::auth::{AuthMode, Runtime};

    #[test]
    fn profiles_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("profiles.yml");

        let mut profiles = HashMap::new();
        profiles.insert(
            "default".to_string(),
            AuthProfileDef {
                runtime: Runtime::ClaudeCode,
                mode: AuthMode::Subscription,
                aws_region: None,
                aws_model: None,
            },
        );
        let config = ProfilesConfig {
            auth_profiles: profiles,
        };

        let yaml = serde_yaml::to_string(&config).unwrap();
        std::fs::write(&path, &yaml).unwrap();

        let contents = std::fs::read_to_string(&path).unwrap();
        let parsed: ProfilesConfig = serde_yaml::from_str(&contents).unwrap();
        assert_eq!(parsed.auth_profiles.len(), 1);
        assert!(parsed.auth_profiles.contains_key("default"));
    }
}
