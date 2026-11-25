//! Message tool for sending messages to channels.
//!
//! Allows the agent to proactively message users on any connected channel.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::bootstrap::axinite_base_dir;
use crate::channels::{ChannelManager, OutgoingResponse};
use crate::context::JobContext;
use crate::tools::tool::{
    ApprovalRequirement, NativeTool, ToolError, ToolOutput, ToolRateLimitConfig, require_str,
};

/// Tool for sending messages to channels.
pub struct MessageTool {
    channel_manager: Arc<ChannelManager>,
    /// Default channel for current conversation (set per-turn).
    /// Uses std::sync::RwLock because requires_approval() is sync and called from async context.
    default_channel: Arc<RwLock<Option<String>>>,
    /// Default target (user_id or group_id) for current conversation (set per-turn).
    default_target: Arc<RwLock<Option<String>>>,
    /// Base directory for attachment path validation (sandbox).
    pub(crate) base_dir: PathBuf,
}

impl MessageTool {
    /// Create the tool, limiting attachment paths to the axinite base directory and `/tmp`.
    pub fn new(channel_manager: Arc<ChannelManager>) -> Self {
        let base_dir = axinite_base_dir();

        Self {
            channel_manager,
            default_channel: Arc::new(RwLock::new(None)),
            default_target: Arc::new(RwLock::new(None)),
            base_dir,
        }
    }

    /// Set the base directory for attachment validation.
    /// This is primarily used for testing or future configuration.
    pub fn with_base_dir(mut self, dir: PathBuf) -> Self {
        self.base_dir = dir;
        self
    }

    /// Set the default channel and target for the current conversation turn.
    /// Call this before each agent turn with the incoming message's channel/target.
    pub async fn set_context(&self, channel: Option<String>, target: Option<String>) {
        *self
            .default_channel
            .write()
            .unwrap_or_else(|e| e.into_inner()) = channel;
        *self
            .default_target
            .write()
            .unwrap_or_else(|e| e.into_inner()) = target;
    }

    /// Pick the channel: param, then conversation default, then job metadata,
    /// else `None` (broadcast to all).
    fn resolve_channel(&self, params: &serde_json::Value, ctx: &JobContext) -> Option<String> {
        if let Some(c) = params.get("channel").and_then(|v| v.as_str()) {
            return Some(c.to_string());
        }
        if let Some(c) = self
            .default_channel
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Some(c);
        }
        ctx.metadata
            .get("notify_channel")
            .and_then(|v| v.as_str())
            .map(|c| c.to_string())
    }

    /// Pick the target: param, then conversation default, then job metadata.
    fn resolve_target(
        &self,
        params: &serde_json::Value,
        ctx: &JobContext,
    ) -> Result<String, ToolError> {
        if let Some(t) = params.get("target").and_then(|v| v.as_str()) {
            return Ok(t.to_string());
        }
        if let Some(t) = self
            .default_target
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Ok(t);
        }
        if let Some(t) = ctx.metadata.get("notify_user").and_then(|v| v.as_str()) {
            return Ok(t.to_string());
        }
        Err(ToolError::ExecutionFailed(
            "No target specified and no active conversation. Provide target parameter.".to_string(),
        ))
    }

    /// Check every attachment path against the sandbox and for existence.
    ///
    /// Allows paths under the base_dir (~/.axinite) or /tmp/.
    fn validate_attachment_paths(&self, attachments: &[String]) -> Result<(), ToolError> {
        for path in attachments {
            let tmp_dir = PathBuf::from("/tmp");
            let resolved =
                crate::tools::builtin::path_utils::validate_path(path, Some(&self.base_dir))
                    .or_else(|_| {
                        crate::tools::builtin::path_utils::validate_path(path, Some(&tmp_dir))
                    })
                    .map_err(|e| {
                        ToolError::ExecutionFailed(format!(
                            "Attachment path must be within {} or /tmp/: {}",
                            self.base_dir.display(),
                            e
                        ))
                    })?;
            if !resolved.exists() {
                return Err(ToolError::ExecutionFailed(format!(
                    "Attachment file not found: {}",
                    path
                )));
            }
        }
        Ok(())
    }

    /// Send to one named channel, listing the connected channels on failure.
    async fn send_to_channel(
        &self,
        channel: &str,
        response: OutgoingResponse,
        delivery: &Delivery<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let target = delivery.target;
        match self
            .channel_manager
            .broadcast(channel, target, response)
            .await
        {
            Ok(()) => {
                tracing::info!(
                    message_sent = true,
                    channel = %channel,
                    target = %target,
                    attachments = delivery.attachment_count,
                    "Message sent via message tool"
                );
                let msg = format!("Sent message to {}:{}", channel, target);
                Ok(ToolOutput::text(msg, delivery.start.elapsed()))
            }
            Err(e) => {
                let available = self.channel_manager.channel_names().await.join(", ");
                let err_msg = if available.is_empty() {
                    format!(
                        "Failed to send to {}:{}: {}. No channels connected.",
                        channel, target, e
                    )
                } else {
                    format!(
                        "Failed to send to {}:{}. Available channels: {}. Error: {}",
                        channel, target, available, e
                    )
                };
                Err(ToolError::ExecutionFailed(err_msg))
            }
        }
    }

    /// No channel specified: broadcast to all channels (routine with notify.channel = None).
    async fn broadcast_to_all(
        &self,
        response: OutgoingResponse,
        delivery: &Delivery<'_>,
    ) -> Result<ToolOutput, ToolError> {
        let target = delivery.target;
        let results = self.channel_manager.broadcast_all(target, response).await;
        let mut succeeded = Vec::new();
        let mut failed: Vec<&str> = Vec::new();
        for (ch, result) in &results {
            match result {
                Ok(()) => succeeded.push(ch.as_str()),
                Err(e) => {
                    tracing::warn!(
                        channel = %ch,
                        target = %target,
                        "broadcast_all: channel failed: {}", e
                    );
                    failed.push(ch.as_str());
                }
            }
        }
        if succeeded.is_empty() {
            let err_msg = if failed.is_empty() {
                "No channels connected.".to_string()
            } else {
                format!("All channels failed: {}", failed.join(", "))
            };
            return Err(ToolError::ExecutionFailed(err_msg));
        }
        tracing::info!(
            message_sent = true,
            channels = ?succeeded,
            target = %target,
            attachments = delivery.attachment_count,
            "Message broadcast via message tool"
        );
        let msg = format!(
            "Broadcast message to {} (target: {})",
            succeeded.join(", "),
            target
        );
        Ok(ToolOutput::text(msg, delivery.start.elapsed()))
    }
}

/// What the send helpers share: the recipient, attachment count and start time.
struct Delivery<'a> {
    target: &'a str,
    attachment_count: usize,
    start: std::time::Instant,
}

/// Read the optional `attachments` array of file paths.
fn parse_attachments(params: &serde_json::Value) -> Result<Vec<String>, ToolError> {
    match params.get("attachments") {
        Some(v) => serde_json::from_value(v.clone())
            .map_err(|e| ToolError::ExecutionFailed(format!("Invalid attachments format: {}", e))),
        None => Ok(Vec::new()),
    }
}

impl NativeTool for MessageTool {
    fn name(&self) -> &str {
        "message"
    }

    fn description(&self) -> &str {
        "Send a message to a channel. If channel/target omitted, uses the current conversation's \
         channel and sender/group. Use to proactively message users on any connected channel. \
         Supports file attachments: first download the file with the http tool using save_to \
         (e.g., http GET https://picsum.photos/800/600 save_to=/tmp/photo.jpg), then pass \
         the file path in the attachments array. Images are sent as photos on Telegram. \
         - Signal: target accepts E.164 (+1234567890) or group ID \
         - Telegram: target accepts username or chat ID \
         - Slack: target accepts channel (#general) or user ID"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Message text to send"
                },
                "channel": {
                    "type": "string",
                    "description": "Target channel (defaults to current channel if omitted)"
                },
                "target": {
                    "type": "string",
                    "description": "Recipient: E.164 phone, group ID, chat ID (defaults to current sender/group if omitted)"
                },
                "attachments": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Optional file paths to attach to the message"
                }
            },
            "required": ["content"]
        })
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let content = require_str(&params, "content")?;

        let channel = self.resolve_channel(&params, ctx);
        let target = self.resolve_target(&params, ctx)?;
        let attachments = parse_attachments(&params)?;
        let attachment_count = attachments.len();
        self.validate_attachment_paths(&attachments)?;

        let mut response = OutgoingResponse::text(content);
        if !attachments.is_empty() {
            response = response.with_attachments(attachments);
        }

        let delivery = Delivery {
            target: &target,
            attachment_count,
            start,
        };
        match channel {
            Some(channel) => self.send_to_channel(&channel, response, &delivery).await,
            None => self.broadcast_to_all(response, &delivery).await,
        }
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        // Message tool only delivers to channels the user has configured
        // (TUI, Telegram, Slack, web gateway, etc.) via ChannelManager::broadcast.
        ApprovalRequirement::Never
    }

    fn rate_limit_config(&self) -> Option<ToolRateLimitConfig> {
        Some(ToolRateLimitConfig::new(10, 100))
    }

    fn requires_sanitization(&self) -> bool {
        false
    }
}

#[cfg(test)]
mod tests;
