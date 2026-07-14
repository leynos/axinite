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
        // Send typing indicator for thinking status.
        if matches!(status, StatusUpdate::Thinking(_))
            && let Some(target_str) = metadata.get("signal_target").and_then(|v| v.as_str())
        {
            let target = Self::parse_recipient_target(target_str);
            let params = self.build_rpc_params(&target, None, None);
            let _ = self.rpc_request("sendTyping", params).await;
        }

        // Send approval prompt to user
        if let StatusUpdate::ApprovalNeeded {
            request_id,
            tool_name,
            description: _,
            parameters,
        } = &status
            && let Some(target_str) = metadata.get("signal_target").and_then(|v| v.as_str())
        {
            let params_json = serde_json::to_string_pretty(parameters).unwrap_or_default();
            let message = format!(
                "⚠️ *Approval Required*\n\n\
                 *Request ID:* `{}`\n\
                 *Tool:* {}\n\
                 *Parameters:*\n```\n{}\n```\n\n\
                 Reply with:\n\
                 • `yes` or `y` - Approve this request\n\
                 • `always` or `a` - Approve and auto-approve future {} requests\n\
                 • `no` or `n` - Deny",
                request_id, tool_name, params_json, tool_name
            );
            self.send_status_message(target_str, &message).await;
        }

        // Filter out well-known UX/terminal status messages to avoid redundant updates.
        let should_forward_status = |msg: &str| {
            let normalized = msg.trim();
            !normalized.eq_ignore_ascii_case("done")
                && !normalized.eq_ignore_ascii_case("awaiting approval")
                && !normalized.eq_ignore_ascii_case("rejected")
        };
        // Filter/send status messages
        if let StatusUpdate::Status(msg) = &status
            && let Some(target_str) = Self::signal_target(metadata)
            && should_forward_status(msg)
        {
            self.send_status_message(target_str, msg).await;
        }

        // Send tool result previews to user (debug mode only)
        if let StatusUpdate::ToolResult { name, preview } = &status
            && let Some(target_str) = Self::signal_target(metadata)
            && self.is_debug()
        {
            let truncated = if preview.chars().count() > 500 {
                let s: String = preview.chars().take(500).collect();
                format!("{s}...")
            } else {
                preview.clone()
            };
            let message = format!("Tool '{}' result:\n{}", name, truncated);
            self.send_status_message(target_str, &message).await;
        }

        // Send tool started notification (debug mode only)
        if let StatusUpdate::ToolStarted { name } = &status
            && let Some(target_str) = Self::signal_target(metadata)
            && self.is_debug()
        {
            let message = format!("\u{25CB} Running tool: {}", name);
            self.send_status_message(target_str, &message).await;
        }

        // Send tool completed notification (debug mode only)
        if let StatusUpdate::ToolCompleted { name, success, .. } = &status
            && let Some(target_str) = Self::signal_target(metadata)
            && self.is_debug()
        {
            let (icon, color) = if *success {
                ("\u{25CF}", "success")
            } else {
                ("\u{2717}", "failed")
            };
            let message = format!("{} Tool '{}' completed ({})", icon, name, color);
            self.send_status_message(target_str, &message).await;
        }

        // Send job started notification (sandbox jobs)
        if let StatusUpdate::JobStarted {
            job_id,
            title,
            browse_url,
        } = &status
            && let Some(target_str) = metadata.get("signal_target").and_then(|v| v.as_str())
        {
            let message = format!(
                "\u{1F680} Job started: {}\nID: {}\nURL: {}",
                title, job_id, browse_url
            );
            self.send_status_message(target_str, &message).await;
        }

        // Send auth required notification
        if let StatusUpdate::AuthRequired {
            extension_name,
            instructions,
            auth_url,
            setup_url,
        } = &status
            && let Some(target_str) = metadata.get("signal_target").and_then(|v| v.as_str())
        {
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
            self.send_status_message(target_str, &message).await;
        }

        // Send auth completed notification
        if let StatusUpdate::AuthCompleted {
            extension_name,
            success,
            message: msg,
        } = &status
            && let Some(target_str) = metadata.get("signal_target").and_then(|v| v.as_str())
        {
            let icon = if *success { "\u{2705}" } else { "\u{274C}" };
            let mut message = format!(
                "{} Authentication {} for {}",
                icon,
                if *success { "completed" } else { "failed" },
                extension_name
            );
            if !msg.is_empty() {
                message.push_str(&format!("\n{}", msg));
            }
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
