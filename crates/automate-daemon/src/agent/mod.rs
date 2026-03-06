pub mod claude_code;
pub mod codex;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use thiserror::Error;

use crate::log_stream::RunLogBroadcaster;

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

pub fn create_runtime(kind: &AgentKind) -> Box<dyn AgentRuntime> {
    match kind {
        AgentKind::ClaudeCode => Box::new(claude_code::ClaudeCodeRuntime::new()),
        AgentKind::Codex => Box::new(codex::CodexRuntime::new()),
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
