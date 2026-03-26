//! Unified HTTP server for all webhook routes.
//!
//! Composes route fragments from HttpChannel, WASM channel router, etc.
//! into a single axum server. Channels define routes but never spawn servers.

use std::net::SocketAddr;

use axum::Router;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use crate::error::ChannelError;

/// Configuration for the unified webhook server.
pub struct WebhookServerConfig {
    /// Address to bind the server to.
    pub addr: SocketAddr,
}

/// A single HTTP server that hosts all webhook routes.
///
/// Channels contribute route fragments via `add_routes()`, then a single
/// `start()` call binds the listener and spawns the server task.
pub struct WebhookServer {
    config: WebhookServerConfig,
    routes: Vec<Router>,
    /// Merged router saved after start() for restart_with_addr().
    merged_router: Option<Router>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    handle: Option<JoinHandle<()>>,
}

impl WebhookServer {
    /// Create a new webhook server with the given bind address.
    pub fn new(config: WebhookServerConfig) -> Self {
        Self {
            config,
            routes: Vec::new(),
            merged_router: None,
            shutdown_tx: None,
            handle: None,
        }
    }

    /// Accumulate a route fragment. Each fragment should already have its
    /// state applied via `.with_state()`.
    pub fn add_routes(&mut self, router: Router) {
        self.routes.push(router);
    }

    /// Bind the listener, merge all route fragments, and spawn the server.
    pub async fn start(&mut self) -> Result<(), ChannelError> {
        let mut app = Router::new();
        for fragment in self.routes.drain(..) {
            app = app.merge(fragment);
        }
        self.merged_router = Some(app.clone());
        self.bind_and_spawn(app).await
    }

    /// Accept a pre-bound listener, merge route fragments, and spawn the
    /// server. Eliminates the TOCTOU window that `start()` has between port
    /// discovery and the actual bind, making it suitable for tests that
    /// allocate ephemeral ports.
    #[cfg(test)]
    pub async fn start_with_listener(
        &mut self,
        listener: tokio::net::TcpListener,
    ) -> Result<(), ChannelError> {
        let mut app = Router::new();
        for fragment in self.routes.drain(..) {
            app = app.merge(fragment);
        }
        self.merged_router = Some(app.clone());

        let local_addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("Failed to get listener local address: {e}"),
            })?;
        self.config.addr = local_addr;

        self.spawn_with_listener(listener, app);
        Ok(())
    }

    /// Bind a listener to the configured address and spawn the server task.
    /// Private helper used by both start() and restart_with_addr().
    async fn bind_and_spawn(&mut self, app: Router) -> Result<(), ChannelError> {
        let listener = tokio::net::TcpListener::bind(self.config.addr)
            .await
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("Failed to bind to {}: {}", self.config.addr, e),
            })?;

        // Overwrite the configured address with the concrete socket address
        // so that an ephemeral `:0` bind is resolved to the real port.
        self.config.addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("Failed to get listener local address: {e}"),
            })?;

        self.spawn_with_listener(listener, app);
        Ok(())
    }

    /// Spawn the axum server on an already-bound listener.
    fn spawn_with_listener(&mut self, listener: tokio::net::TcpListener, app: Router) {
        let addr = self.config.addr;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        let handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                    tracing::debug!("Webhook server shutting down");
                })
                .await
            {
                tracing::error!("Webhook server error: {}", e);
            }
        });

        tracing::info!("Webhook server listening on {}", addr);
        self.handle = Some(handle);
    }

    /// Gracefully shut down the current listener and rebind to a new address.
    /// The merged router from the original `start()` call is reused.
    ///
    /// If binding to the new address fails, the old listener remains active and
    /// state is restored. This prevents a denial-of-service if the new address
    /// is invalid or already in use.
    pub async fn restart_with_addr(&mut self, new_addr: SocketAddr) -> Result<(), ChannelError> {
        let app = self
            .merged_router
            .clone()
            .ok_or_else(|| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: "restart_with_addr called before start()".to_string(),
            })?;

        // Save old state for rollback if new bind fails
        let old_addr = self.config.addr;
        let old_shutdown_tx = self.shutdown_tx.take();
        let old_handle = self.handle.take();

        // Update config to new address and try to bind
        self.config.addr = new_addr;
        match self.bind_and_spawn(app).await {
            Ok(()) => {
                // New listener is running, gracefully shut down the old one
                if let Some(tx) = old_shutdown_tx {
                    let _ = tx.send(());
                }
                if let Some(handle) = old_handle {
                    let _ = handle.await;
                }
                Ok(())
            }
            Err(e) => {
                // Restore old state; old listener remains active
                self.config.addr = old_addr;
                self.shutdown_tx = old_shutdown_tx;
                self.handle = old_handle;
                Err(e)
            }
        }
    }

    /// Gracefully shut down the current listener and rebind using a pre-bound
    /// listener. Mirrors [`restart_with_addr`] but eliminates the TOCTOU
    /// window between port reservation and bind, making it suitable for tests.
    #[cfg(test)]
    pub async fn restart_with_listener(
        &mut self,
        listener: tokio::net::TcpListener,
    ) -> Result<(), ChannelError> {
        let app = self
            .merged_router
            .clone()
            .ok_or_else(|| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: "restart_with_listener called before start()".to_string(),
            })?;

        let new_addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("Failed to get listener local address: {e}"),
            })?;

        // Shut down the old listener before spawning on the new one.
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }

        self.config.addr = new_addr;
        self.spawn_with_listener(listener, app);
        Ok(())
    }

    /// Return the current bind address.
    pub fn current_addr(&self) -> SocketAddr {
        self.config.addr
    }

    /// Signal graceful shutdown and wait for the server task to finish.
    pub async fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.handle.take() {
            let _ = handle.await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::Json;
    use serde_json::json;

    #[tokio::test]
    async fn test_restart_with_addr_rebinds_listener() {
        use std::net::TcpListener as StdTcpListener;

        // Bind port 1 and keep the std listener alive to convert to tokio
        // without a TOCTOU gap.
        let std_listener1 = StdTcpListener::bind("127.0.0.1:0").expect("Failed to bind port 1");
        std_listener1
            .set_nonblocking(true)
            .expect("Failed to set non-blocking on port 1");
        let addr1 = std_listener1
            .local_addr()
            .expect("Failed to get local addr for port 1");

        // Bind port 2 and keep the std listener alive to convert to tokio
        // without a TOCTOU gap.
        let std_listener2 = StdTcpListener::bind("127.0.0.1:0").expect("Failed to bind port 2");
        std_listener2
            .set_nonblocking(true)
            .expect("Failed to set non-blocking on port 2");
        let addr2 = std_listener2
            .local_addr()
            .expect("Failed to get local addr for port 2");

        assert_ne!(addr1, addr2, "Should have different addresses");

        // Convert both listeners to tokio up-front so the ports stay bound
        // continuously; no TOCTOU window.
        let tokio_listener1 = tokio::net::TcpListener::from_std(std_listener1)
            .expect("Failed to convert port 1 to tokio listener");
        let tokio_listener2 = tokio::net::TcpListener::from_std(std_listener2)
            .expect("Failed to convert port 2 to tokio listener");

        let mut server = WebhookServer::new(WebhookServerConfig { addr: addr1 });

        let test_router = axum::Router::new().route(
            "/health",
            axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
        );
        server.add_routes(test_router);

        server
            .start_with_listener(tokio_listener1)
            .await
            .expect("Failed to start server");
        assert_eq!(
            server.current_addr(),
            addr1,
            "Server should be bound to initial address"
        );

        // Verify the first server is actually listening
        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://{addr1}/health"))
            .send()
            .await
            .expect("Failed to send request to first server");
        assert_eq!(
            response.status(),
            200,
            "First server should respond to health check"
        );

        // Hand the pre-bound listener directly to the server — no TOCTOU
        // gap because the port was never released.
        server
            .restart_with_listener(tokio_listener2)
            .await
            .expect("Failed to restart with new listener");

        assert_eq!(
            server.current_addr(),
            addr2,
            "Server address should be updated after restart"
        );
        assert_ne!(
            addr1, addr2,
            "Address should change after restart_with_listener"
        );

        // Verify the new server is actually listening on the new address
        let response = client
            .get(format!("http://{addr2}/health"))
            .send()
            .await
            .expect("Failed to send request to restarted server");
        assert_eq!(
            response.status(),
            200,
            "Restarted server should respond to health check on new address"
        );

        // Verify the old address is no longer responding
        let old_result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            client.get(format!("http://{addr1}/health")).send(),
        )
        .await;
        assert!(
            old_result.is_err() || old_result.as_ref().unwrap().is_err(),
            "Old address should not respond after server restarts"
        );

        // Clean up
        server.shutdown().await;
    }

    #[tokio::test]
    async fn test_restart_with_addr_rollback_on_bind_failure() {
        use std::net::TcpListener as StdTcpListener;

        // Bind an ephemeral port and keep the std listener alive so we can
        // convert it to tokio without a TOCTOU gap.
        let std_listener =
            StdTcpListener::bind("127.0.0.1:0").expect("Failed to bind ephemeral port");
        std_listener
            .set_nonblocking(true)
            .expect("Failed to set non-blocking");
        let addr1 = std_listener.local_addr().expect("Failed to get local addr");

        // Bind a second ephemeral port and hold it open — this is the
        // "occupied" address that restart_with_addr must fail to bind.
        let blocker = StdTcpListener::bind("127.0.0.1:0").expect("Failed to bind blocker port");
        let blocked_addr = blocker
            .local_addr()
            .expect("Failed to get blocker local addr");

        // Convert the first listener to tokio and hand it to the server,
        // eliminating any window where the port could be stolen.
        let tokio_listener = tokio::net::TcpListener::from_std(std_listener)
            .expect("Failed to convert to tokio listener");

        let mut server = WebhookServer::new(WebhookServerConfig { addr: addr1 });

        let test_router = axum::Router::new().route(
            "/health",
            axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
        );
        server.add_routes(test_router);

        server
            .start_with_listener(tokio_listener)
            .await
            .expect("Failed to start server");

        // Verify the server is listening
        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://{addr1}/health"))
            .send()
            .await
            .expect("Failed to send request");
        assert_eq!(response.status(), 200, "Server should be listening");

        // Attempt restart on the occupied address (should fail because
        // blocker still holds the port).
        let result = server.restart_with_addr(blocked_addr).await;
        assert!(result.is_err(), "Restart with occupied address should fail");

        // Verify the old address is still responding (rollback succeeded)
        let response = client
            .get(format!("http://{addr1}/health"))
            .send()
            .await
            .expect("Failed to send request to old address");
        assert_eq!(
            response.status(),
            200,
            "Old listener should still be running after failed restart"
        );

        // Verify the server address is unchanged
        assert_eq!(
            server.current_addr(),
            addr1,
            "Server address should be restored after failed restart"
        );

        // Clean up — drop blocker so its port is released
        drop(blocker);
        server.shutdown().await;
    }
}
