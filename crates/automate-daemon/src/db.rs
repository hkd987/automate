use anyhow::Result;
use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use automate_shared::config::AutomationDef;
use automate_shared::models::{RunRecord, RunStatus};

pub fn init_db(path: &str) -> Result<Connection> {
    let conn = Connection::open(path)?;
    run_migrations(&conn)?;
    Ok(conn)
}

pub fn init_db_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    run_migrations(&conn)?;
    Ok(conn)
}

fn run_migrations(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS automations (
            name TEXT PRIMARY KEY,
            trigger_def TEXT NOT NULL,
            auth_profile TEXT,
            prompt TEXT NOT NULL,
            file_path TEXT,
            created_at TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS runs (
            id TEXT PRIMARY KEY,
            automation_name TEXT NOT NULL,
            status TEXT NOT NULL,
            trigger_source TEXT NOT NULL,
            started_at TEXT NOT NULL,
            finished_at TEXT,
            output TEXT,
            error TEXT
        );
        CREATE TABLE IF NOT EXISTS credentials (
            key TEXT PRIMARY KEY,
            encrypted_value BLOB NOT NULL
        );",
    )?;
    Ok(())
}

// --- Automation CRUD ---

pub fn insert_automation(conn: &Connection, def: &AutomationDef) -> Result<()> {
    let trigger_json = serde_json::to_string(&def.trigger)?;
    let file_path = def.file.as_ref().map(|p| p.to_string_lossy().to_string());
    let now = Utc::now().to_rfc3339();
    conn.execute(
        "INSERT INTO automations (name, trigger_def, auth_profile, prompt, file_path, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            def.name,
            trigger_json,
            def.auth_profile,
            def.prompt,
            file_path,
            now
        ],
    )?;
    Ok(())
}

pub fn list_automations(conn: &Connection) -> Result<Vec<AutomationDef>> {
    let mut stmt = conn.prepare(
        "SELECT name, trigger_def, auth_profile, prompt, file_path FROM automations ORDER BY created_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let trigger_json: String = row.get(1)?;
        let auth_profile: Option<String> = row.get(2)?;
        let file_path: Option<String> = row.get(4)?;
        Ok((
            row.get::<_, String>(0)?,
            trigger_json,
            auth_profile,
            row.get::<_, String>(3)?,
            file_path,
        ))
    })?;
    let mut automations = Vec::new();
    for row in rows {
        let (name, trigger_json, auth_profile, prompt, file_path) = row?;
        let trigger = serde_json::from_str(&trigger_json)?;
        automations.push(AutomationDef {
            name,
            trigger,
            auth_profile,
            prompt,
            file: file_path.map(std::path::PathBuf::from),
        });
    }
    Ok(automations)
}

pub fn get_automation(conn: &Connection, name: &str) -> Result<Option<AutomationDef>> {
    let mut stmt = conn.prepare(
        "SELECT name, trigger_def, auth_profile, prompt, file_path FROM automations WHERE name=?1",
    )?;
    let mut rows = stmt.query_map(params![name], |row| {
        let trigger_json: String = row.get(1)?;
        let auth_profile: Option<String> = row.get(2)?;
        let file_path: Option<String> = row.get(4)?;
        Ok((
            row.get::<_, String>(0)?,
            trigger_json,
            auth_profile,
            row.get::<_, String>(3)?,
            file_path,
        ))
    })?;
    match rows.next() {
        Some(row) => {
            let (name, trigger_json, auth_profile, prompt, file_path) = row?;
            let trigger = serde_json::from_str(&trigger_json)?;
            Ok(Some(AutomationDef {
                name,
                trigger,
                auth_profile,
                prompt,
                file: file_path.map(std::path::PathBuf::from),
            }))
        }
        None => Ok(None),
    }
}

pub fn delete_automation(conn: &Connection, name: &str) -> Result<bool> {
    let rows = conn.execute("DELETE FROM automations WHERE name=?1", params![name])?;
    Ok(rows > 0)
}

// --- Run CRUD ---

pub fn insert_run(conn: &Connection, run: &RunRecord) -> Result<()> {
    let status_json = serde_json::to_string(&run.status)?;
    conn.execute(
        "INSERT INTO runs (id, automation_name, status, trigger_source, started_at, finished_at, output, error)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![
            run.id.to_string(),
            run.automation_name,
            status_json,
            run.trigger_source,
            run.started_at.to_rfc3339(),
            run.finished_at.map(|t| t.to_rfc3339()),
            run.output,
            run.error,
        ],
    )?;
    Ok(())
}

pub fn update_run(conn: &Connection, run: &RunRecord) -> Result<()> {
    let status_json = serde_json::to_string(&run.status)?;
    conn.execute(
        "UPDATE runs SET status=?1, finished_at=?2, output=?3, error=?4 WHERE id=?5",
        params![
            status_json,
            run.finished_at.map(|t| t.to_rfc3339()),
            run.output,
            run.error,
            run.id.to_string(),
        ],
    )?;
    Ok(())
}

pub fn list_runs(conn: &Connection) -> Result<Vec<RunRecord>> {
    let mut stmt = conn.prepare(
        "SELECT id, automation_name, status, trigger_source, started_at, finished_at, output, error FROM runs ORDER BY started_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut runs = Vec::new();
    for row in rows {
        let (
            id,
            automation_name,
            status_json,
            trigger_source,
            started_at,
            finished_at,
            output,
            error,
        ) = row?;
        let status: RunStatus = serde_json::from_str(&status_json)?;
        runs.push(RunRecord {
            id: id.parse().unwrap_or_else(|_| Uuid::nil()),
            automation_name,
            status,
            trigger_source,
            started_at: started_at.parse().unwrap_or_else(|_| Utc::now()),
            finished_at: finished_at.and_then(|s| s.parse().ok()),
            output,
            error,
        });
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use automate_shared::triggers::TriggerDef;

    fn setup() -> Connection {
        init_db_in_memory().unwrap()
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

    #[test]
    fn test_insert_and_get_automation() {
        let conn = setup();
        let auto = make_automation("test-auto");
        insert_automation(&conn, &auto).unwrap();

        let fetched = get_automation(&conn, "test-auto").unwrap().unwrap();
        assert_eq!(fetched.name, "test-auto");
        assert_eq!(fetched.trigger, TriggerDef::Manual);
        assert_eq!(fetched.prompt, "test prompt");
    }

    #[test]
    fn test_list_automations() {
        let conn = setup();
        insert_automation(&conn, &make_automation("auto-1")).unwrap();
        insert_automation(&conn, &make_automation("auto-2")).unwrap();

        let list = list_automations(&conn).unwrap();
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_delete_automation() {
        let conn = setup();
        insert_automation(&conn, &make_automation("to-delete")).unwrap();

        let deleted = delete_automation(&conn, "to-delete").unwrap();
        assert!(deleted);

        let fetched = get_automation(&conn, "to-delete").unwrap();
        assert!(fetched.is_none());
    }

    #[test]
    fn test_delete_nonexistent_automation() {
        let conn = setup();
        let deleted = delete_automation(&conn, "no-such").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_duplicate_automation_fails() {
        let conn = setup();
        let auto = make_automation("dup");
        insert_automation(&conn, &auto).unwrap();
        assert!(insert_automation(&conn, &auto).is_err());
    }

    #[test]
    fn test_automation_with_all_fields() {
        let conn = setup();
        let auto = AutomationDef {
            name: "full".to_string(),
            trigger: TriggerDef::Cron("*/5 * * * *".to_string()),
            auth_profile: Some("prod".to_string()),
            prompt: "deploy it".to_string(),
            file: Some(std::path::PathBuf::from("deploy.sh")),
        };
        insert_automation(&conn, &auto).unwrap();

        let fetched = get_automation(&conn, "full").unwrap().unwrap();
        assert_eq!(fetched.trigger, TriggerDef::Cron("*/5 * * * *".to_string()));
        assert_eq!(fetched.auth_profile, Some("prod".to_string()));
        assert_eq!(fetched.file, Some(std::path::PathBuf::from("deploy.sh")));
    }

    #[test]
    fn test_get_nonexistent_automation() {
        let conn = setup();
        let result = get_automation(&conn, "nope").unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_insert_and_list_runs() {
        let conn = setup();
        let run = make_run("test-auto");
        insert_run(&conn, &run).unwrap();

        let runs = list_runs(&conn).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].automation_name, "test-auto");
        assert_eq!(runs[0].status, RunStatus::Pending);
    }

    #[test]
    fn test_update_run() {
        let conn = setup();
        let mut run = make_run("test-auto");
        insert_run(&conn, &run).unwrap();

        run.status = RunStatus::Completed;
        run.finished_at = Some(Utc::now());
        run.output = Some("all done".to_string());
        update_run(&conn, &run).unwrap();

        let runs = list_runs(&conn).unwrap();
        assert_eq!(runs[0].status, RunStatus::Completed);
        assert!(runs[0].finished_at.is_some());
        assert_eq!(runs[0].output, Some("all done".to_string()));
    }

    #[test]
    fn test_run_with_error() {
        let conn = setup();
        let mut run = make_run("fail-auto");
        run.status = RunStatus::Failed;
        run.error = Some("something broke".to_string());
        insert_run(&conn, &run).unwrap();

        let runs = list_runs(&conn).unwrap();
        assert_eq!(runs[0].status, RunStatus::Failed);
        assert_eq!(runs[0].error, Some("something broke".to_string()));
    }
}
