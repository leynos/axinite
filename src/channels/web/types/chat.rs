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

#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    pub content: String,
    pub thread_id: Option<String>,
    pub timezone: Option<String>,
    /// Optional images attached to the message.
    #[serde(default)]
    pub images: Vec<ImageData>,
}

#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub message_id: Uuid,
    pub status: &'static str,
}

#[derive(Debug, Serialize)]
pub struct ThreadInfo {
    pub id: Uuid,
    pub state: String,
    pub turn_count: usize,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thread_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ThreadListResponse {
    /// The pinned assistant thread (always present after first load).
    pub assistant_thread: Option<ThreadInfo>,
    /// Regular conversation threads.
    pub threads: Vec<ThreadInfo>,
    pub active_thread: Option<Uuid>,
}

#[derive(Debug, Serialize)]
pub struct TurnInfo {
    pub turn_number: usize,
    pub user_input: String,
    pub response: Option<String>,
    pub state: String,
    pub started_at: String,
    pub completed_at: Option<String>,
    pub tool_calls: Vec<ToolCallInfo>,
}

#[derive(Debug, Serialize)]
pub struct ToolCallInfo {
    pub name: String,
    pub has_result: bool,
    pub has_error: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result_preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct HistoryResponse {
    pub thread_id: Uuid,
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
    pub request_id: String,
    pub tool_name: String,
    pub description: String,
    pub parameters: String,
}

// --- Approval ---

#[derive(Debug, Deserialize)]
pub struct ApprovalRequest {
    pub request_id: String,
    /// "approve", "always", or "deny"
    pub action: String,
    /// Thread that owns the pending approval (so the agent loop finds the right session).
    pub thread_id: Option<String>,
}
