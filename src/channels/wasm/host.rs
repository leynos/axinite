//! Host state for WASM channel execution.
//!
//! Extends the base tool host state with channel-specific functionality:
//! - Message emission (queueing messages to send to the agent)
//! - Workspace write access (scoped to channel namespace)
//! - Rate limiting for message emission
//!
//! The module is split into submodules by concern:
//! - [`message`] — emitted message and attachment types
//! - [`state`] — per-callback host state and limit enforcement
//! - [`store`] — cross-callback workspace store and emit rate limiter

mod message;
mod state;
mod store;

#[cfg(test)]
mod tests;

pub use message::{Attachment, EmittedMessage};
pub use state::ChannelHostState;
pub use store::{ChannelEmitRateLimiter, ChannelWorkspaceStore};

/// Maximum emitted messages per callback execution.
const MAX_EMITS_PER_EXECUTION: usize = 100;

/// Maximum message content size (64 KB).
const MAX_MESSAGE_CONTENT_SIZE: usize = 64 * 1024;

/// Maximum total attachment size per message (20 MB).
const MAX_ATTACHMENT_TOTAL_SIZE: u64 = 20 * 1024 * 1024;

/// Maximum number of attachments per message.
const MAX_ATTACHMENTS_PER_MESSAGE: usize = 10;

/// Allowed MIME type prefixes for attachments.
const ALLOWED_MIME_PREFIXES: &[&str] = &[
    "image/",
    "audio/",
    "video/",
    "application/pdf",
    "application/vnd.",
    "application/msword",
    "application/rtf",
    "text/",
    "application/json",
    "application/zip",
    "application/gzip",
    "application/x-tar",
    "application/octet-stream",
];
