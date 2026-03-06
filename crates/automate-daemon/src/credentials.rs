use std::collections::HashMap;

use anyhow::Result;
use rusqlite::{params, Connection};
use tracing::warn;

const XOR_KEY: &[u8] = b"automate-dev-key-do-not-use-in-prod";

fn xor_encrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter()
        .zip(key.iter().cycle())
        .map(|(d, k)| d ^ k)
        .collect()
}

fn xor_decrypt(data: &[u8], key: &[u8]) -> Vec<u8> {
    xor_encrypt(data, key) // XOR is symmetric
}

pub fn init_credentials_table(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS credentials (
            key TEXT PRIMARY KEY,
            encrypted_value BLOB NOT NULL
        );",
    )?;
    Ok(())
}

pub fn set_credential(conn: &Connection, key: &str, value: &str) -> Result<()> {
    let encrypted = xor_encrypt(value.as_bytes(), XOR_KEY);
    conn.execute(
        "INSERT OR REPLACE INTO credentials (key, encrypted_value) VALUES (?1, ?2)",
        params![key, encrypted],
    )?;
    Ok(())
}

pub fn get_credential(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT encrypted_value FROM credentials WHERE key=?1")?;
    let mut rows = stmt.query_map(params![key], |row| row.get::<_, Vec<u8>>(0))?;
    match rows.next() {
        Some(Ok(encrypted)) => {
            let decrypted = xor_decrypt(&encrypted, XOR_KEY);
            Ok(Some(String::from_utf8(decrypted)?))
        }
        Some(Err(e)) => Err(e.into()),
        None => Ok(None),
    }
}

pub fn delete_credential(conn: &Connection, key: &str) -> Result<bool> {
    let rows = conn.execute("DELETE FROM credentials WHERE key=?1", params![key])?;
    Ok(rows > 0)
}

pub fn list_credential_keys(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare("SELECT key FROM credentials ORDER BY key")?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
    let mut keys = Vec::new();
    for row in rows {
        keys.push(row?);
    }
    Ok(keys)
}

pub fn get_credentials_for_job(
    conn: &Connection,
    automation_name: &str,
) -> HashMap<String, String> {
    let mut env = HashMap::new();

    // Try automation-specific credentials first, then fall back to global ones
    let prefixed_keys = [
        (
            format!("{}.ANTHROPIC_API_KEY", automation_name),
            "ANTHROPIC_API_KEY",
        ),
        (
            format!("{}.OPENAI_API_KEY", automation_name),
            "OPENAI_API_KEY",
        ),
        (
            format!("{}.AWS_ACCESS_KEY_ID", automation_name),
            "AWS_ACCESS_KEY_ID",
        ),
        (
            format!("{}.AWS_SECRET_ACCESS_KEY", automation_name),
            "AWS_SECRET_ACCESS_KEY",
        ),
        (
            format!("{}.AWS_DEFAULT_REGION", automation_name),
            "AWS_DEFAULT_REGION",
        ),
    ];

    let global_keys = [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_DEFAULT_REGION",
    ];

    // Check automation-specific keys
    for (prefixed_key, env_name) in &prefixed_keys {
        if let Ok(Some(val)) = get_credential(conn, prefixed_key) {
            env.insert(env_name.to_string(), val);
        }
    }

    // Fill in any missing from global keys
    for key in &global_keys {
        if !env.contains_key(*key) {
            if let Ok(Some(val)) = get_credential(conn, key) {
                env.insert(key.to_string(), val);
            }
        }
    }

    if env.is_empty() {
        warn!(automation = automation_name, "No credentials found for job");
    }

    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init_credentials_table(&conn).unwrap();
        conn
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let original = "my-secret-api-key-12345";
        let encrypted = xor_encrypt(original.as_bytes(), XOR_KEY);
        assert_ne!(encrypted, original.as_bytes());
        let decrypted = xor_decrypt(&encrypted, XOR_KEY);
        assert_eq!(String::from_utf8(decrypted).unwrap(), original);
    }

    #[test]
    fn set_and_get_credential() {
        let conn = setup();
        set_credential(&conn, "ANTHROPIC_API_KEY", "sk-ant-test123").unwrap();
        let val = get_credential(&conn, "ANTHROPIC_API_KEY").unwrap();
        assert_eq!(val, Some("sk-ant-test123".to_string()));
    }

    #[test]
    fn get_nonexistent_credential() {
        let conn = setup();
        let val = get_credential(&conn, "NOPE").unwrap();
        assert_eq!(val, None);
    }

    #[test]
    fn overwrite_credential() {
        let conn = setup();
        set_credential(&conn, "KEY", "first").unwrap();
        set_credential(&conn, "KEY", "second").unwrap();
        let val = get_credential(&conn, "KEY").unwrap();
        assert_eq!(val, Some("second".to_string()));
    }

    #[test]
    fn delete_credential() {
        let conn = setup();
        set_credential(&conn, "TO_DELETE", "value").unwrap();
        assert!(super::delete_credential(&conn, "TO_DELETE").unwrap());
        assert_eq!(get_credential(&conn, "TO_DELETE").unwrap(), None);
    }

    #[test]
    fn delete_nonexistent() {
        let conn = setup();
        assert!(!super::delete_credential(&conn, "NOPE").unwrap());
    }

    #[test]
    fn get_credentials_for_job_global() {
        let conn = setup();
        set_credential(&conn, "ANTHROPIC_API_KEY", "sk-global").unwrap();
        let env = get_credentials_for_job(&conn, "my-automation");
        assert_eq!(env.get("ANTHROPIC_API_KEY"), Some(&"sk-global".to_string()));
    }

    #[test]
    fn get_credentials_for_job_prefixed_overrides_global() {
        let conn = setup();
        set_credential(&conn, "ANTHROPIC_API_KEY", "sk-global").unwrap();
        set_credential(&conn, "my-auto.ANTHROPIC_API_KEY", "sk-specific").unwrap();
        let env = get_credentials_for_job(&conn, "my-auto");
        assert_eq!(
            env.get("ANTHROPIC_API_KEY"),
            Some(&"sk-specific".to_string())
        );
    }

    #[test]
    fn get_credentials_for_job_empty() {
        let conn = setup();
        let env = get_credentials_for_job(&conn, "no-creds");
        assert!(env.is_empty());
    }
}
