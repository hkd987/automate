use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_cron_scheduler::{Job, JobScheduler};
use tracing::{error, info};
use uuid::Uuid;

use crate::job_queue::{self, JobSender};

pub struct Scheduler {
    sched: JobScheduler,
    jobs: Arc<Mutex<HashMap<String, uuid::Uuid>>>,
}

impl Scheduler {
    pub async fn new() -> anyhow::Result<Self> {
        let sched = JobScheduler::new().await?;
        sched.start().await?;
        Ok(Self {
            sched,
            jobs: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub async fn register_cron(
        &self,
        name: String,
        cron_expression: &str,
        prompt: String,
        job_sender: JobSender,
    ) -> anyhow::Result<()> {
        let automation_name = name.clone();
        let job = Job::new_async(cron_expression, move |_uuid, _lock| {
            let sender = job_sender.clone();
            let auto_name = automation_name.clone();
            let auto_prompt = prompt.clone();
            Box::pin(async move {
                let run_id = Uuid::new_v4();
                let job = job_queue::Job {
                    automation_name: auto_name.clone(),
                    trigger_source: "cron".to_string(),
                    prompt: auto_prompt,
                    env: HashMap::new(),
                    max_retries: job_queue::DEFAULT_MAX_RETRIES,
                    retry_count: 0,
                };
                if let Err(e) = sender.send((job, run_id)).await {
                    error!(automation = %auto_name, error = %e, "Failed to enqueue cron job");
                } else {
                    info!(automation = %auto_name, run_id = %run_id, "Cron job enqueued");
                }
            })
        })?;

        let job_id = self.sched.add(job).await?;
        self.jobs.lock().await.insert(name.clone(), job_id);
        info!(automation = %name, "Cron job registered");
        Ok(())
    }

    pub async fn remove_cron(&self, name: &str) -> anyhow::Result<bool> {
        let mut jobs = self.jobs.lock().await;
        if let Some(job_id) = jobs.remove(name) {
            self.sched.remove(&job_id).await?;
            info!(automation = %name, "Cron job removed");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub async fn restore_all(
        &self,
        automations: Vec<(String, String, String)>, // (name, cron_expr, prompt)
        job_sender: JobSender,
    ) -> anyhow::Result<()> {
        for (name, cron_expr, prompt) in automations {
            if let Err(e) = self
                .register_cron(name.clone(), &cron_expr, prompt, job_sender.clone())
                .await
            {
                error!(automation = %name, error = %e, "Failed to restore cron job");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job_queue;

    #[tokio::test]
    async fn test_cron_fires_job() {
        let scheduler = Scheduler::new().await.unwrap();
        let (tx, mut rx) = job_queue::create_channel(10);

        // Every second
        scheduler
            .register_cron(
                "test-cron".to_string(),
                "* * * * * *",
                "test prompt".to_string(),
                tx,
            )
            .await
            .unwrap();

        // Wait up to 3 seconds for a job
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await;
        assert!(
            result.is_ok(),
            "Should have received a job within 3 seconds"
        );
        let (job, _run_id) = result.unwrap().unwrap();
        assert_eq!(job.automation_name, "test-cron");
        assert_eq!(job.trigger_source, "cron");
        assert_eq!(job.prompt, "test prompt");
    }

    #[tokio::test]
    async fn test_remove_cron_stops_jobs() {
        let scheduler = Scheduler::new().await.unwrap();
        let (tx, mut rx) = job_queue::create_channel(100);

        scheduler
            .register_cron(
                "remove-test".to_string(),
                "* * * * * *",
                "prompt".to_string(),
                tx,
            )
            .await
            .unwrap();

        // Wait for first job to confirm it's working
        let result = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await;
        assert!(result.is_ok(), "Cron should fire at least once");

        // Remove the cron
        let removed = scheduler.remove_cron("remove-test").await.unwrap();
        assert!(removed);

        // Verify it's removed from our registry (cannot re-remove)
        let removed_again = scheduler.remove_cron("remove-test").await.unwrap();
        assert!(!removed_again, "Second remove should return false");

        // Count jobs over a window: before removal we'd get ~1/sec.
        // After removal, we should see at most a few in-flight, then stop.
        // Drain everything that arrived during the remove call.
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        let mut count_after = 0;
        while rx.try_recv().is_ok() {
            count_after += 1;
        }
        // There may be 1-2 in-flight jobs, but we should not see a continuous stream.
        // Wait another 3 seconds - if cron were still running we'd get ~3 more.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
        let mut count_final = 0;
        while rx.try_recv().is_ok() {
            count_final += 1;
        }
        // After the initial drain, no new jobs should appear
        assert_eq!(
            count_final, 0,
            "Should not receive jobs 1.5s+ after removal (got {} during drain, {} after)",
            count_after, count_final
        );
    }

    #[tokio::test]
    async fn test_remove_nonexistent_cron() {
        let scheduler = Scheduler::new().await.unwrap();
        let removed = scheduler.remove_cron("no-such").await.unwrap();
        assert!(!removed);
    }

    #[tokio::test]
    async fn test_restore_all() {
        let scheduler = Scheduler::new().await.unwrap();
        let (tx, mut rx) = job_queue::create_channel(10);

        let automations = vec![(
            "restored".to_string(),
            "* * * * * *".to_string(),
            "restored prompt".to_string(),
        )];

        scheduler.restore_all(automations, tx).await.unwrap();

        let result = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await;
        assert!(result.is_ok());
        let (job, _) = result.unwrap().unwrap();
        assert_eq!(job.automation_name, "restored");
        assert_eq!(job.prompt, "restored prompt");
    }
}
