use super::CliAgentRuntime;

pub struct CodexRuntime;

impl CodexRuntime {
    pub fn create() -> CliAgentRuntime {
        CliAgentRuntime::new("codex", &["--quiet"], "Codex")
    }
}
