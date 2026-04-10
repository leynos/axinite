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

    /// Bind using an already-bound [`tokio::net::TcpListener`], merge all route
    /// fragments, and spawn the server. The listener's local address is stored
    /// in `config.addr` so `current_addr()` stays accurate.
    pub async fn start_with_listener(
        &mut self,
        listener: tokio::net::TcpListener,
    ) -> Result<(), ChannelError> {
        let addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("local_addr failed: {e}"),
            })?;
        self.config.addr = addr;
        let mut app = Router::new();
        for fragment in self.routes.drain(..) {
            app = app.merge(fragment);
        }
        self.merged_router = Some(app.clone());
        self.spawn_on_listener(listener, app).await
    }

    /// Bind a listener to the configured address and spawn the server task.
    /// Private helper used by both start() and restart_with_addr().
    async fn bind_and_spawn(&mut self, app: Router) -> Result<(), ChannelError> {
        let listener = tokio::net::TcpListener::bind(self.config.addr)
            .await
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("Failed to bind to {}: {e}", self.config.addr),
            })?;
        let addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("local_addr failed: {e}"),
            })?;
        self.config.addr = addr;
        self.spawn_on_listener(listener, app).await
    }

    /// Spawn the server on an already-bound listener.
    /// Private helper that contains the common shutdown-channel and task-spawn logic.
    async fn spawn_on_listener(
        &mut self,
        listener: tokio::net::TcpListener,
        app: Router,
    ) -> Result<(), ChannelError> {
        tracing::info!("Webhook server listening on {}", self.config.addr);
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
                tracing::error!("Webhook server error: {e}");
            }
        });
        self.handle = Some(handle);
        Ok(())
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

    /// Shut down the running server and restart it on the already-bound
    /// `listener`, inheriting all previously added routes from
    /// `self.merged_router`.
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

        // Save old state for rollback if spawn fails
        let old_addr = self.config.addr;
        let old_shutdown_tx = self.shutdown_tx.take();
        let old_handle = self.handle.take();

        // Extract address from the provided listener and try to spawn
        let addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("local_addr failed: {e}"),
            })?;
        self.config.addr = addr;

        match self.spawn_on_listener(listener, app).await {
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

    /// Return the current bind address.
    pub fn current_addr(&self) -> SocketAddr {
        self.config.addr
    }

    /// Returns whether the server currently has a running listener task.
    pub fn is_running(&self) -> bool {
        self.handle
            .as_ref()
            .map(|handle| !handle.is_finished())
            .unwrap_or(false)
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
    use std::net::TcpListener as StdTcpListener;

    use axum::Json;
    use rstest::{fixture, rstest};
    use serde_json::json;

    use super::*;

    /// A started webhook server with a `/health` route and a pre-built client.
    struct StartedWebhookServer {
        server: WebhookServer,
        addr: SocketAddr,
        client: reqwest::Client,
    }

    /// Binds an ephemeral port, creates a [`WebhookServer`] with a `/health`
    /// route, starts the server on the already-bound listener, and returns the
    /// address and a client.
    #[fixture]
    async fn started_webhook_server()
    -> Result<StartedWebhookServer, Box<dyn std::error::Error + Send + Sync>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        let mut server = WebhookServer::new(WebhookServerConfig { addr });
        server.add_routes(Router::new().route(
            "/health",
            axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
        ));
        server.start_with_listener(listener).await?;
        Ok(StartedWebhookServer {
            server,
            addr,
            client: reqwest::Client::new(),
        })
    }

    #[rstest]
    #[tokio::test]
    async fn test_restart_with_addr_rebinds_listener(
        #[future] started_webhook_server: Result<
            StartedWebhookServer,
            Box<dyn std::error::Error + Send + Sync>,
        >,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let StartedWebhookServer {
            mut server,
            addr: addr1,
            client,
        } = started_webhook_server.await?;

        assert_eq!(
            server.current_addr(),
            addr1,
            "Server should be bound to initial address"
        );

        let response = client
            .get(format!("http://{}/health", addr1))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            200,
            "First server should respond to health check"
        );

        // Find a second available port and restart
        let port2 = {
            let listener = StdTcpListener::bind("127.0.0.1:0")?;
            listener.local_addr()?.port()
        };
        let addr2: SocketAddr = format!("127.0.0.1:{}", port2).parse()?;

        server.restart_with_addr(addr2).await?;

        assert_eq!(
            server.current_addr(),
            addr2,
            "Server address should be updated after restart"
        );
        assert_ne!(
            addr1, addr2,
            "Address should change after restart_with_addr"
        );

        let response = client
            .get(format!("http://{}/health", addr2))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            200,
            "Restarted server should respond to health check on new address"
        );

        let old_result = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            client.get(format!("http://{}/health", addr1)).send(),
        )
        .await;
        assert!(
            old_result.is_err() || old_result.ok().and_then(|r| r.ok()).is_none(),
            "Old address should not respond after server restarts"
        );

        server.shutdown().await;
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_restart_with_addr_rollback_on_bind_failure(
        #[future] started_webhook_server: Result<
            StartedWebhookServer,
            Box<dyn std::error::Error + Send + Sync>,
        >,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let StartedWebhookServer {
            mut server,
            addr: addr1,
            client,
        } = started_webhook_server.await?;

        let response = client
            .get(format!("http://{}/health", addr1))
            .send()
            .await?;
        assert_eq!(response.status(), 200, "Server should be listening");

        // Occupy a second port so the restart bind fails deterministically.
        let occupied_listener = StdTcpListener::bind("127.0.0.1:0")?;
        let conflict_addr = occupied_listener.local_addr()?;

        let result = server.restart_with_addr(conflict_addr).await;
        assert!(
            result.is_err(),
            "Restart with already-bound address should fail"
        );

        drop(occupied_listener);

        let response = client
            .get(format!("http://{}/health", addr1))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            200,
            "Old listener should still be running after failed restart"
        );

        assert_eq!(
            server.current_addr(),
            addr1,
            "Server address should be restored after failed restart"
        );

        server.shutdown().await;
        Ok(())
    }
}
