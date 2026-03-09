use std::sync::OnceLock;

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::any::{AnyPoolOptions, AnyQueryResult, AnyRow};
use sqlx::{Acquire, AnyPool, Row};
use uuid::Uuid;

use automate_shared::config::AutomationDef;
use automate_shared::models::{RunRecord, RunStatus};
use automate_shared::store::Store;

use crate::credentials_crypto::{decrypt, encrypt};

fn install_drivers() {
    static ONCE: OnceLock<()> = OnceLock::new();
    ONCE.get_or_init(|| {
        sqlx::any::install_default_drivers();
    });
}

#[derive(Clone)]
pub struct SqlxStore {
    pool: AnyPool,
}

impl SqlxStore {
    pub async fn connect(url: &str) -> Result<Self> {
        install_drivers();
        let pool = AnyPoolOptions::new()
            .max_connections(5)
            .connect(url)
            .await
            .context("Failed to connect to database")?;
        let store = Self { pool };
        store.run_migrations().await?;
        Ok(store)
    }

    pub async fn connect_in_memory() -> Result<Self> {
        install_drivers();
        // SQLite in-memory databases are per-connection, so we must limit to 1
        // connection to keep the schema visible across all queries.
        let pool = AnyPoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect("sqlite::memory:")
            .await
            .context("Failed to connect to in-memory database")?;
        let store = Self { pool };
        store.run_migrations().await?;
        Ok(store)
    }

    async fn run_migrations(&self) -> Result<()> {
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS automations (
                name TEXT PRIMARY KEY,
                trigger_def TEXT NOT NULL,
                auth_profile TEXT,
                prompt TEXT NOT NULL,
                file_path TEXT,
                created_at TEXT NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS runs (
                id TEXT PRIMARY KEY,
                automation_name TEXT NOT NULL,
                status TEXT NOT NULL,
                trigger_source TEXT NOT NULL,
                started_at TEXT NOT NULL,
                finished_at TEXT,
                output TEXT,
                error TEXT
            )",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query(
            "CREATE TABLE IF NOT EXISTS credentials (
                key TEXT PRIMARY KEY,
                encrypted_value BLOB NOT NULL,
                nonce BLOB NOT NULL
            )",
        )
        .execute(&self.pool)
        .await?;

        // Create indexes - use IF NOT EXISTS for idempotency
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_runs_started ON runs(started_at DESC)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_runs_automation ON runs(automation_name)")
            .execute(&self.pool)
            .await?;

        Ok(())
    }

    fn row_to_automation(row: &AnyRow) -> Result<AutomationDef> {
        let name: String = row.get("name");
        let trigger_json: String = row.get("trigger_def");
        let auth_profile: Option<String> = row.get("auth_profile");
        let prompt: String = row.get("prompt");
        let file_path: Option<String> = row.get("file_path");
        let trigger = serde_json::from_str(&trigger_json)?;
        Ok(AutomationDef {
            name,
            trigger,
            auth_profile,
            prompt,
            file: file_path.map(std::path::PathBuf::from),
        })
    }

    fn row_to_run(row: &AnyRow) -> Result<RunRecord> {
        let id: String = row.get("id");
        let automation_name: String = row.get("automation_name");
        let status_json: String = row.get("status");
        let trigger_source: String = row.get("trigger_source");
        let started_at: String = row.get("started_at");
        let finished_at: Option<String> = row.get("finished_at");
        let output: Option<String> = row.get("output");
        let error: Option<String> = row.get("error");

        let status: RunStatus = serde_json::from_str(&status_json)?;
        Ok(RunRecord {
            id: id.parse().unwrap_or_else(|_| Uuid::nil()),
            automation_name,
            status,
            trigger_source,
            started_at: started_at.parse().unwrap_or_else(|_| Utc::now()),
            finished_at: finished_at.and_then(|s: String| s.parse().ok()),
            output,
            error,
        })
    }
}

#[async_trait::async_trait]
impl Store for SqlxStore {
    // --- Automations ---

    async fn insert_automation(&self, def: &AutomationDef) -> Result<()> {
        let trigger_json = serde_json::to_string(&def.trigger)?;
        let file_path = def.file.as_ref().map(|p| p.to_string_lossy().to_string());
        let now = Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO automations (name, trigger_def, auth_profile, prompt, file_path, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&def.name)
        .bind(&trigger_json)
        .bind(&def.auth_profile)
        .bind(&def.prompt)
        .bind(&file_path)
        .bind(&now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn list_automations(&self) -> Result<Vec<AutomationDef>> {
        let rows: Vec<AnyRow> = sqlx::query(
            "SELECT name, trigger_def, auth_profile, prompt, file_path FROM automations ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await?;

        rows.iter().map(Self::row_to_automation).collect()
    }

    async fn get_automation(&self, name: &str) -> Result<Option<AutomationDef>> {
        let row: Option<AnyRow> = sqlx::query(
            "SELECT name, trigger_def, auth_profile, prompt, file_path FROM automations WHERE name=$1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await?;

        match row {
            Some(ref r) => Ok(Some(Self::row_to_automation(r)?)),
            None => Ok(None),
        }
    }

    async fn delete_automation(&self, name: &str) -> Result<bool> {
        let result: AnyQueryResult = sqlx::query("DELETE FROM automations WHERE name=$1")
            .bind(name)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    // --- Runs ---

    async fn insert_run(&self, run: &RunRecord) -> Result<()> {
        let status_json = serde_json::to_string(&run.status)?;
        sqlx::query(
            "INSERT INTO runs (id, automation_name, status, trigger_source, started_at, finished_at, output, error)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
        )
        .bind(run.id.to_string())
        .bind(&run.automation_name)
        .bind(&status_json)
        .bind(&run.trigger_source)
        .bind(run.started_at.to_rfc3339())
        .bind(run.finished_at.map(|t| t.to_rfc3339()))
        .bind(&run.output)
        .bind(&run.error)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    async fn update_run(&self, run: &RunRecord) -> Result<()> {
        let status_json = serde_json::to_string(&run.status)?;
        sqlx::query("UPDATE runs SET status=$1, finished_at=$2, output=$3, error=$4 WHERE id=$5")
            .bind(&status_json)
            .bind(run.finished_at.map(|t| t.to_rfc3339()))
            .bind(&run.output)
            .bind(&run.error)
            .bind(run.id.to_string())
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    async fn list_runs(&self, limit: Option<u32>) -> Result<Vec<RunRecord>> {
        let rows: Vec<AnyRow> = match limit {
            Some(n) => {
                sqlx::query(
                    "SELECT id, automation_name, status, trigger_source, started_at, finished_at, output, error FROM runs ORDER BY started_at DESC LIMIT $1",
                )
                .bind(n as i64)
                .fetch_all(&self.pool)
                .await?
            }
            None => {
                sqlx::query(
                    "SELECT id, automation_name, status, trigger_source, started_at, finished_at, output, error FROM runs ORDER BY started_at DESC",
                )
                .fetch_all(&self.pool)
                .await?
            }
        };

        rows.iter().map(Self::row_to_run).collect()
    }

    // --- Credentials ---

    async fn set_credential(&self, key: &str, value: &str) -> Result<()> {
        let (encrypted, nonce) = encrypt(value.as_bytes())?;
        // DELETE + INSERT in a transaction for atomicity (works for both SQLite and Postgres)
        let mut conn = self.pool.acquire().await?;
        let mut tx = conn.begin().await?;
        sqlx::query("DELETE FROM credentials WHERE key=$1")
            .bind(key)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO credentials (key, encrypted_value, nonce) VALUES ($1, $2, $3)")
            .bind(key)
            .bind(&encrypted)
            .bind(&nonce)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    async fn get_credential(&self, key: &str) -> Result<Option<String>> {
        let row: Option<AnyRow> =
            sqlx::query("SELECT encrypted_value, nonce FROM credentials WHERE key=$1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?;
        match row {
            Some(row) => {
                let encrypted: Vec<u8> = row.get("encrypted_value");
                let nonce: Vec<u8> = row.get("nonce");
                let decrypted = decrypt(&encrypted, &nonce)?;
                Ok(Some(String::from_utf8(decrypted)?))
            }
            None => Ok(None),
        }
    }

    async fn delete_credential(&self, key: &str) -> Result<bool> {
        let result: AnyQueryResult = sqlx::query("DELETE FROM credentials WHERE key=$1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    async fn list_credential_keys(&self) -> Result<Vec<String>> {
        let rows: Vec<AnyRow> = sqlx::query("SELECT key FROM credentials ORDER BY key")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.iter().map(|r: &AnyRow| r.get("key")).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use automate_shared::triggers::TriggerDef;

    async fn setup() -> SqlxStore {
        SqlxStore::connect_in_memory().await.unwrap()
    }

    fn make_automation(name: &str) -> AutomationDef {
        AutomationDef {
            name: name.to_string(),
            trigger: TriggerDef::Manual,
            auth_profile: None,
            prompt: "test prompt".to_string(),
            file: None,
        }
    }

    fn make_run(name: &str) -> RunRecord {
        RunRecord {
            id: Uuid::new_v4(),
            automation_name: name.to_string(),
            status: RunStatus::Pending,
            trigger_source: "manual".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            output: None,
            error: None,
        }
    }

    #[tokio::test]
    async fn test_insert_and_get_automation() {
        let store = setup().await;
        let auto = make_automation("test-auto");
        store.insert_automation(&auto).await.unwrap();

        let fetched = store.get_automation("test-auto").await.unwrap().unwrap();
        assert_eq!(fetched.name, "test-auto");
        assert_eq!(fetched.trigger, TriggerDef::Manual);
        assert_eq!(fetched.prompt, "test prompt");
    }

    #[tokio::test]
    async fn test_list_automations() {
        let store = setup().await;
        store
            .insert_automation(&make_automation("auto-1"))
            .await
            .unwrap();
        store
            .insert_automation(&make_automation("auto-2"))
            .await
            .unwrap();

        let list = store.list_automations().await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_delete_automation() {
        let store = setup().await;
        store
            .insert_automation(&make_automation("to-delete"))
            .await
            .unwrap();

        let deleted = store.delete_automation("to-delete").await.unwrap();
        assert!(deleted);

        let fetched = store.get_automation("to-delete").await.unwrap();
        assert!(fetched.is_none());
    }

    #[tokio::test]
    async fn test_delete_nonexistent_automation() {
        let store = setup().await;
        let deleted = store.delete_automation("no-such").await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn test_duplicate_automation_fails() {
        let store = setup().await;
        let auto = make_automation("dup");
        store.insert_automation(&auto).await.unwrap();
        assert!(store.insert_automation(&auto).await.is_err());
    }

    #[tokio::test]
    async fn test_automation_with_all_fields() {
        let store = setup().await;
        let auto = AutomationDef {
            name: "full".to_string(),
            trigger: TriggerDef::Cron("*/5 * * * *".to_string()),
            auth_profile: Some("prod".to_string()),
            prompt: "deploy it".to_string(),
            file: Some(std::path::PathBuf::from("deploy.sh")),
        };
        store.insert_automation(&auto).await.unwrap();

        let fetched = store.get_automation("full").await.unwrap().unwrap();
        assert_eq!(fetched.trigger, TriggerDef::Cron("*/5 * * * *".to_string()));
        assert_eq!(fetched.auth_profile, Some("prod".to_string()));
        assert_eq!(fetched.file, Some(std::path::PathBuf::from("deploy.sh")));
    }

    #[tokio::test]
    async fn test_get_nonexistent_automation() {
        let store = setup().await;
        let result = store.get_automation("nope").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_insert_and_list_runs() {
        let store = setup().await;
        let run = make_run("test-auto");
        store.insert_run(&run).await.unwrap();

        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].automation_name, "test-auto");
        assert_eq!(runs[0].status, RunStatus::Pending);
    }

    #[tokio::test]
    async fn test_update_run() {
        let store = setup().await;
        let mut run = make_run("test-auto");
        store.insert_run(&run).await.unwrap();

        run.status = RunStatus::Completed;
        run.finished_at = Some(Utc::now());
        run.output = Some("all done".to_string());
        store.update_run(&run).await.unwrap();

        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs[0].status, RunStatus::Completed);
        assert!(runs[0].finished_at.is_some());
        assert_eq!(runs[0].output, Some("all done".to_string()));
    }

    #[tokio::test]
    async fn test_run_with_error() {
        let store = setup().await;
        let mut run = make_run("fail-auto");
        run.status = RunStatus::Failed;
        run.error = Some("something broke".to_string());
        store.insert_run(&run).await.unwrap();

        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs[0].status, RunStatus::Failed);
        assert_eq!(runs[0].error, Some("something broke".to_string()));
    }

    #[tokio::test]
    async fn test_set_and_get_credential() {
        let store = setup().await;
        store
            .set_credential("ANTHROPIC_API_KEY", "sk-ant-test123")
            .await
            .unwrap();
        let val = store.get_credential("ANTHROPIC_API_KEY").await.unwrap();
        assert_eq!(val, Some("sk-ant-test123".to_string()));
    }

    #[tokio::test]
    async fn test_get_nonexistent_credential() {
        let store = setup().await;
        let val = store.get_credential("NOPE").await.unwrap();
        assert_eq!(val, None);
    }

    #[tokio::test]
    async fn test_overwrite_credential() {
        let store = setup().await;
        store.set_credential("KEY", "first").await.unwrap();
        store.set_credential("KEY", "second").await.unwrap();
        let val = store.get_credential("KEY").await.unwrap();
        assert_eq!(val, Some("second".to_string()));
    }

    #[tokio::test]
    async fn test_delete_credential() {
        let store = setup().await;
        store.set_credential("TO_DELETE", "value").await.unwrap();
        assert!(store.delete_credential("TO_DELETE").await.unwrap());
        assert_eq!(store.get_credential("TO_DELETE").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_delete_nonexistent_credential() {
        let store = setup().await;
        assert!(!store.delete_credential("NOPE").await.unwrap());
    }

    #[tokio::test]
    async fn test_list_credential_keys() {
        let store = setup().await;
        store.set_credential("B_KEY", "val").await.unwrap();
        store.set_credential("A_KEY", "val").await.unwrap();
        let keys = store.list_credential_keys().await.unwrap();
        assert_eq!(keys, vec!["A_KEY", "B_KEY"]);
    }

    #[tokio::test]
    async fn test_get_credentials_for_job_global() {
        let store = setup().await;
        store
            .set_credential("ANTHROPIC_API_KEY", "sk-global")
            .await
            .unwrap();
        let env = automate_shared::store::get_credentials_for_job(&store, "my-automation").await;
        assert_eq!(env.get("ANTHROPIC_API_KEY"), Some(&"sk-global".to_string()));
    }

    #[tokio::test]
    async fn test_get_credentials_for_job_prefixed_overrides_global() {
        let store = setup().await;
        store
            .set_credential("ANTHROPIC_API_KEY", "sk-global")
            .await
            .unwrap();
        store
            .set_credential("my-auto.ANTHROPIC_API_KEY", "sk-specific")
            .await
            .unwrap();
        let env = automate_shared::store::get_credentials_for_job(&store, "my-auto").await;
        assert_eq!(
            env.get("ANTHROPIC_API_KEY"),
            Some(&"sk-specific".to_string())
        );
    }

    #[tokio::test]
    async fn test_get_credentials_for_job_empty() {
        let store = setup().await;
        let env = automate_shared::store::get_credentials_for_job(&store, "no-creds").await;
        assert!(env.is_empty());
    }
}
