use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct VmProfile {
    pub id: Uuid,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub key_path: String,
    pub arch: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RunRecord {
    pub id: Uuid,
    pub automation_name: String,
    pub status: RunStatus,
    pub trigger_source: String,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub output: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelConfig {
    pub channel_type: ChannelType,
    pub enabled: bool,
    pub allowlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ChannelType {
    Slack,
    WhatsApp,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_status_serde() {
        for (status, expected) in [
            (RunStatus::Pending, "\"pending\""),
            (RunStatus::Running, "\"running\""),
            (RunStatus::Completed, "\"completed\""),
            (RunStatus::Failed, "\"failed\""),
        ] {
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, expected);
            let parsed: RunStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(parsed, status);
        }
    }

    #[test]
    fn channel_type_serde() {
        let json = serde_json::to_string(&ChannelType::WhatsApp).unwrap();
        assert_eq!(json, "\"whats_app\"");
        let parsed: ChannelType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ChannelType::WhatsApp);
    }

    #[test]
    fn channel_config_round_trip() {
        let config = ChannelConfig {
            channel_type: ChannelType::Slack,
            enabled: true,
            allowlist: vec!["user1".to_string(), "user2".to_string()],
        };
        let json = serde_json::to_string(&config).unwrap();
        let reparsed: ChannelConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(config, reparsed);
    }

    #[test]
    fn run_record_round_trip() {
        let record = RunRecord {
            id: Uuid::new_v4(),
            automation_name: "test-auto".to_string(),
            status: RunStatus::Completed,
            trigger_source: "manual".to_string(),
            started_at: Utc::now(),
            finished_at: Some(Utc::now()),
            output: Some("done".to_string()),
            error: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        let reparsed: RunRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record, reparsed);
    }

    #[test]
    fn run_record_optional_fields_null() {
        let record = RunRecord {
            id: Uuid::new_v4(),
            automation_name: "test".to_string(),
            status: RunStatus::Running,
            trigger_source: "cron".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            output: None,
            error: None,
        };
        let json = serde_json::to_string(&record).unwrap();
        assert!(json.contains("\"finished_at\":null"));
        let reparsed: RunRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(record, reparsed);
    }

    #[test]
    fn vm_profile_round_trip() {
        let profile = VmProfile {
            id: Uuid::new_v4(),
            name: "dev-vm".to_string(),
            host: "192.168.1.100".to_string(),
            port: 22,
            user: "ubuntu".to_string(),
            key_path: "/home/user/.ssh/id_rsa".to_string(),
            arch: Some("aarch64".to_string()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let json = serde_json::to_string(&profile).unwrap();
        let reparsed: VmProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(profile, reparsed);
    }
}
