use serde::{Deserialize, Serialize};

use crate::config::AutomationDef;
use crate::models::{RunRecord, RunStatus};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonRequest {
    Health,
    ListAutomations,
    CreateAutomation { automation: AutomationDef },
    DeleteAutomation { name: String },
    TriggerRun { name: String },
    ListRuns,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonResponse {
    Health { version: String, uptime_secs: u64 },
    Automations { automations: Vec<AutomationDef> },
    Runs { runs: Vec<RunRecord> },
    Ok,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunRequest {
    pub automation_name: String,
    pub trigger_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunResponse {
    pub run_id: String,
    pub status: RunStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::triggers::TriggerDef;

    #[test]
    fn daemon_request_health_serde() {
        let req = DaemonRequest::Health;
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"health\""));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonRequest::Health));
    }

    #[test]
    fn daemon_request_trigger_run_serde() {
        let req = DaemonRequest::TriggerRun {
            name: "my-auto".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"trigger_run\""));
        assert!(json.contains("\"name\":\"my-auto\""));
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::TriggerRun { name } => assert_eq!(name, "my-auto"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn daemon_request_create_automation_serde() {
        let automation = AutomationDef {
            name: "new".to_string(),
            trigger: TriggerDef::Webhook,
            auth_profile: None,
            prompt: "do it".to_string(),
            file: None,
        };
        let req = DaemonRequest::CreateAutomation {
            automation: automation.clone(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: DaemonRequest = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonRequest::CreateAutomation { automation: a } => {
                assert_eq!(a.name, "new");
                assert_eq!(a.trigger, TriggerDef::Webhook);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn daemon_response_health_serde() {
        let resp = DaemonResponse::Health {
            version: "0.1.0".to_string(),
            uptime_secs: 3600,
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"health\""));
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonResponse::Health {
                version,
                uptime_secs,
            } => {
                assert_eq!(version, "0.1.0");
                assert_eq!(uptime_secs, 3600);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn daemon_response_error_serde() {
        let resp = DaemonResponse::Error {
            message: "not found".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        match parsed {
            DaemonResponse::Error { message } => assert_eq!(message, "not found"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn daemon_response_ok_serde() {
        let resp = DaemonResponse::Ok;
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"type\":\"ok\""));
        let parsed: DaemonResponse = serde_json::from_str(&json).unwrap();
        assert!(matches!(parsed, DaemonResponse::Ok));
    }

    #[test]
    fn run_request_round_trip() {
        let req = RunRequest {
            automation_name: "deploy".to_string(),
            trigger_source: "webhook".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: RunRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.automation_name, "deploy");
        assert_eq!(parsed.trigger_source, "webhook");
    }

    #[test]
    fn run_response_round_trip() {
        let resp = RunResponse {
            run_id: "abc-123".to_string(),
            status: RunStatus::Pending,
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: RunResponse = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.run_id, "abc-123");
        assert_eq!(parsed.status, RunStatus::Pending);
    }
}
