//! Background SSE stream reader for the relay channel.
//!
//! Owns the reconnect/backoff loop: reads events from the current stream,
//! converts them to `IncomingMessage`s, and on stream end reconnects with
//! exponential backoff, renewing the stream token when it expires.

use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};

use crate::channels::IncomingMessage;
use crate::channels::relay::client::{ChannelEvent, ChannelEventStream, RelayClient, RelayError};

/// Whether a relay event lacks any of the identifiers required for routing.
fn missing_required_fields(event: &ChannelEvent) -> bool {
    // sender and channel identify the message origin; provider scope routes it.
    let missing_ids = event.sender_id.is_empty() || event.channel_id.is_empty();
    missing_ids || event.provider_scope.is_empty()
}

/// State and collaborators for the relay stream reader task.
pub(super) struct RelayStreamTask {
    pub(super) client: RelayClient,
    pub(super) stream_token: Arc<RwLock<String>>,
    pub(super) instance_id: String,
    pub(super) user_id: String,
    pub(super) team_id: String,
    pub(super) stream_timeout_secs: u64,
    pub(super) backoff_initial_ms: u64,
    pub(super) backoff_max_ms: u64,
    pub(super) max_consecutive_failures: u64,
    pub(super) parser_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
    pub(super) provider_str: String,
    pub(super) relay_name: String,
    pub(super) tx: mpsc::Sender<IncomingMessage>,
}

impl RelayStreamTask {
    /// Run the read/reconnect loop until the receiver drops, the team
    /// disconnects, or too many consecutive failures accumulate.
    pub(super) async fn run(self, initial_stream: ChannelEventStream) {
        let mut current_stream = initial_stream;
        let mut backoff_ms = self.backoff_initial_ms;
        let mut consecutive_failures: u64 = 0;

        loop {
            if !self
                .read_events(
                    &mut current_stream,
                    &mut backoff_ms,
                    &mut consecutive_failures,
                )
                .await
            {
                return;
            }

            // Stream ended, attempt reconnect with backoff
            consecutive_failures += 1;
            if consecutive_failures >= self.max_consecutive_failures {
                tracing::error!(
                    channel = %self.relay_name,
                    failures = consecutive_failures,
                    "Relay channel giving up after {} consecutive failures",
                    consecutive_failures
                );
                return;
            }

            tracing::warn!(
                backoff_ms = backoff_ms,
                failures = consecutive_failures,
                "Relay SSE stream ended, reconnecting..."
            );
            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
            backoff_ms = (backoff_ms * 2).min(self.backoff_max_ms);

            if let Some(new_stream) = self.reconnect().await {
                current_stream = new_stream;
            }

            if !self.team_still_connected().await {
                return;
            }
        }
    }

    /// Read events from the stream until it ends.
    ///
    /// Returns `false` when the message receiver has dropped and the task
    /// should stop.
    async fn read_events(
        &self,
        stream: &mut ChannelEventStream,
        backoff_ms: &mut u64,
        consecutive_failures: &mut u64,
    ) -> bool {
        use futures::StreamExt;

        while let Some(event) = stream.next().await {
            // Reset backoff and failure count on successful event
            *backoff_ms = self.backoff_initial_ms;
            *consecutive_failures = 0;

            if !self.is_routable_message(&event) {
                continue;
            }

            tracing::info!(
                event_type = %event.event_type,
                sender = %event.sender_id,
                channel = %event.channel_id,
                provider = %self.provider_str,
                "Relay: received message from {}", self.provider_str
            );

            if self.tx.send(self.convert_event(&event)).await.is_err() {
                tracing::info!("Relay channel receiver dropped, stopping");
                return false;
            }
        }
        true
    }

    /// Whether the event carries all routing identifiers and is a message
    /// (skipped events are logged at debug level).
    fn is_routable_message(&self, event: &ChannelEvent) -> bool {
        // Validate required fields
        if missing_required_fields(event) {
            tracing::debug!(
                event_type = %event.event_type,
                sender_id = %event.sender_id,
                channel_id = %event.channel_id,
                "Relay: skipping event with missing required fields"
            );
            return false;
        }

        // Skip non-message events
        if !event.is_message() {
            tracing::debug!(
                event_type = %event.event_type,
                "Relay: skipping non-message event"
            );
            return false;
        }
        true
    }

    /// Convert a relay event to an incoming message, threading by the
    /// event's thread ID or falling back to the channel ID.
    fn convert_event(&self, event: &ChannelEvent) -> IncomingMessage {
        let msg = IncomingMessage::new(&self.relay_name, &event.sender_id, event.text())
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

        if let Some(ref thread_id) = event.thread_id {
            msg.with_thread(thread_id)
        } else {
            msg.with_thread(&event.channel_id)
        }
    }

    /// Attempt to reconnect the SSE stream, renewing the token when it has
    /// expired. Returns the new stream on success.
    async fn reconnect(&self) -> Option<ChannelEventStream> {
        let token = self.stream_token.read().await.clone();
        match self
            .client
            .connect_stream(&token, self.stream_timeout_secs)
            .await
        {
            Ok((new_stream, new_parser)) => {
                tracing::info!("Relay SSE stream reconnected");
                self.replace_parser(new_parser).await;
                Some(new_stream)
            }
            Err(RelayError::TokenExpired) => self.reconnect_with_renewed_token().await,
            Err(e) => {
                tracing::error!(error = %e, "Failed to reconnect relay SSE stream");
                None
            }
        }
    }

    /// Renew the stream token and reconnect with it.
    async fn reconnect_with_renewed_token(&self) -> Option<ChannelEventStream> {
        tracing::info!("Relay stream token expired, renewing...");
        let new_token = match self
            .client
            .renew_token(&self.instance_id, &self.user_id)
            .await
        {
            Ok(token) => token,
            Err(e) => {
                tracing::error!(error = %e, "Failed to renew relay stream token");
                return None;
            }
        };
        *self.stream_token.write().await = new_token.clone();
        match self
            .client
            .connect_stream(&new_token, self.stream_timeout_secs)
            .await
        {
            Ok((new_stream, new_parser)) => {
                tracing::info!("Relay SSE stream reconnected with new token");
                self.replace_parser(new_parser).await;
                Some(new_stream)
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to reconnect after token renewal");
                None
            }
        }
    }

    /// Abort the previous SSE parser task and store the new one.
    async fn replace_parser(&self, new_parser: tokio::task::JoinHandle<()>) {
        if let Some(old) = self.parser_handle.write().await.take() {
            old.abort();
        }
        *self.parser_handle.write().await = Some(new_parser);
    }

    /// Whether the relay team is still connected.
    ///
    /// Returns `true` (continue) when `team_id` is unknown — e.g. when no DB
    /// store was available at activation time — or when the check itself
    /// fails (it is retried on the next iteration).
    async fn team_still_connected(&self) -> bool {
        if self.team_id.is_empty() {
            return true;
        }
        match self.client.list_connections(&self.instance_id).await {
            Ok(conns) => {
                let has_team = conns
                    .iter()
                    .any(|c| c.team_id == self.team_id && c.connected);
                if !has_team {
                    tracing::warn!(
                        team_id = %self.team_id,
                        "Team no longer connected, stopping relay channel"
                    );
                }
                has_team
            }
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "Could not verify team connection, will retry next iteration"
                );
                true
            }
        }
    }
}
