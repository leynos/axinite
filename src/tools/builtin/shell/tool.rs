//! The `ShellTool` type: configuration builders, blocked-command checks, and
//! the `NativeTool` implementation. Execution paths live in [`super::exec`].

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::context::JobContext;
use crate::sandbox::{SandboxManager, SandboxPolicy};
use crate::tools::tool::{
    ApprovalRequirement, NativeTool, ToolDomain, ToolError, ToolOutput, require_str,
};

use super::policy::{DEFAULT_TIMEOUT, matches_blocked_command, matches_dangerous_pattern};
use super::requires_explicit_approval;

/// Shell command execution tool.
pub struct ShellTool {
    /// Working directory for commands (if None, uses job's working dir or cwd).
    pub(super) working_dir: Option<PathBuf>,
    /// Command timeout.
    pub(super) timeout: Duration,
    /// Whether to allow potentially dangerous commands (requires explicit approval).
    pub(super) allow_dangerous: bool,
    /// Optional sandbox manager for Docker execution.
    pub(super) sandbox: Option<Arc<SandboxManager>>,
    /// Sandbox policy to use when sandbox is available.
    pub(super) sandbox_policy: SandboxPolicy,
}

impl std::fmt::Debug for ShellTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShellTool")
            .field("working_dir", &self.working_dir)
            .field("timeout", &self.timeout)
            .field("allow_dangerous", &self.allow_dangerous)
            .field("sandbox", &self.sandbox.is_some())
            .field("sandbox_policy", &self.sandbox_policy)
            .finish()
    }
}

impl ShellTool {
    /// Create a new shell tool with default settings.
    pub fn new() -> Self {
        Self {
            working_dir: None,
            timeout: DEFAULT_TIMEOUT,
            allow_dangerous: false,
            sandbox: None,
            sandbox_policy: SandboxPolicy::ReadOnly,
        }
    }

    /// Set the working directory.
    pub fn with_working_dir(mut self, dir: PathBuf) -> Self {
        self.working_dir = Some(dir);
        self
    }

    /// Set the command timeout.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Enable sandbox execution with the given manager.
    pub fn with_sandbox(mut self, sandbox: Arc<SandboxManager>) -> Self {
        self.sandbox = Some(sandbox);
        self
    }

    /// Set the sandbox policy.
    pub fn with_sandbox_policy(mut self, policy: SandboxPolicy) -> Self {
        self.sandbox_policy = policy;
        self
    }

    /// Check if a command is blocked, returning the rejection reason.
    pub(super) fn is_blocked(&self, cmd: &str) -> Option<&'static str> {
        let normalized = cmd.to_lowercase();

        if matches_blocked_command(&normalized) {
            return Some("Command contains blocked pattern");
        }

        if self.allow_dangerous {
            return None;
        }

        matches_dangerous_pattern(&normalized)
            .then_some("Command contains potentially dangerous pattern")
    }
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeTool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute shell commands. Use for running builds, tests, git operations, and other CLI tasks. \
         Commands run in a subprocess with captured output. Long-running commands have a timeout. \
         When Docker sandbox is enabled, commands run in isolated containers for security."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute"
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the command (optional)"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in seconds (optional, default 120)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let command = require_str(&params, "command")?;

        let workdir = params.get("workdir").and_then(|v| v.as_str());
        let timeout = params.get("timeout").and_then(|v| v.as_u64());

        let start = std::time::Instant::now();
        let (output, exit_code) = self
            .execute_command(command, workdir, timeout, &ctx.extra_env)
            .await?;
        let duration = start.elapsed();

        let sandboxed = self.sandbox.is_some();

        let result = serde_json::json!({
            "output": output,
            "exit_code": exit_code,
            "success": exit_code == 0,
            "sandboxed": sandboxed
        });

        Ok(ToolOutput::success(result, duration))
    }

    fn requires_approval(&self, params: &serde_json::Value) -> ApprovalRequirement {
        let cmd = params
            .get("command")
            .and_then(|c| c.as_str().map(String::from))
            .or_else(|| {
                params
                    .as_str()
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .and_then(|v| v.get("command").and_then(|c| c.as_str().map(String::from)))
            });

        if let Some(ref cmd) = cmd
            && requires_explicit_approval(cmd)
        {
            return ApprovalRequirement::Always;
        }

        ApprovalRequirement::UnlessAutoApproved
    }

    fn requires_sanitization(&self) -> bool {
        true // Shell output could contain anything
    }

    fn domain(&self) -> ToolDomain {
        ToolDomain::Container
    }

    fn rate_limit_config(&self) -> Option<crate::tools::tool::ToolRateLimitConfig> {
        Some(crate::tools::tool::ToolRateLimitConfig::new(30, 300))
    }
}
