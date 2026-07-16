//! HTTP webhook channel for receiving messages via HTTP POST.
//!
//! Request handling (payload types, validation, rate limiting, message
//! forwarding) lives in the `handlers` submodule; this file holds the
//! channel type, its state, and the `NativeChannel` implementation.

use std::sync::Arc;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    routing::{get, post},
};
use secrecy::{ExposeSecret, SecretString};
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio_stream::wrappers::ReceiverStream;
use uuid::Uuid;

use crate::channels::{IncomingMessage, MessageStream, NativeChannel, OutgoingResponse};
use crate::config::HttpConfig;
use crate::error::ChannelError;

mod handlers;

use handlers::{health_handler, webhook_handler};

/// HTTP webhook channel.
pub struct HttpChannel {
    config: HttpConfig,
    state: Arc<HttpChannelState>,
}

pub struct HttpChannelState {
    /// Sender for incoming messages.
    tx: RwLock<Option<mpsc::Sender<IncomingMessage>>>,
    /// Pending responses keyed by message ID.
    pending_responses: RwLock<std::collections::HashMap<Uuid, oneshot::Sender<String>>>,
    /// Expected webhook secret for authentication (if configured).
    /// Stored in a separate Arc<RwLock<>> to avoid contending with other state operations.
    /// Rarely changes (only on SIGHUP), so isolated from hot-path state accesses.
    /// Uses SecretString to prevent accidental logging and memory dump exposure.
    webhook_secret: Arc<RwLock<Option<SecretString>>>,
    /// Fixed user ID for this HTTP channel.
    user_id: String,
    /// Rate limiting state.
    rate_limit: tokio::sync::Mutex<RateLimitState>,
}

#[derive(Debug)]
struct RateLimitState {
    window_start: std::time::Instant,
    request_count: u32,
}

impl HttpChannelState {
    /// Update the webhook secret in-place without restarting the listener.
    /// Called during SIGHUP to hot-swap credentials.
    pub async fn update_secret(&self, new_secret: Option<SecretString>) {
        *self.webhook_secret.write().await = new_secret;
    }
}

/// Maximum JSON body size for webhook requests (15 MB, to support base64 image attachments
/// with ~33% overhead from base64 encoding).
const MAX_BODY_BYTES: usize = 15 * 1024 * 1024;

impl HttpChannel {
    /// Create a new HTTP channel.
    pub fn new(config: HttpConfig) -> Self {
        let webhook_secret = config
            .webhook_secret
            .as_ref()
            .map(|s| SecretString::from(s.expose_secret().to_string()));
        let user_id = config.user_id.clone();

        Self {
            config,
            state: Arc::new(HttpChannelState {
                tx: RwLock::new(None),
                pending_responses: RwLock::new(std::collections::HashMap::new()),
                webhook_secret: Arc::new(RwLock::new(webhook_secret)),
                user_id,
                rate_limit: tokio::sync::Mutex::new(RateLimitState {
                    window_start: std::time::Instant::now(),
                    request_count: 0,
                }),
            }),
        }
    }

    /// Return the channel's axum routes with state applied.
    ///
    /// The returned `Router` shares the same `Arc<HttpChannelState>` that
    /// `start()` later populates. Before `start()` is called the webhook
    /// handler returns 503 ("Channel not started").
    pub fn routes(&self) -> Router {
        Router::new()
            .route("/health", get(health_handler))
            .route("/webhook", post(webhook_handler))
            .layer(DefaultBodyLimit::max(MAX_BODY_BYTES))
            .with_state(self.state.clone())
    }

    /// Return the configured host and port for this channel.
    pub fn addr(&self) -> (&str, u16) {
        (&self.config.host, self.config.port)
    }

    /// Return a shared handle to the channel state for out-of-band updates.
    pub fn shared_state(&self) -> Arc<HttpChannelState> {
        Arc::clone(&self.state)
    }

    /// Update the webhook secret in-place without restarting the listener.
    pub async fn update_secret(&self, new_secret: Option<SecretString>) {
        self.state.update_secret(new_secret).await;
    }
}

impl NativeChannel for HttpChannel {
    fn name(&self) -> &str {
        "http"
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        if self.state.webhook_secret.read().await.is_none() {
            return Err(ChannelError::StartupFailed {
                name: "http".to_string(),
                reason: "HTTP webhook secret is required (set HTTP_WEBHOOK_SECRET)".to_string(),
            });
        }

        let (tx, rx) = mpsc::channel(256);
        *self.state.tx.write().await = Some(tx);

        tracing::info!(
            "HTTP channel ready ({}:{})",
            self.config.host,
            self.config.port
        );

        Ok(Box::pin(ReceiverStream::new(rx)))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        // Check if there's a pending response waiter
        if let Some(tx) = self.state.pending_responses.write().await.remove(&msg.id) {
            let _ = tx.send(response.content);
        }
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        if self.state.tx.read().await.is_some() {
            Ok(())
        } else {
            Err(ChannelError::HealthCheckFailed {
                name: "http".to_string(),
            })
        }
    }

    async fn shutdown(&self) -> Result<(), ChannelError> {
        *self.state.tx.write().await = None;
        Ok(())
    }
}

/// Implement secret update for HTTP channel state.
/// This allows SIGHUP handler to update secrets generically via the trait.
impl crate::channels::channel::NativeChannelSecretUpdater for HttpChannelState {
    async fn update_secret(&self, new_secret: Option<SecretString>) {
        *self.webhook_secret.write().await = new_secret;
        tracing::info!("HTTP webhook secret updated");
    }
}

#[cfg(test)]
mod tests;
