//! API-facing worker transport types shared with the orchestrator.
//!
//! This module defines the serialized request and response shapes used for
//! worker chat completions, hosted remote-tool catalog fetch and execution,
//! status updates, and credential delivery, including shared types such as
//! [`ChatMessage`], [`ToolCall`], [`ToolDefinition`], and [`ToolOutput`].

use serde::{Deserialize, Serialize};

use crate::llm::{ChatMessage, ToolCall, ToolDefinition};
use crate::tools::ToolOutput;

/// Relative worker path for the hosted remote-tool catalog endpoint.
pub const REMOTE_TOOL_CATALOG_PATH: &str = "tools/catalog";
/// Relative worker path for hosted remote-tool execution.
pub const REMOTE_TOOL_EXECUTE_PATH: &str = "tools/execute";
/// Axum route for the hosted remote-tool catalog endpoint.
pub const REMOTE_TOOL_CATALOG_ROUTE: &str = "/worker/{job_id}/tools/catalog";
/// Axum route for hosted remote-tool execution.
pub const REMOTE_TOOL_EXECUTE_ROUTE: &str = "/worker/{job_id}/tools/execute";

/// Status update sent from worker to orchestrator.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusUpdate {
    pub state: String,
    pub message: Option<String>,
    pub iteration: u32,
}

/// Job description fetched from orchestrator.
#[derive(Debug, Serialize, Deserialize)]
pub struct JobDescription {
    pub title: String,
    pub description: String,
    pub project_dir: Option<String>,
}

/// Completion result from the orchestrator (proxied from the real LLM).
#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyCompletionRequest {
    pub messages: Vec<ChatMessage>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stop_sequences: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyCompletionResponse {
    pub content: String,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub finish_reason: String,
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyToolCompletionRequest {
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDefinition>,
    pub model: Option<String>,
    pub max_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub tool_choice: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ProxyToolCompletionResponse {
    pub content: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub finish_reason: String,
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteToolExecutionRequest {
    pub tool_name: String,
    pub params: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteToolExecutionResponse {
    pub output: ToolOutput,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RemoteToolCatalogResponse {
    pub tools: Vec<ToolDefinition>,
    #[serde(default)]
    pub toolset_instructions: Vec<String>,
    pub catalog_version: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CompletionReport {
    pub success: bool,
    pub message: Option<String>,
    pub iterations: u32,
}

/// Payload sent to the orchestrator for each job event (shared by worker and Claude Code bridge).
#[derive(Debug, Serialize, Deserialize)]
pub struct JobEventPayload {
    pub event_type: String,
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
