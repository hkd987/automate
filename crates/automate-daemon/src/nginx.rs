use std::path::Path;

use anyhow::Result;
use tracing::info;

pub fn generate_server_block(domain: &str, webhook_names: &[String], listen_port: u16) -> String {
    let mut locations = String::new();
    for name in webhook_names {
        locations.push_str(&format!(
            r#"
    location /hooks/{name} {{
        proxy_pass http://127.0.0.1:{listen_port}/hooks/{name};
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $proxy_add_x_forwarded_for;
        proxy_set_header X-Signature-256 $http_x_signature_256;
    }}
"#,
        ));
    }

    format!(
        r#"server {{
    listen 443 ssl;
    server_name {domain};

    ssl_certificate /etc/letsencrypt/live/{domain}/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/{domain}/privkey.pem;
{locations}}}
"#,
    )
}

pub fn write_config(path: &Path, content: &str) -> Result<()> {
    std::fs::write(path, content)?;
    info!(path = %path.display(), "Wrote nginx config");
    Ok(())
}

pub fn reload_nginx() -> Result<()> {
    let output = std::process::Command::new("nginx")
        .arg("-s")
        .arg("reload")
        .output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("nginx reload failed: {}", stderr);
    }
    info!("Nginx reloaded");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_server_block_basic() {
        let names = vec!["deploy".to_string(), "ci".to_string()];
        let config = generate_server_block("automate.example.com", &names, 4111);

        assert!(config.contains("server_name automate.example.com;"));
        assert!(config.contains("listen 443 ssl;"));
        assert!(config.contains("location /hooks/deploy"));
        assert!(config.contains("location /hooks/ci"));
        assert!(config.contains("proxy_pass http://127.0.0.1:4111/hooks/deploy"));
        assert!(config.contains("proxy_pass http://127.0.0.1:4111/hooks/ci"));
        assert!(config.contains("X-Signature-256"));
    }

    #[test]
    fn test_generate_server_block_empty_webhooks() {
        let config = generate_server_block("example.com", &[], 4111);
        assert!(config.contains("server_name example.com;"));
        assert!(!config.contains("location /hooks/"));
    }

    #[test]
    fn test_generate_server_block_contains_ssl() {
        let config = generate_server_block("test.com", &[], 4111);
        assert!(config.contains("ssl_certificate"));
        assert!(config.contains("ssl_certificate_key"));
        assert!(config.contains("/etc/letsencrypt/live/test.com/"));
    }

    #[test]
    fn test_write_config() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.conf");
        write_config(&path, "server { }").unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "server { }");
    }
}
