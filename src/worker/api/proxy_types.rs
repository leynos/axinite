//! Proxy transport types used by the worker-orchestrator boundary.
//!
//! This module defines the serializable request and response payloads used for
//! proxied completions, including shapes built around [`ChatMessage`],
//! [`ToolCall`], and [`ToolDefinition`].

use serde::{Deserialize, Serialize};

use crate::llm::{ChatMessage, ToolCall, ToolDefinition};

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

/// Request payload for a completion proxied through the orchestrator.
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
    /// Provider finish reason normalized into a transport enum.
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
    /// Provider finish reason normalized into a transport enum.
    pub finish_reason: FinishReason,
    /// Tokens served from cache when the provider exposes that metric.
    #[serde(default)]
    pub cache_read_input_tokens: u32,
    /// Tokens written into cache when the provider exposes that metric.
    #[serde(default)]
    pub cache_creation_input_tokens: u32,
}
