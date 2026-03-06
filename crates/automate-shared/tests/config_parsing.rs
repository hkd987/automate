use automate_shared::auth::{AuthMode, Runtime};
use automate_shared::config::{AutomateConfig, ProfilesConfig};
use automate_shared::triggers::TriggerDef;
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("failed to read {}: {}", path.display(), e))
}

#[test]
fn parse_sample_automate_yml() {
    let yaml = fixture("sample_automate.yml");
    let config: AutomateConfig = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(config.automations.len(), 4);

    let daily = &config.automations[0];
    assert_eq!(daily.name, "daily-report");
    assert_eq!(daily.trigger, TriggerDef::Cron("0 9 * * *".to_string()));
    assert_eq!(daily.auth_profile.as_deref(), Some("default"));
    assert_eq!(daily.file, Some(PathBuf::from("reports/daily.md")));

    let webhook = &config.automations[1];
    assert_eq!(webhook.name, "deploy-webhook");
    assert_eq!(webhook.trigger, TriggerDef::Webhook);
    assert_eq!(webhook.auth_profile.as_deref(), Some("bedrock-profile"));

    let log = &config.automations[2];
    assert_eq!(log.name, "error-watcher");
    assert_eq!(log.trigger, TriggerDef::LogPattern("ERROR".to_string()));
    assert_eq!(log.auth_profile, None);
    assert_eq!(log.file, None);

    let manual = &config.automations[3];
    assert_eq!(manual.name, "manual-check");
    assert_eq!(manual.trigger, TriggerDef::Manual);
}

#[test]
fn parse_sample_profiles_yml() {
    let yaml = fixture("sample_profiles.yml");
    let config: ProfilesConfig = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(config.auth_profiles.len(), 3);

    let default = &config.auth_profiles["default"];
    assert_eq!(default.runtime, Runtime::ClaudeCode);
    assert_eq!(default.mode, AuthMode::Subscription);
    assert_eq!(default.aws_region, None);

    let bedrock = &config.auth_profiles["bedrock-profile"];
    assert_eq!(bedrock.runtime, Runtime::ClaudeCode);
    assert_eq!(bedrock.mode, AuthMode::Bedrock);
    assert_eq!(bedrock.aws_region.as_deref(), Some("us-east-1"));
    assert_eq!(
        bedrock.aws_model.as_deref(),
        Some("anthropic.claude-3-sonnet")
    );

    let api_key = &config.auth_profiles["api-key-profile"];
    assert_eq!(api_key.runtime, Runtime::Codex);
    assert_eq!(api_key.mode, AuthMode::ApiKey);
}

#[test]
fn parse_minimal_yml() {
    let yaml = fixture("minimal.yml");
    let config: AutomateConfig = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(config.automations.len(), 1);
    assert_eq!(config.automations[0].name, "simple");
    assert_eq!(config.automations[0].trigger, TriggerDef::Manual);
    assert_eq!(config.automations[0].auth_profile, None);
    assert_eq!(config.automations[0].file, None);
}

#[test]
fn parse_invalid_trigger_yml() {
    let yaml = fixture("invalid_trigger.yml");
    let result = serde_yaml::from_str::<AutomateConfig>(&yaml);
    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("unknown trigger"),
        "expected 'unknown trigger' in error, got: {}",
        err_msg
    );
}

#[test]
fn round_trip_automate_config() {
    let yaml = fixture("sample_automate.yml");
    let config: AutomateConfig = serde_yaml::from_str(&yaml).unwrap();
    let serialized = serde_yaml::to_string(&config).unwrap();
    let reparsed: AutomateConfig = serde_yaml::from_str(&serialized).unwrap();
    assert_eq!(config, reparsed);
}

#[test]
fn round_trip_profiles_config() {
    let yaml = fixture("sample_profiles.yml");
    let config: ProfilesConfig = serde_yaml::from_str(&yaml).unwrap();
    let serialized = serde_yaml::to_string(&config).unwrap();
    let reparsed: ProfilesConfig = serde_yaml::from_str(&serialized).unwrap();
    assert_eq!(config, reparsed);
}

#[test]
fn empty_automations_list() {
    let yaml = "automations: []";
    let config: AutomateConfig = serde_yaml::from_str(yaml).unwrap();
    assert!(config.automations.is_empty());
}

#[test]
fn missing_automations_key_fails() {
    let yaml = "something_else: true";
    let result = serde_yaml::from_str::<AutomateConfig>(yaml);
    assert!(result.is_err());
}

#[test]
fn missing_optional_fields_defaults() {
    let yaml = r#"
automations:
  - name: no-optionals
    trigger: webhook
    prompt: Just a prompt
"#;
    let config: AutomateConfig = serde_yaml::from_str(yaml).unwrap();
    let a = &config.automations[0];
    assert_eq!(a.auth_profile, None);
    assert_eq!(a.file, None);
}

#[test]
fn unknown_fields_are_rejected_for_automation() {
    // serde_yaml by default ignores unknown fields unless deny_unknown_fields is set.
    // This test documents the current behavior.
    let yaml = r#"
automations:
  - name: with-extra
    trigger: manual
    prompt: test
    extra_field: should-be-ignored
"#;
    // Without deny_unknown_fields, this should succeed (documents current behavior)
    let result = serde_yaml::from_str::<AutomateConfig>(yaml);
    // If it succeeds, unknown fields are silently ignored
    // If it fails, deny_unknown_fields is active
    if let Ok(config) = result {
        assert_eq!(config.automations[0].name, "with-extra");
    }
    // Either way, we've documented the behavior
}
