//! API-facing worker transport types shared with the orchestrator.
//!
//! This module defines the serialized request and response shapes used for
//! worker chat completions, hosted remote-tool catalogue fetch and execution,
//! status updates, and credential delivery, including shared types such as
//! [`ChatMessage`], [`ToolCall`], [`ToolDefinition`], and [`ToolOutput`].

#[path = "proxy_types.rs"]
mod proxy_types;
#[path = "remote_tool_types.rs"]
mod remote_tool_types;

use const_format::concatcp;
use serde::{Deserialize, Serialize};

pub use proxy_types::{
    FinishReason, ProxyCompletionRequest, ProxyCompletionResponse, ProxyToolCompletionRequest,
    ProxyToolCompletionResponse,
};
pub use remote_tool_types::{
    RemoteToolCatalogResponse, RemoteToolExecutionRequest, RemoteToolExecutionResponse,
};

/// Worker lifecycle state sent to the orchestrator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerState {
    InProgress,
    Running,
    Completed,
    Failed,
    #[serde(other)]
    Unknown,
}

impl WorkerState {
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for WorkerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// Route prefix for all per-job worker endpoints.
const WORKER_PREFIX: &str = "/worker/{job_id}/";

/// Relative worker path for the hosted remote-tool catalogue endpoint.
pub const REMOTE_TOOL_CATALOG_PATH: &str = "tools/catalog";
/// Relative worker path for hosted remote-tool execution.
pub const REMOTE_TOOL_EXECUTE_PATH: &str = "tools/execute";
/// Axum route for the hosted remote-tool catalogue endpoint.
pub const REMOTE_TOOL_CATALOG_ROUTE: &str = concatcp!(WORKER_PREFIX, REMOTE_TOOL_CATALOG_PATH);
/// Axum route for hosted remote-tool execution.
pub const REMOTE_TOOL_EXECUTE_ROUTE: &str = concatcp!(WORKER_PREFIX, REMOTE_TOOL_EXECUTE_PATH);

/// Relative worker path for job description endpoint.
pub const JOB_PATH: &str = "job";
/// Axum route for job description endpoint.
pub const JOB_ROUTE: &str = concatcp!(WORKER_PREFIX, JOB_PATH);

/// Relative worker path for credentials endpoint.
pub const CREDENTIALS_PATH: &str = "credentials";
/// Axum route for credentials endpoint.
pub const CREDENTIALS_ROUTE: &str = concatcp!(WORKER_PREFIX, CREDENTIALS_PATH);

/// Relative worker path for status update endpoint.
pub const STATUS_PATH: &str = "status";
/// Axum route for status update endpoint.
pub const STATUS_ROUTE: &str = concatcp!(WORKER_PREFIX, STATUS_PATH);

/// Relative worker path for completion report endpoint.
pub const COMPLETE_PATH: &str = "complete";
/// Axum route for completion report endpoint.
pub const COMPLETE_ROUTE: &str = concatcp!(WORKER_PREFIX, COMPLETE_PATH);

/// Relative worker path for job event endpoint.
pub const EVENT_PATH: &str = "event";
/// Axum route for job event endpoint.
pub const EVENT_ROUTE: &str = concatcp!(WORKER_PREFIX, EVENT_PATH);

/// Relative worker path for prompt polling endpoint.
pub const PROMPT_PATH: &str = "prompt";
/// Axum route for prompt polling endpoint.
pub const PROMPT_ROUTE: &str = concatcp!(WORKER_PREFIX, PROMPT_PATH);

/// Relative worker path for LLM completion endpoint.
pub const LLM_COMPLETE_PATH: &str = "llm/complete";
/// Axum route for LLM completion endpoint.
pub const LLM_COMPLETE_ROUTE: &str = concatcp!(WORKER_PREFIX, LLM_COMPLETE_PATH);

/// Relative worker path for LLM tool completion endpoint.
pub const LLM_COMPLETE_WITH_TOOLS_PATH: &str = "llm/complete_with_tools";
/// Axum route for LLM tool completion endpoint.
pub const LLM_COMPLETE_WITH_TOOLS_ROUTE: &str =
    concatcp!(WORKER_PREFIX, LLM_COMPLETE_WITH_TOOLS_PATH);

/// Relative path for health check endpoint (no job_id path component).
pub const WORKER_HEALTH_PATH: &str = "health";
/// Axum route for health check endpoint (no job_id path component).
pub const WORKER_HEALTH_ROUTE: &str = concatcp!("/", WORKER_HEALTH_PATH);

/// Build a concrete job-scoped path from a job ID and relative suffix.
///
/// Uses the canonical `WORKER_PREFIX` pattern so route registration and
/// client URL construction share the same source of truth.
pub fn job_scoped_path(job_id: &str, relative: &str) -> String {
    WORKER_PREFIX.replace("{job_id}", job_id) + relative
}

/// Build a worker job URL path from the orchestrator URL, job ID, and path suffix.
///
/// Returns a canonical URL of the form `{orchestrator_url}/worker/{job_id}/{path}`.
pub fn worker_job_url(orchestrator_url: &str, job_id: &str, path: &str) -> String {
    let base = orchestrator_url.trim_end_matches('/');
    let scoped_path = job_scoped_path(job_id, "");
    let scoped = scoped_path.trim_start_matches('/').trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{}/{}/{}", base, scoped, path)
}

/// Status update sent from worker to orchestrator.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusUpdate {
    pub state: WorkerState,
    pub message: Option<String>,
    pub iteration: u32,
}

impl StatusUpdate {
    /// Build a canonical worker status payload for the orchestrator API.
    ///
    /// Using this constructor keeps call sites aligned with the shared
    /// transport type and makes iteration counts explicit at the reporting
    /// boundary.
    pub fn new(state: WorkerState, message: Option<String>, iteration: u32) -> Self {
        Self {
            state,
            message,
            iteration,
        }
    }
}

/// Job description fetched from orchestrator.
#[derive(Debug, Serialize, Deserialize)]
pub struct JobDescription {
    pub title: String,
    pub description: String,
    pub project_dir: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompletionReport {
    /// Whether the worker completed the job successfully.
    pub success: bool,
    /// Optional human-readable completion summary or failure message.
    pub message: Option<String>,
    /// Number of worker iterations completed before exit.
    pub iterations: u32,
}

/// Event discriminator understood by the worker-orchestrator event pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobEventType {
    Status,
    Message,
    ToolUse,
    ToolResult,
    Result,
    #[serde(other)]
    Unknown,
}

impl JobEventType {
    pub const fn as_wire(self) -> &'static str {
        match self {
            Self::Status => "status",
            Self::Message => "message",
            Self::ToolUse => "tool_use",
            Self::ToolResult => "tool_result",
            Self::Result => "result",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for JobEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_wire())
    }
}

/// Payload sent to the orchestrator for each job event (shared by worker and Claude Code bridge).
#[derive(Debug, Serialize, Deserialize)]
pub struct JobEventPayload {
    /// Event discriminator understood by the orchestrator event pipeline.
    pub event_type: JobEventType,
    /// Event-specific JSON payload.
    pub data: serde_json::Value,
}

/// Response from the prompt polling endpoint.
#[derive(Debug, Serialize, Deserialize)]
pub struct PromptResponse {
    pub content: String,
    #[serde(default)]
    pub done: bool,
}

/// A single credential delivered from the orchestrator to a container worker.
///
/// Shared between the orchestrator endpoint and the worker client.
#[derive(Serialize, Deserialize)]
pub struct CredentialResponse {
    pub env_var: String,
    pub value: String,
}

impl std::fmt::Debug for CredentialResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialResponse")
            .field("env_var", &self.env_var)
            .field("value", &"[REDACTED]")
            .finish()
    }
}

/// Terminal result payload emitted with [`JobEventType::Result`].
///
/// Provides a consistent serialized shape for job completion events,
/// whether successful or failed.
#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalResult {
    /// Whether the job completed successfully.
    pub success: bool,
    /// Human-readable completion summary or failure message.
    pub message: String,
    /// Number of iterations completed before exit, if applicable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub iterations: Option<u32>,
}

impl TerminalResult {
    /// Create a new terminal result for a successful job.
    ///
    /// This is the result payload carried in `JobEventType::Result`, which
    /// complements but does not replace the authoritative completion report.
    pub fn success(message: impl Into<String>, iterations: Option<u32>) -> Self {
        Self {
            success: true,
            message: message.into(),
            iterations,
        }
    }

    /// Create a new terminal result for a failed job.
    ///
    /// Failure payloads intentionally carry a sanitized, user-facing summary
    /// rather than arbitrary internal error detail.
    pub fn failure(message: impl Into<String>, iterations: Option<u32>) -> Self {
        Self {
            success: false,
            message: message.into(),
            iterations,
        }
    }
}
