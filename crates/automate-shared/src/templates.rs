use serde::{Deserialize, Serialize};

use crate::config::AutomationDef;
use crate::triggers::TriggerDef;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Template {
    pub id: String,
    pub name: String,
    pub description: String,
    pub category: TemplateCategory,
    pub config: AutomationDef,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TemplateCategory {
    Monitoring,
    DevOps,
    CodeReview,
    Reporting,
    Custom,
}

pub fn built_in_templates() -> Vec<Template> {
    vec![
        Template {
            id: "daily-git-summary".to_string(),
            name: "Daily Git Summary".to_string(),
            description: "Summarize daily git activity across repositories and post a digest."
                .to_string(),
            category: TemplateCategory::Reporting,
            config: AutomationDef {
                name: "daily-git-summary".to_string(),
                trigger: TriggerDef::Cron("0 18 * * *".to_string()),
                auth_profile: None,
                prompt: "Review today's git commits across all repos. Summarize changes by author and project, highlight breaking changes, and post a daily digest.".to_string(),
                file: None,
            },
        },
        Template {
            id: "error-log-monitor".to_string(),
            name: "Error Log Monitor".to_string(),
            description: "Watch logs for error patterns and alert when detected.".to_string(),
            category: TemplateCategory::Monitoring,
            config: AutomationDef {
                name: "error-log-monitor".to_string(),
                trigger: TriggerDef::LogPattern("ERROR".to_string()),
                auth_profile: None,
                prompt: "Analyze the matched error log entry. Determine severity, check for related errors, and send an alert with context and suggested fixes.".to_string(),
                file: None,
            },
        },
        Template {
            id: "pr-review-bot".to_string(),
            name: "PR Review Bot".to_string(),
            description: "Automatically review code when changes are pushed.".to_string(),
            category: TemplateCategory::CodeReview,
            config: AutomationDef {
                name: "pr-review-bot".to_string(),
                trigger: TriggerDef::Webhook,
                auth_profile: None,
                prompt: "Review the pushed code changes. Check for bugs, style issues, security vulnerabilities, and suggest improvements. Post review comments.".to_string(),
                file: None,
            },
        },
        Template {
            id: "scheduled-backup-verify".to_string(),
            name: "Scheduled Backup Verify".to_string(),
            description: "Periodically verify that backups are valid and complete.".to_string(),
            category: TemplateCategory::DevOps,
            config: AutomationDef {
                name: "scheduled-backup-verify".to_string(),
                trigger: TriggerDef::Cron("0 3 * * *".to_string()),
                auth_profile: None,
                prompt: "Check the latest backup files for integrity. Verify checksums, confirm expected file sizes, and report any missing or corrupted backups.".to_string(),
                file: None,
            },
        },
        Template {
            id: "deploy-notifier".to_string(),
            name: "Deploy Notifier".to_string(),
            description: "Notify the team when a deployment occurs.".to_string(),
            category: TemplateCategory::DevOps,
            config: AutomationDef {
                name: "deploy-notifier".to_string(),
                trigger: TriggerDef::Webhook,
                auth_profile: None,
                prompt: "A deployment event was received. Extract the service name, version, and environment. Notify the team channel with deployment details and changelog.".to_string(),
                file: None,
            },
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_templates_are_valid() {
        let templates = built_in_templates();
        assert_eq!(templates.len(), 5);
        for t in &templates {
            assert!(!t.id.is_empty());
            assert!(!t.name.is_empty());
            assert!(!t.description.is_empty());
            assert!(!t.config.name.is_empty());
            assert!(!t.config.prompt.is_empty());
        }
    }

    #[test]
    fn templates_serialize_and_deserialize() {
        let templates = built_in_templates();
        for t in &templates {
            let json = serde_json::to_string(t).expect("serialize template");
            let reparsed: Template = serde_json::from_str(&json).expect("deserialize template");
            assert_eq!(reparsed.id, t.id);
            assert_eq!(reparsed.name, t.name);
            assert_eq!(reparsed.category, t.category);
        }
    }

    #[test]
    fn template_category_serde_round_trip() {
        let categories = vec![
            TemplateCategory::Monitoring,
            TemplateCategory::DevOps,
            TemplateCategory::CodeReview,
            TemplateCategory::Reporting,
            TemplateCategory::Custom,
        ];
        for cat in categories {
            let json = serde_json::to_string(&cat).unwrap();
            let reparsed: TemplateCategory = serde_json::from_str(&json).unwrap();
            assert_eq!(reparsed, cat);
        }
    }

    #[test]
    fn template_category_rename_all() {
        assert_eq!(
            serde_json::to_string(&TemplateCategory::CodeReview).unwrap(),
            "\"code_review\""
        );
        assert_eq!(
            serde_json::to_string(&TemplateCategory::DevOps).unwrap(),
            "\"dev_ops\""
        );
    }
}
