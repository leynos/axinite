//! Server-sent event (SSE) and WebSocket message types for the web
//! gateway API.

use serde::{Deserialize, Serialize};

use super::chat::ImageData;

// --- SSE Event Types ---

/// Events streamed to web clients as SSE frames. Each serializes as a JSON object
/// whose `type` field names the variant. Optional fields are omitted when `None`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum SseEvent {
    /// Final assistant reply for a thread.
    #[serde(rename = "response")]
    Response {
        /// Full reply text.
        content: String,
        /// Thread the reply belongs to.
        thread_id: String,
    },
    /// Progress text while the agent is reasoning.
    #[serde(rename = "thinking")]
    Thinking {
        /// Progress text to show.
        message: String,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    /// A tool call has begun.
    #[serde(rename = "tool_started")]
    ToolStarted {
        /// Name of the tool that started.
        name: String,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    /// A tool call has finished, successfully or not.
    #[serde(rename = "tool_completed")]
    ToolCompleted {
        /// Name of the tool that finished.
        name: String,
        /// Whether the tool call succeeded.
        success: bool,
        /// Error text, present when `success` is false.
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        /// Tool input as a JSON string. Present only on failure, with sensitive
        /// values redacted.
        #[serde(skip_serializing_if = "Option::is_none")]
        parameters: Option<String>,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    /// A short preview of a tool's output.
    #[serde(rename = "tool_result")]
    ToolResult {
        /// Name of the tool that produced the output.
        name: String,
        /// Brief preview of the output, not the full text.
        preview: String,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    /// One piece of a streamed reply, to be appended in order.
    #[serde(rename = "stream_chunk")]
    StreamChunk {
        /// Text fragment of the reply.
        content: String,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    /// General status text for the thread.
    #[serde(rename = "status")]
    Status {
        /// Status text to show.
        message: String,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    /// A sandbox job has started; shown as a clickable card.
    #[serde(rename = "job_started")]
    JobStarted {
        /// Identifier of the sandbox job.
        job_id: String,
        /// Human-readable job title shown on the card.
        title: String,
        /// URL of the page where the job can be viewed.
        browse_url: String,
    },
    /// A tool call is waiting for the user to approve or deny it.
    #[serde(rename = "approval_needed")]
    ApprovalNeeded {
        /// Identifier to echo back in an `approval` message to answer this request.
        request_id: String,
        /// Name of the tool awaiting approval.
        tool_name: String,
        /// Human-readable summary of what the tool will do.
        description: String,
        /// Tool input as a JSON string, shown to the user before approval.
        parameters: String,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    /// An extension needs credentials or an OAuth sign-in.
    #[serde(rename = "auth_required")]
    AuthRequired {
        /// Extension that needs authentication.
        extension_name: String,
        /// Setup instructions to show the user, if any.
        #[serde(skip_serializing_if = "Option::is_none")]
        instructions: Option<String>,
        /// URL for an OAuth sign-in, if the extension uses one.
        #[serde(skip_serializing_if = "Option::is_none")]
        auth_url: Option<String>,
        /// Setup page for the extension, if one is provided.
        #[serde(skip_serializing_if = "Option::is_none")]
        setup_url: Option<String>,
    },
    /// An extension's authentication attempt has finished.
    #[serde(rename = "auth_completed")]
    AuthCompleted {
        /// Extension whose authentication finished.
        extension_name: String,
        /// Whether authentication succeeded.
        success: bool,
        /// Outcome text for the user.
        message: String,
    },
    /// An error to show the user.
    #[serde(rename = "error")]
    Error {
        /// Error text to show.
        message: String,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },
    /// Keep-alive event with no payload.
    #[serde(rename = "heartbeat")]
    Heartbeat,

    // Sandbox job streaming events (worker + Claude Code bridge)
    /// A message produced by a sandbox job.
    #[serde(rename = "job_message")]
    JobMessage {
        /// Identifier of the sandbox job that emitted the message.
        job_id: String,
        /// Speaker role, such as `assistant` or `user`; defaults to `assistant`.
        role: String,
        /// Message text.
        content: String,
    },
    /// A sandbox job has invoked a tool.
    #[serde(rename = "job_tool_use")]
    JobToolUse {
        /// Identifier of the sandbox job.
        job_id: String,
        /// Name of the tool being invoked.
        tool_name: String,
        /// Tool input arguments as a JSON value.
        input: serde_json::Value,
    },
    /// The result of a tool call made by a sandbox job.
    #[serde(rename = "job_tool_result")]
    JobToolResult {
        /// Identifier of the sandbox job.
        job_id: String,
        /// Name of the tool that produced the output.
        tool_name: String,
        /// Tool output text.
        output: String,
    },
    /// A status update for a sandbox job.
    #[serde(rename = "job_status")]
    JobStatus {
        /// Identifier of the sandbox job.
        job_id: String,
        /// Human-readable status text.
        message: String,
    },
    /// A sandbox job has finished.
    #[serde(rename = "job_result")]
    JobResult {
        /// Identifier of the sandbox job.
        job_id: String,
        /// Final job status, such as `completed`; `unknown` when the worker omits it.
        status: String,
        /// Session identifier recorded for the job, if one was reported.
        #[serde(skip_serializing_if = "Option::is_none")]
        session_id: Option<String>,
    },

    /// An image was generated by a tool.
    #[serde(rename = "image_generated")]
    ImageGenerated {
        /// Base64 data URL of the generated image.
        data_url: String,
        /// Workspace path where the image was saved, if it was saved.
        #[serde(skip_serializing_if = "Option::is_none")]
        path: Option<String>,
        /// Thread the event belongs to; omitted when not tied to a thread.
        #[serde(skip_serializing_if = "Option::is_none")]
        thread_id: Option<String>,
    },

    /// Extension activation status change (WASM channels).
    #[serde(rename = "extension_status")]
    ExtensionStatus {
        /// Name of the extension whose activation state changed.
        extension_name: String,
        /// New activation state, such as `active`.
        status: String,
        /// Optional detail for the change, such as an error.
        #[serde(skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

// --- WebSocket ---

/// Message sent by a WebSocket client to the server.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum WsClientMessage {
    /// Send a chat message to the agent.
    #[serde(rename = "message")]
    Message {
        /// Text of the user's chat message.
        content: String,
        /// Thread to post into; `None` when the client did not specify one.
        thread_id: Option<String>,
        /// Client timezone name. Takes precedence over the `X-Timezone` header.
        timezone: Option<String>,
        /// Optional images attached to the message.
        #[serde(default)]
        images: Vec<ImageData>,
    },
    /// Approve or deny a pending tool execution.
    #[serde(rename = "approval")]
    Approval {
        /// Request identifier from the matching `approval_needed` event.
        request_id: String,
        /// "approve", "always", or "deny"
        action: String,
        /// Thread that owns the pending approval.
        thread_id: Option<String>,
    },
    /// Submit an auth token for an extension (bypasses message pipeline).
    #[serde(rename = "auth_token")]
    AuthToken {
        /// Extension the token is for.
        extension_name: String,
        /// Credential the user entered for the extension.
        token: String,
    },
    /// Cancel an in-progress auth flow.
    #[serde(rename = "auth_cancel")]
    AuthCancel {
        /// Extension whose pending auth flow should be cancelled.
        extension_name: String,
    },
    /// Client heartbeat ping.
    #[serde(rename = "ping")]
    Ping,
}

/// Message sent by the server to a WebSocket client.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum WsServerMessage {
    /// An SSE-style event forwarded over WebSocket.
    #[serde(rename = "event")]
    Event {
        /// The event sub-type (response, thinking, tool_started, etc.)
        event_type: String,
        /// The event payload as a JSON value.
        data: serde_json::Value,
    },
    /// Server heartbeat pong.
    #[serde(rename = "pong")]
    Pong,
    /// Error message.
    #[serde(rename = "error")]
    Error {
        /// Error text to show the client.
        message: String,
    },
}

impl WsServerMessage {
    /// Create a WsServerMessage from an SseEvent.
    pub fn from_sse_event(event: &SseEvent) -> Self {
        let event_type = match event {
            SseEvent::Response { .. } => "response",
            SseEvent::Thinking { .. } => "thinking",
            SseEvent::ToolStarted { .. } => "tool_started",
            SseEvent::ToolCompleted { .. } => "tool_completed",
            SseEvent::ToolResult { .. } => "tool_result",
            SseEvent::StreamChunk { .. } => "stream_chunk",
            SseEvent::Status { .. } => "status",
            SseEvent::JobStarted { .. } => "job_started",
            SseEvent::ApprovalNeeded { .. } => "approval_needed",
            SseEvent::AuthRequired { .. } => "auth_required",
            SseEvent::AuthCompleted { .. } => "auth_completed",
            SseEvent::Error { .. } => "error",
            SseEvent::Heartbeat => "heartbeat",
            SseEvent::JobMessage { .. } => "job_message",
            SseEvent::JobToolUse { .. } => "job_tool_use",
            SseEvent::JobToolResult { .. } => "job_tool_result",
            SseEvent::JobStatus { .. } => "job_status",
            SseEvent::JobResult { .. } => "job_result",
            SseEvent::ImageGenerated { .. } => "image_generated",
            SseEvent::ExtensionStatus { .. } => "extension_status",
        };
        let data = serde_json::to_value(event).unwrap_or(serde_json::Value::Null);
        WsServerMessage::Event {
            event_type: event_type.to_string(),
            data,
        }
    }
}
