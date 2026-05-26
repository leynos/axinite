//! Status update handling and typing indicator lifecycle.

use std::sync::Arc;
use std::time::Duration;

use crate::channels::StatusUpdate;
use crate::error::ChannelError;

use super::{
    WasmChannel, clone_wit_status_update, resolve_channel_host_credentials, status_to_wit,
};

impl WasmChannel {
    pub(super) async fn cancel_typing_task(&self) {
        if let Some(handle) = self.typing_task.write().await.take() {
            handle.abort();
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
        fn is_terminal_text_status(msg: &str) -> bool {
            let trimmed = msg.trim();
            trimmed.eq_ignore_ascii_case("done")
                || trimmed.eq_ignore_ascii_case("interrupted")
                || trimmed.eq_ignore_ascii_case("awaiting approval")
                || trimmed.eq_ignore_ascii_case("rejected")
        }

        match &status {
            StatusUpdate::Thinking(_) => {
                // Cancel any existing typing task
                self.cancel_typing_task().await;

                // Fire once immediately
                if let Err(e) = self.call_on_status(&status, metadata).await {
                    tracing::debug!(
                        channel = %self.name,
                        error = %e,
                        "on_status(Thinking) failed (best-effort)"
                    );
                }

                // Spawn background repeater
                let channel_name = self.name.clone();
                let runtime = Arc::clone(&self.runtime);
                let prepared = Arc::clone(&self.prepared);
                let capabilities = self.capabilities.clone();
                let credentials = self.credentials.clone();
                // Pre-resolve host credentials once for the lifetime of the repeater.
                // Channels tokens rarely change, so a snapshot per-repeater is correct.
                let repeater_host_credentials = resolve_channel_host_credentials(
                    &self.capabilities,
                    self.secrets_store.as_deref(),
                )
                .await;
                let pairing_store = self.pairing_store.clone();
                let callback_timeout = self.runtime.config().callback_timeout;
                let wit_update = status_to_wit(&status, metadata);

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
            StatusUpdate::StreamChunk(_) => {
                // No-op, too noisy
            }
            StatusUpdate::ApprovalNeeded {
                tool_name,
                description,
                parameters,
                ..
            } => {
                // WASM channels (Telegram, Slack, etc.) cannot render
                // interactive approval overlays.  Send the approval prompt
                // as an actual message so the user can reply yes/no.
                self.cancel_typing_task().await;

                let params_preview = parameters
                    .as_object()
                    .map(|obj| {
                        obj.iter()
                            .map(|(k, v)| {
                                let val = match v {
                                    serde_json::Value::String(s) => {
                                        if s.chars().count() > 80 {
                                            let truncated: String = s.chars().take(77).collect();
                                            format!("\"{}...\"", truncated)
                                        } else {
                                            format!("\"{}\"", s)
                                        }
                                    }
                                    other => {
                                        let s = other.to_string();
                                        if s.chars().count() > 80 {
                                            let truncated: String = s.chars().take(77).collect();
                                            format!("{}...", truncated)
                                        } else {
                                            s
                                        }
                                    }
                                };
                                format!("  {}: {}", k, val)
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();

                let prompt = format!(
                    "Approval needed: {tool_name}\n\
                     {description}\n\
                     \n\
                     Parameters:\n\
                     {params_preview}\n\
                     \n\
                     Reply \"yes\" to approve, \"no\" to deny, or \"always\" to auto-approve."
                );

                let metadata_json = serde_json::to_string(metadata).unwrap_or_default();
                if let Err(e) = self
                    .call_on_respond(uuid::Uuid::new_v4(), &prompt, None, &metadata_json, &[])
                    .await
                {
                    tracing::warn!(
                        channel = %self.name,
                        error = %e,
                        "Failed to send approval prompt via on_respond, falling back to on_status"
                    );
                    // Fall back to status update (typing indicator)
                    let _ = self.call_on_status(&status, metadata).await;
                }
            }
            StatusUpdate::AuthRequired { .. } => {
                // Waiting on user action: stop typing and fire once.
                self.cancel_typing_task().await;

                if let Err(e) = self.call_on_status(&status, metadata).await {
                    tracing::debug!(
                        channel = %self.name,
                        error = %e,
                        "on_status failed (best-effort)"
                    );
                }
            }
            StatusUpdate::Status(msg) if is_terminal_text_status(msg) => {
                // Waiting on user or terminal states: stop typing and fire once.
                self.cancel_typing_task().await;

                if let Err(e) = self.call_on_status(&status, metadata).await {
                    tracing::debug!(
                        channel = %self.name,
                        error = %e,
                        "on_status failed (best-effort)"
                    );
                }
            }
            _ => {
                // Intermediate progress status: keep any existing typing task alive.
                if let Err(e) = self.call_on_status(&status, metadata).await {
                    tracing::debug!(
                        channel = %self.name,
                        error = %e,
                        "on_status failed (best-effort)"
                    );
                }
            }
        }

        Ok(())
    }
}
