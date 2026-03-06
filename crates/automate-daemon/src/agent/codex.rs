use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tracing::info;

use super::{AgentError, AgentRuntime, RunOutput};
use crate::log_stream::{LogLine, RunLogBroadcaster};

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

pub struct CodexRuntime {
    timeout: Duration,
}

impl Default for CodexRuntime {
    fn default() -> Self {
        Self::new()
    }
}

impl CodexRuntime {
    pub fn new() -> Self {
        Self {
            timeout: DEFAULT_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub fn with_timeout(timeout: Duration) -> Self {
        Self { timeout }
    }
}

#[async_trait::async_trait]
impl AgentRuntime for CodexRuntime {
    async fn check_installed(&self) -> bool {
        Command::new("codex")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    async fn run(
        &self,
        prompt: &str,
        env: HashMap<String, String>,
    ) -> Result<RunOutput, AgentError> {
        self.run_streaming(prompt, env, None).await
    }

    async fn run_streaming(
        &self,
        prompt: &str,
        env: HashMap<String, String>,
        broadcaster: Option<Arc<RunLogBroadcaster>>,
    ) -> Result<RunOutput, AgentError> {
        let start = Instant::now();

        info!(prompt_len = prompt.len(), "Starting codex agent");

        let mut cmd = Command::new("codex");
        cmd.arg("--quiet").arg(prompt);
        cmd.envs(&env);
        cmd.stdout(std::process::Stdio::piped());
        cmd.stderr(std::process::Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| AgentError::ProcessFailed(e.to_string()))?;

        let child_stdout = child.stdout.take();
        let child_stderr = child.stderr.take();

        let bc_stdout = broadcaster.clone();
        let stdout_handle = tokio::spawn(async move {
            let mut lines = Vec::new();
            if let Some(stdout) = child_stdout {
                let mut reader = tokio::io::BufReader::new(stdout).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    if let Some(ref bc) = bc_stdout {
                        bc.send(LogLine {
                            timestamp: chrono::Utc::now(),
                            stream: "stdout",
                            content: line.clone(),
                        });
                    }
                    lines.push(line);
                }
            }
            lines.join("\n")
        });

        let bc_stderr = broadcaster.clone();
        let stderr_handle = tokio::spawn(async move {
            let mut lines = Vec::new();
            if let Some(stderr) = child_stderr {
                let mut reader = tokio::io::BufReader::new(stderr).lines();
                while let Ok(Some(line)) = reader.next_line().await {
                    if let Some(ref bc) = bc_stderr {
                        bc.send(LogLine {
                            timestamp: chrono::Utc::now(),
                            stream: "stderr",
                            content: line.clone(),
                        });
                    }
                    lines.push(line);
                }
            }
            lines.join("\n")
        });

        let result = tokio::time::timeout(self.timeout, child.wait()).await;

        match result {
            Ok(Ok(status)) => {
                let stdout = stdout_handle.await.unwrap_or_default();
                let stderr = stderr_handle.await.unwrap_or_default();
                let duration = start.elapsed();
                let exit_code = status.code().unwrap_or(-1);

                Ok(RunOutput {
                    stdout,
                    stderr,
                    exit_code,
                    duration,
                })
            }
            Ok(Err(e)) => Err(AgentError::ProcessFailed(e.to_string())),
            Err(_) => {
                let _ = child.kill().await;
                Err(AgentError::Timeout(self.timeout))
            }
        }
    }
}
