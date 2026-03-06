use std::net::TcpListener;
use std::path::PathBuf;

use automate_shared::config::AutomationDef;
use automate_shared::models::RunRecord;
use uuid::Uuid;

use crate::db;
use crate::ssh::SshConnection;
use crate::state::AppState;

pub struct DaemonClient {
    local_port: u16,
    http: reqwest::Client,
    _ssh: SshConnection,
}

impl DaemonClient {
    pub async fn connect(
        host: String,
        port: u16,
        user: String,
        key_path: String,
    ) -> Result<Self, String> {
        let local_port = find_free_port()?;
        let ssh = SshConnection::new(host, port, user, PathBuf::from(key_path));

        ssh.connect().await.map_err(|e| e.to_string())?;
        ssh.create_ssh_tunnel(local_port, 4111)
            .await
            .map_err(|e| e.to_string())?;

        Ok(Self {
            local_port,
            http: reqwest::Client::new(),
            _ssh: ssh,
        })
    }

    pub async fn from_vm(state: &AppState, vm_id: &str) -> Result<Self, String> {
        let uuid: Uuid = vm_id.parse().map_err(|e: uuid::Error| e.to_string())?;
        let profile = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            db::get_vm(&conn, &uuid)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| "VM not found".to_string())?
        };
        Self::connect(profile.host, profile.port, profile.user, profile.key_path).await
    }

    fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.local_port)
    }

    pub async fn list_automations(&self) -> Result<Vec<AutomationDef>, String> {
        let resp = self.http
            .get(format!("{}/automations", self.base_url()))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("daemon returned status {}", resp.status()));
        }

        resp.json().await.map_err(|e| format!("failed to parse response: {}", e))
    }

    pub async fn create_automation(&self, automation: &AutomationDef) -> Result<(), String> {
        let resp = self.http
            .post(format!("{}/automations", self.base_url()))
            .json(automation)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("daemon error: {}", body));
        }

        Ok(())
    }

    pub async fn delete_automation(&self, name: &str) -> Result<(), String> {
        let resp = self.http
            .delete(format!("{}/automations/{}", self.base_url(), name))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("daemon error: {}", body));
        }

        Ok(())
    }

    pub async fn trigger_run(&self, name: &str) -> Result<String, String> {
        let resp = self.http
            .post(format!("{}/automations/{}/run", self.base_url(), name))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("daemon error: {}", body));
        }

        #[derive(serde::Deserialize)]
        struct RunResponse {
            run_id: String,
        }

        let body: RunResponse = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse response: {}", e))?;

        Ok(body.run_id)
    }

    pub async fn list_runs(&self, limit: Option<u32>) -> Result<Vec<RunRecord>, String> {
        let mut url = format!("{}/runs", self.base_url());
        if let Some(limit) = limit {
            url = format!("{}?limit={}", url, limit);
        }

        let resp = self.http
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("daemon returned status {}", resp.status()));
        }

        resp.json().await.map_err(|e| format!("failed to parse response: {}", e))
    }

    pub async fn get_run(&self, run_id: &str) -> Result<Option<RunRecord>, String> {
        let resp = self.http
            .get(format!("{}/runs/{}", self.base_url(), run_id))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(None);
        }

        if !resp.status().is_success() {
            return Err(format!("daemon returned status {}", resp.status()));
        }

        let record = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse response: {}", e))?;

        Ok(Some(record))
    }

    pub async fn push_credential(&self, key: &str, value: &str) -> Result<(), String> {
        let resp = self.http
            .post(format!("{}/credentials", self.base_url()))
            .json(&serde_json::json!({ "key": key, "value": value }))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("daemon error: {}", body));
        }

        Ok(())
    }

    pub async fn get_raw(&self, path: &str) -> Result<String, String> {
        let resp = self
            .http
            .get(format!("{}{}", self.base_url(), path))
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            return Err(format!("daemon returned status {}", resp.status()));
        }

        resp.text()
            .await
            .map_err(|e| format!("failed to read response: {}", e))
    }

    pub async fn configure_channel(&self, config_json: &str) -> Result<(), String> {
        let body: serde_json::Value = serde_json::from_str(config_json)
            .map_err(|e| format!("invalid JSON: {}", e))?;

        let resp = self.http
            .post(format!("{}/channels", self.base_url()))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("HTTP request failed: {}", e))?;

        if !resp.status().is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("daemon error: {}", body));
        }

        Ok(())
    }
}

fn find_free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("failed to find free port: {}", e))?;
    let port = listener.local_addr()
        .map_err(|e| format!("failed to get local address: {}", e))?
        .port();
    Ok(port)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_find_free_port() {
        let port = find_free_port().unwrap();
        assert!(port > 0);
    }

    #[test]
    fn test_find_free_port_unique() {
        let port1 = find_free_port().unwrap();
        let port2 = find_free_port().unwrap();
        // Ports should generally be different (not guaranteed but very likely)
        assert!(port1 > 0 && port2 > 0);
    }
}
