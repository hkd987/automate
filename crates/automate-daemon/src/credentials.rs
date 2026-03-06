use std::collections::HashMap;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use rand::RngCore;
use rusqlite::{params, Connection};
use sha2::Digest;
use tracing::warn;

fn derive_key() -> [u8; 32] {
    if let Ok(key_str) = std::env::var("AUTOMATE_ENCRYPTION_KEY") {
        let mut hasher = sha2::Sha256::new();
        hasher.update(key_str.as_bytes());
        hasher.finalize().into()
    } else {
        // Fallback: derive from hostname + username for dev
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "localhost".to_string());
        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "default".to_string());
        let mut hasher = sha2::Sha256::new();
        hasher.update(format!("automate:{}:{}", hostname, username).as_bytes());
        hasher.finalize().into()
    }
}

fn encrypt(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key).context("invalid key length")?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| anyhow::anyhow!("encryption failed: {}", e))?;

    Ok((ciphertext, nonce_bytes.to_vec()))
}

fn decrypt(ciphertext: &[u8], nonce_bytes: &[u8]) -> Result<Vec<u8>> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(&key).context("invalid key length")?;

    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed: {}", e))?;

    Ok(plaintext)
}

pub fn set_credential(conn: &Connection, key: &str, value: &str) -> Result<()> {
    let (encrypted, nonce) = encrypt(value.as_bytes())?;
    conn.execute(
        "INSERT OR REPLACE INTO credentials (key, encrypted_value, nonce) VALUES (?1, ?2, ?3)",
        params![key, encrypted, nonce],
    )?;
    Ok(())
}

pub fn get_credential(conn: &Connection, key: &str) -> Result<Option<String>> {
    let mut stmt = conn.prepare("SELECT encrypted_value, nonce FROM credentials WHERE key=?1")?;
    let mut rows = stmt.query_map(params![key], |row| {
        Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
    })?;
    match rows.next() {
        Some(Ok((encrypted, nonce))) => {
            let decrypted = decrypt(&encrypted, &nonce)?;
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

    let global_keys = [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "AWS_ACCESS_KEY_ID",
        "AWS_SECRET_ACCESS_KEY",
        "AWS_DEFAULT_REGION",
    ];

    // Fetch all credentials in one query and filter in memory
    let all_creds = match list_all_credentials(conn) {
        Ok(creds) => creds,
        Err(e) => {
            warn!(automation = automation_name, error = %e, "Failed to fetch credentials");
            return env;
        }
    };

    // Check automation-specific keys first
    for key in &global_keys {
        let prefixed = format!("{}.{}", automation_name, key);
        if let Some(val) = all_creds.get(&prefixed) {
            env.insert(key.to_string(), val.clone());
        }
    }

    // Fill in any missing from global keys
    for key in &global_keys {
        if !env.contains_key(*key) {
            if let Some(val) = all_creds.get(*key) {
                env.insert(key.to_string(), val.clone());
            }
        }
    }

    if env.is_empty() {
        warn!(automation = automation_name, "No credentials found for job");
    }

    env
}

fn list_all_credentials(conn: &Connection) -> Result<HashMap<String, String>> {
    let mut stmt = conn.prepare("SELECT key, encrypted_value, nonce FROM credentials")?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, Vec<u8>>(1)?,
            row.get::<_, Vec<u8>>(2)?,
        ))
    })?;
    let mut creds = HashMap::new();
    for row in rows {
        let (key, encrypted, nonce) = row?;
        if let Ok(decrypted) = decrypt(&encrypted, &nonce) {
            if let Ok(val) = String::from_utf8(decrypted) {
                creds.insert(key, val);
            }
        }
    }
    Ok(creds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db;

    fn setup() -> Connection {
        db::init_db_in_memory().unwrap()
    }

    #[test]
    fn encrypt_decrypt_round_trip() {
        let original = "my-secret-api-key-12345";
        let (encrypted, nonce) = encrypt(original.as_bytes()).unwrap();
        assert_ne!(encrypted, original.as_bytes());
        let decrypted = decrypt(&encrypted, &nonce).unwrap();
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
