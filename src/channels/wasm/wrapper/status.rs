//! Status update handling and typing indicator lifecycle.

use std::sync::Arc;
use std::time::Duration;

use crate::channels::StatusUpdate;
use crate::error::ChannelError;

use super::{
    WasmChannel, clone_wit_status_update, resolve_channel_host_credentials, status_to_wit,
};

fn is_terminal_text_status(msg: &str) -> bool {
    let trimmed = msg.trim();
    trimmed.eq_ignore_ascii_case("done")
        || trimmed.eq_ignore_ascii_case("interrupted")
        || trimmed.eq_ignore_ascii_case("awaiting approval")
        || trimmed.eq_ignore_ascii_case("rejected")
}

fn should_cancel_typing_for_status(status: &StatusUpdate) -> bool {
    matches!(status, StatusUpdate::AuthRequired { .. })
        || matches!(status, StatusUpdate::Status(msg) if is_terminal_text_status(msg))
}

fn truncate_parameter_text(text: &str) -> String {
    if text.chars().count() > 80 {
        let truncated: String = text.chars().take(77).collect();
        format!("{}...", truncated)
    } else {
        text.to_string()
    }
}

fn format_parameter_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => format!("\"{}\"", truncate_parameter_text(s)),
        other => truncate_parameter_text(&other.to_string()),
    }
}

fn format_approval_parameters(parameters: &serde_json::Value) -> String {
    parameters
        .as_object()
        .map(|obj| {
            obj.iter()
                .map(|(k, v)| format!("  {}: {}", k, format_parameter_value(v)))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default()
}

fn build_approval_prompt(
    tool_name: &str,
    description: &str,
    parameters: &serde_json::Value,
) -> String {
    let params_preview = format_approval_parameters(parameters);

    format!(
        "Approval needed: {tool_name}\n\
         {description}\n\
         \n\
         Parameters:\n\
         {params_preview}\n\
         \n\
        Reply \"yes\" to approve, \"no\" to deny, or \"always\" to auto-approve."
    )
}

struct ApprovalNeededContext<'a> {
    status: &'a StatusUpdate,
    metadata: &'a serde_json::Value,
    tool_name: &'a str,
    description: &'a str,
    parameters: &'a serde_json::Value,
}

impl WasmChannel {
    pub(super) async fn cancel_typing_task(&self) {
        if let Some(handle) = self.typing_task.write().await.take() {
            handle.abort();
        }
    }

    async fn send_status_best_effort(
        &self,
        status: &StatusUpdate,
        metadata: &serde_json::Value,
        failure_message: &'static str,
    ) {
        if let Err(e) = self.call_on_status(status, metadata).await {
            match failure_message {
                "on_status(Thinking) failed (best-effort)" => {
                    tracing::debug!(
                        channel = %self.name,
                        error = %e,
                        "on_status(Thinking) failed (best-effort)"
                    );
                }
                _ => {
                    tracing::debug!(
                        channel = %self.name,
                        error = %e,
                        "on_status failed (best-effort)"
                    );
                }
            }
        }
    }

    async fn start_typing_repeater(&self, status: &StatusUpdate, metadata: &serde_json::Value) {
        let channel_name = self.name.clone();
        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = self.capabilities.clone();
        let credentials = self.credentials.clone();

        // Pre-resolve host credentials once for the lifetime of the repeater.
        // Channels tokens rarely change, so a snapshot per-repeater is correct.
        let repeater_host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;

        let pairing_store = self.pairing_store.clone();
        let callback_timeout = self.runtime.config().callback_timeout;
        let wit_update = status_to_wit(status, metadata);

        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(4));
            // Skip the first tick (we already fired above)
            interval.tick().await;

            loop {
                interval.tick().await;

                let wit_update_clone = clone_wit_status_update(&wit_update);
                let hc = repeater_host_credentials.clone();

                if let Err(e) = Self::execute_status(
                    &channel_name,
                    &runtime,
                    &prepared,
                    &capabilities,
                    &credentials,
                    hc,
                    pairing_store.clone(),
                    callback_timeout,
                    wit_update_clone,
                )
                .await
                {
                    tracing::debug!(
                        channel = %channel_name,
                        error = %e,
                        "Typing repeat on_status failed (best-effort)"
                    );
                }
            }
        });

        *self.typing_task.write().await = Some(handle);
    }

    async fn handle_thinking_status(&self, status: &StatusUpdate, metadata: &serde_json::Value) {
        // Cancel any existing typing task
        self.cancel_typing_task().await;

        // Fire once immediately
        if let Err(e) = self.call_on_status(status, metadata).await {
            tracing::debug!(
                channel = %self.name,
                error = %e,
                "on_status(Thinking) failed (best-effort)"
            );
        }

        // Spawn background repeater
        self.start_typing_repeater(status, metadata).await;
    }

    async fn cancel_typing_and_send_status(
        &self,
        status: &StatusUpdate,
        metadata: &serde_json::Value,
    ) {
        // Waiting on user or terminal states: stop typing and fire once.
        self.cancel_typing_task().await;

        self.send_status_best_effort(status, metadata, "on_status failed (best-effort)")
            .await;
    }

    async fn handle_approval_needed_status(&self, ctx: ApprovalNeededContext<'_>) {
        // WASM channels (Telegram, Slack, etc.) cannot render
        // interactive approval overlays. Send the approval prompt
        // as an actual message so the user can reply yes/no.
        self.cancel_typing_task().await;

        let prompt = build_approval_prompt(ctx.tool_name, ctx.description, ctx.parameters);
        let metadata_json = serde_json::to_string(ctx.metadata).unwrap_or_default();

        if let Err(e) = self
            .call_on_respond(super::RespondInvocation {
                message_id: uuid::Uuid::new_v4(),
                content: &prompt,
                thread_id: None,
                metadata_json: &metadata_json,
                attachments: &[],
            })
            .await
        {
            tracing::warn!(
                channel = %self.name,
                error = %e,
                "Failed to send approval prompt via on_respond, falling back to on_status"
            );
            // Fall back to status update (typing indicator)
            let _ = self.call_on_status(ctx.status, ctx.metadata).await;
        }
    }

    /// Handle a status update, managing the typing repeat timer.
    ///
    /// On Thinking: fires on_status once, then spawns a background task
    /// that repeats the call every 4 seconds (Telegram's typing indicator
    /// expires after ~5s).
    ///
    /// On terminal or user-action-required states: cancels the repeat task,
    /// then fires on_status once.
    ///
    /// On intermediate progress states (tool/auth/job/status updates), keeps
    /// the typing repeater running and fires on_status once.
    /// On StreamChunk: no-op (too noisy).
    pub(super) async fn handle_status_update(
        &self,
        status: StatusUpdate,
        metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        match &status {
            StatusUpdate::Thinking(_) => {
                self.handle_thinking_status(&status, metadata).await;
            }
            StatusUpdate::StreamChunk(_) => {
                // No-op, too noisy
            }
            StatusUpdate::ApprovalNeeded {
                tool_name,
                description,
                parameters,
                ..
            } => {
                self.handle_approval_needed_status(ApprovalNeededContext {
                    status: &status,
                    metadata,
                    tool_name,
                    description,
                    parameters,
                })
                .await;
            }
            status if should_cancel_typing_for_status(status) => {
                self.cancel_typing_and_send_status(status, metadata).await;
            }
            _ => {
                // Intermediate progress status: keep any existing typing task alive.
                self.send_status_best_effort(&status, metadata, "on_status failed (best-effort)")
                    .await;
            }
        }

        Ok(())
    }
}
