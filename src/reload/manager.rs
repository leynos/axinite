//! Hot-reload orchestration manager.

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use crate::channels::ChannelSecretUpdater;
use crate::reload::{ConfigLoader, ListenerController, ReloadError, SecretInjector};

const DNS_LOOKUP_TIMEOUT: Duration = Duration::from_secs(15);

/// Orchestrates hot-reload operations across config, listeners, and secrets.
///
/// Composes config loading, listener control, secret injection, and
/// channel secret updates. Each boundary is testable
/// via hand-rolled stubs.
pub struct HotReloadManager {
    config_loader: Arc<dyn ConfigLoader>,
    listener_controller: Option<Arc<dyn ListenerController>>,
    secret_injector: Option<Arc<dyn SecretInjector>>,
    secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>>,
    reload_lock: tokio::sync::Mutex<()>,
}

impl HotReloadManager {
    /// Create a new hot-reload manager with the given dependencies.
    ///
    /// - `config_loader`: Required — loads config from DB or environment
    /// - `listener_controller`: Optional — restarts HTTP listeners
    /// - `secret_injector`: Optional — injects secrets into environment overlay
    /// - `secret_updaters`: Channel instances that support secret swapping
    pub fn new(
        config_loader: Arc<dyn ConfigLoader>,
        listener_controller: Option<Arc<dyn ListenerController>>,
        secret_injector: Option<Arc<dyn SecretInjector>>,
        secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>>,
    ) -> Self {
        Self {
            config_loader,
            listener_controller,
            secret_injector,
            secret_updaters,
            reload_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Perform a hot-reload cycle.
    ///
    /// Executes the following steps in order:
    /// 1. Inject secrets (if configured)
    /// 2. Load new configuration
    /// 3. Restart listener if the configured address changed, or if the
    ///    listener stopped while the configured address stayed the same
    /// 4. Update channel secrets
    ///
    /// If the HTTP channel is removed, the current listener is shut down and
    /// channel secrets are cleared. Errors are logged and returned early; the
    /// reload path never panics.
    pub async fn perform_reload(&self) -> Result<(), ReloadError> {
        let _reload_guard = self.reload_lock.lock().await;

        // Step 1: Inject secrets into the environment overlay
        // Errors are logged internally by the injector; we continue regardless.
        if let Some(ref injector) = self.secret_injector {
            injector.inject().await;
        }

        // Step 2: Load new configuration
        let new_config = self.config_loader.load().await.map_err(|e| {
            tracing::error!("Config reload failed: {}", e);
            ReloadError::from(e)
        })?;

        // Step 3: Parse HTTP config and restart listener if address changed
        let Some(new_http) = new_config.channels.http else {
            tracing::warn!("HTTP channel no longer configured, shutting down listener");
            // Teardown the existing HTTP listener and clear secrets
            if let Some(ref controller) = self.listener_controller {
                controller.shutdown().await;
            }
            for updater in &self.secret_updaters {
                updater.update_secret(None).await;
            }
            return Ok(());
        };

        // Step 3: Restart listener if address changed (DNS lookup only if listener exists)
        self.maybe_restart_listener(&new_http).await?;

        // Step 4: Update channel secrets
        self.update_channel_secrets(&new_http).await;

        Ok(())
    }

    async fn resolve_http_addrs<L, Fut, I>(
        http: &crate::config::HttpConfig,
        lookup_host: L,
    ) -> Result<Vec<SocketAddr>, ReloadError>
    where
        L: Fn(String, u16) -> Fut,
        Fut: Future<Output = std::io::Result<I>>,
        I: IntoIterator<Item = SocketAddr>,
    {
        Self::resolve_http_addrs_with_lookup(http, lookup_host).await
    }

    async fn resolve_http_addrs_with_lookup<L, Fut, I>(
        http: &crate::config::HttpConfig,
        lookup_host: L,
    ) -> Result<Vec<SocketAddr>, ReloadError>
    where
        L: Fn(String, u16) -> Fut,
        Fut: Future<Output = std::io::Result<I>>,
        I: IntoIterator<Item = SocketAddr>,
    {
        // Prefer structured construction when host is a valid IP (handles IPv6 correctly).
        if let Ok(ip) = http.host.parse::<std::net::IpAddr>() {
            return Ok(vec![SocketAddr::new(ip, http.port)]);
        }

        // Fall back to async DNS/hostname resolution for non-IP host values.
        let resolved_addrs: Vec<_> = tokio::time::timeout(
            DNS_LOOKUP_TIMEOUT,
            lookup_host(http.host.clone(), http.port),
        )
        .await
        .map_err(|_| {
            tracing::error!(
                "Timed out resolving socket address for {}:{} after {:?}",
                http.host,
                http.port,
                DNS_LOOKUP_TIMEOUT
            );
            ReloadError::from(crate::error::ConfigError::InvalidValue {
                key: "http.host:http.port".to_string(),
                message: format!(
                    "Timed out resolving socket address after {:?}",
                    DNS_LOOKUP_TIMEOUT
                ),
            })
        })?
        .map_err(|e| {
            tracing::error!("Invalid socket address in reloaded config: {}", e);
            ReloadError::from(crate::error::ConfigError::InvalidValue {
                key: "http.host:http.port".to_string(),
                message: format!("Failed to parse or resolve socket address: {}", e),
            })
        })?
        .into_iter()
        .collect();

        if resolved_addrs.is_empty() {
            return Err(ReloadError::from(crate::error::ConfigError::InvalidValue {
                key: "http.host:http.port".to_string(),
                message: "No socket addresses resolved".to_string(),
            }));
        }

        Ok(resolved_addrs)
    }

    async fn maybe_restart_listener(
        &self,
        http: &crate::config::HttpConfig,
    ) -> Result<(), ReloadError> {
        let Some(ref controller) = self.listener_controller else {
            return Ok(());
        };

        let resolved_addrs =
            Self::resolve_http_addrs(http, |host, port| tokio::net::lookup_host((host, port)))
                .await?;

        let is_running = controller.is_running().await;
        let old_addr = controller.current_addr().await;
        if is_running && resolved_addrs.contains(&old_addr) {
            tracing::debug!("HTTP listener address unchanged, skipping restart");
            return Ok(());
        }

        // Try each candidate address in priority order:
        // 1. old_addr if listener is stopped and old_addr is still valid
        // 2. Each address in resolved_addrs in order
        let candidates: Vec<SocketAddr> = if !is_running && resolved_addrs.contains(&old_addr) {
            vec![old_addr]
        } else {
            resolved_addrs.clone()
        };

        let mut last_error = None;
        for (idx, addr) in candidates.iter().enumerate() {
            tracing::info!(
                "Restarting HTTP listener: {} -> {} (attempt {}/{})",
                old_addr,
                addr,
                idx + 1,
                candidates.len()
            );
            match controller.restart_with_addr(*addr).await {
                Ok(()) => {
                    tracing::info!("HTTP listener restarted on {}", addr);
                    return Ok(());
                }
                Err(e) => {
                    tracing::error!(
                        "Listener restart failed on {} (attempt {}/{}): {}",
                        addr,
                        idx + 1,
                        candidates.len(),
                        e
                    );
                    last_error = Some(e);
                }
            }
        }

        // All candidates failed
        Err(last_error.expect("at least one candidate address").into())
    }

    async fn update_channel_secrets(&self, http: &crate::config::HttpConfig) {
        let new_secret = http.webhook_secret.clone();

        for updater in &self.secret_updaters {
            updater.update_secret(new_secret.clone()).await;
        }
    }
}

#[cfg(test)]
mod tests;
