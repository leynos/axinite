//! `NativeChannel` trait implementation for [`GatewayChannel`]:
//! startup of the HTTP server, responding over SSE, status-update
//! translation to SSE events, broadcast, health check, and shutdown.

use std::net::SocketAddr;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::channels::{
    IncomingMessage, MessageStream, NativeChannel, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;

use super::GatewayChannel;
use super::server;
use super::types::SseEvent;

/// Translate a [`StatusUpdate`] into the matching [`SseEvent`], attaching the
/// optional `thread_id` where the event carries one. Exactly one match arm
/// runs, so `thread_id` is moved rather than cloned.
fn status_to_sse_event(status: StatusUpdate, thread_id: Option<String>) -> SseEvent {
    match status {
        StatusUpdate::Thinking(msg) => SseEvent::Thinking {
            message: msg,
            thread_id,
        },
        StatusUpdate::ToolStarted { name } => SseEvent::ToolStarted { name, thread_id },
        StatusUpdate::ToolCompleted {
            name,
            success,
            error,
            parameters,
        } => SseEvent::ToolCompleted {
            name,
            success,
            error,
            parameters,
            thread_id,
        },
        StatusUpdate::ToolResult { name, preview } => SseEvent::ToolResult {
            name,
            preview,
            thread_id,
        },
        StatusUpdate::StreamChunk(content) => SseEvent::StreamChunk { content, thread_id },
        StatusUpdate::Status(msg) => SseEvent::Status {
            message: msg,
            thread_id,
        },
        StatusUpdate::JobStarted {
            job_id,
            title,
            browse_url,
        } => SseEvent::JobStarted {
            job_id,
            title,
            browse_url,
        },
        StatusUpdate::ApprovalNeeded {
            request_id,
            tool_name,
            description,
            parameters,
        } => SseEvent::ApprovalNeeded {
            request_id,
            tool_name,
            description,
            parameters: serde_json::to_string_pretty(&parameters)
                .unwrap_or_else(|_| parameters.to_string()),
            thread_id,
        },
        StatusUpdate::AuthRequired {
            extension_name,
            instructions,
            auth_url,
            setup_url,
        } => SseEvent::AuthRequired {
            extension_name,
            instructions,
            auth_url,
            setup_url,
        },
        StatusUpdate::AuthCompleted {
            extension_name,
            success,
            message,
        } => SseEvent::AuthCompleted {
            extension_name,
            success,
            message,
        },
        StatusUpdate::ImageGenerated { data_url, path } => SseEvent::ImageGenerated {
            data_url,
            path,
            thread_id,
        },
    }
}

impl GatewayChannel {
    /// Broadcast a response over SSE, skipping (with a warning naming the
    /// calling `operation`) when no thread ID is available — clients would
    /// drop such events.
    fn broadcast_response(&self, thread_id: Option<String>, content: String, operation: &str) {
        let Some(thread_id) = thread_id else {
            tracing::warn!(
                "Gateway {} with no thread_id — skipping (clients would drop it)",
                operation
            );
            return;
        };
        self.state
            .sse
            .broadcast(SseEvent::Response { content, thread_id });
    }
}

impl NativeChannel for GatewayChannel {
    fn name(&self) -> &str {
        "gateway"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let (tx, rx) = mpsc::channel(256);
        *self.state.msg_tx.write().await = Some(tx);

        let addr: SocketAddr = format!("{}:{}", self.config.host, self.config.port)
            .parse()
            .map_err(|e| ChannelError::StartupFailed {
                name: "gateway".to_string(),
                reason: format!(
                    "Invalid address '{}:{}': {}",
                    self.config.host, self.config.port, e
                ),
            })?;

        server::start_server(addr, self.state.clone(), self.auth_token.clone()).await?;

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.broadcast_response(msg.thread_id.clone(), response.content, "respond");
        Ok(())
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        let thread_id = metadata
            .get("thread_id")
            .and_then(|v| v.as_str())
            .map(String::from);
        let event = status_to_sse_event(status, thread_id);

        self.state.sse.broadcast(event);
        Ok(())
    }

    async fn broadcast(
        &self,
        _user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.broadcast_response(response.thread_id, response.content, "broadcast");
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        if self.state.msg_tx.read().await.is_some() {
            Ok(())
        } else {
            Err(ChannelError::HealthCheckFailed {
                name: "gateway".to_string(),
            })
        }
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        if let Some(tx) = self.state.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        }
        *self.state.msg_tx.write().await = None;
        Ok(())
    }
}
