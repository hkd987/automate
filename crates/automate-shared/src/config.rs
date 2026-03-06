use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::auth::AuthProfileDef;
use crate::triggers::TriggerDef;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutomateConfig {
    pub automations: Vec<AutomationDef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AutomationDef {
    pub name: String,
    pub trigger: TriggerDef,
    #[serde(default)]
    pub auth_profile: Option<String>,
    pub prompt: String,
    #[serde(default)]
    pub file: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProfilesConfig {
    pub auth_profiles: HashMap<String, AuthProfileDef>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth::{AuthMode, Runtime};

    #[test]
    fn serialize_automate_config() {
        let config = AutomateConfig {
            automations: vec![AutomationDef {
                name: "test".to_string(),
                trigger: TriggerDef::Manual,
                auth_profile: None,
                prompt: "do stuff".to_string(),
                file: None,
            }],
        };
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("name: test"));
        assert!(yaml.contains("trigger: manual"));
    }

    #[test]
    fn deserialize_automate_config() {
        let yaml = r#"
automations:
  - name: my-auto
    trigger: webhook
    prompt: run it
"#;
        let config: AutomateConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.automations.len(), 1);
        assert_eq!(config.automations[0].name, "my-auto");
        assert_eq!(config.automations[0].trigger, TriggerDef::Webhook);
    }

    #[test]
    fn round_trip_automation_def_with_all_fields() {
        let def = AutomationDef {
            name: "full".to_string(),
            trigger: TriggerDef::Cron("*/5 * * * *".to_string()),
            auth_profile: Some("prod".to_string()),
            prompt: "deploy".to_string(),
            file: Some(PathBuf::from("deploy.sh")),
        };
        let yaml = serde_yaml::to_string(&def).unwrap();
        let reparsed: AutomationDef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(def, reparsed);
    }

    #[test]
    fn profiles_config_round_trip() {
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
        let reparsed: ProfilesConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config, reparsed);
    }
}
