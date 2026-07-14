//! Conversation message types for LLM providers.
//!
//! Defines roles, multimodal content parts, the `ChatMessage` type and its
//! constructors, plus `sanitize_tool_messages` which repairs orphaned
//! tool-result messages before requests are sent to a provider.

use serde::{Deserialize, Serialize};

use super::ToolCall;

/// Role in a conversation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

/// A part of multimodal message content (OpenAI Chat Completions format).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentPart {
    /// Text content part.
    #[serde(rename = "text")]
    Text { text: String },
    /// Image URL content part (supports data: URLs for inline base64 images).
    #[serde(rename = "image_url")]
    ImageUrl { image_url: ImageUrl },
}

/// Image URL reference for multimodal content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageUrl {
    /// URL or data: URI (e.g., "data:image/jpeg;base64,...").
    pub url: String,
    /// Detail level hint: "auto", "low", or "high".
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
    /// Multimodal content parts (images, etc.).
    /// When non-empty, providers serialize content as an array of parts
    /// (with `content` included as a text part) instead of a plain string.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub content_parts: Vec<ContentPart>,
    /// Tool call ID if this is a tool result message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// Name of the tool for tool results.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Tool calls made by the assistant (OpenAI protocol requires these
    /// to appear on the assistant message preceding tool result messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
}

impl ChatMessage {
    /// Create a system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
            content_parts: Vec::new(),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        }
    }

    /// Create a user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            content_parts: Vec::new(),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        }
    }

    /// Create a user message with multimodal content parts (e.g., images).
    ///
    /// The text `content` is included as the primary text alongside the parts.
    pub fn user_with_parts(content: impl Into<String>, parts: Vec<ContentPart>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
            content_parts: parts,
            tool_call_id: None,
            name: None,
            tool_calls: None,
        }
    }

    /// Create an assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.into(),
            content_parts: Vec::new(),
            tool_call_id: None,
            name: None,
            tool_calls: None,
        }
    }

    /// Create an assistant message that includes tool calls.
    ///
    /// Per the OpenAI protocol, an assistant message with tool_calls must
    /// precede the corresponding tool result messages in the conversation.
    pub fn assistant_with_tool_calls(content: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: content.unwrap_or_default(),
            content_parts: Vec::new(),
            tool_call_id: None,
            name: None,
            tool_calls: if tool_calls.is_empty() {
                None
            } else {
                Some(tool_calls)
            },
        }
    }

    /// Create a tool result message.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: Role::Tool,
            content: content.into(),
            content_parts: Vec::new(),
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
            tool_calls: None,
        }
    }
}

/// Sanitize a message list to ensure tool_use / tool_result integrity.
///
/// LLM APIs (especially Anthropic) require every tool_result to reference a
/// tool_call_id that exists in an immediately preceding assistant message's
/// tool_calls. Orphaned tool_results cause HTTP 400 errors.
///
/// This function:
/// 1. Tracks all tool_call_ids emitted by assistant messages.
/// 2. Rewrites orphaned tool_result messages (whose tool_call_id has no
///    matching assistant tool_call) as user messages so the content is
///    preserved without violating the protocol.
///
/// Call this before sending messages to any LLM provider.
pub fn sanitize_tool_messages(messages: &mut [ChatMessage]) {
    use std::collections::HashSet;

    // Collect all tool_call_ids from assistant messages with tool_calls.
    let mut known_ids: HashSet<String> = HashSet::new();
    for msg in messages.iter() {
        if msg.role == Role::Assistant
            && let Some(ref calls) = msg.tool_calls
        {
            for tc in calls {
                known_ids.insert(tc.id.clone());
            }
        }
    }

    // Rewrite orphaned tool_result messages as user messages.
    for msg in messages.iter_mut() {
        if msg.role != Role::Tool {
            continue;
        }
        let is_orphaned = match &msg.tool_call_id {
            Some(id) => !known_ids.contains(id),
            None => true,
        };
        if is_orphaned {
            let tool_name = msg.name.as_deref().unwrap_or("unknown");
            tracing::debug!(
                tool_call_id = ?msg.tool_call_id,
                tool_name,
                "Rewriting orphaned tool_result as user message",
            );
            msg.role = Role::User;
            msg.content = format!("[Tool `{}` returned: {}]", tool_name, msg.content);
            msg.tool_call_id = None;
            msg.name = None;
        }
    }
}
