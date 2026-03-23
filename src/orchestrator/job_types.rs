//! Plain data types for container job management.

use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};

/// Which mode a sandbox container runs in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JobMode {
    /// Standard IronClaw worker with proxied LLM calls.
    Worker,
    /// Claude Code bridge that spawns the `claude` CLI directly.
    ClaudeCode,
}

impl JobMode {
    /// Return the stable string form used in environment and container metadata.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Worker => "worker",
            Self::ClaudeCode => "claude_code",
        }
    }
}

impl std::fmt::Display for JobMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Configuration for the container job manager.
#[derive(Clone)]
pub struct ContainerJobConfig {
    /// Docker image for worker containers.
    pub image: String,
    /// Default memory limit in MB.
    pub memory_limit_mb: u64,
    /// Default CPU shares.
    pub cpu_shares: u32,
    /// Port the orchestrator internal API listens on.
    pub orchestrator_port: u16,
    /// Anthropic API key for Claude Code containers (read from ANTHROPIC_API_KEY).
    /// Takes priority over OAuth token.
    pub claude_code_api_key: Option<String>,
    /// OAuth access token extracted from the host's `claude login` session.
    /// Passed as CLAUDE_CODE_OAUTH_TOKEN to containers. Falls back to this
    /// when no ANTHROPIC_API_KEY is available.
    pub claude_code_oauth_token: Option<String>,
    /// Claude model to use in ClaudeCode mode.
    pub claude_code_model: String,
    /// Maximum turns for Claude Code.
    pub claude_code_max_turns: u32,
    /// Memory limit in MB for Claude Code containers (heavier than workers).
    pub claude_code_memory_limit_mb: u64,
    /// Allowed tool patterns for Claude Code (passed as CLAUDE_CODE_ALLOWED_TOOLS env var).
    pub claude_code_allowed_tools: Vec<String>,
}

impl fmt::Debug for ContainerJobConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContainerJobConfig")
            .field("image", &self.image)
            .field("memory_limit_mb", &self.memory_limit_mb)
            .field("cpu_shares", &self.cpu_shares)
            .field("orchestrator_port", &self.orchestrator_port)
            .field(
                "claude_code_api_key",
                &self.claude_code_api_key.as_ref().map(|_| "<redacted>"),
            )
            .field(
                "claude_code_oauth_token",
                &self.claude_code_oauth_token.as_ref().map(|_| "<redacted>"),
            )
            .field("claude_code_model", &self.claude_code_model)
            .field("claude_code_max_turns", &self.claude_code_max_turns)
            .field(
                "claude_code_memory_limit_mb",
                &self.claude_code_memory_limit_mb,
            )
            .field("claude_code_allowed_tools", &self.claude_code_allowed_tools)
            .finish()
    }
}

impl Default for ContainerJobConfig {
    fn default() -> Self {
        Self {
            image: "ironclaw-worker:latest".to_string(),
            memory_limit_mb: 2048,
            cpu_shares: 1024,
            orchestrator_port: 50051,
            claude_code_api_key: None,
            claude_code_oauth_token: None,
            claude_code_model: "sonnet".to_string(),
            claude_code_max_turns: 50,
            claude_code_memory_limit_mb: 4096,
            claude_code_allowed_tools: crate::config::ClaudeCodeConfig::default().allowed_tools,
        }
    }
}

/// State of a container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainerState {
    Creating,
    Running,
    Stopped,
    Failed,
}

impl std::fmt::Display for ContainerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Creating => write!(f, "creating"),
            Self::Running => write!(f, "running"),
            Self::Stopped => write!(f, "stopped"),
            Self::Failed => write!(f, "failed"),
        }
    }
}

/// Docker container identifier for a managed job.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ContainerId(String);

impl ContainerId {
    /// Construct a managed container identifier from a Docker-provided value.
    pub fn new(container_id: impl Into<String>) -> Self {
        Self(container_id.into())
    }

    /// Borrow the raw Docker container identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ContainerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// Handle to a running container job.
#[derive(Debug, Clone)]
pub struct ContainerHandle {
    /// Stable job identifier used to correlate orchestrator and Docker state.
    pub job_id: uuid::Uuid,
    /// Docker container identifier once the container has been created.
    pub container_id: Option<ContainerId>,
    /// Current lifecycle state tracked by the job manager.
    pub state: ContainerState,
    /// Execution mode that determines which container entrypoint was launched.
    pub mode: JobMode,
    /// Timestamp recorded when the handle was created.
    pub created_at: DateTime<Utc>,
    /// Optional workspace directory bind-mounted into the container.
    pub project_dir: Option<PathBuf>,
    /// Human-readable task description kept for status and diagnostics.
    pub task_description: String,
    /// Last status message reported by the worker (iteration count, progress, etc.).
    pub last_worker_status: Option<String>,
    /// Which iteration the worker is on (updated via status reports).
    pub worker_iteration: u32,
    /// Completion result from the worker (set when the worker reports done).
    pub completion_result: Option<CompletionResult>,
    // NOTE: auth_token is intentionally NOT in this struct.
    // It lives only in the TokenStore (never logged, serialized, or persisted).
}

/// Result reported by a worker on completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionResult {
    /// Whether the worker reported overall success for the job.
    pub success: bool,
    /// Optional human-readable completion or error message from the worker.
    pub message: Option<String>,
}
