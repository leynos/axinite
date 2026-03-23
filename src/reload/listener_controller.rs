//! Listener control abstraction for hot-reload.

use std::net::SocketAddr;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::channels::WebhookServer;
use crate::error::ChannelError;

/// Trait for controlling HTTP listeners during hot-reload.
///
/// Implementations manage listener restarts without exposing
/// internal server details.
#[async_trait]
pub trait ListenerController: Send + Sync {
    /// Get the current bind address.
    fn current_addr(&self) -> SocketAddr;

    /// Restart the listener on a new address.
    ///
    /// If the restart fails, the listener should remain on the old address.
    async fn restart_with_addr(&self, addr: SocketAddr) -> Result<(), ChannelError>;
}

/// Listener controller for the webhook server.
pub struct WebhookListenerController {
    server: Arc<Mutex<WebhookServer>>,
}

impl WebhookListenerController {
    pub fn new(server: Arc<Mutex<WebhookServer>>) -> Self {
        Self { server }
    }
}

#[async_trait]
impl ListenerController for WebhookListenerController {
    fn current_addr(&self) -> SocketAddr {
        // Note: This is a blocking read, but the lock should be uncontended
        // in the common case. The underlying WebhookServer stores addr in
        // a plain field, not behind async I/O.
        //
        // We cannot make this `async fn` because the trait signature requires
        // a synchronous return, and the WebhookServer's current_addr() does
        // not need to be async.
        //
        // If contention becomes an issue in the future, consider storing the
        // address in an AtomicU64 or similar lockless structure.
        let server = self.server.blocking_lock();
        server.current_addr()
    }

    async fn restart_with_addr(&self, addr: SocketAddr) -> Result<(), ChannelError> {
        let mut server = self.server.lock().await;
        server.restart_with_addr(addr).await
    }
}
