//! Request and response types for LLM completions.
//!
//! Defines the plain and tool-augmented completion request/response types,
//! tool definitions and calls, model metadata, and helpers for stripping
//! parameters a provider does not support.

use serde::{Deserialize, Serialize};

use super::ChatMessage;

/// Generate the sampling-parameter builder setters and the [`TunableParams`]
/// impl shared by [`CompletionRequest`] and [`ToolCompletionRequest`], which
/// hold the same `model`, `max_tokens`, and `temperature` fields.
macro_rules! impl_sampling_params {
    ($t:ty) => {
        impl $t {
            /// Set model override.
            pub fn with_model(mut self, model: impl Into<String>) -> Self {
                self.model = Some(model.into());
                self
            }

            /// Set max tokens.
            pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
                self.max_tokens = Some(max_tokens);
                self
            }

            /// Set temperature.
            pub fn with_temperature(mut self, temperature: f32) -> Self {
                self.temperature = Some(temperature);
                self
            }
        }

        impl TunableParams for $t {
            fn clear_temperature(&mut self) {
                self.temperature = None;
            }
            fn clear_max_tokens(&mut self) {
                self.max_tokens = None;
            }
        }
    };
}

/// Define a completion-request struct carrying the `messages` and sampling
/// fields (`model`, `max_tokens`, `temperature`, `metadata`) common to the
/// plain and tool-augmented requests.
///
/// The caller supplies only the request-specific fields (e.g. `stop_sequences`
/// or `tools`); the shared block is emitted identically for each, keeping those
/// fields defined in exactly one place.
macro_rules! completion_request {
    (
        $(#[$meta:meta])*
        pub struct $name:ident { $($(#[$fmeta:meta])* pub $field:ident : $ty:ty,)* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone)]
        pub struct $name {
            pub messages: Vec<ChatMessage>,
            $($(#[$fmeta])* pub $field: $ty,)*
            /// Optional per-request model override.
            pub model: Option<String>,
            pub max_tokens: Option<u32>,
            pub temperature: Option<f32>,
            /// Opaque metadata passed through to the provider (e.g. thread_id for chaining).
            pub metadata: std::collections::HashMap<String, String>,
        }
    };
}

completion_request! {
    /// Request for a chat completion.
    pub struct CompletionRequest {
        pub stop_sequences: Option<Vec<String>>,
    }
}

impl CompletionRequest {
    /// Create a new completion request.
    pub fn new(messages: Vec<ChatMessage>) -> Self {
        Self {
            messages,
            model: None,
            max_tokens: None,
            temperature: None,
            stop_sequences: None,
            metadata: std::collections::HashMap::new(),
        }
    }
}

impl_sampling_params!(CompletionRequest);

/// Define a completion response struct carrying the token-usage and prompt-cache
/// accounting fields common to plain and tool-augmented responses.
///
/// The caller supplies the response-specific leading fields (e.g. `content` or
/// `tool_calls`); the shared usage/cache tail is emitted identically for each,
/// keeping the accounting fields defined in exactly one place.
macro_rules! completion_response {
    (
        $(#[$meta:meta])*
        pub struct $name:ident { $($(#[$fmeta:meta])* pub $field:ident : $ty:ty,)* }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Default)]
        pub struct $name {
            $($(#[$fmeta])* pub $field: $ty,)*
            pub input_tokens: u32,
            pub output_tokens: u32,
            pub finish_reason: FinishReason,
            /// Tokens read from the provider's server-side prompt cache (Anthropic).
            /// Zero when caching is not supported or on a cache miss.
            pub cache_read_input_tokens: u32,
            /// Tokens written to the provider's server-side prompt cache (Anthropic).
            /// Zero when caching is not supported or no new prefix was cached.
            pub cache_creation_input_tokens: u32,
        }
    };
}

completion_response! {
    /// Response from a chat completion.
    pub struct CompletionResponse {
        pub content: String,
    }
}

/// Why the completion finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FinishReason {
    #[default]
    Stop,
    Length,
    ToolUse,
    ContentFilter,
    Unknown,
}

/// Definition of a tool for the LLM.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A tool call requested by the LLM.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Result of a tool execution to send back to the LLM.
#[derive(Debug, Clone)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub name: String,
    pub content: String,
    pub is_error: bool,
}

completion_request! {
    /// Request for a completion with tool use.
    pub struct ToolCompletionRequest {
        pub tools: Vec<ToolDefinition>,
        /// How to handle tool use: "auto", "required", or "none".
        pub tool_choice: Option<String>,
    }
}

impl ToolCompletionRequest {
    /// Create a new tool completion request.
    pub fn new(messages: Vec<ChatMessage>, tools: Vec<ToolDefinition>) -> Self {
        Self {
            messages,
            tools,
            model: None,
            max_tokens: None,
            temperature: None,
            tool_choice: None,
            metadata: std::collections::HashMap::new(),
        }
    }

    /// Set tool choice mode.
    pub fn with_tool_choice(mut self, choice: impl Into<String>) -> Self {
        self.tool_choice = Some(choice.into());
        self
    }
}

impl_sampling_params!(ToolCompletionRequest);

completion_response! {
    /// Response from a completion with potential tool calls.
    pub struct ToolCompletionResponse {
        /// Text content (may be empty if tool calls are present).
        pub content: Option<String>,
        /// Tool calls requested by the model.
        pub tool_calls: Vec<ToolCall>,
    }
}

/// Metadata about a model returned by the provider's API.
#[derive(Debug, Clone)]
pub struct ModelMetadata {
    pub id: String,
    /// Total context window size in tokens.
    pub context_length: Option<u32>,
}

/// Represents a request parameter that may not be supported by all LLM providers.
///
/// This typed enum replaces stringly-typed parameter names across the codebase,
/// providing type safety and single-point-of-maintenance for parameter handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UnsupportedParam {
    Temperature,
    MaxTokens,
    StopSequences,
}

impl UnsupportedParam {
    /// Get the string name of this parameter for config/error messages.
    pub fn name(&self) -> &'static str {
        match self {
            UnsupportedParam::Temperature => "temperature",
            UnsupportedParam::MaxTokens => "max_tokens",
            UnsupportedParam::StopSequences => "stop_sequences",
        }
    }
}

/// Mutable access to the sampling parameters shared by both request types.
///
/// Implemented for each request type by [`impl_sampling_params`].
trait TunableParams {
    fn clear_temperature(&mut self);
    fn clear_max_tokens(&mut self);
}

/// Strip the sampling parameters common to both request types.
fn strip_common_params(
    unsupported: &std::collections::HashSet<String>,
    req: &mut impl TunableParams,
) {
    if unsupported.contains(UnsupportedParam::Temperature.name()) {
        req.clear_temperature();
    }
    if unsupported.contains(UnsupportedParam::MaxTokens.name()) {
        req.clear_max_tokens();
    }
}

/// Strip unsupported parameters from a `CompletionRequest` in place.
///
/// This is the single helper function used by all providers to remove
/// parameters they don't support, replacing duplicate stringly-typed logic.
pub fn strip_unsupported_completion_params(
    unsupported: &std::collections::HashSet<String>,
    req: &mut CompletionRequest,
) {
    if unsupported.is_empty() {
        return;
    }
    strip_common_params(unsupported, req);
    if unsupported.contains(UnsupportedParam::StopSequences.name()) {
        req.stop_sequences = None;
    }
}

/// Strip unsupported parameters from a `ToolCompletionRequest` in place.
///
/// This is the single helper function used by all providers to remove
/// parameters they don't support from tool calls, replacing duplicate stringly-typed logic.
///
/// Note: Only `Temperature` and `MaxTokens` are supported in `ToolCompletionRequest`.
/// `StopSequences` is only available in `CompletionRequest` and is not applicable to tool calls.
pub fn strip_unsupported_tool_params(
    unsupported: &std::collections::HashSet<String>,
    req: &mut ToolCompletionRequest,
) {
    if unsupported.is_empty() {
        return;
    }
    strip_common_params(unsupported, req);
    // Note: StopSequences is not a field in ToolCompletionRequest, so no action needed
}
