use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Runtime {
    ClaudeCode,
    Codex,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AuthMode {
    Subscription,
    Bedrock,
    ApiKey,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AuthProfileDef {
    pub runtime: Runtime,
    pub mode: AuthMode,
    #[serde(default)]
    pub aws_region: Option<String>,
    #[serde(default)]
    pub aws_model: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_serde_snake_case() {
        let json = serde_json::to_string(&Runtime::ClaudeCode).unwrap();
        assert_eq!(json, "\"claude_code\"");
        let parsed: Runtime = serde_json::from_str("\"claude_code\"").unwrap();
        assert_eq!(parsed, Runtime::ClaudeCode);
    }

    #[test]
    fn auth_mode_serde_snake_case() {
        for (mode, expected) in [
            (AuthMode::Subscription, "\"subscription\""),
            (AuthMode::Bedrock, "\"bedrock\""),
            (AuthMode::ApiKey, "\"api_key\""),
        ] {
            let json = serde_json::to_string(&mode).unwrap();
            assert_eq!(json, expected);
            let parsed: AuthMode = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, mode);
        }
    }

    #[test]
    fn auth_profile_round_trip_minimal() {
        let profile = AuthProfileDef {
            runtime: Runtime::Codex,
            mode: AuthMode::ApiKey,
            aws_region: None,
            aws_model: None,
        };
        let yaml = serde_yaml::to_string(&profile).unwrap();
        let reparsed: AuthProfileDef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(profile, reparsed);
    }

    #[test]
    fn auth_profile_round_trip_full() {
        let profile = AuthProfileDef {
            runtime: Runtime::ClaudeCode,
            mode: AuthMode::Bedrock,
            aws_region: Some("us-west-2".to_string()),
            aws_model: Some("anthropic.claude-v2".to_string()),
        };
        let yaml = serde_yaml::to_string(&profile).unwrap();
        let reparsed: AuthProfileDef = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(profile, reparsed);
    }

    #[test]
    fn auth_profile_missing_optional_aws_fields() {
        let yaml = r#"
runtime: codex
mode: api_key
"#;
        let profile: AuthProfileDef = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(profile.aws_region, None);
        assert_eq!(profile.aws_model, None);
    }
}
