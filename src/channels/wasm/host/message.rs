//! Message and attachment types emitted by WASM channels.
//!
//! Defines [`Attachment`], [`EmittedMessage`], and [`PendingWorkspaceWrite`]
//! — the data carried from channel callbacks back to the host.

use std::time::{SystemTime, UNIX_EPOCH};

/// A file or media attachment on an incoming message.
#[derive(Debug, Clone)]
pub struct Attachment {
    /// Unique identifier within the channel (e.g., Telegram file_id).
    pub id: String,
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

/// A message emitted by a WASM channel to be sent to the agent.
#[derive(Debug, Clone)]
pub struct EmittedMessage {
    /// User identifier within the channel.
    pub user_id: String,

    /// Optional user display name.
    pub user_name: Option<String>,

    /// Message content.
    pub content: String,

    /// Optional thread ID for threaded conversations.
    pub thread_id: Option<String>,

    /// Channel-specific metadata as JSON string.
    pub metadata_json: String,

    /// File or media attachments on this message.
    pub attachments: Vec<Attachment>,

    /// Timestamp when the message was emitted.
    pub emitted_at_millis: u64,
}

impl EmittedMessage {
    /// Create a new emitted message.
    pub fn new(user_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            user_name: None,
            content: content.into(),
            thread_id: None,
            metadata_json: "{}".to_string(),
            attachments: Vec::new(),
            emitted_at_millis: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        }
    }

    /// Set the user name.
    pub fn with_user_name(mut self, name: impl Into<String>) -> Self {
        self.user_name = Some(name.into());
        self
    }

    /// Set the thread ID.
    pub fn with_thread_id(mut self, thread_id: impl Into<String>) -> Self {
        self.thread_id = Some(thread_id.into());
        self
    }

    /// Set metadata JSON.
    pub fn with_metadata(mut self, metadata_json: impl Into<String>) -> Self {
        self.metadata_json = metadata_json.into();
        self
    }

    /// Set attachments.
    pub fn with_attachments(mut self, attachments: Vec<Attachment>) -> Self {
        self.attachments = attachments;
        self
    }
}

/// A pending workspace write operation.
#[derive(Debug, Clone)]
pub struct PendingWorkspaceWrite {
    /// Full path (already prefixed with channel namespace).
    pub path: String,

    /// Content to write.
    pub content: String,
}
