use tracing::{error, info};

use crate::job_queue::{Job, JobSender};

use super::WhatsAppConfig;

pub async fn start_whatsapp(config: WhatsAppConfig, job_tx: JobSender) {
    info!("WhatsApp channel starting (stub implementation)");

    // WhatsApp integration is a stub -- a real implementation would use
    // a WhatsApp Business API client or a library like whatsapp-web.rs.
    // For now, this just logs and waits to be cancelled.
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        info!("WhatsApp channel heartbeat (stub)");

        // In a real implementation this loop would receive incoming messages,
        // check the allowlist, and enqueue jobs just like the Slack channel.
        let _ = (&config, &job_tx);
    }
}

/// Check if a phone number is allowed by the allowlist.
/// An empty allowlist allows all numbers.
pub fn is_number_allowed(allowed_numbers: &[String], number: &str) -> bool {
    if allowed_numbers.is_empty() {
        return true;
    }
    allowed_numbers.iter().any(|n| n == number)
}

/// Process an incoming WhatsApp message (for use by a real implementation or tests).
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
}
