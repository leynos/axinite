//! API-facing worker transport types shared with the orchestrator.
//!
//! This module defines the serialized request/response shapes used for worker
//! chat completions, extension-tool proxying, status updates, and credential
//! delivery, including shared types such as [`ChatMessage`], [`ToolCall`],
//! [`ToolDefinition`], and [`ToolOutput`].

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

/// Request body sent by a worker to execute a hosted extension-management tool.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyExtensionToolRequest {
    /// Stable extension-management tool identifier known to both worker and orchestrator.
    pub tool_name: String,
    /// JSON parameters passed through to the tool implementation.
    pub params: serde_json::Value,
}

/// Response body returned after hosted extension-management tool execution.
#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyExtensionToolResponse {
    /// Tool execution output returned by the orchestrator.
    pub output: ToolOutput,
}

/// Completion status reported by a worker once it finishes a job.
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
