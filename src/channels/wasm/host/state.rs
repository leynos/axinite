//! Channel host state: side-effect tracking and limits during callback
//! execution, including message emission and workspace write queues.

use std::collections::HashMap;

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::error::WasmChannelError;
use crate::tools::wasm::{HostState, LogLevel};

use super::message::{EmittedMessage, PendingWorkspaceWrite};
use super::{
    ALLOWED_MIME_PREFIXES, MAX_ATTACHMENT_TOTAL_SIZE, MAX_ATTACHMENTS_PER_MESSAGE,
    MAX_EMITS_PER_EXECUTION, MAX_MESSAGE_CONTENT_SIZE,
};

/// Host state for WASM channel callbacks.
///
/// Maintains all side effects during callback execution and enforces limits.
/// This is the channel-specific equivalent of HostState for tools.
pub struct ChannelHostState {
    /// Base tool host state (logging, time, HTTP, etc.).
    base: HostState,

    /// Channel name (for error messages).
    channel_name: String,

    /// Channel capabilities.
    capabilities: ChannelCapabilities,

    /// Emitted messages (queued for delivery).
    emitted_messages: Vec<EmittedMessage>,

    /// Pending workspace writes.
    pending_writes: Vec<PendingWorkspaceWrite>,

    /// Emit count for rate limiting within this execution.
    emit_count: u32,

    /// Whether emit is still allowed (false after rate limit hit).
    emit_enabled: bool,

    /// Count of emits dropped due to rate limiting.
    emits_dropped: usize,

    /// Binary data stored for attachments via `store-attachment-data`.
    /// Keyed by attachment ID, cleared after callback completes.
    attachment_data: HashMap<String, Vec<u8>>,

    /// Total bytes stored in attachment_data (for enforcing limits).
    attachment_data_total: u64,
}

impl std::fmt::Debug for ChannelHostState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChannelHostState")
            .field("channel_name", &self.channel_name)
            .field("emitted_messages_count", &self.emitted_messages.len())
            .field("pending_writes_count", &self.pending_writes.len())
            .field("emit_count", &self.emit_count)
            .field("emit_enabled", &self.emit_enabled)
            .field("emits_dropped", &self.emits_dropped)
            .finish()
    }
}

impl ChannelHostState {
    /// Create a new channel host state.
    pub fn new(channel_name: impl Into<String>, capabilities: ChannelCapabilities) -> Self {
        let base = HostState::new(capabilities.tool_capabilities.clone());

        Self {
            base,
            channel_name: channel_name.into(),
            capabilities,
            emitted_messages: Vec::new(),
            pending_writes: Vec::new(),
            emit_count: 0,
            emit_enabled: true,
            emits_dropped: 0,
            attachment_data: HashMap::new(),
            attachment_data_total: 0,
        }
    }

    /// Get the channel name.
    pub fn channel_name(&self) -> &str {
        &self.channel_name
    }

    /// Get the capabilities.
    pub fn capabilities(&self) -> &ChannelCapabilities {
        &self.capabilities
    }

    /// Get the base host state for tool capabilities.
    pub fn base(&self) -> &HostState {
        &self.base
    }

    /// Get mutable access to the base host state.
    pub fn base_mut(&mut self) -> &mut HostState {
        &mut self.base
    }

    /// Emit a message from the channel.
    ///
    /// Messages are queued and delivered after callback execution completes.
    /// Rate limiting is enforced per-execution and globally.
    /// Attachments are validated for count, total size, and MIME type.
    pub fn emit_message(&mut self, msg: EmittedMessage) -> Result<(), WasmChannelError> {
        // Check per-execution limit
        if !self.emit_enabled {
            self.emits_dropped += 1;
            return Ok(()); // Silently drop, don't fail execution
        }

        if self.emitted_messages.len() >= MAX_EMITS_PER_EXECUTION {
            self.emit_enabled = false;
            self.emits_dropped += 1;
            tracing::warn!(
                channel = %self.channel_name,
                limit = MAX_EMITS_PER_EXECUTION,
                "Channel emit limit reached, further messages dropped"
            );
            return Ok(());
        }

        // Validate attachments
        let msg = self.validate_attachments(msg);

        // Validate message content size
        if msg.content.len() > MAX_MESSAGE_CONTENT_SIZE {
            tracing::warn!(
                channel = %self.channel_name,
                size = msg.content.len(),
                max = MAX_MESSAGE_CONTENT_SIZE,
                "Message content too large, truncating"
            );
            let mut truncated = msg.content[..MAX_MESSAGE_CONTENT_SIZE].to_string();
            truncated.push_str("... (truncated)");
            let msg = EmittedMessage {
                content: truncated,
                ..msg
            };
            self.emitted_messages.push(msg);
        } else {
            self.emitted_messages.push(msg);
        }

        self.emit_count += 1;
        Ok(())
    }

    /// Validate and sanitize attachments on an emitted message.
    ///
    /// Enforces count limits, total size limits, and MIME type allowlist.
    /// Invalid attachments are dropped with a warning.
    fn validate_attachments(&self, mut msg: EmittedMessage) -> EmittedMessage {
        if msg.attachments.is_empty() {
            return msg;
        }

        // Enforce attachment count limit
        if msg.attachments.len() > MAX_ATTACHMENTS_PER_MESSAGE {
            tracing::warn!(
                channel = %self.channel_name,
                count = msg.attachments.len(),
                max = MAX_ATTACHMENTS_PER_MESSAGE,
                "Too many attachments, truncating"
            );
            msg.attachments.truncate(MAX_ATTACHMENTS_PER_MESSAGE);
        }

        // Filter by MIME type and enforce total size limit
        let mut total_size: u64 = 0;
        msg.attachments.retain(|att| {
            let mime_ok = ALLOWED_MIME_PREFIXES
                .iter()
                .any(|prefix| att.mime_type.starts_with(prefix));
            if !mime_ok {
                tracing::warn!(
                    channel = %self.channel_name,
                    mime_type = %att.mime_type,
                    "Attachment MIME type not allowed, dropping"
                );
                return false;
            }

            // Use the larger of reported size_bytes and actual stored data size
            // to prevent WASM channels from under-reporting to bypass limits.
            let stored_size = self
                .attachment_data
                .get(&att.id)
                .map(|d| d.len() as u64)
                .unwrap_or(att.data.len() as u64);
            let size = att
                .size_bytes
                .map(|reported| reported.max(stored_size))
                .unwrap_or(stored_size);
            if size > 0 {
                total_size = total_size.saturating_add(size);
                if total_size > MAX_ATTACHMENT_TOTAL_SIZE {
                    tracing::warn!(
                        channel = %self.channel_name,
                        total_size,
                        max = MAX_ATTACHMENT_TOTAL_SIZE,
                        "Attachment total size exceeded, dropping"
                    );
                    return false;
                }
            }

            true
        });

        msg
    }

    /// Take all emitted messages (clears the queue).
    pub fn take_emitted_messages(&mut self) -> Vec<EmittedMessage> {
        std::mem::take(&mut self.emitted_messages)
    }

    /// Get the number of emitted messages.
    pub fn emitted_count(&self) -> usize {
        self.emitted_messages.len()
    }

    /// Get the number of emits dropped due to rate limiting.
    pub fn emits_dropped(&self) -> usize {
        self.emits_dropped
    }

    /// Store binary data for an attachment.
    ///
    /// Called by WASM channels to associate downloaded bytes with an attachment ID.
    /// The data is retrieved after callback completion and merged into `Attachment::data`.
    pub fn store_attachment_data(
        &mut self,
        attachment_id: &str,
        data: Vec<u8>,
    ) -> Result<(), WasmChannelError> {
        const MAX_PER_ATTACHMENT: u64 = 20 * 1024 * 1024; // 20 MB
        const MAX_TOTAL: u64 = 50 * 1024 * 1024; // 50 MB

        let size = data.len() as u64;
        if size > MAX_PER_ATTACHMENT {
            return Err(WasmChannelError::CallbackFailed {
                name: self.channel_name.clone(),
                reason: format!(
                    "Attachment data too large: {} bytes (max {})",
                    size, MAX_PER_ATTACHMENT
                ),
            });
        }

        // Subtract the old entry size (if overwriting) before adding new size
        let old_size = self
            .attachment_data
            .get(attachment_id)
            .map(|d| d.len() as u64)
            .unwrap_or(0);
        let adjusted_total = self.attachment_data_total.saturating_sub(old_size);
        let new_total = adjusted_total.saturating_add(size);
        if new_total > MAX_TOTAL {
            return Err(WasmChannelError::CallbackFailed {
                name: self.channel_name.clone(),
                reason: format!(
                    "Total attachment data too large: {} bytes (max {})",
                    new_total, MAX_TOTAL
                ),
            });
        }

        self.attachment_data_total = new_total;
        self.attachment_data.insert(attachment_id.to_string(), data);
        Ok(())
    }

    /// Remove stored binary data for a specific attachment ID.
    pub fn remove_attachment_data(&mut self, id: &str) -> Option<Vec<u8>> {
        if let Some(data) = self.attachment_data.remove(id) {
            self.attachment_data_total =
                self.attachment_data_total.saturating_sub(data.len() as u64);
            Some(data)
        } else {
            None
        }
    }

    /// Take all stored attachment data (clears the store).
    pub fn take_attachment_data(&mut self) -> HashMap<String, Vec<u8>> {
        self.attachment_data_total = 0;
        std::mem::take(&mut self.attachment_data)
    }

    /// Write to workspace (scoped to channel namespace).
    ///
    /// Writes are queued and committed after callback execution completes.
    pub fn workspace_write(&mut self, path: &str, content: String) -> Result<(), WasmChannelError> {
        // Validate and prefix path
        let full_path = self
            .capabilities
            .validate_workspace_path(path)
            .map_err(|reason| WasmChannelError::WorkspaceEscape {
                name: self.channel_name.clone(),
                path: reason,
            })?;

        self.pending_writes.push(PendingWorkspaceWrite {
            path: full_path,
            content,
        });

        Ok(())
    }

    /// Take all pending workspace writes (clears the queue).
    pub fn take_pending_writes(&mut self) -> Vec<PendingWorkspaceWrite> {
        std::mem::take(&mut self.pending_writes)
    }

    /// Get the number of pending workspace writes.
    pub fn pending_writes_count(&self) -> usize {
        self.pending_writes.len()
    }

    /// Log a message (delegates to base).
    pub fn log(
        &mut self,
        level: LogLevel,
        message: String,
    ) -> Result<(), crate::tools::wasm::WasmError> {
        self.base.log(level, message)
    }

    /// Get current timestamp in milliseconds (delegates to base).
    pub fn now_millis(&self) -> u64 {
        self.base.now_millis()
    }

    /// Read from workspace (delegates to base).
    pub fn workspace_read(
        &self,
        path: &str,
    ) -> Result<Option<String>, crate::tools::wasm::WasmError> {
        // Prefix the path with channel namespace before reading
        let full_path = self.capabilities.prefix_workspace_path(path);
        self.base.workspace_read(&full_path)
    }

    /// Check if a secret exists (delegates to base).
    pub fn secret_exists(&self, name: &str) -> bool {
        self.base.secret_exists(name)
    }

    /// Check if HTTP is allowed (delegates to base).
    pub fn check_http_allowed(&self, url: &str, method: &str) -> Result<(), String> {
        self.base.check_http_allowed(url, method)
    }

    /// Record an HTTP request (delegates to base).
    pub fn record_http_request(&mut self) -> Result<(), String> {
        self.base.record_http_request()
    }

    /// Take logs (delegates to base).
    pub fn take_logs(&mut self) -> Vec<crate::tools::wasm::LogEntry> {
        self.base.take_logs()
    }
}
