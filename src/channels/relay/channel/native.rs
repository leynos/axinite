//! `NativeChannel` trait implementation for `RelayChannel`.
//!
//! Handles SSE stream startup with reconnect/backoff, token renewal,
//! message conversion, response sending via the provider proxy, and
//! clean shutdown of background tasks.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::channels::relay::client::{ChannelEvent, RelayError};
use crate::channels::{
    IncomingMessage, MessageStream, NativeChannel, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;

use super::RelayChannel;

/// Whether a relay event lacks any of the identifiers required for routing.
fn missing_required_fields(event: &ChannelEvent) -> bool {
    // sender and channel identify the message origin; provider scope routes it.
    let missing_ids = event.sender_id.is_empty() || event.channel_id.is_empty();
    missing_ids || event.provider_scope.is_empty()
}

impl NativeChannel for RelayChannel {
    fn name(&self) -> &str {
        self.provider.channel_name()
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let channel_name = self.name().to_string();
        let token = self.stream_token.read().await.clone();
        let (stream, initial_parser_handle) = self
            .client
            .connect_stream(&token, self.stream_timeout_secs)
            .await
            .map_err(|e| ChannelError::StartupFailed {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?;

        *self.parser_handle.write().await = Some(initial_parser_handle);

        let (tx, rx) = mpsc::channel(64);

        // Spawn the stream reader + reconnect task
        let client = self.client.clone();
        let stream_token = Arc::clone(&self.stream_token);
        let instance_id = self.instance_id.clone();
        let user_id = self.user_id.clone();
        let team_id = self.team_id.clone();
        let stream_timeout_secs = self.stream_timeout_secs;
        let backoff_initial_ms = self.backoff_initial_ms;
        let backoff_max_ms = self.backoff_max_ms;
        let max_consecutive_failures = self.max_consecutive_failures;
        let parser_handle = Arc::clone(&self.parser_handle);
        let provider_str = self.provider.as_str().to_string();
        let relay_name = channel_name.clone();

        let handle = tokio::spawn(async move {
            use futures::StreamExt;

            let mut current_stream = stream;
            let mut backoff_ms = backoff_initial_ms;
            let mut consecutive_failures: u64 = 0;

            loop {
                // Read events from the current stream
                while let Some(event) = current_stream.next().await {
                    // Reset backoff and failure count on successful event
                    backoff_ms = backoff_initial_ms;
                    consecutive_failures = 0;

                    // Validate required fields
                    if missing_required_fields(&event) {
                        tracing::debug!(
                            event_type = %event.event_type,
                            sender_id = %event.sender_id,
                            channel_id = %event.channel_id,
                            "Relay: skipping event with missing required fields"
                        );
                        continue;
                    }

                    // Skip non-message events
                    if !event.is_message() {
                        tracing::debug!(
                            event_type = %event.event_type,
                            "Relay: skipping non-message event"
                        );
                        continue;
                    }

                    tracing::info!(
                        event_type = %event.event_type,
                        sender = %event.sender_id,
                        channel = %event.channel_id,
                        provider = %provider_str,
                        "Relay: received message from {}", provider_str
                    );

                    let msg = IncomingMessage::new(&relay_name, &event.sender_id, event.text())
                        .with_user_name(event.display_name())
                        .with_metadata(serde_json::json!({
                            "team_id": event.team_id(),
                            "channel_id": event.channel_id,
                            "sender_id": event.sender_id,
                            "sender_name": event.display_name(),
                            "event_type": event.event_type,
                            "thread_id": event.thread_id,
                            "provider": event.provider,
                        }));

                    let msg = if let Some(ref thread_id) = event.thread_id {
                        msg.with_thread(thread_id)
                    } else {
                        msg.with_thread(&event.channel_id)
                    };

                    if tx.send(msg).await.is_err() {
                        tracing::info!("Relay channel receiver dropped, stopping");
                        return;
                    }
                }

                // Stream ended, attempt reconnect with backoff
                consecutive_failures += 1;
                if consecutive_failures >= max_consecutive_failures {
                    tracing::error!(
                        channel = %relay_name,
                        failures = consecutive_failures,
                        "Relay channel giving up after {} consecutive failures",
                        consecutive_failures
                    );
                    break;
                }

                tracing::warn!(
                    backoff_ms = backoff_ms,
                    failures = consecutive_failures,
                    "Relay SSE stream ended, reconnecting..."
                );
                tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                backoff_ms = (backoff_ms * 2).min(backoff_max_ms);

                // Try to reconnect
                let token = stream_token.read().await.clone();
                match client.connect_stream(&token, stream_timeout_secs).await {
                    Ok((new_stream, new_parser)) => {
                        tracing::info!("Relay SSE stream reconnected");
                        current_stream = new_stream;
                        // Abort old parser before replacing
                        if let Some(old) = parser_handle.write().await.take() {
                            old.abort();
                        }
                        *parser_handle.write().await = Some(new_parser);
                    }
                    Err(RelayError::TokenExpired) => {
                        // Attempt token renewal
                        tracing::info!("Relay stream token expired, renewing...");
                        match client.renew_token(&instance_id, &user_id).await {
                            Ok(new_token) => {
                                *stream_token.write().await = new_token.clone();
                                match client.connect_stream(&new_token, stream_timeout_secs).await {
                                    Ok((new_stream, new_parser)) => {
                                        tracing::info!(
                                            "Relay SSE stream reconnected with new token"
                                        );
                                        current_stream = new_stream;
                                        if let Some(old) = parser_handle.write().await.take() {
                                            old.abort();
                                        }
                                        *parser_handle.write().await = Some(new_parser);
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            error = %e,
                                            "Failed to reconnect after token renewal"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    error = %e,
                                    "Failed to renew relay stream token"
                                );
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to reconnect relay SSE stream");
                    }
                }

                // Check if the team is still valid (skip when team_id is unknown,
                // e.g. when no DB store was available at activation time)
                if !team_id.is_empty() {
                    match client.list_connections(&instance_id).await {
                        Ok(conns) => {
                            let has_team =
                                conns.iter().any(|c| c.team_id == team_id && c.connected);
                            if !has_team {
                                tracing::warn!(
                                    team_id = %team_id,
                                    "Team no longer connected, stopping relay channel"
                                );
                                return;
                            }
                        }
                        Err(e) => {
                            tracing::warn!(
                                error = %e,
                                "Could not verify team connection, will retry next iteration"
                            );
                        }
                    }
                }
            }
        });

        *self.reconnect_handle.write().await = Some(handle);

        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let channel_name = self.name().to_string();
        let metadata = &msg.metadata;
        let team_id = metadata
            .get("team_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.team_id);
        let channel_id = metadata
            .get("channel_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ChannelError::SendFailed {
                name: channel_name.clone(),
                reason: "Missing channel_id in message metadata".to_string(),
            })?;

        // Determine thread_id from response or metadata
        let thread_id = response
            .thread_id
            .as_deref()
            .or_else(|| metadata.get("thread_id").and_then(|v| v.as_str()));

        let (method, body) = self.build_send_body(channel_id, &response.content, thread_id);

        self.proxy_send(team_id, &method, body)
            .await
            .map_err(|e| ChannelError::SendFailed {
                name: channel_name,
                reason: e.to_string(),
            })?;

        Ok(())
    }

    /// Status updates are not forwarded to messaging providers to avoid noise.
    async fn send_status(
        &self,
        _status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        Ok(())
    }

    async fn broadcast(
        &self,
        target: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let channel_name = self.name().to_string();

        // Determine thread_id from response or metadata
        let thread_id = response
            .thread_id
            .as_deref()
            .or_else(|| response.metadata.get("thread_ts").and_then(|v| v.as_str()));

        let (method, body) = self.build_send_body(target, &response.content, thread_id);

        self.proxy_send(&self.team_id, &method, body)
            .await
            .map_err(|e| ChannelError::SendFailed {
                name: channel_name,
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        self.client
            .list_connections(&self.instance_id)
            .await
            .map_err(|_| ChannelError::HealthCheckFailed {
                name: self.name().to_string(),
            })?;
        Ok(())
    }

    fn conversation_context(&self, metadata: &serde_json::Value) -> HashMap<String, String> {
        let mut ctx = HashMap::new();

        if let Some(sender) = metadata.get("sender_name").and_then(|v| v.as_str()) {
            ctx.insert("sender".to_string(), sender.to_string());
        }
        if let Some(sender_id) = metadata.get("sender_id").and_then(|v| v.as_str()) {
            ctx.insert("sender_uuid".to_string(), sender_id.to_string());
        }
        if let Some(channel_id) = metadata.get("channel_id").and_then(|v| v.as_str()) {
            ctx.insert("group".to_string(), channel_id.to_string());
        }
        ctx.insert("platform".to_string(), self.provider.as_str().to_string());

        ctx
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        if let Some(handle) = self.reconnect_handle.write().await.take() {
            handle.abort();
        }
        if let Some(handle) = self.parser_handle.write().await.take() {
            handle.abort();
        }
        Ok(())
    }
}
