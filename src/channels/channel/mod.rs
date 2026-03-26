//! Channel trait and message types.

use core::future::Future;
use std::collections::HashMap;
use std::pin::Pin;

use chrono::{DateTime, Utc};
use futures::Stream;
use uuid::Uuid;

use crate::error::ChannelError;

/// Kind of attachment carried on an incoming message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachmentKind {
    /// Audio content (voice notes, audio files).
    Audio,
    /// Image content (photos, screenshots).
    Image,
    /// Document content (PDFs, files).
    Document,
}

impl AttachmentKind {
    /// Infer attachment kind from MIME type.
    pub fn from_mime_type(mime: &str) -> Self {
        let base = mime.split(';').next().unwrap_or(mime).trim();
        if base.starts_with("audio/") {
            Self::Audio
        } else if base.starts_with("image/") {
            Self::Image
        } else {
            Self::Document
        }
    }
}

/// A file or media attachment on an incoming message.
#[derive(Debug, Clone)]
pub struct IncomingAttachment {
    /// Unique identifier within the channel (e.g., Telegram file_id).
    pub id: String,
    /// What kind of content this is.
    pub kind: AttachmentKind,
    /// MIME type (e.g., "image/jpeg", "audio/ogg", "application/pdf").
    pub mime_type: String,
    /// Original filename, if known.
    pub filename: Option<String>,
    /// File size in bytes, if known.
    pub size_bytes: Option<u64>,
    /// URL to download the file from the channel's API.
    pub source_url: Option<String>,
    /// Opaque key for host-side storage (e.g., after download/caching).
    pub storage_key: Option<String>,
    /// Extracted text content (e.g., OCR result, PDF text, audio transcript).
    pub extracted_text: Option<String>,
    /// Raw file bytes (for small files downloaded by the channel).
    pub data: Vec<u8>,
    /// Duration in seconds (for audio/video).
    pub duration_secs: Option<u32>,
}

/// A message received from an external channel.
#[derive(Debug, Clone)]
pub struct IncomingMessage {
    /// Unique message ID.
    pub id: Uuid,
    /// Channel this message came from.
    pub channel: String,
    /// User identifier within the channel.
    pub user_id: String,
    /// Optional display name.
    pub user_name: Option<String>,
    /// Message content.
    pub content: String,
    /// Thread/conversation ID for threaded conversations.
    pub thread_id: Option<String>,
    /// When the message was received.
    pub received_at: DateTime<Utc>,
    /// Channel-specific metadata.
    pub metadata: serde_json::Value,
    /// IANA timezone string from the client (e.g. "America/New_York").
    pub timezone: Option<String>,
    /// File or media attachments on this message.
    pub attachments: Vec<IncomingAttachment>,
}

impl IncomingMessage {
    /// Create a new incoming message.
    pub fn new(
        channel: impl Into<String>,
        user_id: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            channel: channel.into(),
            user_id: user_id.into(),
            user_name: None,
            content: content.into(),
            thread_id: None,
            received_at: Utc::now(),
            metadata: serde_json::Value::Null,
            timezone: None,
            attachments: Vec::new(),
        }
    }

    /// Set the thread ID.
    pub fn with_thread(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }

    /// Set metadata.
    pub fn with_metadata(mut self, metadata: serde_json::Value) -> Self {
        self.metadata = metadata;
        self
    }

    /// Set user name.
    pub fn with_user_name(mut self, name: impl Into<String>) -> Self {
        self.user_name = Some(name.into());
        self
    }

    /// Set the client timezone.
    pub fn with_timezone(mut self, tz: impl Into<String>) -> Self {
        self.timezone = Some(tz.into());
        self
    }

    /// Set attachments.
    pub fn with_attachments(mut self, attachments: Vec<IncomingAttachment>) -> Self {
        self.attachments = attachments;
        self
    }
}

/// Stream of incoming messages.
pub type MessageStream = Pin<Box<dyn Stream<Item = IncomingMessage> + Send>>;

/// Response to send back to a channel.
#[derive(Debug, Clone)]
pub struct OutgoingResponse {
    /// The content to send.
    pub content: String,
    /// Optional thread ID to reply in.
    pub thread_id: Option<String>,
    /// Optional file paths to attach.
    pub attachments: Vec<String>,
    /// Channel-specific metadata for the response.
    pub metadata: serde_json::Value,
}

impl OutgoingResponse {
    /// Create a simple text response.
    pub fn text(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            thread_id: None,
            attachments: Vec::new(),
            metadata: serde_json::Value::Null,
        }
    }

    /// Set the thread ID for the response.
    pub fn in_thread(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }

    /// Add attachments to the response.
    pub fn with_attachments(mut self, paths: Vec<String>) -> Self {
        self.attachments = paths;
        self
    }
}

/// Status update types for showing agent activity.
#[derive(Debug, Clone)]
pub enum StatusUpdate {
    /// Agent is thinking/processing.
    Thinking(String),
    /// Tool execution started.
    ToolStarted { name: String },
    /// Tool execution completed.
    ///
    /// Use [`StatusUpdate::tool_completed`] to construct this variant — it
    /// handles redaction of sensitive parameters and keeps the 9-line pattern
    /// in one place.
    ToolCompleted {
        name: String,
        success: bool,
        /// Error message when success is false.
        error: Option<String>,
        /// Tool input parameters (JSON string) for display on failure.
        /// Only populated when `success` is `false`. Values listed in the
        /// tool's `sensitive_params()` are replaced with `"[REDACTED]"`.
        parameters: Option<String>,
    },
    /// Brief preview of tool execution output.
    ToolResult { name: String, preview: String },
    /// Streaming text chunk.
    StreamChunk(String),
    /// General status message.
    Status(String),
    /// A sandbox job has started (shown as a clickable card in the UI).
    JobStarted {
        job_id: String,
        title: String,
        browse_url: String,
    },
    /// Tool requires user approval before execution.
    ApprovalNeeded {
        request_id: String,
        tool_name: String,
        description: String,
        parameters: serde_json::Value,
    },
    /// Extension needs user authentication (token or OAuth).
    AuthRequired {
        extension_name: String,
        instructions: Option<String>,
        auth_url: Option<String>,
        setup_url: Option<String>,
    },
    /// Extension authentication completed.
    AuthCompleted {
        extension_name: String,
        success: bool,
        message: String,
    },
    /// An image was generated by a tool.
    ImageGenerated {
        /// Base64 data URL of the generated image.
        data_url: String,
        /// Optional workspace path where the image was saved.
        path: Option<String>,
    },
}

impl StatusUpdate {
    /// Build a `ToolCompleted` status with redacted parameters.
    ///
    /// On failure, serializes the tool's input parameters as pretty JSON after
    /// replacing any keys listed in the tool's `sensitive_params()` with
    /// `"[REDACTED]"`. On success, no parameters or error are included.
    ///
    /// Pass the resolved `Tool` reference (if available) so this method can
    /// query `sensitive_params()` directly — callers don't need to manage the
    /// borrow lifetime of the sensitive slice.
    pub fn tool_completed(
        name: String,
        result: &Result<String, crate::error::Error>,
        params: &serde_json::Value,
        tool: Option<&dyn crate::tools::Tool>,
    ) -> Self {
        let success = result.is_ok();
        let sensitive = tool.map(|t| t.sensitive_params()).unwrap_or(&[]);
        Self::ToolCompleted {
            name,
            success,
            error: result.as_ref().err().map(|e| e.to_string()),
            parameters: if !success {
                let safe = crate::tools::redact_params(params, sensitive);
                Some(serde_json::to_string_pretty(&safe).unwrap_or_else(|_| safe.to_string()))
            } else {
                None
            },
        }
    }
}

/// Boxed future used at the dyn `Channel` boundary.
pub type ChannelFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for message channels.
///
/// This is the dyn-safe object boundary. Concrete implementations should
/// implement [`NativeChannel`] instead; the blanket adapter provides this
/// trait automatically.
///
/// Channels receive messages from external sources and convert them to
/// a unified format. They also handle sending responses back.
pub trait Channel: Send + Sync {
    /// Get the channel name (e.g., "cli", "slack", "telegram", "http").
    fn name(&self) -> &str;

    /// Start listening for messages.
    ///
    /// Returns a stream of incoming messages. The channel should handle
    /// reconnection and error recovery internally.
    fn start<'a>(&'a self) -> ChannelFuture<'a, Result<MessageStream, ChannelError>>;

    /// Send a response back to the user.
    ///
    /// The response is sent in the context of the original message
    /// (same channel, same thread if applicable).
    fn respond<'a>(
        &'a self,
        msg: &'a IncomingMessage,
        response: OutgoingResponse,
    ) -> ChannelFuture<'a, Result<(), ChannelError>>;

    /// Send a status update (thinking, tool execution, etc.).
    ///
    /// The metadata contains channel-specific routing info (e.g., Telegram chat_id)
    /// needed to deliver the status to the correct destination.
    ///
    /// Default implementation does nothing (for channels that don't support status).
    fn send_status<'a>(
        &'a self,
        _status: StatusUpdate,
        _metadata: &'a serde_json::Value,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(async { Ok(()) })
    }

    /// Send a proactive message without a prior incoming message.
    ///
    /// Used for alerts, heartbeat notifications, and other agent-initiated communication.
    /// The user_id helps target a specific user within the channel.
    ///
    /// Default implementation does nothing (for channels that don't support broadcast).
    fn broadcast<'a>(
        &'a self,
        _user_id: &'a str,
        _response: OutgoingResponse,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(async { Ok(()) })
    }

    /// Check if the channel is healthy.
    fn health_check<'a>(&'a self) -> ChannelFuture<'a, Result<(), ChannelError>>;

    /// Get conversation context from message metadata for system prompt.
    ///
    /// Returns key-value pairs like "sender", "sender_uuid", "group" that
    /// help the LLM understand who it's talking to.
    ///
    /// Default implementation returns empty map.
    fn conversation_context(&self, _metadata: &serde_json::Value) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Gracefully shut down the channel.
    fn shutdown<'a>(&'a self) -> ChannelFuture<'a, Result<(), ChannelError>>;
}

/// Native (non-dyn) sibling of [`Channel`] for concrete implementations.
///
/// Implement this trait instead of [`Channel`] directly. The blanket adapter
/// below automatically implements [`Channel`] for every `T: NativeChannel`.
pub trait NativeChannel: Send + Sync {
    /// Get the channel name (e.g., "cli", "slack", "telegram", "http").
    fn name(&self) -> &str;

    /// Start listening for messages.
    fn start(&self) -> impl Future<Output = Result<MessageStream, ChannelError>> + Send + '_;

    /// Send a response back to the user.
    fn respond<'a>(
        &'a self,
        msg: &'a IncomingMessage,
        response: OutgoingResponse,
    ) -> impl Future<Output = Result<(), ChannelError>> + Send + 'a;

    /// Send a status update (thinking, tool execution, etc.).
    ///
    /// Default implementation does nothing (for channels that don't support status).
    fn send_status<'a>(
        &'a self,
        _status: StatusUpdate,
        _metadata: &'a serde_json::Value,
    ) -> impl Future<Output = Result<(), ChannelError>> + Send + 'a {
        async { Ok(()) }
    }

    /// Send a proactive message without a prior incoming message.
    ///
    /// Default implementation does nothing (for channels that don't support broadcast).
    fn broadcast<'a>(
        &'a self,
        _user_id: &'a str,
        _response: OutgoingResponse,
    ) -> impl Future<Output = Result<(), ChannelError>> + Send + 'a {
        async { Ok(()) }
    }

    /// Check if the channel is healthy.
    fn health_check(&self) -> impl Future<Output = Result<(), ChannelError>> + Send + '_;

    /// Get conversation context from message metadata for system prompt.
    ///
    /// Default implementation returns empty map.
    fn conversation_context(&self, _metadata: &serde_json::Value) -> HashMap<String, String> {
        HashMap::new()
    }

    /// Gracefully shut down the channel.
    fn shutdown(&self) -> impl Future<Output = Result<(), ChannelError>> + Send + '_ {
        async { Ok(()) }
    }
}

impl<T: NativeChannel> Channel for T {
    fn name(&self) -> &str {
        NativeChannel::name(self)
    }

    fn start<'a>(&'a self) -> ChannelFuture<'a, Result<MessageStream, ChannelError>> {
        Box::pin(NativeChannel::start(self))
    }

    fn respond<'a>(
        &'a self,
        msg: &'a IncomingMessage,
        response: OutgoingResponse,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::respond(self, msg, response))
    }

    fn send_status<'a>(
        &'a self,
        status: StatusUpdate,
        metadata: &'a serde_json::Value,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::send_status(self, status, metadata))
    }

    fn broadcast<'a>(
        &'a self,
        user_id: &'a str,
        response: OutgoingResponse,
    ) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::broadcast(self, user_id, response))
    }

    fn health_check<'a>(&'a self) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::health_check(self))
    }

    fn conversation_context(&self, metadata: &serde_json::Value) -> HashMap<String, String> {
        NativeChannel::conversation_context(self, metadata)
    }

    fn shutdown<'a>(&'a self) -> ChannelFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeChannel::shutdown(self))
    }
}

/// Boxed future used at the dyn channel-secret-updater boundary.
pub type ChannelSecretUpdaterFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// Trait for channels that support hot-secret-swapping during SIGHUP reload.
///
/// This allows channels to update authentication credentials without restarting,
/// enabling zero-downtime configuration reloads. Channels that don't support
/// secret updates can simply not implement this trait.
pub trait ChannelSecretUpdater: Send + Sync {
    /// Update the secret for this channel.
    ///
    /// Called during SIGHUP configuration reload. Implementation should:
    /// - Apply the new secret atomically
    /// - Not fail the entire reload if secret update fails
    /// - Log appropriate errors/info messages
    ///
    /// The secret is optional (may be None if secret is no longer configured).
    fn update_secret<'a>(
        &'a self,
        new_secret: Option<secrecy::SecretString>,
    ) -> ChannelSecretUpdaterFuture<'a>;
}

/// Native async sibling trait for concrete channel-secret-updater implementations.
pub trait NativeChannelSecretUpdater: Send + Sync {
    /// See [`ChannelSecretUpdater::update_secret`].
    fn update_secret(
        &self,
        new_secret: Option<secrecy::SecretString>,
    ) -> impl Future<Output = ()> + Send + '_;
}

impl<T> ChannelSecretUpdater for T
where
    T: NativeChannelSecretUpdater + Send + Sync,
{
    fn update_secret<'a>(
        &'a self,
        new_secret: Option<secrecy::SecretString>,
    ) -> ChannelSecretUpdaterFuture<'a> {
        Box::pin(NativeChannelSecretUpdater::update_secret(self, new_secret))
    }
}

#[cfg(test)]
mod tests;
