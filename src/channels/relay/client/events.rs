//! Relay event and connection data models.
//!
//! Deserializable types matching the channel-relay wire format: parsed SSE
//! channel events, known event-type constants, and connection listings.

use serde::{Deserialize, Serialize};

pub mod event_types {
    //! Known relay event types: string constants naming each event.

    /// A message posted in a channel the bot can see.
    pub const MESSAGE: &str = "message";
    /// A direct message sent to the bot.
    pub const DIRECT_MESSAGE: &str = "direct_message";
    /// A message that mentions the bot.
    pub const MENTION: &str = "mention";
}

/// A parsed SSE event from the channel-relay stream.
///
/// Field names match the channel-relay `ChannelEvent` struct exactly.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelEvent {
    /// Unique event ID.
    #[serde(default)]
    pub id: String,
    /// Event type enum from channel-relay (e.g., "direct_message", "message", "mention").
    pub event_type: String,
    /// Provider (e.g., "slack").
    #[serde(default)]
    pub provider: String,
    /// Team/workspace ID (called `provider_scope` in channel-relay).
    #[serde(alias = "team_id", default)]
    pub provider_scope: String,
    /// Channel or DM conversation ID.
    #[serde(default)]
    pub channel_id: String,
    /// Sender user ID.
    #[serde(default)]
    pub sender_id: String,
    /// Sender display name.
    #[serde(default)]
    pub sender_name: Option<String>,
    /// Message text content (called `content` in channel-relay).
    #[serde(alias = "text", default)]
    pub content: Option<String>,
    /// Thread ID (for threaded replies, called `thread_id` in channel-relay).
    #[serde(alias = "thread_ts", default)]
    pub thread_id: Option<String>,
    /// Full raw event data.
    #[serde(default)]
    pub raw: serde_json::Value,
    /// Event timestamp (ISO 8601 from channel-relay).
    #[serde(default)]
    pub timestamp: Option<String>,
}

impl ChannelEvent {
    /// Get the team_id (provider_scope).
    pub fn team_id(&self) -> &str {
        &self.provider_scope
    }

    /// Get the message text content.
    pub fn text(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }

    /// Get the sender name or fallback to sender_id.
    pub fn display_name(&self) -> &str {
        self.sender_name.as_deref().unwrap_or(&self.sender_id)
    }

    /// Check if this is a message-like event that should be forwarded to the agent.
    pub fn is_message(&self) -> bool {
        matches!(
            self.event_type.as_str(),
            event_types::MESSAGE | event_types::DIRECT_MESSAGE | event_types::MENTION
        )
    }
}

/// Connection info returned by list_connections.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    /// Provider identifier, such as `slack`.
    pub provider: String,
    /// Workspace identifier the connection is bound to, matching `ChannelEvent::provider_scope`.
    pub team_id: String,
    /// Workspace display name, or `None` when the relay does not supply one.
    pub team_name: Option<String>,
    /// Whether the connection is currently active.
    pub connected: bool,
}
