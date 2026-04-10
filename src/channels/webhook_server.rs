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

        // Extract address from the provided listener before mutating self,
        // so that old_addr, old_shutdown_tx and old_handle remain intact
        // until we know local_addr() succeeds.
        let addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("local_addr failed: {e}"),
            })?;

        self.swap_listener(addr, listener, app).await
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
