//! Dispatch WASM-emitted messages into native channel messages.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::channels::IncomingMessage;
use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::host::{ChannelEmitRateLimiter, EmittedMessage};

use super::{WasmChannel, do_update_broadcast_metadata};

/// Bundles the per-channel async state references required by
/// `dispatch_emitted_messages`.
///
/// Using a parameter object keeps the function signature within the
/// four-argument threshold whilst avoiding the overhead of an extra `Arc`.
pub(super) struct DispatchContext<'a> {
    pub(super) message_tx: &'a tokio::sync::RwLock<Option<mpsc::Sender<IncomingMessage>>>,
    pub(super) rate_limiter: &'a tokio::sync::RwLock<ChannelEmitRateLimiter>,
    pub(super) last_broadcast_metadata: &'a tokio::sync::RwLock<Option<String>>,
    pub(super) settings_store: Option<&'a Arc<dyn crate::db::SettingsStore>>,
}

/// Converts a single [`EmittedMessage`] into an [`IncomingMessage`], applying
/// optional user name, thread id, and attachments.
///
/// Metadata parsing is left to the caller because it triggers a side-effect
/// (broadcast metadata persistence).
fn convert_emitted_to_incoming(channel_name: &str, emitted: &EmittedMessage) -> IncomingMessage {
    let mut msg = IncomingMessage::new(channel_name, &emitted.user_id, &emitted.content);

    if let Some(name) = emitted.user_name.clone() {
        msg = msg.with_user_name(name);
    }

    if let Some(thread_id) = emitted.thread_id.clone() {
        msg = msg.with_thread(thread_id);
    }

    if !emitted.attachments.is_empty() {
        let incoming_attachments = emitted
            .attachments
            .iter()
            .map(|a| crate::channels::IncomingAttachment {
                id: a.id.clone(),
                kind: crate::channels::AttachmentKind::from_mime_type(&a.mime_type),
                mime_type: a.mime_type.clone(),
                filename: a.filename.clone(),
                size_bytes: a.size_bytes,
                source_url: a.source_url.clone(),
                storage_key: a.storage_key.clone(),
                extracted_text: a.extracted_text.clone(),
                data: a.data.clone(),
                duration_secs: a.duration_secs,
            })
            .collect();
        msg = msg.with_attachments(incoming_attachments);
    }

    msg
}

/// Checks the rate limiter and, if permitted, sends `msg` on `tx`.
///
/// Returns `Err(WasmChannelError::EmitRateLimited)` when the rate limit is
/// exceeded, and `Ok(false)` when the sender is closed (so callers can
/// `break` out of their dispatch loop).
async fn send_with_rate_limit(
    channel_name: &str,
    msg: IncomingMessage,
    tx: &mpsc::Sender<IncomingMessage>,
    limiter: &mut ChannelEmitRateLimiter,
) -> Result<bool, WasmChannelError> {
    if !limiter.check_and_record() {
        tracing::warn!(channel = %channel_name, "Message emission rate limited");
        return Err(WasmChannelError::EmitRateLimited {
            name: channel_name.to_string(),
        });
    }

    tracing::info!(
        channel = %channel_name,
        user_id = %msg.user_id,
        content_len = msg.content.len(),
        attachment_count = msg.attachments.len(),
        "Sending message to agent"
    );

    if tx.send(msg).await.is_err() {
        tracing::error!(channel = %channel_name, "Failed to send message, channel closed");
        return Ok(false);
    }

    tracing::info!(channel = %channel_name, "Message successfully sent to agent queue");
    Ok(true)
}

impl WasmChannel {
    pub(super) async fn process_emitted_messages(
        &self,
        messages: Vec<EmittedMessage>,
    ) -> Result<(), WasmChannelError> {
        tracing::info!(
            channel = %self.name,
            message_count = messages.len(),
            "Processing emitted messages from WASM callback"
        );

        if messages.is_empty() {
            tracing::debug!(channel = %self.name, "No messages emitted");
            return Ok(());
        }

        let tx_guard = self.message_tx.read().await;
        let Some(tx) = tx_guard.as_ref() else {
            tracing::error!(
                channel = %self.name,
                count = messages.len(),
                "Messages emitted but no sender available - channel may not be started!"
            );
            return Ok(());
        };

        let mut rate_limiter = self.rate_limiter.write().await;

        for emitted in messages {
            let mut msg = convert_emitted_to_incoming(&self.name, &emitted);

            if let Ok(metadata) = serde_json::from_str(&emitted.metadata_json) {
                msg = msg.with_metadata(metadata);
                self.update_broadcast_metadata(&emitted.metadata_json).await;
            }

            if !send_with_rate_limit(&self.name, msg, tx, &mut rate_limiter).await? {
                break;
            }
        }

        Ok(())
    }

    /// Start the polling loop if configured.
    ///
    /// Since we can't hold `Arc<Self>` from `&self`, we pass all the components
    /// Dispatch emitted messages to the message channel.
    ///
    /// This is a static helper used by the polling loop since it doesn't have
    /// access to `&self`.
    pub(super) async fn dispatch_emitted_messages(
        channel_name: &str,
        messages: Vec<EmittedMessage>,
        ctx: DispatchContext<'_>,
    ) -> Result<(), WasmChannelError> {
        tracing::info!(
            channel = %channel_name,
            message_count = messages.len(),
            "Processing emitted messages from polling callback"
        );

        let tx_guard = ctx.message_tx.read().await;
        let Some(tx) = tx_guard.as_ref() else {
            tracing::error!(
                channel = %channel_name,
                count = messages.len(),
                "Messages emitted but no sender available - channel may not be started!"
            );
            return Ok(());
        };

        let mut limiter = ctx.rate_limiter.write().await;

        for emitted in messages {
            let mut msg = convert_emitted_to_incoming(channel_name, &emitted);

            if let Ok(metadata) = serde_json::from_str(&emitted.metadata_json) {
                msg = msg.with_metadata(metadata);
                do_update_broadcast_metadata(
                    channel_name,
                    &emitted.metadata_json,
                    ctx.last_broadcast_metadata,
                    ctx.settings_store,
                )
                .await;
            }

            if !send_with_rate_limit(channel_name, msg, tx, &mut limiter).await? {
                break;
            }
        }

        Ok(())
    }
}
