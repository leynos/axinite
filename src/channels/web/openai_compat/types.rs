//! OpenAI-compatible wire types: requests, responses, streaming chunks,
//! and error payloads.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// OpenAI request types
// ---------------------------------------------------------------------------

/// Body of a `POST /v1/chat/completions` request. Unknown fields are ignored.
#[derive(Debug, Deserialize)]
pub struct OpenAiChatRequest {
    /// Model name requested by the client, checked by `validate_model_name`.
    pub model: String,
    /// Conversation history in order, converted to internal chat messages.
    pub messages: Vec<OpenAiMessage>,
    /// Sampling temperature. Absent means the provider default applies.
    #[serde(default)]
    pub temperature: Option<f32>,
    /// Upper bound on generated tokens. Absent means the provider default applies.
    #[serde(default)]
    pub max_tokens: Option<u32>,
    /// When `true`, the reply is sent as `chat.completion.chunk` server-sent events.
    /// Absent is treated as `false`.
    #[serde(default)]
    pub stream: Option<bool>,
    /// Function tools the model may call. Absent or empty disables tool use.
    #[serde(default)]
    pub tools: Option<Vec<OpenAiTool>>,
    /// Raw tool-choice value: a string such as `auto`, or an object naming a function.
    /// Normalised by `normalize_tool_choice`.
    #[serde(default)]
    pub tool_choice: Option<serde_json::Value>,
    /// Stop sequences, as either one string or an array of strings. Parsed by `parse_stop`.
    #[serde(default)]
    pub stop: Option<serde_json::Value>,
}

/// A chat message in OpenAI wire form, used in both requests and responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiMessage {
    /// Speaker role: `system`, `user`, `assistant` or `tool`.
    pub role: String,
    /// Text content. Omitted from serialised output when `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Name of the function whose result a `tool` message carries. Required for `tool`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// ID of the assistant tool call this `tool` message answers. Required for `tool`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Tool calls made by an assistant message.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCall>>,
}

/// A tool definition in a request's `tools` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiTool {
    /// Tool kind, carried on the wire as `type`. Only `function` entries are converted.
    #[serde(rename = "type")]
    pub tool_type: String,
    /// Signature of the function the model may call.
    pub function: OpenAiFunction,
}

/// Function signature offered to the model in a request's `tools` array.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiFunction {
    /// Name the model uses when calling the function.
    pub name: String,
    /// Natural-language description shown to the model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// JSON Schema for the arguments, passed through unchanged. Defaults to an empty object
    /// schema when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parameters: Option<serde_json::Value>,
}

/// A tool call made by an assistant message, in OpenAI form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiToolCall {
    /// Call identifier. A later `tool` message echoes it in `tool_call_id`.
    pub id: String,
    /// Call kind, carried on the wire as `type`. This adapter always emits `function`.
    #[serde(rename = "type")]
    pub call_type: String,
    /// Name and arguments of the call.
    pub function: OpenAiToolCallFunction,
}

/// Name and arguments of a function call.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OpenAiToolCallFunction {
    /// Name of the called function.
    pub name: String,
    /// Arguments as a JSON-encoded string, not a JSON object. Decoding falls back to `{}`
    /// when the string is not valid JSON.
    pub arguments: String,
}

// ---------------------------------------------------------------------------
// OpenAI response types (non-streaming)
// ---------------------------------------------------------------------------

/// Non-streaming chat completion body.
#[derive(Debug, Serialize)]
pub struct OpenAiChatResponse {
    /// Completion identifier, formatted as `chatcmpl-` followed by a 32-character hex UUID.
    pub id: String,
    /// Object type string, always `chat.completion`.
    pub object: &'static str,
    /// Creation time as Unix seconds.
    pub created: u64,
    /// Model that served the request, which may differ from the name requested.
    pub model: String,
    /// Completion choices. This adapter always returns exactly one, at index 0.
    pub choices: Vec<OpenAiChoice>,
    /// Token accounting for the request.
    pub usage: OpenAiUsage,
}

/// One completion alternative within a non-streaming response.
#[derive(Debug, Serialize)]
pub struct OpenAiChoice {
    /// Position of this choice in `choices`, starting at 0.
    pub index: u32,
    /// The assistant message produced.
    pub message: OpenAiMessage,
    /// Why generation stopped: `stop`, `length`, `tool_calls` or `content_filter`.
    pub finish_reason: String,
}

/// Token counts for one request.
#[derive(Debug, Serialize)]
pub struct OpenAiUsage {
    /// Input tokens consumed by the prompt.
    pub prompt_tokens: u32,
    /// Output tokens generated.
    pub completion_tokens: u32,
    /// Sum of `prompt_tokens` and `completion_tokens`.
    pub total_tokens: u32,
}

// ---------------------------------------------------------------------------
// OpenAI response types (streaming)
// ---------------------------------------------------------------------------

/// One server-sent event in a streamed completion.
#[derive(Debug, Serialize)]
pub struct OpenAiChatChunk {
    /// Completion identifier, the same on every chunk of one stream.
    pub id: String,
    /// Object type string, always `chat.completion.chunk`.
    pub object: &'static str,
    /// Stream creation time as Unix seconds, identical across the stream's chunks.
    pub created: u64,
    /// Model name for the stream.
    pub model: String,
    /// Single-element list carrying this chunk's delta.
    pub choices: Vec<OpenAiChunkChoice>,
}

/// One choice inside a streamed chunk.
#[derive(Debug, Serialize)]
pub struct OpenAiChunkChoice {
    /// Choice index, always 0.
    pub index: u32,
    /// Incremental message fields carried by this chunk.
    pub delta: OpenAiDelta,
    /// Set only on the terminal chunk, and omitted from the JSON otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

/// Incremental message fields in a streamed chunk. Unset fields are omitted from the JSON.
#[derive(Debug, Serialize)]
pub struct OpenAiDelta {
    /// Role marker, sent only on the opening chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Text fragment to append to the message so far.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    /// Tool-call entries carried by this chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<OpenAiToolCallDelta>>,
}

impl OpenAiDelta {
    /// A delta carrying only the assistant role marker (opening chunk).
    pub(super) fn with_role(role: impl Into<String>) -> Self {
        Self {
            role: Some(role.into()),
            content: None,
            tool_calls: None,
        }
    }

    /// A delta carrying a single content fragment.
    pub(super) fn with_content(content: String) -> Self {
        Self {
            role: None,
            content: Some(content),
            tool_calls: None,
        }
    }

    /// A delta carrying tool-call fragments.
    pub(super) fn with_tool_calls(tool_calls: Vec<OpenAiToolCallDelta>) -> Self {
        Self {
            role: None,
            content: None,
            tool_calls: Some(tool_calls),
        }
    }

    /// An empty delta, used by the terminal finish-reason chunk.
    pub(super) fn empty() -> Self {
        Self {
            role: None,
            content: None,
            tool_calls: None,
        }
    }
}

/// A tool call carried in a streamed chunk.
#[derive(Debug, Serialize)]
pub struct OpenAiToolCallDelta {
    /// Position of this call within the message, used by clients to merge fragments.
    pub index: u32,
    /// Call identifier. Always set here, as each call is sent whole in one chunk.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    /// Call kind, carried on the wire as `type`. Always `function` here.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub call_type: Option<String>,
    /// Name and arguments of the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub function: Option<OpenAiToolCallFunctionDelta>,
}

/// Function name and arguments carried by a streamed tool call.
#[derive(Debug, Serialize)]
pub struct OpenAiToolCallFunctionDelta {
    /// Name of the called function.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// JSON-encoded arguments string. Sent whole, not as incremental fragments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// Error response
// ---------------------------------------------------------------------------

/// Top-level error body returned for a failed request.
#[derive(Debug, Serialize)]
pub struct OpenAiErrorResponse {
    /// The error details.
    pub error: OpenAiErrorDetail,
}

/// Error payload in OpenAI form.
#[derive(Debug, Serialize)]
pub struct OpenAiErrorDetail {
    /// Human-readable description of the failure.
    pub message: String,
    /// Error category, carried on the wire as `type`, such as `authentication_error`,
    /// `rate_limit_error`, `invalid_request_error` or `server_error`.
    #[serde(rename = "type")]
    pub error_type: String,
    /// Name of the offending request parameter. Always `null` from this adapter.
    pub param: Option<String>,
    /// Machine-readable code such as `rate_limit` or `model_not_found`. `null` when none applies.
    pub code: Option<String>,
}
