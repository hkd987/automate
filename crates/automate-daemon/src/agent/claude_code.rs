use super::CliAgentRuntime;

pub struct ClaudeCodeRuntime;

impl ClaudeCodeRuntime {
    pub fn create() -> CliAgentRuntime {
        CliAgentRuntime::new("claude", &["--print", "-p"], "Claude Code")
    }
}
