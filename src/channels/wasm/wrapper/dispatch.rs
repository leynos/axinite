//! Dispatch WASM-emitted messages into native channel messages.

use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};

use crate::channels::IncomingMessage;
use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::host::{ChannelEmitRateLimiter, EmittedMessage};

use super::{WasmChannel, do_update_broadcast_metadata};

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
            // Check rate limit
            if !rate_limiter.check_and_record() {
                tracing::warn!(
                    channel = %self.name,
                    "Message emission rate limited"
                );
                return Err(WasmChannelError::EmitRateLimited {
                    name: self.name.clone(),
                });
            }

            // Convert to IncomingMessage
            let mut msg = IncomingMessage::new(&self.name, &emitted.user_id, &emitted.content);

            if let Some(name) = emitted.user_name {
                msg = msg.with_user_name(name);
            }

            if let Some(thread_id) = emitted.thread_id {
                msg = msg.with_thread(thread_id);
            }

            // Convert attachments
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

            // Parse metadata JSON
            if let Ok(metadata) = serde_json::from_str(&emitted.metadata_json) {
                msg = msg.with_metadata(metadata);
                // Store for broadcast routing (chat_id etc.)
                self.update_broadcast_metadata(&emitted.metadata_json).await;
            }

            // Send to stream
            tracing::info!(
                channel = %self.name,
                user_id = %emitted.user_id,
                content_len = emitted.content.len(),
                attachment_count = msg.attachments.len(),
                "Sending emitted message to agent"
            );

            if tx.send(msg).await.is_err() {
                tracing::error!(
                    channel = %self.name,
                    "Failed to send emitted message, channel closed"
                );
                break;
            }

            tracing::info!(
                channel = %self.name,
                "Message successfully sent to agent queue"
            );
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
        message_tx: &RwLock<Option<mpsc::Sender<IncomingMessage>>>,
        rate_limiter: &RwLock<ChannelEmitRateLimiter>,
        last_broadcast_metadata: &tokio::sync::RwLock<Option<String>>,
        settings_store: Option<&Arc<dyn crate::db::SettingsStore>>,
    ) -> Result<(), WasmChannelError> {
        tracing::info!(
            channel = %channel_name,
            message_count = messages.len(),
            "Processing emitted messages from polling callback"
        );

        let tx_guard = message_tx.read().await;
        let Some(tx) = tx_guard.as_ref() else {
            tracing::error!(
                channel = %channel_name,
                count = messages.len(),
                "Messages emitted but no sender available - channel may not be started!"
            );
            return Ok(());
        };

        let mut limiter = rate_limiter.write().await;

        for emitted in messages {
            // Check rate limit
            if !limiter.check_and_record() {
                tracing::warn!(
                    channel = %channel_name,
                    "Message emission rate limited"
                );
                return Err(WasmChannelError::EmitRateLimited {
                    name: channel_name.to_string(),
                });
            }

            // Convert to IncomingMessage
            let mut msg = IncomingMessage::new(channel_name, &emitted.user_id, &emitted.content);

            if let Some(name) = emitted.user_name {
                msg = msg.with_user_name(name);
            }

            if let Some(thread_id) = emitted.thread_id {
                msg = msg.with_thread(thread_id);
            }

            // Convert attachments
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

            // Parse metadata JSON
            if let Ok(metadata) = serde_json::from_str(&emitted.metadata_json) {
                msg = msg.with_metadata(metadata);
                // Store for broadcast routing (chat_id etc.)
                do_update_broadcast_metadata(
                    channel_name,
                    &emitted.metadata_json,
                    last_broadcast_metadata,
                    settings_store,
                )
                .await;
            }

            // Send to stream
            tracing::info!(
                channel = %channel_name,
                user_id = %emitted.user_id,
                content_len = emitted.content.len(),
                attachment_count = msg.attachments.len(),
                "Sending polled message to agent"
            );

            if tx.send(msg).await.is_err() {
                tracing::error!(
                    channel = %channel_name,
                    "Failed to send polled message, channel closed"
                );
                break;
            }

            tracing::info!(
                channel = %channel_name,
                "Message successfully sent to agent queue"
            );
        }

        Ok(())
    }
}
