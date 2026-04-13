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
    /// server.
    ///
    /// Unlike [`Self::start`], this test-only entrypoint accepts a listener
    /// that is already bound, eliminating the TOCTOU window between external
    /// test port allocation and the server bind. That makes it suitable for
    /// tests that reserve ephemeral ports before handing ownership to the
    /// server.
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

    /// Shared restart kernel. Saves current listener state, spawns the server on
    /// `listener` bound at `new_addr`, shuts down the old server on success, or
    /// restores the previous state on failure.
    async fn swap_listener(
        &mut self,
        new_addr: SocketAddr,
        listener: tokio::net::TcpListener,
        app: Router,
    ) -> Result<(), ChannelError> {
        let old_addr = self.config.addr;
        let old_shutdown_tx = self.shutdown_tx.take();
        let old_handle = self.handle.take();

        self.config.addr = new_addr;
        match self.spawn_on_listener(listener, app).await {
            Ok(()) => {
                if let Some(tx) = old_shutdown_tx {
                    let _ = tx.send(());
                }
                if let Some(handle) = old_handle {
                    let _ = handle.await;
                }
                Ok(())
            }
            Err(e) => {
                self.config.addr = old_addr;
                self.shutdown_tx = old_shutdown_tx;
                self.handle = old_handle;
                Err(e)
            }
        }
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

        let listener = tokio::net::TcpListener::bind(new_addr).await.map_err(|e| {
            ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("Failed to bind to {new_addr}: {e}"),
            }
        })?;
        let addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("local_addr failed: {e}"),
            })?;

        self.swap_listener(addr, listener, app).await
    }

    /// Gracefully shut down the current listener and rebind using a pre-bound
    /// listener. Eliminates the TOCTOU window between port reservation and
    /// bind, making it suitable for tests.
    ///
    /// Unlike [`restart_with_addr`], this test-only helper does not support
    /// rollback. [`restart_with_addr`] binds the replacement first and can keep
    /// the old listener alive if that bind fails. This method shuts down the
    /// old listener before calling [`Self::spawn_with_listener`], so there is
    /// no rollback path if spawning were to fail. That trade-off is acceptable
    /// in tests because [`Self::spawn_with_listener`] is infallible.
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

        // Stop the old listener before spawning the new one. Unlike
        // restart_with_addr, we do not provide rollback semantics because the
        // new listener is already bound and assumed to be valid.
        self.shutdown().await;

        self.config.addr = new_addr;
        self.spawn_with_listener(listener, app);
        Ok(())
    }

    /// Return the server address currently stored in `self.config.addr`.
    ///
    /// Before the first successful [`Self::start`], `start_with_listener`,
    /// [`Self::restart_with_addr`], or `restart_with_listener` call,
    /// this is only the configured address and may not correspond to a live
    /// bound listener. After a successful start or restart, it reflects the
    /// actual bound address, including any OS-assigned port chosen for `:0`
    /// binds.
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
        client: reqwest::Client,
    }

    /// Bind an ephemeral localhost listener and keep it reserved until the
    /// server takes ownership, eliminating port-probe races in tests.
    async fn bind_ephemeral_listener()
    -> Result<(tokio::net::TcpListener, SocketAddr), Box<dyn std::error::Error + Send + Sync>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
        let addr = listener.local_addr()?;
        Ok((listener, addr))
    }

    /// Create a [`WebhookServer`] with a `/health` route, start it on a
    /// pre-bound ephemeral listener, and return the server and a client.
    #[fixture]
    async fn started_webhook_server()
    -> Result<StartedWebhookServer, Box<dyn std::error::Error + Send + Sync>> {
        let (listener, _) = bind_ephemeral_listener().await?;
        let mut server = WebhookServer::new(WebhookServerConfig {
            addr: "127.0.0.1:0".parse()?,
        });
        server.add_routes(Router::new().route(
            "/health",
            axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
        ));
        server.start_with_listener(listener).await?;
        Ok(StartedWebhookServer {
            server,
            client: reqwest::Client::new(),
        })
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_binds_ephemeral_addr_and_serves_health_check()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut server = WebhookServer::new(WebhookServerConfig {
            addr: "127.0.0.1:0".parse()?,
        });
        server.add_routes(Router::new().route(
            "/health",
            axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
        ));

        server.start().await?;
        let addr = server.current_addr();

        assert_ne!(
            addr.port(),
            0,
            "Server should resolve an ephemeral bind to a concrete port"
        );

        let response = reqwest::Client::new()
            .get(format!("http://{}/health", addr))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            200,
            "Server should respond to health check after start()"
        );

        server.shutdown().await;
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_start_and_restart_with_addr_use_production_bind_path()
    -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut server = WebhookServer::new(WebhookServerConfig {
            addr: "127.0.0.1:0".parse()?,
        });
        server.add_routes(Router::new().route(
            "/health",
            axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
        ));

        server.start().await?;
        let addr1 = server.current_addr();
        assert_ne!(
            addr1.port(),
            0,
            "Server should resolve the initial ephemeral bind to a concrete port"
        );

        let client = reqwest::Client::new();
        let response = client
            .get(format!("http://{}/health", addr1))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            200,
            "Server should respond to health check after start()"
        );

        let restart_addr: SocketAddr = "127.0.0.1:0".parse()?;
        server.restart_with_addr(restart_addr).await?;
        let addr2 = server.current_addr();

        assert_ne!(
            addr2.port(),
            0,
            "Server should resolve the restarted ephemeral bind to a concrete port"
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
            "Server should respond to health check after restart_with_addr"
        );

        server.shutdown().await;
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn test_restart_with_listener(
        #[future] started_webhook_server: Result<
            StartedWebhookServer,
            Box<dyn std::error::Error + Send + Sync>,
        >,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let StartedWebhookServer { mut server, client } = started_webhook_server.await?;
        let addr1 = server.current_addr();

        assert_eq!(
            server.current_addr(),
            addr1,
            "Server should be bound to the initial address before restart_with_listener"
        );

        let response = client
            .get(format!("http://{}/health", addr1))
            .send()
            .await?;
        assert_eq!(
            response.status(),
            200,
            "Server should respond to health check before restart_with_listener"
        );

        let (listener, addr2) = bind_ephemeral_listener().await?;
        server.restart_with_listener(listener).await?;

        assert_eq!(
            server.current_addr(),
            addr2,
            "Server address should be updated after restart_with_listener"
        );
        assert_ne!(
            addr1, addr2,
            "Address should change after restart_with_listener"
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
            matches!(old_result, Err(_) | Ok(Err(_))),
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
        let StartedWebhookServer { mut server, client } = started_webhook_server.await?;
        let addr1 = server.current_addr();

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
