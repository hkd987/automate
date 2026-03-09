use std::collections::HashMap;

use anyhow::Result;
use tracing::warn;

use crate::config::AutomationDef;
use crate::models::RunRecord;

/// Well-known credential keys that are injected into job environments.
pub const GLOBAL_CREDENTIAL_KEYS: &[&str] = &[
    "ANTHROPIC_API_KEY",
    "OPENAI_API_KEY",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_DEFAULT_REGION",
];

#[async_trait::async_trait]
pub trait Store: Send + Sync + Clone + 'static {
    // Automations
    async fn insert_automation(&self, def: &AutomationDef) -> Result<()>;
    async fn list_automations(&self) -> Result<Vec<AutomationDef>>;
    async fn get_automation(&self, name: &str) -> Result<Option<AutomationDef>>;
    async fn delete_automation(&self, name: &str) -> Result<bool>;

    // Runs
    async fn insert_run(&self, run: &RunRecord) -> Result<()>;
    async fn update_run(&self, run: &RunRecord) -> Result<()>;
    async fn list_runs(&self, limit: Option<u32>) -> Result<Vec<RunRecord>>;

    // Credentials
    async fn set_credential(&self, key: &str, value: &str) -> Result<()>;
    async fn get_credential(&self, key: &str) -> Result<Option<String>>;
    async fn delete_credential(&self, key: &str) -> Result<bool>;
    async fn list_credential_keys(&self) -> Result<Vec<String>>;
}

/// Resolve credentials for a job by checking automation-specific overrides first,
/// then falling back to global credential keys. This is application-level policy,
/// not a storage concern.
pub async fn get_credentials_for_job<S: Store>(
    store: &S,
    automation_name: &str,
) -> HashMap<String, String> {
    let mut env = HashMap::new();

    for key in GLOBAL_CREDENTIAL_KEYS {
        // Check automation-specific override first (e.g., "my-auto.ANTHROPIC_API_KEY")
        let prefixed = format!("{}.{}", automation_name, key);
        match store.get_credential(&prefixed).await {
            Ok(Some(val)) => {
                env.insert(key.to_string(), val);
                continue;
            }
            Err(e) => {
                warn!(key = %prefixed, error = %e, "Failed to fetch credential");
                continue;
            }
            Ok(None) => {}
        }

        // Fall back to global key
        match store.get_credential(key).await {
            Ok(Some(val)) => {
                env.insert(key.to_string(), val);
            }
            Err(e) => {
                warn!(key = %key, error = %e, "Failed to fetch credential");
            }
            Ok(None) => {}
        }
    }

    if env.is_empty() {
        warn!(automation = automation_name, "No credentials found for job");
    }

    env
}
