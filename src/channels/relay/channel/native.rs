//! `NativeChannel` trait implementation for `RelayChannel`.
//!
//! Handles SSE stream startup with reconnect/backoff, token renewal,
//! message conversion, response sending via the provider proxy, and
//! clean shutdown of background tasks.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::mpsc;

use crate::channels::{
    IncomingMessage, MessageStream, NativeChannel, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;

use super::RelayChannel;
use super::stream_task::RelayStreamTask;

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
        let task = RelayStreamTask {
            client: self.client.clone(),
            stream_token: Arc::clone(&self.stream_token),
            instance_id: self.instance_id.clone(),
            user_id: self.user_id.clone(),
            team_id: self.team_id.clone(),
            stream_timeout_secs: self.stream_timeout_secs,
            backoff_initial_ms: self.backoff_initial_ms,
            backoff_max_ms: self.backoff_max_ms,
            max_consecutive_failures: self.max_consecutive_failures,
            parser_handle: Arc::clone(&self.parser_handle),
            provider_str: self.provider.as_str().to_string(),
            relay_name: channel_name.clone(),
            tx,
        };
        let handle = tokio::spawn(task.run(stream));

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
