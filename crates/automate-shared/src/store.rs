use std::collections::HashMap;

use anyhow::Result;

use crate::config::AutomationDef;
use crate::models::RunRecord;

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
    async fn get_credentials_for_job(&self, automation_name: &str) -> HashMap<String, String>;
}
