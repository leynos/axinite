//! Configurable channel stub for tests.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::channels::{
    IncomingMessage, MessageStream, NativeChannel, OutgoingResponse, StatusUpdate,
};
use crate::error::ChannelError;

/// A configurable channel stub for tests.
///
/// Supports:
/// - Message injection via the returned `mpsc::Sender`
/// - Response capture for assertion
/// - Status update capture
/// - Configurable health check failure
///
/// # Usage
///
/// ```rust,no_run
/// use ironclaw::prelude::IncomingMessage;
/// use ironclaw::testing::StubChannel;
///
/// # async fn example() {
/// let (channel, sender) = StubChannel::new("test");
/// sender
///     .send(IncomingMessage::new("test", "user1", "hello"))
///     .await
///     .unwrap();
/// // ... run agent logic that calls channel.respond() ...
/// let responses = channel.captured_responses();
/// # let _ = responses;
/// # }
/// ```
pub struct StubChannel {
    name: String,
    rx: tokio::sync::Mutex<Option<mpsc::Receiver<IncomingMessage>>>,
    responses: Arc<Mutex<Vec<(IncomingMessage, OutgoingResponse)>>>,
    statuses: Arc<Mutex<Vec<StatusUpdate>>>,
    healthy: AtomicBool,
}

impl StubChannel {
    /// Create a new stub channel and its message sender.
    ///
    /// The sender is used by tests to inject messages into the channel's stream.
    /// The channel captures all responses and status updates for later assertion.
    pub fn new(name: impl Into<String>) -> (Self, mpsc::Sender<IncomingMessage>) {
        let (tx, rx) = mpsc::channel(64);
        let channel = Self {
            name: name.into(),
            rx: tokio::sync::Mutex::new(Some(rx)),
            responses: Arc::new(Mutex::new(Vec::new())),
            statuses: Arc::new(Mutex::new(Vec::new())),
            healthy: AtomicBool::new(true),
        };
        (channel, tx)
    }

    /// Get all captured (message, response) pairs.
    pub fn captured_responses(&self) -> Vec<(IncomingMessage, OutgoingResponse)> {
        self.responses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Get a shared handle to the response capture list.
    ///
    /// Call this *before* moving the channel into a `ChannelManager`,
    /// since `add()` takes ownership.
    pub fn captured_responses_handle(
        &self,
    ) -> Arc<Mutex<Vec<(IncomingMessage, OutgoingResponse)>>> {
        Arc::clone(&self.responses)
    }

    /// Get all captured status updates.
    pub fn captured_statuses(&self) -> Vec<StatusUpdate> {
        self.statuses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// Get a shared handle to the status capture list.
    pub fn captured_statuses_handle(&self) -> Arc<Mutex<Vec<StatusUpdate>>> {
        Arc::clone(&self.statuses)
    }

    /// Set whether `health_check()` succeeds or fails.
    pub fn set_healthy(&self, healthy: bool) {
        self.healthy.store(healthy, Ordering::Relaxed);
    }
}

impl NativeChannel for StubChannel {
    fn name(&self) -> &str {
        &self.name
    }

    async fn start(&self) -> Result<MessageStream, ChannelError> {
        let rx = self
            .rx
            .lock()
            .await
            .take()
            .ok_or_else(|| ChannelError::StartupFailed {
                name: self.name.clone(),
                reason: "start() already called".to_string(),
            })?;
        let stream = tokio_stream::wrappers::ReceiverStream::new(rx);
        Ok(Box::pin(stream))
    }

    async fn respond(
        &self,
        msg: &IncomingMessage,
        response: OutgoingResponse,
    ) -> Result<(), ChannelError> {
        self.responses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((msg.clone(), response));
        Ok(())
    }

    async fn send_status(
        &self,
        status: StatusUpdate,
        _metadata: &serde_json::Value,
    ) -> Result<(), ChannelError> {
        self.statuses
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(status);
        Ok(())
    }

    async fn health_check(&self) -> Result<(), ChannelError> {
        if self.healthy.load(Ordering::Relaxed) {
            Ok(())
        } else {
            Err(ChannelError::HealthCheckFailed {
                name: self.name.clone(),
            })
        }
    }
}
