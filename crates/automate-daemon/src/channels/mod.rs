pub mod slack;
pub mod whatsapp;

use serde::{Deserialize, Serialize};
use tokio::task::JoinHandle;
use tracing::info;

use crate::job_queue::JobSender;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    pub bot_token: String,
    pub app_token: String,
    pub allowed_user_ids: Vec<String>,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppConfig {
    pub allowed_numbers: Vec<String>,
    pub enabled: bool,
}

pub struct ChannelManager {
    slack_handle: Option<JoinHandle<()>>,
    whatsapp_handle: Option<JoinHandle<()>>,
}

impl Default for ChannelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelManager {
    pub fn new() -> Self {
        Self {
            slack_handle: None,
            whatsapp_handle: None,
        }
    }

    pub fn start_slack(&mut self, config: SlackConfig, job_tx: JobSender) {
        if !config.enabled {
            info!("Slack channel disabled, skipping");
            return;
        }
        info!("Starting Slack channel");
        let handle = tokio::spawn(async move {
            slack::start_slack(config, job_tx).await;
        });
        self.slack_handle = Some(handle);
    }

    pub fn start_whatsapp(&mut self, config: WhatsAppConfig, job_tx: JobSender) {
        if !config.enabled {
            info!("WhatsApp channel disabled, skipping");
            return;
        }
        info!("Starting WhatsApp channel");
        let handle = tokio::spawn(async move {
            whatsapp::start_whatsapp(config, job_tx).await;
        });
        self.whatsapp_handle = Some(handle);
    }

    pub fn stop_slack(&mut self) {
        if let Some(handle) = self.slack_handle.take() {
            handle.abort();
            info!("Slack channel stopped");
        }
    }

    pub fn stop_whatsapp(&mut self) {
        if let Some(handle) = self.whatsapp_handle.take() {
            handle.abort();
            info!("WhatsApp channel stopped");
        }
    }

    pub fn stop_all(&mut self) {
        self.stop_slack();
        self.stop_whatsapp();
    }
}

impl Drop for ChannelManager {
    fn drop(&mut self) {
        self.stop_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_manager_new() {
        let mgr = ChannelManager::new();
        assert!(mgr.slack_handle.is_none());
        assert!(mgr.whatsapp_handle.is_none());
    }
}
