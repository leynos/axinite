//! API-facing worker transport types shared with the orchestrator.
//!
//! This module defines the serialized request and response shapes used for
//! worker chat completions, hosted remote-tool catalogue fetch and execution,
//! status updates, and credential delivery, including shared types such as
//! [`ChatMessage`], [`ToolCall`], [`ToolDefinition`], and [`ToolOutput`].

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

/// Relative worker path for the hosted remote-tool catalogue endpoint.
pub const REMOTE_TOOL_CATALOG_PATH: &str = "tools/catalog";
/// Relative worker path for hosted remote-tool execution.
pub const REMOTE_TOOL_EXECUTE_PATH: &str = "tools/execute";
/// Axum route for the hosted remote-tool catalogue endpoint.
pub const REMOTE_TOOL_CATALOG_ROUTE: &str = "/worker/{job_id}/tools/catalog";
/// Axum route for hosted remote-tool execution.
pub const REMOTE_TOOL_EXECUTE_ROUTE: &str = "/worker/{job_id}/tools/execute";

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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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
#[derive(Debug, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_and_orchestrator_share_remote_tool_route_constants() {
        assert_eq!(
            REMOTE_TOOL_CATALOG_ROUTE, "/worker/{job_id}/tools/catalog",
            "catalog route constant must match the expected orchestrator route"
        );
        assert_eq!(
            REMOTE_TOOL_EXECUTE_ROUTE, "/worker/{job_id}/tools/execute",
            "execute route constant must match the expected orchestrator route"
        );

        let test_job_id = "12345678-1234-1234-1234-123456789012";
        let catalog_route = REMOTE_TOOL_CATALOG_ROUTE.replace("{job_id}", test_job_id);
        let execute_route = REMOTE_TOOL_EXECUTE_ROUTE.replace("{job_id}", test_job_id);

        assert_eq!(
            catalog_route,
            format!("/worker/{}/tools/catalog", test_job_id),
            "catalog route must expand job_id parameter correctly"
        );
        assert_eq!(
            execute_route,
            format!("/worker/{}/tools/execute", test_job_id),
            "execute route must expand job_id parameter correctly"
        );
    }

    #[test]
    fn remote_tool_catalog_response_round_trip_without_field_loss() {
        let catalog_response = RemoteToolCatalogResponse {
            tools: vec![ToolDefinition {
                name: "test_tool".to_string(),
                description: "A **complex** test tool with UTF-8: \u{1F680}\u{1F4A1}.".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "title": "TestParams",
                    "properties": {
                        "query": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 100
                        },
                        "options": {
                            "type": "object",
                            "properties": {
                                "limit": {"type": "integer", "minimum": 1, "maximum": 50}
                            },
                            "required": ["limit"]
                        }
                    },
                    "required": ["query", "options"]
                }),
            }],
            toolset_instructions: vec![
                "Prefer remote tools for external systems.".to_string(),
                "Use local tools for filesystem operations.".to_string(),
            ],
            catalog_version: 42,
        };

        let serialized =
            serde_json::to_string(&catalog_response).expect("serialize RemoteToolCatalogResponse");
        let deserialized: RemoteToolCatalogResponse =
            serde_json::from_str(&serialized).expect("deserialize RemoteToolCatalogResponse");

        assert_eq!(deserialized.tools.len(), catalog_response.tools.len());
        assert_eq!(deserialized.tools[0].name, catalog_response.tools[0].name);
        assert_eq!(
            deserialized.tools[0].description,
            catalog_response.tools[0].description
        );
        assert_eq!(
            deserialized.tools[0].parameters,
            catalog_response.tools[0].parameters
        );
        assert_eq!(
            deserialized.toolset_instructions,
            catalog_response.toolset_instructions
        );
        assert_eq!(
            deserialized.catalog_version,
            catalog_response.catalog_version
        );
    }

    #[test]
    fn remote_tool_execution_request_round_trip_without_field_loss() {
        let execution_request = RemoteToolExecutionRequest {
            tool_name: "complex_tool".to_string(),
            params: serde_json::json!({
                "query": "test query",
                "options": {"limit": 25}
            }),
        };

        let serialized = serde_json::to_string(&execution_request)
            .expect("serialize RemoteToolExecutionRequest");
        let deserialized: RemoteToolExecutionRequest =
            serde_json::from_str(&serialized).expect("deserialize RemoteToolExecutionRequest");

        assert_eq!(deserialized.tool_name, execution_request.tool_name);
        assert_eq!(deserialized.params, execution_request.params);
    }

    #[test]
    fn remote_tool_execution_response_round_trip_without_field_loss() {
        let execution_response = RemoteToolExecutionResponse {
            output: ToolOutput::success(
                serde_json::json!({"result": "success", "data": [1, 2, 3]}),
                std::time::Duration::from_millis(42),
            )
            .with_cost(rust_decimal::Decimal::new(150, 2))
            .with_raw("raw execution output"),
        };

        let serialized = serde_json::to_string(&execution_response)
            .expect("serialize RemoteToolExecutionResponse");
        let deserialized: RemoteToolExecutionResponse =
            serde_json::from_str(&serialized).expect("deserialize RemoteToolExecutionResponse");

        assert_eq!(deserialized.output.result, execution_response.output.result);
        assert_eq!(deserialized.output.cost, execution_response.output.cost);
        assert_eq!(deserialized.output.raw, execution_response.output.raw);
        assert_eq!(
            deserialized.output.duration,
            execution_response.output.duration
        );
    }
}
