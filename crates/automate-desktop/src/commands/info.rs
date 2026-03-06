use serde::Serialize;

#[derive(Serialize)]
pub struct AppInfo {
    pub version: String,
    pub platform: String,
    pub build_date: String,
}

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    let platform = if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "windows") {
        "windows"
    } else {
        "unknown"
    };

    AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: platform.to_string(),
        build_date: option_env!("BUILD_DATE").unwrap_or("dev").to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_app_info() {
        let info = get_app_info();
        assert!(!info.version.is_empty());
        assert!(!info.platform.is_empty());
    }
}
