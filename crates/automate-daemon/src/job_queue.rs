use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use tokio::sync::mpsc;
use tracing::{info, warn};
use uuid::Uuid;

use automate_shared::models::{RunRecord, RunStatus};
use automate_shared::store::Store;

use crate::agent::{AgentRuntime, RunOutput};
use crate::log_stream::LogStreamManager;

pub const DEFAULT_MAX_RETRIES: u32 = 3;

#[derive(Debug, Clone)]
pub struct Job {
    pub automation_name: String,
    pub trigger_source: String,
    pub prompt: String,
    pub env: HashMap<String, String>,
    pub max_retries: u32,
    pub retry_count: u32,
}

impl Job {
    pub fn new(automation_name: String, trigger_source: String, prompt: String) -> Self {
        Self {
            automation_name,
            trigger_source,
            prompt,
            env: HashMap::new(),
            max_retries: DEFAULT_MAX_RETRIES,
            retry_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct JobResult {
    pub job: Job,
    pub status: RunStatus,
    pub output: Option<String>,
    pub error: Option<String>,
    pub duration: std::time::Duration,
}

pub type JobSender = mpsc::Sender<(Job, Uuid)>;
pub type JobReceiver = mpsc::Receiver<(Job, Uuid)>;

pub fn create_channel(buffer: usize) -> (JobSender, JobReceiver) {
    mpsc::channel(buffer)
}

pub async fn run_consumer<S: Store>(rx: JobReceiver, store: S, runtime: Arc<dyn AgentRuntime>) {
    run_consumer_with_sender(rx, store, runtime, None, None).await
}

pub async fn run_consumer_with_log_stream<S: Store>(
    rx: JobReceiver,
    store: S,
    runtime: Arc<dyn AgentRuntime>,
    log_stream_mgr: Arc<LogStreamManager>,
) {
    run_consumer_with_sender(rx, store, runtime, None, Some(log_stream_mgr)).await
}

pub async fn run_consumer_with_sender<S: Store>(
    mut rx: JobReceiver,
    store: S,
    runtime: Arc<dyn AgentRuntime>,
    retry_tx: Option<JobSender>,
    log_stream_mgr: Option<Arc<LogStreamManager>>,
) {
    info!("Job queue consumer started");
    while let Some((job, run_id)) = rx.recv().await {
        info!(
            run_id = %run_id,
            automation = %job.automation_name,
            trigger = %job.trigger_source,
            retry = job.retry_count,
            "Processing job"
        );

        // Mark as running
        {
            let run = RunRecord {
                id: run_id,
                automation_name: job.automation_name.clone(),
                status: RunStatus::Running,
                trigger_source: job.trigger_source.clone(),
                started_at: Utc::now(),
                finished_at: None,
                output: None,
                error: None,
            };
            if let Err(e) = store.update_run(&run).await {
                warn!(error = %e, "Failed to update run status to running");
            }
        }

        // Get credentials for this job
        let mut env = job.env.clone();
        let creds =
            automate_shared::store::get_credentials_for_job(&store, &job.automation_name).await;
        env.extend(creds);

        // Interpolate prompt variables
        let prompt = job.prompt.replace("$TIMESTAMP", &Utc::now().to_rfc3339());

        let start = Instant::now();

        // Create log stream if manager is available
        let broadcaster = if let Some(ref mgr) = log_stream_mgr {
            Some(mgr.create_stream(&run_id.to_string()).await)
        } else {
            None
        };

        // Run the agent with streaming support
        let (status, output, error) = match runtime
            .run_streaming(&prompt, env, broadcaster.clone())
            .await
        {
            Ok(run_output) => {
                let combined_output = format_output(&run_output);
                write_log_file(run_id, &combined_output).await;

                if run_output.exit_code == 0 {
                    (RunStatus::Completed, Some(run_output.stdout), None)
                } else {
                    (
                        RunStatus::Failed,
                        Some(run_output.stdout),
                        Some(run_output.stderr),
                    )
                }
            }
            Err(e) => {
                let err_msg = e.to_string();
                write_log_file(run_id, &format!("Agent error: {}", err_msg)).await;
                (RunStatus::Failed, None, Some(err_msg))
            }
        };

        let duration = start.elapsed();
        info!(
            automation = %job.automation_name,
            status = ?status,
            duration_ms = duration.as_millis(),
            "Job completed"
        );

        // Record this attempt
        {
            let run = RunRecord {
                id: run_id,
                automation_name: job.automation_name.clone(),
                status: status.clone(),
                trigger_source: job.trigger_source.clone(),
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                output: output.clone(),
                error: error.clone(),
            };
            if let Err(e) = store.update_run(&run).await {
                warn!(error = %e, "Failed to update run record");
            }
        }

        // Close the log stream after job completion
        if let Some(ref mgr) = log_stream_mgr {
            mgr.close_stream(&run_id.to_string()).await;
        }

        // Retry logic: if failed and retries remaining, re-enqueue (only if retry_tx is available)
        if status == RunStatus::Failed && job.retry_count < job.max_retries && retry_tx.is_some() {
            let mut retried_job = job.clone();
            retried_job.retry_count += 1;
            let new_run_id = Uuid::new_v4();

            info!(
                automation = %retried_job.automation_name,
                retry = retried_job.retry_count,
                max_retries = retried_job.max_retries,
                "Retrying failed job"
            );

            // Insert a new pending run record for the retry
            {
                let run = RunRecord {
                    id: new_run_id,
                    automation_name: retried_job.automation_name.clone(),
                    status: RunStatus::Pending,
                    trigger_source: retried_job.trigger_source.clone(),
                    started_at: Utc::now(),
                    finished_at: None,
                    output: None,
                    error: None,
                };
                if let Err(e) = store.insert_run(&run).await {
                    warn!(error = %e, "Failed to insert retry run record");
                }
            }

            if let Some(ref tx) = retry_tx {
                if let Err(e) = tx.send((retried_job, new_run_id)).await {
                    warn!(error = %e, "Failed to re-enqueue retry job");
                }
            }
        } else if status == RunStatus::Failed {
            warn!(
                automation = %job.automation_name,
                retries = job.retry_count,
                "Job failed after all retries exhausted"
            );
        }
    }
    info!("Job queue consumer shutting down");
}

fn format_output(output: &RunOutput) -> String {
    let mut result = String::new();
    if !output.stdout.is_empty() {
        result.push_str("=== STDOUT ===\n");
        result.push_str(&output.stdout);
        result.push('\n');
    }
    if !output.stderr.is_empty() {
        result.push_str("=== STDERR ===\n");
        result.push_str(&output.stderr);
        result.push('\n');
    }
    result.push_str(&format!(
        "=== EXIT CODE: {} | DURATION: {:?} ===\n",
        output.exit_code, output.duration
    ));
    result
}

async fn write_log_file(run_id: Uuid, content: &str) {
    let content = content.to_string();
    let _ = tokio::task::spawn_blocking(move || {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let log_dir = std::path::Path::new(&home).join(".automate/logs");
        if std::fs::create_dir_all(&log_dir).is_ok() {
            let log_path = log_dir.join(format!("{}.log", run_id));
            if let Err(e) = std::fs::write(&log_path, &content) {
                warn!(error = %e, path = %log_path.display(), "Failed to write log file");
            }
        }
    })
    .await;
}

/// Calculate exponential backoff duration.
/// Returns `base * 2^attempt` capped at `max_secs`.
pub fn exponential_backoff(attempt: u32, base_secs: u64, max_secs: u64) -> std::time::Duration {
    let secs = base_secs.saturating_mul(1u64.checked_shl(attempt).unwrap_or(u64::MAX));
    std::time::Duration::from_secs(secs.min(max_secs))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::{AgentError, AgentRuntime};
    use crate::store_sqlx::SqlxStore;
    use std::time::Duration;

    struct MockRuntime {
        output: RunOutput,
    }

    #[async_trait::async_trait]
    impl AgentRuntime for MockRuntime {
        async fn check_installed(&self) -> bool {
            true
        }

        async fn run(
            &self,
            _prompt: &str,
            _env: HashMap<String, String>,
        ) -> Result<RunOutput, AgentError> {
            Ok(self.output.clone())
        }
    }

    struct FailingRuntime;

    #[async_trait::async_trait]
    impl AgentRuntime for FailingRuntime {
        async fn check_installed(&self) -> bool {
            false
        }

        async fn run(
            &self,
            _prompt: &str,
            _env: HashMap<String, String>,
        ) -> Result<RunOutput, AgentError> {
            Err(AgentError::NotInstalled)
        }
    }

    fn make_job(name: &str) -> Job {
        Job {
            automation_name: name.to_string(),
            trigger_source: "manual".to_string(),
            prompt: "test prompt".to_string(),
            env: HashMap::new(),
            max_retries: DEFAULT_MAX_RETRIES,
            retry_count: 0,
        }
    }

    #[tokio::test]
    async fn test_enqueue_dequeue() {
        let (tx, mut rx) = create_channel(10);
        let run_id = Uuid::new_v4();
        let job = make_job("test-auto");

        tx.send((job.clone(), run_id)).await.unwrap();

        let (received_job, received_id) = rx.recv().await.unwrap();
        assert_eq!(received_job.automation_name, "test-auto");
        assert_eq!(received_id, run_id);
    }

    #[tokio::test]
    async fn test_multiple_jobs() {
        let (tx, mut rx) = create_channel(10);

        for i in 0..3 {
            let job = make_job(&format!("auto-{}", i));
            tx.send((job, Uuid::new_v4())).await.unwrap();
        }

        for i in 0..3 {
            let (job, _) = rx.recv().await.unwrap();
            assert_eq!(job.automation_name, format!("auto-{}", i));
        }
    }

    #[tokio::test]
    async fn test_consumer_with_mock_agent() {
        let store = SqlxStore::connect_in_memory().await.unwrap();

        let mock_runtime = Arc::new(MockRuntime {
            output: RunOutput {
                stdout: "mock agent output".to_string(),
                stderr: String::new(),
                exit_code: 0,
                duration: Duration::from_millis(50),
            },
        });

        let (tx, rx) = create_channel(10);
        let run_id = Uuid::new_v4();
        let job = make_job("consumer-test");

        let run = RunRecord {
            id: run_id,
            automation_name: "consumer-test".to_string(),
            status: RunStatus::Pending,
            trigger_source: "manual".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            output: None,
            error: None,
        };
        store.insert_run(&run).await.unwrap();

        tx.send((job, run_id)).await.unwrap();
        drop(tx);

        run_consumer(rx, store.clone(), mock_runtime).await;

        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Completed);
        assert_eq!(runs[0].output, Some("mock agent output".to_string()));
    }

    #[tokio::test]
    async fn test_consumer_with_failing_agent() {
        let store = SqlxStore::connect_in_memory().await.unwrap();

        let failing_runtime = Arc::new(FailingRuntime);

        let (tx, rx) = create_channel(10);
        let run_id = Uuid::new_v4();
        let job = make_job("fail-test");

        let run = RunRecord {
            id: run_id,
            automation_name: "fail-test".to_string(),
            status: RunStatus::Pending,
            trigger_source: "manual".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            output: None,
            error: None,
        };
        store.insert_run(&run).await.unwrap();

        tx.send((job, run_id)).await.unwrap();
        drop(tx);

        run_consumer(rx, store.clone(), failing_runtime).await;

        let runs = store.list_runs(None).await.unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].status, RunStatus::Failed);
        assert!(runs[0].error.is_some());
    }

    #[tokio::test]
    async fn test_retry_on_failure() {
        let store = SqlxStore::connect_in_memory().await.unwrap();

        let failing_runtime = Arc::new(FailingRuntime);

        let (tx, rx) = create_channel(10);
        let (retry_tx, mut retry_rx) = create_channel(10);

        let run_id = Uuid::new_v4();
        let mut job = make_job("retry-test");
        job.max_retries = 2;
        job.retry_count = 0;

        let run = RunRecord {
            id: run_id,
            automation_name: "retry-test".to_string(),
            status: RunStatus::Pending,
            trigger_source: "manual".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            output: None,
            error: None,
        };
        store.insert_run(&run).await.unwrap();

        tx.send((job, run_id)).await.unwrap();
        drop(tx);

        run_consumer_with_sender(rx, store, failing_runtime, Some(retry_tx), None).await;

        let (retried_job, _new_run_id) = retry_rx.recv().await.unwrap();
        assert_eq!(retried_job.retry_count, 1);
        assert_eq!(retried_job.automation_name, "retry-test");
    }

    #[tokio::test]
    async fn test_no_retry_after_max() {
        let store = SqlxStore::connect_in_memory().await.unwrap();

        let failing_runtime = Arc::new(FailingRuntime);

        let (tx, rx) = create_channel(10);
        let (retry_tx, mut retry_rx) = create_channel(10);

        let run_id = Uuid::new_v4();
        let mut job = make_job("max-retry-test");
        job.max_retries = 2;
        job.retry_count = 2;

        let run = RunRecord {
            id: run_id,
            automation_name: "max-retry-test".to_string(),
            status: RunStatus::Pending,
            trigger_source: "manual".to_string(),
            started_at: Utc::now(),
            finished_at: None,
            output: None,
            error: None,
        };
        store.insert_run(&run).await.unwrap();

        tx.send((job, run_id)).await.unwrap();
        drop(tx);

        run_consumer_with_sender(rx, store, failing_runtime, Some(retry_tx), None).await;

        drop(retry_rx.try_recv().ok());
    }

    #[tokio::test]
    async fn test_channel_closed() {
        let (tx, mut rx) = create_channel(10);
        drop(tx);
        assert!(rx.recv().await.is_none());
    }

    #[test]
    fn test_exponential_backoff() {
        assert_eq!(exponential_backoff(0, 1, 60), Duration::from_secs(1));
        assert_eq!(exponential_backoff(1, 1, 60), Duration::from_secs(2));
        assert_eq!(exponential_backoff(2, 1, 60), Duration::from_secs(4));
        assert_eq!(exponential_backoff(3, 1, 60), Duration::from_secs(8));
        assert_eq!(exponential_backoff(6, 1, 60), Duration::from_secs(60));
        assert_eq!(exponential_backoff(10, 1, 60), Duration::from_secs(60));
    }

    #[test]
    fn test_job_new_defaults() {
        let job = Job::new(
            "test".to_string(),
            "manual".to_string(),
            "prompt".to_string(),
        );
        assert_eq!(job.max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(job.retry_count, 0);
    }
}
