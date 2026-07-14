//! JSON-RPC plumbing for the Signal channel: recipient target parsing, RPC
//! request/response handling, send-parameter construction, and attachment
//! path validation.

use std::time::Duration;

use futures::StreamExt;
use uuid::Uuid;

use crate::bootstrap::ironclaw_base_dir;
use crate::error::ChannelError;

use super::{GROUP_TARGET_PREFIX, MAX_HTTP_RESPONSE_SIZE, RecipientTarget, SignalChannel};

impl SignalChannel {
    /// Redact credentials from a URL for safe logging.
    ///
    /// Replaces any embedded username/password with `**REDACTED**` and returns
    /// the sanitised string. Returns `"<invalid-url>"` when parsing fails.
    pub fn redact_url(url: &str) -> String {
        reqwest::Url::parse(url)
            .map(|mut u| {
                if u.password().is_some() || !u.username().is_empty() {
                    let _ = u.set_username("**REDACTED**");
                    let _ = u.set_password(None);
                }
                u.to_string()
            })
            .unwrap_or_else(|_| "<invalid-url>".to_string())
    }

    pub(super) fn is_e164(recipient: &str) -> bool {
        let Some(number) = recipient.strip_prefix('+') else {
            return false;
        };
        (7..=15).contains(&number.len()) && number.chars().all(|c| c.is_ascii_digit())
    }

    /// Check whether a string is a valid UUID (signal-cli uses these for
    /// privacy-enabled users who have opted out of sharing their phone number).
    pub(super) fn is_uuid(s: &str) -> bool {
        Uuid::parse_str(s).is_ok()
    }

    pub(super) fn parse_recipient_target(recipient: &str) -> RecipientTarget {
        if let Some(group_id) = recipient.strip_prefix(GROUP_TARGET_PREFIX) {
            return RecipientTarget::Group(group_id.to_string());
        }

        if Self::is_e164(recipient) || Self::is_uuid(recipient) {
            RecipientTarget::Direct(recipient.to_string())
        } else {
            RecipientTarget::Group(recipient.to_string())
        }
    }

    /// Send a JSON-RPC request to signal-cli daemon.
    pub(super) async fn rpc_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<Option<serde_json::Value>, ChannelError> {
        let url = format!("{}/api/v1/rpc", self.config.http_url);
        let id = Uuid::new_v4().to_string();

        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": id,
        });

        let resp = self
            .client
            .post(&url)
            .timeout(Duration::from_secs(30))
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| ChannelError::SendFailed {
                name: "signal".to_string(),
                reason: format!("RPC request failed to {}: {e}", Self::redact_url(&url)),
            })?;

        // 201 = success with no body (e.g. typing indicators).
        if resp.status().as_u16() == 201 {
            return Ok(None);
        }

        // Reject obviously oversized responses before buffering.
        if let Some(len) = resp.content_length()
            && len as usize > MAX_HTTP_RESPONSE_SIZE
        {
            return Err(ChannelError::SendFailed {
                name: "signal".to_string(),
                reason: format!(
                    "RPC response Content-Length too large: {} bytes (max {})",
                    len, MAX_HTTP_RESPONSE_SIZE
                ),
            });
        }

        let status = resp.status();
        let mut stream = resp.bytes_stream();
        let mut total_bytes = 0usize;
        let mut body = Vec::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ChannelError::SendFailed {
                name: "signal".to_string(),
                reason: format!("Failed to read RPC response: {e}"),
            })?;
            let chunk_len = chunk.len();
            total_bytes += chunk_len;

            if total_bytes > MAX_HTTP_RESPONSE_SIZE {
                return Err(ChannelError::SendFailed {
                    name: "signal".to_string(),
                    reason: format!(
                        "RPC response too large: {} bytes (max {})",
                        total_bytes, MAX_HTTP_RESPONSE_SIZE
                    ),
                });
            }

            body.extend_from_slice(&chunk);
        }

        let bytes = body;

        if bytes.is_empty() {
            return Ok(None);
        }

        // Check for non-success HTTP status codes before parsing as JSON.
        if !status.is_success() {
            let truncated_len = std::cmp::min(bytes.len(), 512);
            let truncated_body = String::from_utf8_lossy(&bytes[..truncated_len]);
            return Err(ChannelError::SendFailed {
                name: "signal".to_string(),
                reason: format!("HTTP error {}: {}", status.as_u16(), truncated_body),
            });
        }

        let parsed: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|e| ChannelError::SendFailed {
                name: "signal".to_string(),
                reason: format!("Invalid RPC response JSON: {e}"),
            })?;

        if let Some(err) = parsed.get("error") {
            let code = err.get("code").and_then(|c| c.as_i64()).unwrap_or(-1);
            let msg = err
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown");
            return Err(ChannelError::SendFailed {
                name: "signal".to_string(),
                reason: format!("Signal RPC error {code}: {msg}"),
            });
        }

        Ok(parsed.get("result").cloned())
    }

    /// Attach optional message text and non-empty attachments to RPC params.
    fn apply_message_params(
        params: &mut serde_json::Value,
        message: Option<&str>,
        attachments: Option<&[String]>,
    ) {
        if let Some(msg) = message {
            params["message"] = serde_json::Value::String(msg.to_string());
        }
        if let Some(attachments) = attachments
            && !attachments.is_empty()
        {
            params["attachments"] = serde_json::Value::Array(
                attachments
                    .iter()
                    .map(|s| serde_json::Value::String(s.clone()))
                    .collect(),
            );
        }
    }

    /// Extract the Signal reply target from message metadata, if present.
    pub(super) fn signal_target(metadata: &serde_json::Value) -> Option<&str> {
        metadata.get("signal_target").and_then(|v| v.as_str())
    }

    /// Extract the Signal reply target, but only when debug mode is active.
    ///
    /// Debug-only notifications (tool lifecycle previews) route through this
    /// so their conditions stay within the two-operand lint limit.
    pub(super) fn debug_signal_target<'m>(
        &self,
        metadata: &'m serde_json::Value,
    ) -> Option<&'m str> {
        Self::signal_target(metadata).filter(|_| self.is_debug())
    }

    /// Build the base JSON-RPC params identifying the account and recipient.
    fn base_rpc_params(account: &str, target: &RecipientTarget) -> serde_json::Value {
        match target {
            RecipientTarget::Direct(id) => serde_json::json!({
                "recipient": [id],
                "account": account,
            }),
            RecipientTarget::Group(group_id) => serde_json::json!({
                "groupId": group_id,
                "account": account,
            }),
        }
    }

    /// Build JSON-RPC params for a send/typing call.
    pub(super) fn build_rpc_params(
        &self,
        target: &RecipientTarget,
        message: Option<&str>,
        attachments: Option<&[String]>,
    ) -> serde_json::Value {
        let mut params = Self::base_rpc_params(&self.config.account, target);
        Self::apply_message_params(&mut params, message, attachments);
        params
    }

    /// Validate that attachment paths are safe and within the sandbox.
    /// Uses the shared path validation logic from path_utils to ensure:
    /// - No path traversal attacks (../, URL-encoded, null bytes)
    /// - Paths are canonicalized and symlinks resolved
    /// - All paths are within ~/.ironclaw/ sandbox
    pub(super) fn validate_attachment_paths(paths: &[String]) -> Result<(), ChannelError> {
        // Get the sandbox base directory (same as MessageTool uses)
        let base_dir = ironclaw_base_dir();

        for path in paths {
            crate::tools::builtin::path_utils::validate_path(path, Some(&base_dir)).map_err(
                |e| {
                    ChannelError::InvalidMessage(format!(
                        "Attachment path must be within {}: {}",
                        base_dir.display(),
                        e
                    ))
                },
            )?;
        }
        Ok(())
    }

    /// Send a message with attachments (if any).
    /// Combines text and attachments into a single RPC call when both are present.
    pub(super) async fn send_with_attachments(
        &self,
        target: &RecipientTarget,
        content: &str,
        attachments: &[String],
    ) -> Result<(), ChannelError> {
        Self::validate_attachment_paths(attachments)?;

        // Text and attachments always go out in a single RPC call. Message
        // text is omitted only for attachment-only sends; a plain send with
        // empty content still carries the (empty) message field.
        let has_attachments = !attachments.is_empty();
        let message = (!has_attachments || !content.is_empty()).then_some(content);
        let params = self.build_rpc_params(target, message, has_attachments.then_some(attachments));
        self.rpc_request("send", params).await?;
        Ok(())
    }

    /// Build JSON-RPC params for a send/typing call (static version).
    pub(super) fn build_rpc_params_static(
        _http_url: &str,
        account: &str,
        target: &RecipientTarget,
        message: Option<&str>,
        attachments: Option<&[String]>,
    ) -> serde_json::Value {
        let mut params = Self::base_rpc_params(account, target);
        Self::apply_message_params(&mut params, message, attachments);
        params
    }

    pub(super) async fn send_status_message(&self, target: &str, message: &str) {
        let target = Self::parse_recipient_target(target);
        let params = self.build_rpc_params(&target, Some(message), None);
        if let Err(e) = self.rpc_request("send", params).await {
            tracing::warn!("Signal: failed to send status message: {}", e);
        }
    }
}
