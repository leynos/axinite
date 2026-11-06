//! Chat and approval DTOs for the web gateway API.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// --- Chat ---

/// Base64-encoded image data sent from the web frontend.
#[derive(Debug, Clone, Deserialize)]
pub struct ImageData {
    /// MIME type (e.g., "image/png", "image/jpeg").
    pub media_type: String,
    /// Base64-encoded image data (without data: URL prefix).
    pub data: String,
}

/// Request body for posting a user message to the agent.
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    /// The message text as typed by the user.
    pub content: String,
    /// Thread to post into, as a UUID string. `None` leaves the message unassigned to a thread.
    pub thread_id: Option<String>,
    /// Client timezone name, used when the message is processed. When absent, the
    /// `X-Timezone` request header is used instead.
    pub timezone: Option<String>,
    /// Optional images attached to the message.
    #[serde(default)]
    pub images: Vec<ImageData>,
}

/// Response returned once a chat message has been accepted for processing.
#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    /// Identifier assigned to the accepted message.
    pub message_id: Uuid,
    /// Acceptance status; currently always `"accepted"`.
    pub status: &'static str,
}

/// Summary of one conversation thread, as listed in the thread index.
#[derive(Debug, Serialize)]
pub struct ThreadInfo {
    /// Thread identifier.
    pub id: Uuid,
    /// State label: `"Idle"` for threads read from storage, otherwise the debug name of the
    /// in-memory thread state.
    pub state: String,
    /// Number of turns recorded in the thread.
    pub turn_count: usize,
    /// Creation time as an RFC 3339 timestamp.
    pub created_at: String,
    /// Time of last activity as an RFC 3339 timestamp.
    pub updated_at: String,
    /// Display title, omitted when the thread has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Thread category such as `"assistant"` or `"thread"`, omitted when unknown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_type: Option<String>,
    /// Name of the channel the thread originated on, such as `"gateway"`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

/// Response listing the user's threads and the active one.
#[derive(Debug, Serialize)]
pub struct ThreadListResponse {
    /// The pinned assistant thread (always present after first load).
    pub assistant_thread: Option<ThreadInfo>,
    /// Regular conversation threads.
    pub threads: Vec<ThreadInfo>,
    /// Identifier of the thread currently selected, if any.
    pub active_thread: Option<Uuid>,
}

/// One user message and the agent's handling of it within a thread.
#[derive(Debug, Serialize)]
pub struct TurnInfo {
    /// Zero-based position of the turn within its thread.
    pub turn_number: usize,
    /// The user message that started the turn.
    pub user_input: String,
    /// The agent's reply, or `None` while no reply has been recorded.
    pub response: Option<String>,
    /// Turn state label: `"Processing"`, `"Completed"`, `"Failed"` or `"Interrupted"`.
    pub state: String,
    /// Start time as an RFC 3339 timestamp.
    pub started_at: String,
    /// Completion time as an RFC 3339 timestamp, `None` while the turn is still running.
    pub completed_at: Option<String>,
    /// Tool calls made during the turn, in the order they were issued.
    pub tool_calls: Vec<ToolCallInfo>,
}

/// Summary of one tool call made during a turn.
#[derive(Debug, Serialize)]
pub struct ToolCallInfo {
    /// Name of the tool that was called.
    pub name: String,
    /// Whether the call produced a result value.
    pub has_result: bool,
    /// Whether the call recorded an error.
    pub has_error: bool,
    /// Result text cut to about 500 bytes for display, with `...` appended when shortened.
    /// Omitted when there is no result.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
    /// Error message recorded for the call, omitted when there is none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Page of conversation history for one thread.
#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    /// Thread the history belongs to.
    pub thread_id: Uuid,
    /// Turns included in this page.
    pub turns: Vec<TurnInfo>,
    /// Whether there are older messages available.
    #[serde(default)]
    pub has_more: bool,
    /// Cursor for the next page as an opaque `timestamp|message_id` token.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oldest_timestamp: Option<String>,
    /// Pending tool approval that needs user action (re-rendered on thread switch).
    ///
    /// Only populated from in-memory state; not persisted to DB.
    /// Server restart clears pending approvals.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pending_approval: Option<PendingApprovalInfo>,
}

/// Lightweight DTO for a pending tool approval (excludes context_messages).
#[derive(Debug, Serialize)]
pub struct PendingApprovalInfo {
    /// Identifier to echo back in the approval response.
    pub request_id: String,
    /// Name of the tool awaiting approval.
    pub tool_name: String,
    /// Human-readable summary of the pending tool call.
    pub description: String,
    /// Tool input parameters, pretty-printed as a JSON string.
    pub parameters: String,
}

// --- Approval ---

/// Request body answering a pending tool approval.
#[derive(Debug, Deserialize)]
pub struct ApprovalRequest {
    /// Identifier of the approval request being answered.
    pub request_id: String,
    /// "approve", "always", or "deny"
    pub action: String,
    /// Thread that owns the pending approval (so the agent loop finds the right session).
    pub thread_id: Option<String>,
}
