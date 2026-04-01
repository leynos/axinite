//! API-facing worker transport types shared with the orchestrator.
//!
//! This module defines the serialized request and response shapes used for
//! worker chat completions, hosted remote-tool catalogue fetch and execution,
//! status updates, and credential delivery, including shared types such as
//! [`ChatMessage`], [`ToolCall`], [`ToolDefinition`], and [`ToolOutput`].

use const_format::concatcp;
use serde::{Deserialize, Serialize};

use crate::llm::{ChatMessage, ToolCall, ToolDefinition};
use crate::tools::ToolOutput;

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
    format!("{}/{}/{}", base, job_scoped_path(job_id, "").trim_start_matches('/'), path)
}

/// Status update sent from worker to orchestrator.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusUpdate {
    pub state: WorkerState,
    pub message: Option<String>,
    pub iteration: u32,
}

impl StatusUpdate {
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

/// Provider finish reason transported between orchestrator and worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    Stop,
    Length,
    #[serde(alias = "tool_calls")]
    ToolUse,
    ContentFilter,
    #[serde(other)]
    Unknown,
}

impl From<crate::llm::FinishReason> for FinishReason {
    fn from(value: crate::llm::FinishReason) -> Self {
        match value {
            crate::llm::FinishReason::Stop => Self::Stop,
            crate::llm::FinishReason::Length => Self::Length,
            crate::llm::FinishReason::ToolUse => Self::ToolUse,
            crate::llm::FinishReason::ContentFilter => Self::ContentFilter,
            crate::llm::FinishReason::Unknown => Self::Unknown,
        }
    }
}

impl From<FinishReason> for crate::llm::FinishReason {
    fn from(value: FinishReason) -> Self {
        match value {
            FinishReason::Stop => Self::Stop,
            FinishReason::Length => Self::Length,
            FinishReason::ToolUse => Self::ToolUse,
            FinishReason::ContentFilter => Self::ContentFilter,
            FinishReason::Unknown => Self::Unknown,
        }
    }
}

/// Completion result from the orchestrator (proxied from the real LLM).
#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyCompletionRequest {
    /// Conversation history forwarded to the orchestrator-backed provider.
    pub messages: Vec<ChatMessage>,
    /// Optional model override requested by the worker.
    pub model: Option<String>,
    /// Optional token ceiling for the completion.
    pub max_tokens: Option<u32>,
    /// Optional sampling temperature for the completion.
    pub temperature: Option<f32>,
    /// Optional stop-sequence list forwarded unchanged to the provider.
    pub stop_sequences: Option<Vec<String>>,
}

/// Completion result returned by the orchestrator-backed provider.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyCompletionResponse {
    /// Assistant text produced by the proxied completion call.
    pub content: String,
    /// Provider-reported prompt token usage.
    pub input_tokens: u32,
    /// Provider-reported completion token usage.
    pub output_tokens: u32,
    /// Provider finish reason normalised into a transport enum.
    pub finish_reason: FinishReason,
    /// Tokens served from cache when the provider exposes that metric.
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    /// Tokens written into cache when the provider exposes that metric.
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
}

/// Tool-capable completion request forwarded to the orchestrator.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyToolCompletionRequest {
    /// Conversation history forwarded to the orchestrator-backed provider.
    pub messages: Vec<ChatMessage>,
    /// Tool definitions currently visible to the worker.
    pub tools: Vec<ToolDefinition>,
    /// Optional model override requested by the worker.
    pub model: Option<String>,
    /// Optional token ceiling for the completion.
    pub max_tokens: Option<u32>,
    /// Optional sampling temperature for the completion.
    pub temperature: Option<f32>,
    /// Optional provider-specific tool-choice override.
    pub tool_choice: Option<String>,
}

/// Tool-capable completion result returned by the orchestrator.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyToolCompletionResponse {
    /// Optional assistant text returned alongside tool calls.
    pub content: Option<String>,
    /// Tool calls selected by the orchestrator-backed provider.
    pub tool_calls: Vec<ToolCall>,
    /// Provider-reported prompt token usage.
    pub input_tokens: u32,
    /// Provider-reported completion token usage.
    pub output_tokens: u32,
    /// Provider finish reason normalised into a transport enum.
    pub finish_reason: FinishReason,
    /// Tokens served from cache when the provider exposes that metric.
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    /// Tokens written into cache when the provider exposes that metric.
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
}

/// Request sent from a worker to the orchestrator for hosted remote-tool execution.
///
/// `tool_name` is the orchestrator tool identifier. `params` must match that
/// tool's JSON Schema because the orchestrator validates and executes the call.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteToolExecutionRequest {
    /// Stable hosted remote-tool identifier known to both worker and orchestrator.
    pub tool_name: String,
    /// JSON parameters passed through to the tool implementation.
    pub params: serde_json::Value,
}

/// Response returned after the orchestrator executes a hosted remote tool.
///
/// `output` is the tool's `ToolOutput`, including its result payload and
/// reported side-effect metadata such as duration and optional cost.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteToolExecutionResponse {
    /// Tool execution output returned by the orchestrator.
    pub output: ToolOutput,
}

/// Catalogue payload returned to workers for hosted-visible remote tools.
///
/// `tools` is the current model-facing tool list. `toolset_instructions` is
/// optional human-readable guidance and defaults to an empty list.
/// `catalog_version` is a deterministic content version derived from the
/// serialized catalogue payload.
#[derive(Debug, Serialize, Deserialize, PartialEq)]
pub struct RemoteToolCatalogResponse {
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub toolset_instructions: Vec<String>,
    pub catalog_version: u64,
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
#[derive(Debug, Deserialize)]
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
/// Provides a consistent serialised shape for job completion events,
/// whether successful or failed.
#[derive(Debug, Serialize, Deserialize)]
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
    pub fn success(message: impl Into<String>, iterations: Option<u32>) -> Self {
        Self {
            success: true,
            message: message.into(),
            iterations,
        }
    }

    /// Create a new terminal result for a failed job.
    pub fn failure(message: impl Into<String>, iterations: Option<u32>) -> Self {
        Self {
            success: false,
            message: message.into(),
            iterations,
        }
    }
}
