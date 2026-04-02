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

    /// Shutdown the listener gracefully.
    ///
    /// Signals the listener to stop accepting new connections and
    /// waits for existing connections to complete.
    fn shutdown<'a>(&'a self) -> ListenerControllerFuture<'a, ()>;
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

    /// See [`ListenerController::shutdown`].
    fn shutdown(&self) -> impl Future<Output = ()> + Send + '_;
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

    fn shutdown<'a>(&'a self) -> ListenerControllerFuture<'a, ()> {
        Box::pin(NativeListenerController::shutdown(self))
    }
}

/// Listener controller for the webhook server.
pub struct WebhookListenerController {
    server: Arc<Mutex<WebhookServer>>,
}

impl WebhookListenerController {
    /// Create a new webhook listener controller.
    ///
    /// `server` — shared webhook server wrapped in an async mutex; the Arc is
    /// cloned but the mutex must be held to access the server. Thread-safe
    /// because all state changes go through the mutex guard.
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

    async fn shutdown(&self) {
        let mut server = self.server.lock().await;
        server.shutdown().await;
    }
}

#[cfg(test)]
mod tests {
    use axum::{Json, Router, routing::get};
    use serde_json::json;
    use std::net::TcpListener as StdTcpListener;

    use super::*;
    use crate::channels::WebhookServerConfig;

    fn reserve_addr() -> SocketAddr {
        let listener =
            StdTcpListener::bind("127.0.0.1:0").expect("should reserve an ephemeral port");
        listener
            .local_addr()
            .expect("reserved listener should report its address")
    }

    #[tokio::test]
    async fn webhook_listener_controller_drives_webhook_server_lifecycle() {
        let addr1 = reserve_addr();
        let addr2 = reserve_addr();

        let mut server = WebhookServer::new(WebhookServerConfig { addr: addr1 });
        server.add_routes(
            Router::new().route("/health", get(|| async { Json(json!({ "status": "ok" })) })),
        );
        server.start().await.expect("server should start");

        let server = Arc::new(Mutex::new(server));
        let controller = WebhookListenerController::new(Arc::clone(&server));

        assert_eq!(
            NativeListenerController::current_addr(&controller).await,
            addr1,
            "current_addr() should report the bound listener address"
        );

        NativeListenerController::restart_with_addr(&controller, addr2)
            .await
            .expect("restart_with_addr() should rebind the server");
        assert_eq!(
            NativeListenerController::current_addr(&controller).await,
            addr2,
            "restart_with_addr() should update the listener address"
        );

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://{addr2}/health"))
            .send()
            .await
            .expect("restarted listener should serve requests");
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        NativeListenerController::shutdown(&controller).await;

        let shutdown_result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            client.get(format!("http://{addr2}/health")).send(),
        )
        .await;
        assert!(
            shutdown_result.is_err()
                || shutdown_result
                    .as_ref()
                    .is_ok_and(|request_result| request_result.is_err()),
            "shutdown() should stop the listener from serving requests"
        );
    }
}
