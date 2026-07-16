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
    resolved_addr: Option<SocketAddr>,
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
            resolved_addr: None,
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

    /// Bind a listener to the configured address and spawn the server task.
    /// Private helper used by both start() and restart_with_addr().
    async fn bind_and_spawn(&mut self, app: Router) -> Result<(), ChannelError> {
        let listener = tokio::net::TcpListener::bind(self.config.addr)
            .await
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("Failed to bind to {}: {}", self.config.addr, e),
            })?;

        let resolved_addr = listener
            .local_addr()
            .map_err(|e| ChannelError::StartupFailed {
                name: "webhook_server".to_string(),
                reason: format!("Failed to get listener local address: {e}"),
            })?;
        self.resolved_addr = Some(resolved_addr);

        self.spawn_with_listener(listener, app, resolved_addr);
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
        let old_resolved_addr = self.resolved_addr;
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
                self.resolved_addr = old_resolved_addr;
                self.shutdown_tx = old_shutdown_tx;
                self.handle = old_handle;
                Err(e)
            }
        }
    }

    /// Return the current listener address.
    ///
    /// Before the first successful [`Self::start`], start_with_listener,
    /// [`Self::restart_with_addr`], or restart_with_listener call, this
    /// returns the configured bind address from `self.config.addr` and it may
    /// not correspond to a live listener. After a successful start or restart,
    /// it returns the resolved listener address, including any OS-assigned port
    /// chosen for `:0` binds, while leaving `self.config.addr` unchanged.
    pub fn current_addr(&self) -> SocketAddr {
        self.resolved_addr.unwrap_or(self.config.addr)
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

    /// Accept a pre-bound listener, merge route fragments, and spawn the
    /// server.
    ///
    /// Unlike [`Self::start`], this test-only entrypoint accepts a listener
    /// that is already bound, eliminating the TOCTOU window between external
    /// test port allocation and the server bind. That makes it suitable for
    /// tests that reserve ephemeral ports before handing ownership to the
    /// server.
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
        self.resolved_addr = Some(local_addr);

        self.spawn_with_listener(listener, app, local_addr);
        Ok(())
    }

    /// Spawn the axum server on an already-bound listener.
    fn spawn_with_listener(
        &mut self,
        listener: tokio::net::TcpListener,
        app: Router,
        addr: SocketAddr,
    ) {
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

        self.resolved_addr = Some(new_addr);
        self.spawn_with_listener(listener, app, new_addr);
        Ok(())
    }
}

#[cfg(test)]
mod tests;
