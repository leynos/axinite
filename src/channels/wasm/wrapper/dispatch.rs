//! Dispatch WASM-emitted messages into native channel messages.

use std::sync::Arc;

use tokio::sync::mpsc;

use crate::channels::IncomingMessage;
use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::host::{ChannelEmitRateLimiter, EmittedMessage};

use super::{WasmChannel, do_update_broadcast_metadata};

/// Bundles the per-channel async state required when dispatching messages
/// from the polling path.
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
            .map(|attachment| crate::channels::IncomingAttachment {
                id: attachment.id.clone(),
                kind: crate::channels::AttachmentKind::from_mime_type(&attachment.mime_type),
                mime_type: attachment.mime_type.clone(),
                filename: attachment.filename.clone(),
                size_bytes: attachment.size_bytes,
                source_url: attachment.source_url.clone(),
                storage_key: attachment.storage_key.clone(),
                extracted_text: attachment.extracted_text.clone(),
                data: attachment.data.clone(),
                duration_secs: attachment.duration_secs,
            })
            .collect();
        msg = msg.with_attachments(incoming_attachments);
    }

    msg
}

fn apply_emitted_metadata(
    mut msg: IncomingMessage,
    emitted: &EmittedMessage,
) -> (IncomingMessage, bool) {
    if let Ok(metadata) = serde_json::from_str(&emitted.metadata_json) {
        msg = msg.with_metadata(metadata);
        return (msg, true);
    }

    (msg, false)
}

/// Reserve one emit slot before performing any message side effects.
fn reserve_emit_slot(
    channel_name: &str,
    limiter: &mut ChannelEmitRateLimiter,
) -> Result<(), WasmChannelError> {
    if !limiter.check_and_record() {
        tracing::warn!(channel = %channel_name, "Message emission rate limited");
        return Err(WasmChannelError::EmitRateLimited {
            name: channel_name.to_string(),
        });
    }

    Ok(())
}

/// Sends `msg` on `tx`.
///
/// Returns `Ok(false)` when the sender is closed, so callers can `break` out
/// of their dispatch loop.
async fn send_to_agent(
    channel_name: &str,
    msg: IncomingMessage,
    tx: &mpsc::Sender<IncomingMessage>,
) -> Result<bool, WasmChannelError> {
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
            reserve_emit_slot(&self.name, &mut rate_limiter)?;

            let msg = convert_emitted_to_incoming(&self.name, &emitted);
            let (msg, has_metadata) = apply_emitted_metadata(msg, &emitted);

            if has_metadata {
                self.update_broadcast_metadata(&emitted.metadata_json).await;
            }

            if !send_to_agent(&self.name, msg, tx).await? {
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
            reserve_emit_slot(channel_name, &mut limiter)?;

            let msg = convert_emitted_to_incoming(channel_name, &emitted);
            let (msg, has_metadata) = apply_emitted_metadata(msg, &emitted);

            if has_metadata {
                do_update_broadcast_metadata(
                    channel_name,
                    &emitted.metadata_json,
                    ctx.last_broadcast_metadata,
                    ctx.settings_store,
                )
                .await;
            }

            if !send_to_agent(channel_name, msg, tx).await? {
                break;
            }
        }

        Ok(())
    }
}
