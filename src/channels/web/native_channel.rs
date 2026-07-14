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
        let thread_id = match &msg.thread_id {
            Some(tid) => tid.clone(),
            None => {
                tracing::warn!(
                    "Gateway respond with no thread_id — skipping (clients would drop it)"
                );
                return Ok(());
            }
        };

        self.state.sse.broadcast(SseEvent::Response {
            content: response.content,
            thread_id,
        });

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
        let event = match status {
            StatusUpdate::Thinking(msg) => SseEvent::Thinking {
                message: msg,
                thread_id: thread_id.clone(),
            },
            StatusUpdate::ToolStarted { name } => SseEvent::ToolStarted {
                name,
                thread_id: thread_id.clone(),
            },
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
                thread_id: thread_id.clone(),
            },
            StatusUpdate::ToolResult { name, preview } => SseEvent::ToolResult {
                name,
                preview,
                thread_id: thread_id.clone(),
            },
            StatusUpdate::StreamChunk(content) => SseEvent::StreamChunk {
                content,
                thread_id: thread_id.clone(),
            },
            StatusUpdate::Status(msg) => SseEvent::Status {
                message: msg,
                thread_id: thread_id.clone(),
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
        };

        self.state.sse.broadcast(event);
        Ok(())
    }

    async fn broadcast(
        &self,
        _user_id: &str,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        let thread_id = match response.thread_id {
            Some(tid) => tid,
            None => {
                tracing::warn!(
                    "Gateway broadcast with no thread_id — skipping (clients would drop it)"
                );
                return Ok(());
            }
        };
        self.state.sse.broadcast(SseEvent::Response {
            content: response.content,
            thread_id,
        });
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
