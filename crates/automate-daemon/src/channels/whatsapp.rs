use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::{Mutex, Notify};
use tracing::{error, info, warn};

use crate::job_queue::{self, Job, JobSender};

use super::WhatsAppConfig;

/// Parsed event from the Node.js WhatsApp bridge (stdout JSON lines).
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "type")]
#[allow(dead_code)]
enum BridgeEvent {
    #[serde(rename = "started")]
    Started,
    #[serde(rename = "qr")]
    Qr { data: String },
    #[serde(rename = "authenticated")]
    Authenticated,
    #[serde(rename = "ready")]
    Ready,
    #[serde(rename = "message")]
    Message {
        from: String,
        body: String,
        timestamp: u64,
    },
    #[serde(rename = "disconnected")]
    Disconnected { reason: String },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Command sent to the bridge via stdin.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
enum BridgeCommand {
    #[serde(rename = "send")]
    Send { to: String, body: String },
    #[serde(rename = "shutdown")]
    Shutdown,
}

/// WhatsApp channel that spawns a Node.js bridge child process and communicates
/// via JSON lines over stdin/stdout.
pub struct WhatsAppChannel {
    config: WhatsAppConfig,
    job_tx: JobSender,
    shutdown: Arc<Notify>,
    /// Current QR code string (if any). Shared so the API can serve it.
    qr_code: Arc<Mutex<Option<String>>>,
    /// Handle to the bridge's stdin for sending commands.
    bridge_stdin: Arc<Mutex<Option<tokio::process::ChildStdin>>>,
}

impl WhatsAppChannel {
    pub fn new(config: WhatsAppConfig, job_tx: JobSender) -> Self {
        Self {
            config,
            job_tx,
            shutdown: Arc::new(Notify::new()),
            qr_code: Arc::new(Mutex::new(None)),
            bridge_stdin: Arc::new(Mutex::new(None)),
        }
    }

    /// Get a reference to the shared QR code state.
    pub fn qr_code(&self) -> Arc<Mutex<Option<String>>> {
        Arc::clone(&self.qr_code)
    }

    /// Send a reply message through the bridge.
    pub async fn send_reply(&self, to: &str, body: &str) -> Result<(), String> {
        let cmd = BridgeCommand::Send {
            to: to.to_string(),
            body: body.to_string(),
        };
        self.send_command(&cmd).await
    }

    /// Send a command to the bridge via stdin.
    async fn send_command(&self, cmd: &BridgeCommand) -> Result<(), String> {
        let mut stdin_guard = self.bridge_stdin.lock().await;
        let stdin = stdin_guard
            .as_mut()
            .ok_or_else(|| "Bridge not running".to_string())?;

        let mut line = serde_json::to_string(cmd).map_err(|e| e.to_string())?;
        line.push('\n');

        stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Failed to write to bridge stdin: {}", e))?;
        stdin
            .flush()
            .await
            .map_err(|e| format!("Failed to flush bridge stdin: {}", e))?;

        Ok(())
    }

    /// Start the WhatsApp channel. Returns when the channel is stopped or the
    /// connection loop exits.
    pub async fn start(&self) {
        info!("WhatsApp channel starting");

        let mut attempt: u32 = 0;

        loop {
            match self.connect_and_listen().await {
                Ok(()) => {
                    info!("WhatsApp connection closed cleanly");
                    break;
                }
                Err(e) => {
                    let backoff = job_queue::exponential_backoff(attempt, 2, 120);
                    warn!(
                        error = %e,
                        attempt = attempt,
                        backoff_secs = backoff.as_secs(),
                        "WhatsApp connection error, reconnecting"
                    );

                    // Wait for backoff or shutdown, whichever comes first
                    tokio::select! {
                        () = tokio::time::sleep(backoff) => {}
                        () = self.shutdown.notified() => {
                            info!("WhatsApp channel stopped during reconnection backoff");
                            break;
                        }
                    }

                    attempt = attempt.saturating_add(1);
                }
            }
        }

        info!("WhatsApp channel shut down");
    }

    /// Request the channel to stop gracefully.
    pub fn stop(&self) {
        info!("WhatsApp channel stop requested");
        self.shutdown.notify_waiters();
    }

    /// Spawn the Node.js bridge and process its JSON line events.
    async fn connect_and_listen(&self) -> Result<(), String> {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
        let bridge_script = format!("{}/.automate/whatsapp-bridge/index.js", home);

        info!(script = %bridge_script, "Spawning WhatsApp bridge");

        let mut child = Command::new("node")
            .arg(&bridge_script)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit()) // bridge logs go to daemon stderr
            .spawn()
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    "Node.js is not installed on this system. Install Node.js (v18+) to use the WhatsApp integration.".to_string()
                } else {
                    format!("Failed to spawn WhatsApp bridge: {}", e)
                }
            })?;

        // Take ownership of stdin/stdout
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| "Failed to capture bridge stdin".to_string())?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "Failed to capture bridge stdout".to_string())?;

        // Store stdin handle for sending commands
        {
            let mut guard = self.bridge_stdin.lock().await;
            *guard = Some(stdin);
        }

        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        let result = loop {
            tokio::select! {
                line_result = lines.next_line() => {
                    match line_result {
                        Ok(Some(line)) => {
                            if let Err(e) = self.handle_bridge_line(&line).await {
                                warn!(error = %e, "Error handling bridge event");
                            }
                        }
                        Ok(None) => {
                            // Bridge stdout closed (process exited)
                            break Err("Bridge process exited".to_string());
                        }
                        Err(e) => {
                            break Err(format!("Error reading bridge stdout: {}", e));
                        }
                    }
                }
                () = self.shutdown.notified() => {
                    info!("Shutdown requested, sending shutdown to bridge");
                    let _ = self.send_command(&BridgeCommand::Shutdown).await;
                    // Give the bridge a moment to shut down
                    let _ = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        child.wait(),
                    ).await;
                    break Ok(());
                }
            }
        };

        // Clean up stdin handle
        {
            let mut guard = self.bridge_stdin.lock().await;
            *guard = None;
        }

        result
    }

    /// Parse and handle a single JSON line from the bridge.
    async fn handle_bridge_line(&self, line: &str) -> Result<(), String> {
        let event: BridgeEvent =
            serde_json::from_str(line).map_err(|e| format!("Invalid bridge JSON: {}", e))?;

        match event {
            BridgeEvent::Started => {
                info!("WhatsApp bridge started");
            }
            BridgeEvent::Qr { data } => {
                info!("WhatsApp QR code received (length={})", data.len());
                let mut qr = self.qr_code.lock().await;
                *qr = Some(data);
            }
            BridgeEvent::Authenticated => {
                info!("WhatsApp session authenticated");
                // Clear QR since we're authenticated
                let mut qr = self.qr_code.lock().await;
                *qr = None;
            }
            BridgeEvent::Ready => {
                info!("WhatsApp client ready");
                let mut qr = self.qr_code.lock().await;
                *qr = None;
            }
            BridgeEvent::Message {
                from,
                body,
                timestamp: _,
            } => {
                process_incoming_message(&self.config, &from, &body, &self.job_tx).await;
            }
            BridgeEvent::Disconnected { reason } => {
                warn!(reason = %reason, "WhatsApp bridge disconnected");
                return Err(format!("Bridge disconnected: {}", reason));
            }
            BridgeEvent::Error { message } => {
                error!(message = %message, "WhatsApp bridge error");
            }
        }

        Ok(())
    }
}

/// Legacy entry point -- delegates to WhatsAppChannel for backwards compat
/// with ChannelManager::start_whatsapp.
pub async fn start_whatsapp(config: WhatsAppConfig, job_tx: JobSender) {
    let channel = WhatsAppChannel::new(config, job_tx);
    channel.start().await;
}

/// Check if a phone number is allowed by the allowlist.
/// An empty allowlist allows all numbers.
pub fn is_number_allowed(allowed_numbers: &[String], number: &str) -> bool {
    if allowed_numbers.is_empty() {
        return true;
    }
    allowed_numbers.iter().any(|n| n == number)
}

/// Process an incoming WhatsApp message -- checks the allowlist and enqueues a
/// job if the sender is permitted.
pub async fn process_incoming_message(
    config: &WhatsAppConfig,
    from_number: &str,
    text: &str,
    job_tx: &JobSender,
) {
    if text.is_empty() {
        return;
    }

    if !is_number_allowed(&config.allowed_numbers, from_number) {
        info!(from = %from_number, "Dropping message from non-allowlisted number");
        return;
    }

    info!(from = %from_number, "Processing WhatsApp message");

    let job = Job {
        automation_name: format!("whatsapp-{}", from_number),
        trigger_source: format!("whatsapp:{}", from_number),
        prompt: text.to_string(),
        env: std::collections::HashMap::new(),
        max_retries: crate::job_queue::DEFAULT_MAX_RETRIES,
        retry_count: 0,
    };

    if let Err(e) = job_tx.send((job, uuid::Uuid::new_v4())).await {
        error!(error = %e, "Failed to enqueue WhatsApp job");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_queue;

    #[test]
    fn test_allowlist_allows_listed_number() {
        let allowed = vec!["+1234567890".to_string()];
        assert!(is_number_allowed(&allowed, "+1234567890"));
    }

    #[test]
    fn test_allowlist_rejects_unlisted_number() {
        let allowed = vec!["+1234567890".to_string()];
        assert!(!is_number_allowed(&allowed, "+9999999999"));
    }

    #[test]
    fn test_empty_allowlist_allows_all() {
        let allowed: Vec<String> = vec![];
        assert!(is_number_allowed(&allowed, "+anything"));
    }

    #[test]
    fn test_allowlist_multiple_numbers() {
        let allowed = vec![
            "+1111111111".to_string(),
            "+2222222222".to_string(),
            "+3333333333".to_string(),
        ];
        assert!(is_number_allowed(&allowed, "+2222222222"));
        assert!(!is_number_allowed(&allowed, "+4444444444"));
    }

    #[tokio::test]
    async fn test_allowed_number_creates_job() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let config = WhatsAppConfig {
            allowed_numbers: vec!["+1111111111".to_string()],
            enabled: true,
        };

        process_incoming_message(&config, "+1111111111", "deploy it", &tx).await;

        let (job, _) = rx.recv().await.unwrap();
        assert_eq!(job.prompt, "deploy it");
        assert!(job.trigger_source.contains("whatsapp"));
    }

    #[tokio::test]
    async fn test_disallowed_number_dropped() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let config = WhatsAppConfig {
            allowed_numbers: vec!["+1111111111".to_string()],
            enabled: true,
        };

        process_incoming_message(&config, "+9999999999", "hack", &tx).await;

        drop(tx);
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn test_empty_text_ignored() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let config = WhatsAppConfig {
            allowed_numbers: vec![],
            enabled: true,
        };

        process_incoming_message(&config, "+1111111111", "", &tx).await;

        drop(tx);
        assert!(rx.recv().await.is_none());
    }

    #[tokio::test]
    async fn test_job_fields_correct() {
        let (tx, mut rx) = job_queue::create_channel(10);
        let config = WhatsAppConfig {
            allowed_numbers: vec![],
            enabled: true,
        };

        process_incoming_message(&config, "+5551234", "run backup", &tx).await;

        let (job, _) = rx.recv().await.unwrap();
        assert_eq!(job.automation_name, "whatsapp-+5551234");
        assert_eq!(job.trigger_source, "whatsapp:+5551234");
        assert_eq!(job.prompt, "run backup");
        assert_eq!(job.max_retries, job_queue::DEFAULT_MAX_RETRIES);
        assert_eq!(job.retry_count, 0);
    }

    #[tokio::test]
    async fn test_channel_stop_before_start() {
        let (tx, _rx) = job_queue::create_channel(10);
        let config = WhatsAppConfig {
            allowed_numbers: vec![],
            enabled: true,
        };

        let channel = WhatsAppChannel::new(config, tx);

        // Stop before starting -- should be a no-op (no panic)
        channel.stop();
    }

    #[tokio::test]
    async fn test_channel_start_and_stop() {
        let (tx, _rx) = job_queue::create_channel(10);
        let config = WhatsAppConfig {
            allowed_numbers: vec![],
            enabled: true,
        };

        let channel = Arc::new(WhatsAppChannel::new(config, tx));
        let channel_clone = Arc::clone(&channel);

        let handle = tokio::spawn(async move {
            channel_clone.start().await;
        });

        // Give the channel a moment to start
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Stop it
        channel.stop();

        // Should terminate within a reasonable time
        tokio::time::timeout(std::time::Duration::from_secs(5), handle)
            .await
            .expect("channel should stop within timeout")
            .expect("channel task should not panic");
    }

    #[test]
    fn test_parse_bridge_event_started() {
        let json = r#"{"type":"started"}"#;
        let event: BridgeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, BridgeEvent::Started));
    }

    #[test]
    fn test_parse_bridge_event_qr() {
        let json = r#"{"type":"qr","data":"some-qr-data"}"#;
        let event: BridgeEvent = serde_json::from_str(json).unwrap();
        match event {
            BridgeEvent::Qr { data } => assert_eq!(data, "some-qr-data"),
            _ => panic!("Expected Qr event"),
        }
    }

    #[test]
    fn test_parse_bridge_event_message() {
        let json = r#"{"type":"message","from":"15551234567@c.us","body":"hello","timestamp":1700000000}"#;
        let event: BridgeEvent = serde_json::from_str(json).unwrap();
        match event {
            BridgeEvent::Message {
                from,
                body,
                timestamp,
            } => {
                assert_eq!(from, "15551234567@c.us");
                assert_eq!(body, "hello");
                assert_eq!(timestamp, 1700000000);
            }
            _ => panic!("Expected Message event"),
        }
    }

    #[test]
    fn test_parse_bridge_event_disconnected() {
        let json = r#"{"type":"disconnected","reason":"logout"}"#;
        let event: BridgeEvent = serde_json::from_str(json).unwrap();
        match event {
            BridgeEvent::Disconnected { reason } => assert_eq!(reason, "logout"),
            _ => panic!("Expected Disconnected event"),
        }
    }

    #[test]
    fn test_parse_bridge_event_error() {
        let json = r#"{"type":"error","message":"something broke"}"#;
        let event: BridgeEvent = serde_json::from_str(json).unwrap();
        match event {
            BridgeEvent::Error { message } => assert_eq!(message, "something broke"),
            _ => panic!("Expected Error event"),
        }
    }

    #[test]
    fn test_parse_bridge_event_authenticated() {
        let json = r#"{"type":"authenticated"}"#;
        let event: BridgeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, BridgeEvent::Authenticated));
    }

    #[test]
    fn test_parse_bridge_event_ready() {
        let json = r#"{"type":"ready"}"#;
        let event: BridgeEvent = serde_json::from_str(json).unwrap();
        assert!(matches!(event, BridgeEvent::Ready));
    }

    #[test]
    fn test_serialize_bridge_command_send() {
        let cmd = BridgeCommand::Send {
            to: "15551234567@c.us".to_string(),
            body: "hello".to_string(),
        };
        let json = serde_json::to_string(&cmd).unwrap();
        assert!(json.contains(r#""type":"send""#));
        assert!(json.contains(r#""to":"15551234567@c.us""#));
        assert!(json.contains(r#""body":"hello""#));
    }

    #[test]
    fn test_serialize_bridge_command_shutdown() {
        let cmd = BridgeCommand::Shutdown;
        let json = serde_json::to_string(&cmd).unwrap();
        assert_eq!(json, r#"{"type":"shutdown"}"#);
    }
}
