use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use tokio_tungstenite::tungstenite::Message;
use tracing::{error, info, warn};

use crate::job_queue::{self, Job, JobSender};

use super::SlackConfig;

#[derive(Debug, Deserialize)]
struct SlackSocketEvent {
    #[serde(default)]
    envelope_id: Option<String>,
    #[serde(default)]
    payload: Option<SlackPayload>,
    #[serde(rename = "type", default)]
    #[allow(dead_code)]
    event_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SlackPayload {
    #[serde(default)]
    event: Option<SlackMessageEvent>,
}

#[derive(Debug, Deserialize)]
struct SlackMessageEvent {
    #[serde(rename = "type", default)]
    event_type: Option<String>,
    #[serde(default)]
    user: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    channel: Option<String>,
}

#[derive(Serialize)]
struct SocketAck {
    envelope_id: String,
}

pub async fn start_slack(config: SlackConfig, job_tx: JobSender) {
    info!("Slack channel starting socket mode connection");

    let mut attempt: u32 = 0;

    loop {
        match connect_and_listen(&config, &job_tx).await {
            Ok(()) => {
                info!("Slack socket connection closed cleanly");
                break;
            }
            Err(e) => {
                let backoff = job_queue::exponential_backoff(attempt, 1, 60);
                error!(
                    error = %e,
                    attempt = attempt,
                    backoff_secs = backoff.as_secs(),
                    "Slack socket connection error, reconnecting"
                );
                tokio::time::sleep(backoff).await;
                attempt = attempt.saturating_add(1);
            }
        }
    }
}

async fn connect_and_listen(config: &SlackConfig, job_tx: &JobSender) -> Result<(), String> {
    // Request WebSocket URL from Slack apps.connections.open
    let ws_url = request_ws_url(&config.app_token).await?;

    let (ws_stream, _) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .map_err(|e| format!("WebSocket connect failed: {}", e))?;

    info!("Connected to Slack socket mode");

    let (mut write, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        let msg = match msg {
            Ok(m) => m,
            Err(e) => {
                warn!(error = %e, "WebSocket read error");
                return Err(e.to_string());
            }
        };

        if let Message::Text(text) = msg {
            if let Ok(event) = serde_json::from_str::<SlackSocketEvent>(&text) {
                // Acknowledge the envelope
                if let Some(envelope_id) = &event.envelope_id {
                    let ack = serde_json::to_string(&SocketAck {
                        envelope_id: envelope_id.clone(),
                    })
                    .unwrap();
                    if let Err(e) = write.send(Message::Text(ack)).await {
                        warn!(error = %e, "Failed to send ack");
                    }
                }

                // Process message events
                if let Some(payload) = event.payload {
                    if let Some(msg_event) = payload.event {
                        handle_message_event(&config.allowed_user_ids, &msg_event, job_tx).await;
                    }
                }
            }
        }
    }

    Ok(())
}

async fn handle_message_event(
    allowed_user_ids: &[String],
    event: &SlackMessageEvent,
    job_tx: &JobSender,
) {
    // Only process "message" type events
    if event.event_type.as_deref() != Some("message") {
        return;
    }

    let user = match &event.user {
        Some(u) => u,
        None => return,
    };

    let text = match &event.text {
        Some(t) if !t.is_empty() => t,
        _ => return,
    };

    // Allowlist enforcement
    if !allowed_user_ids.is_empty() && !allowed_user_ids.contains(user) {
        info!(user = %user, "Dropping message from non-allowlisted user");
        return;
    }

    let channel = event.channel.clone().unwrap_or_default();
    info!(user = %user, channel = %channel, "Processing Slack message");

    let job = Job {
        automation_name: format!("slack-{}", channel),
        trigger_source: format!("slack:{}:{}", channel, user),
        prompt: text.clone(),
        env: std::collections::HashMap::new(),
        max_retries: job_queue::DEFAULT_MAX_RETRIES,
        retry_count: 0,
    };

    if let Err(e) = job_tx.send((job, uuid::Uuid::new_v4())).await {
        error!(error = %e, "Failed to enqueue Slack job");
    }
}

async fn request_ws_url(app_token: &str) -> Result<String, String> {
    let client = reqwest::Client::new();
    let resp = client
        .post("https://slack.com/api/apps.connections.open")
        .header("Authorization", format!("Bearer {}", app_token))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .send()
        .await
        .map_err(|e| format!("Failed to request WS URL: {}", e))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse WS URL response: {}", e))?;

    if body["ok"].as_bool() != Some(true) {
        return Err(format!(
            "Slack API error: {}",
            body["error"].as_str().unwrap_or("unknown")
        ));
    }

    body["url"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "No URL in response".to_string())
}

/// Process a raw message for testing purposes - checks allowlist and enqueues job.
pub async fn process_message_for_test(
    allowed_user_ids: &[String],
    user: &str,
    text: &str,
    channel: &str,
    job_tx: &JobSender,
) {
    let event = SlackMessageEvent {
        event_type: Some("message".to_string()),
        user: Some(user.to_string()),
        text: Some(text.to_string()),
        channel: Some(channel.to_string()),
    };
    handle_message_event(allowed_user_ids, &event, job_tx).await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_queue;

    #[tokio::test]
    async fn test_allowlisted_user_creates_job() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let allowed = vec!["U123".to_string()];

        process_message_for_test(&allowed, "U123", "deploy staging", "C001", &tx).await;

        let (job, _id) = rx.recv().await.unwrap();
        assert_eq!(job.prompt, "deploy staging");
        assert!(job.trigger_source.contains("slack"));
        assert!(job.trigger_source.contains("U123"));
    }

    #[tokio::test]
    async fn test_non_allowlisted_user_dropped() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let allowed = vec!["U123".to_string()];

        process_message_for_test(&allowed, "U999", "hack the planet", "C001", &tx).await;

        // Channel should be empty
        drop(tx);
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn test_empty_allowlist_allows_all() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let allowed: Vec<String> = vec![];

        process_message_for_test(&allowed, "UANY", "hello", "C001", &tx).await;

        let (job, _) = rx.recv().await.unwrap();
        assert_eq!(job.prompt, "hello");
    }

    #[tokio::test]
    async fn test_empty_text_ignored() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let allowed: Vec<String> = vec![];

        process_message_for_test(&allowed, "U123", "", "C001", &tx).await;

        drop(tx);
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn test_job_prompt_contains_message_text() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let allowed = vec!["U123".to_string()];

        process_message_for_test(
            &allowed,
            "U123",
            "run database migration on prod",
            "C001",
            &tx,
        )
        .await;

        let (job, _) = rx.recv().await.unwrap();
        assert_eq!(job.prompt, "run database migration on prod");
    }

    #[test]
    fn test_backoff_timing() {
        use std::time::Duration;
        // Verify the exponential backoff values used by Slack reconnection
        assert_eq!(
            job_queue::exponential_backoff(0, 1, 60),
            Duration::from_secs(1)
        );
        assert_eq!(
            job_queue::exponential_backoff(1, 1, 60),
            Duration::from_secs(2)
        );
        assert_eq!(
            job_queue::exponential_backoff(2, 1, 60),
            Duration::from_secs(4)
        );
        assert_eq!(
            job_queue::exponential_backoff(3, 1, 60),
            Duration::from_secs(8)
        );
        assert_eq!(
            job_queue::exponential_backoff(5, 1, 60),
            Duration::from_secs(32)
        );
        // Should cap at 60
        assert_eq!(
            job_queue::exponential_backoff(6, 1, 60),
            Duration::from_secs(60)
        );
        assert_eq!(
            job_queue::exponential_backoff(100, 1, 60),
            Duration::from_secs(60)
        );
    }
}
