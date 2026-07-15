//! `NativeChannel` trait implementation for the Signal channel: startup,
//! reply routing, status updates, broadcasts, health checks, and
//! conversation context extraction.

use std::sync::Arc;
use std::time::Duration;

use crate::channels::{
    IncomingMessage, MessageStream, NativeChannel, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;

use super::{SIGNAL_HEALTH_ENDPOINT, SignalChannel, sse_listener};

/// Whether a plain status message should be forwarded.
///
/// Filters out well-known UX/terminal status messages to avoid redundant
/// updates.
fn should_forward_status(msg: &str) -> bool {
    let normalized = msg.trim();
    !normalized.eq_ignore_ascii_case("done")
        && !normalized.eq_ignore_ascii_case("awaiting approval")
        && !normalized.eq_ignore_ascii_case("rejected")
}

/// Format the approval prompt sent when a tool requires user approval.
fn format_approval_prompt(
    request_id: &str,
    tool_name: &str,
    parameters: &serde_json::Value,
) -> String {
    let params_json = serde_json::to_string_pretty(parameters).unwrap_or_default();
    format!(
        "⚠️ *Approval Required*\n\n\
         *Request ID:* `{}`\n\
         *Tool:* {}\n\
         *Parameters:*\n```\n{}\n```\n\n\
         Reply with:\n\
         • `yes` or `y` - Approve this request\n\
         • `always` or `a` - Approve and auto-approve future {} requests\n\
         • `no` or `n` - Deny",
        request_id, tool_name, params_json, tool_name
    )
}

/// Format a tool result preview, truncated to 500 characters.
fn format_tool_result(name: &str, preview: &str) -> String {
    let truncated = if preview.chars().count() > 500 {
        let s: String = preview.chars().take(500).collect();
        format!("{s}...")
    } else {
        preview.to_string()
    };
    format!("Tool '{}' result:\n{}", name, truncated)
}

/// Format a tool completion notification with a success/failure icon.
fn format_tool_completed(name: &str, success: bool) -> String {
    let (icon, color) = if success {
        ("\u{25CF}", "success")
    } else {
        ("\u{2717}", "failed")
    };
    format!("{} Tool '{}' completed ({})", icon, name, color)
}

/// Format an authentication-required notification with optional details.
fn format_auth_required(
    extension_name: &str,
    instructions: &Option<String>,
    auth_url: &Option<String>,
    setup_url: &Option<String>,
) -> String {
    let mut message = format!("\u{1F512} Authentication required for: {}", extension_name);
    if let Some(instr) = instructions {
        message.push_str(&format!("\n\n{}", instr));
    }
    if let Some(url) = auth_url {
        message.push_str(&format!("\n\nAuth URL: {}", url));
    }
    if let Some(url) = setup_url {
        message.push_str(&format!("\nSetup URL: {}", url));
    }
    message
}

/// Format an authentication-completed notification.
fn format_auth_completed(extension_name: &str, success: bool, msg: &str) -> String {
    let icon = if success { "\u{2705}" } else { "\u{274C}" };
    let mut message = format!(
        "{} Authentication {} for {}",
        icon,
        if success { "completed" } else { "failed" },
        extension_name
    );
    if !msg.is_empty() {
        message.push_str(&format!("\n{}", msg));
    }
    message
}

impl SignalChannel {
    /// Send a Signal typing indicator to the reply target, when known.
    async fn send_typing_indicator(&self, metadata: &serde_json::Value) {
        if let Some(target_str) = Self::signal_target(metadata) {
            let target = Self::parse_recipient_target(target_str);
            let params = self.build_rpc_params(&target, None, None);
            let _ = self.rpc_request("sendTyping", params).await;
        }
    }

    /// Resolve the reply target and rendered text for a status update.
    ///
    /// Returns `None` for updates that should not be forwarded: unknown
    /// target, filtered status text, debug-only updates outside debug mode,
    /// or update kinds Signal does not surface.
    fn render_status<'m>(
        &self,
        status: &StatusUpdate,
        metadata: &'m serde_json::Value,
    ) -> Option<(&'m str, String)> {
        match status {
            StatusUpdate::ApprovalNeeded {
                request_id,
                tool_name,
                description: _,
                parameters,
            } => Some((
                Self::signal_target(metadata)?,
                format_approval_prompt(request_id, tool_name, parameters),
            )),

            StatusUpdate::Status(msg) => Some((
                Self::signal_target(metadata).filter(|_| should_forward_status(msg))?,
                msg.clone(),
            )),

            // Tool lifecycle previews are debug mode only.
            StatusUpdate::ToolResult { name, preview } => Some((
                self.debug_signal_target(metadata)?,
                format_tool_result(name, preview),
            )),
            StatusUpdate::ToolStarted { name } => Some((
                self.debug_signal_target(metadata)?,
                format!("\u{25CB} Running tool: {}", name),
            )),
            StatusUpdate::ToolCompleted { name, success, .. } => Some((
                self.debug_signal_target(metadata)?,
                format_tool_completed(name, *success),
            )),

            StatusUpdate::JobStarted {
                job_id,
                title,
                browse_url,
            } => Some((
                Self::signal_target(metadata)?,
                format!(
                    "\u{1F680} Job started: {}\nID: {}\nURL: {}",
                    title, job_id, browse_url
                ),
            )),

            StatusUpdate::AuthRequired {
                extension_name,
                instructions,
                auth_url,
                setup_url,
            } => Some((
                Self::signal_target(metadata)?,
                format_auth_required(extension_name, instructions, auth_url, setup_url),
            )),

            StatusUpdate::AuthCompleted {
                extension_name,
                success,
                message: msg,
            } => Some((
                Self::signal_target(metadata)?,
                format_auth_completed(extension_name, *success, msg),
            )),

            _ => None,
        }
    }
}

impl NativeChannel for SignalChannel {
    fn name(&self) -> &str {
        "signal"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = tokio::sync::mpsc::channel(256);

        let config = self.config.clone();
        let client = self.client.clone();
        let reply_targets = Arc::clone(&self.reply_targets);
        let debug_mode = Arc::clone(&self.debug_mode);

        tokio::spawn(async move {
            if let Err(e) = sse_listener(config, client, tx, reply_targets, debug_mode).await {
                tracing::error!("Signal SSE listener exited with error: {e}");
            }
        });

        // Log the URL with credentials redacted (if any).
        let safe_url = Self::redact_url(&self.config.http_url);
        tracing::info!(
            url = %safe_url,
            "Signal channel started"
        );

        Ok(Box::pin(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        // Resolve reply target from stored metadata.
        let target_str = {
            let targets = self.reply_targets.read().await;
            targets.peek(&msg.id).cloned()
        }
        .or_else(|| {
            // Fall back to metadata if not in the map.
            msg.metadata
                .get("signal_target")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| msg.user_id.clone());

        let target = Self::parse_recipient_target(&target_str);

        // Use shared helper for sending with attachments (includes validation)
        let result = self
            .send_with_attachments(&target, &response.content, &response.attachments)
            .await;

        // Clean up stored target regardless of success or failure.
        self.reply_targets.write().await.pop(&msg.id);

        result
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        // Thinking maps to a typing indicator, not a message.
        if matches!(status, StatusUpdate::Thinking(_)) {
            self.send_typing_indicator(metadata).await;
            return Ok(());
        }

        if let Some((target_str, message)) = self.render_status(&status, metadata) {
            self.send_status_message(target_str, &message).await;
        }
        Ok(())
    }

    async fn broadcast(
        &self,
        user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let target = Self::parse_recipient_target(user_id);

        // Use shared helper for sending with attachments (includes validation)
        self.send_with_attachments(&target, &response.content, &response.attachments)
            .await
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        let url = format!("{}{}", self.config.http_url, SIGNAL_HEALTH_ENDPOINT);
        let resp = self
            .client
            .get(&url)
            .timeout(Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| ChannelError::HealthCheckFailed {
                name: format!("signal ({}): {e}", Self::redact_url(&url)),
            })?;

        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ChannelError::HealthCheckFailed {
                name: format!("signal: HTTP {}", resp.status()),
            })
        }
    }

    fn conversation_context(
        &self,
        metadata: &serde_json::Value,
    ) -> std::collections::HashMap<String, String> {
        use std::collections::HashMap;
        let mut ctx = HashMap::new();

        if let Some(sender) = metadata.get("signal_sender").and_then(|v| v.as_str()) {
            ctx.insert("sender".to_string(), sender.to_string());
        }
        if let Some(sender_uuid) = metadata.get("signal_sender_uuid").and_then(|v| v.as_str()) {
            ctx.insert("sender_uuid".to_string(), sender_uuid.to_string());
        }
        if let Some(target) = metadata.get("signal_target").and_then(|v| v.as_str())
            && target.starts_with("group:")
        {
            ctx.insert("group".to_string(), target.to_string());
        }

        ctx
    }
}
