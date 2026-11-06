//! Trace fixture format types for recorded LLM sessions.
//!
//! These types define the JSON schema written by `RecordingLlm` and
//! consumed by `TraceLlm` during deterministic replay.

use serde::{Deserialize, Serialize};

/// Top-level trace file — extended format with memory snapshot and HTTP exchanges.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceFile {
    /// Name of the model the trace was recorded against.
    pub model_name: String,
    /// Workspace memory documents captured before the recording session.
    /// Replay should restore these before running the trace.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory_snapshot: Vec<MemorySnapshotEntry>,
    /// HTTP exchanges recorded during the session, in order.
    /// Replay should return these instead of making real HTTP requests.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub http_exchanges: Vec<HttpExchange>,
    /// Ordered steps for replay: LLM responses and `user_input` markers.
    pub steps: Vec<TraceStep>,
}

/// A memory document captured at recording start.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemorySnapshotEntry {
    /// Workspace path of the memory document.
    pub path: String,
    /// Full text of the document when the recording started.
    pub content: String,
}

/// A recorded HTTP request/response pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpExchange {
    /// The request that was sent.
    pub request: HttpExchangeRequest,
    /// The response that came back.
    pub response: HttpExchangeResponse,
}

/// The request side of an HTTP exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpExchangeRequest {
    /// HTTP method, such as `GET` or `POST`.
    pub method: String,
    /// Full request URL.
    pub url: String,
    /// Request headers as ordered `(name, value)` pairs. Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    /// Request body as a string. `None` when the request had no body.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}

/// The response side of an HTTP exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HttpExchangeResponse {
    /// HTTP status code of the response.
    pub status: u16,
    /// Response headers as ordered `(name, value)` pairs. Omitted when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<(String, String)>,
    /// Response body as a string.
    pub body: String,
}

/// A single step in the trace.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceStep {
    /// Soft match hint checked against the incoming request. `None` for `user_input` markers.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_hint: Option<RequestHint>,
    /// The LLM response for this step, or a `user_input` marker.
    pub response: TraceResponse,
    /// Tool results that appeared in the message context since the previous step.
    /// During replay, the test harness can compare actual tool results against
    /// these to verify tool output hasn't changed (regression detection).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub expected_tool_results: Vec<ExpectedToolResult>,
}

/// Soft validation hints for matching a step to a request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestHint {
    /// Start of the last user message at recording time, cut to at most 80 bytes on a
    /// character boundary.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_user_message_contains: Option<String>,
    /// Number of messages in the request at recording time, used as a soft match.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_message_count: Option<usize>,
}

/// Tagged response enum — text, tool_calls, or user_input.
///
/// `user_input` steps are metadata markers — they record what the user said
/// but do **not** correspond to an LLM call. During replay, `TraceLlm` must
/// skip `user_input` steps and only consume `text`/`tool_calls` steps.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TraceResponse {
    /// Plain-text assistant reply.
    Text {
        /// Text of the assistant reply.
        content: String,
        /// Prompt tokens reported for the call.
        input_tokens: u32,
        /// Completion tokens reported for the call.
        output_tokens: u32,
    },
    /// Assistant reply that asks for one or more tool calls.
    ToolCalls {
        /// Tool calls requested by the assistant, in the order it issued them.
        tool_calls: Vec<TraceToolCall>,
        /// Prompt tokens reported for the call.
        input_tokens: u32,
        /// Completion tokens reported for the call.
        output_tokens: u32,
    },
    /// Marker for a user message that triggered subsequent LLM calls.
    /// Not an LLM response — replay providers must skip these.
    UserInput {
        /// Text of the user message.
        content: String,
    },
}

/// A tool call in a trace step.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraceToolCall {
    /// Provider-assigned tool call ID, echoed back in the matching tool result.
    pub id: String,
    /// Name of the tool called.
    pub name: String,
    /// JSON arguments the model supplied for the call.
    pub arguments: serde_json::Value,
}

/// Recorded tool result for regression checking during replay.
///
/// During replay, after tools execute and before returning the canned LLM
/// response, the test harness should compare actual `Role::Tool` messages
/// against these entries. A content mismatch indicates a tool behaviour change.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpectedToolResult {
    /// ID of the tool call this result answers.
    pub tool_call_id: String,
    /// Name of the tool that produced the result.
    pub name: String,
    /// The full tool result content as it appeared in the message context.
    pub content: String,
}
