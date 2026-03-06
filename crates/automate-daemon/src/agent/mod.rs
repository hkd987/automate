pub mod claude_code;
pub mod codex;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::io::AsyncBufReadExt;
use tokio::process::Command;
use tracing::info;

use crate::log_stream::{LogLine, RunLogBroadcaster};

#[derive(Debug, Clone, PartialEq)]
pub enum AgentKind {
    ClaudeCode,
    Codex,
}

#[derive(Debug, Clone)]
pub struct RunOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
    pub duration: Duration,
}

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("agent not installed")]
    NotInstalled,
    #[error("agent timed out after {0:?}")]
    Timeout(Duration),
    #[error("agent process failed: {0}")]
    ProcessFailed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait::async_trait]
pub trait AgentRuntime: Send + Sync {
    async fn check_installed(&self) -> bool;
    async fn run(
        &self,
        prompt: &str,
        env: HashMap<String, String>,
    ) -> Result<RunOutput, AgentError>;

    async fn run_streaming(
        &self,
        prompt: &str,
        env: HashMap<String, String>,
        broadcaster: Option<Arc<RunLogBroadcaster>>,
    ) -> Result<RunOutput, AgentError> {
        // Default: fall back to non-streaming run
        let _ = broadcaster;
        self.run(prompt, env).await
    }
}

pub struct CliAgentRuntime {
    binary_name: &'static str,
    args: &'static [&'static str],
    name: &'static str,
    timeout: Duration,
}

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(600);

impl CliAgentRuntime {
    pub fn new(
        binary_name: &'static str,
        args: &'static [&'static str],
        name: &'static str,
    ) -> Self {
        Self {
            binary_name,
            args,
            name,
            timeout: DEFAULT_TIMEOUT,
        }
    }

    #[cfg(test)]
    pub fn with_timeout(
        binary_name: &'static str,
        args: &'static [&'static str],
        name: &'static str,
        timeout: Duration,
    ) -> Self {
        Self {
            binary_name,
            args,
            name,
            timeout,
        }
    }
}

#[async_trait::async_trait]
impl AgentRuntime for CliAgentRuntime {
    async fn check_installed(&self) -> bool {
        Command::new(self.binary_name)
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

        info!(
            prompt_len = prompt.len(),
            agent = self.name,
            "Starting agent"
        );

        let mut cmd = Command::new(self.binary_name);
        for arg in self.args {
            cmd.arg(arg);
        }
        cmd.arg(prompt);
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
                        })
                        .await;
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
                        })
                        .await;
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

pub fn create_runtime(kind: &AgentKind) -> Box<dyn AgentRuntime> {
    match kind {
        AgentKind::ClaudeCode => Box::new(claude_code::ClaudeCodeRuntime::create()),
        AgentKind::Codex => Box::new(codex::CodexRuntime::create()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub struct MockRuntime {
        pub installed: bool,
        pub output: RunOutput,
    }

    #[async_trait::async_trait]
    impl AgentRuntime for MockRuntime {
        async fn check_installed(&self) -> bool {
            self.installed
        }

        async fn run(
            &self,
            _prompt: &str,
            _env: HashMap<String, String>,
        ) -> Result<RunOutput, AgentError> {
            if !self.installed {
                return Err(AgentError::NotInstalled);
            }
            Ok(self.output.clone())
        }
    }

    #[tokio::test]
    async fn mock_runtime_returns_canned_output() {
        let rt = MockRuntime {
            installed: true,
            output: RunOutput {
                stdout: "hello from mock".to_string(),
                stderr: String::new(),
                exit_code: 0,
                duration: Duration::from_millis(100),
            },
        };

        assert!(rt.check_installed().await);
        let result = rt.run("test prompt", HashMap::new()).await.unwrap();
        assert_eq!(result.stdout, "hello from mock");
        assert_eq!(result.exit_code, 0);
    }

    #[tokio::test]
    async fn mock_runtime_not_installed() {
        let rt = MockRuntime {
            installed: false,
            output: RunOutput {
                stdout: String::new(),
                stderr: String::new(),
                exit_code: 1,
                duration: Duration::from_millis(0),
            },
        };

        assert!(!rt.check_installed().await);
        let err = rt.run("test", HashMap::new()).await.unwrap_err();
        assert!(matches!(err, AgentError::NotInstalled));
    }

    #[test]
    fn create_runtime_claude_code() {
        let rt = create_runtime(&AgentKind::ClaudeCode);
        // Just verify it creates without panic
        let _ = rt;
    }

    #[test]
    fn create_runtime_codex() {
        let rt = create_runtime(&AgentKind::Codex);
        let _ = rt;
    }
}
