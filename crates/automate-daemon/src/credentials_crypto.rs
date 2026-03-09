use std::sync::OnceLock;

use aes_gcm::aead::{Aead, KeyInit, OsRng};
use aes_gcm::{Aes256Gcm, Nonce};
use anyhow::{Context, Result};
use rand::RngCore;
use sha2::Digest;

fn derive_key_inner() -> [u8; 32] {
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

fn derive_key() -> &'static [u8; 32] {
    static KEY: OnceLock<[u8; 32]> = OnceLock::new();
    KEY.get_or_init(derive_key_inner)
}

pub fn encrypt(data: &[u8]) -> Result<(Vec<u8>, Vec<u8>)> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(key).context("invalid key length")?;

    let mut nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, data)
        .map_err(|e| anyhow::anyhow!("encryption failed: {}", e))?;

    Ok((ciphertext, nonce_bytes.to_vec()))
}

pub fn decrypt(ciphertext: &[u8], nonce_bytes: &[u8]) -> Result<Vec<u8>> {
    let key = derive_key();
    let cipher = Aes256Gcm::new_from_slice(key).context("invalid key length")?;

    let nonce = Nonce::from_slice(nonce_bytes);
    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|e| anyhow::anyhow!("decryption failed: {}", e))?;

    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_decrypt_round_trip() {
        let original = "my-secret-api-key-12345";
        let (encrypted, nonce) = encrypt(original.as_bytes()).unwrap();
        assert_ne!(encrypted, original.as_bytes());
        let decrypted = decrypt(&encrypted, &nonce).unwrap();
        assert_eq!(String::from_utf8(decrypted).unwrap(), original);
    }
}
