//! Listener control abstraction for hot-reload.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;

use tokio::sync::Mutex;

use crate::channels::WebhookServer;
use crate::error::ChannelError;

/// Boxed future used at the dyn listener-controller boundary.
pub type ListenerControllerFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for controlling HTTP listeners during hot-reload.
///
/// Implementations manage listener restarts without exposing
/// internal server details.
pub trait ListenerController: Send + Sync {
    /// Get the current bind address.
    fn current_addr<'a>(&'a self) -> ListenerControllerFuture<'a, SocketAddr>;

    /// Restart the listener on a new address.
    ///
    /// If the restart fails, the listener should remain on the old address.
    fn restart_with_addr<'a>(
        &'a self,
        addr: SocketAddr,
    ) -> ListenerControllerFuture<'a, Result<(), ChannelError>>;
}

/// Native async sibling trait for concrete listener-controller implementations.
pub trait NativeListenerController: Send + Sync {
    /// See [`ListenerController::current_addr`].
    fn current_addr(&self) -> impl Future<Output = SocketAddr> + Send + '_;

    /// See [`ListenerController::restart_with_addr`].
    fn restart_with_addr(
        &self,
        addr: SocketAddr,
    ) -> impl Future<Output = Result<(), ChannelError>> + Send + '_;
}

impl<T> ListenerController for T
where
    T: NativeListenerController + Send + Sync,
{
    fn current_addr<'a>(&'a self) -> ListenerControllerFuture<'a, SocketAddr> {
        Box::pin(NativeListenerController::current_addr(self))
    }

    fn restart_with_addr<'a>(
        &'a self,
        addr: SocketAddr,
    ) -> ListenerControllerFuture<'a, Result<(), ChannelError>> {
        Box::pin(NativeListenerController::restart_with_addr(self, addr))
    }
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

impl NativeListenerController for WebhookListenerController {
    async fn current_addr(&self) -> SocketAddr {
        let server = self.server.lock().await;
        server.current_addr()
    }

    async fn restart_with_addr(&self, addr: SocketAddr) -> Result<(), ChannelError> {
        let mut server = self.server.lock().await;
        server.restart_with_addr(addr).await
    }
}
