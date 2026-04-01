//! Hot-reload orchestration manager.

use std::sync::Arc;

use crate::channels::ChannelSecretUpdater;
use crate::reload::{ConfigLoader, ListenerController, ReloadError, SecretInjector};

/// Orchestrates hot-reload operations across config, listeners, and secrets.
///
/// Composes three injected boundaries (config loading, listener control,
/// secret injection) with channel secret updates. Each boundary is testable
/// via hand-rolled stubs.
pub struct HotReloadManager {
    config_loader: Arc<dyn ConfigLoader>,
    listener_controller: Option<Arc<dyn ListenerController>>,
    secret_injector: Option<Arc<dyn SecretInjector>>,
    secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>>,
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
        }
    }

    /// Perform a hot-reload cycle.
    ///
    /// Executes the following steps in order:
    /// 1. Inject secrets (if configured)
    /// 2. Load new configuration
    /// 3. Restart listener if address changed (if configured)
    /// 4. Update channel secrets
    ///
    /// Returns early on any error. Errors are logged but not panicked.
    pub async fn perform_reload(&self) -> Result<(), ReloadError> {
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

        let new_addr = Self::parse_http_addr(&new_http).await?;
        self.maybe_restart_listener(new_addr).await?;

        // Step 4: Update channel secrets
        self.update_channel_secrets(&new_http).await;

        Ok(())
    }

    async fn parse_http_addr(
        http: &crate::config::HttpConfig,
    ) -> Result<std::net::SocketAddr, ReloadError> {
        // Prefer structured construction when host is a valid IP (handles IPv6 correctly).
        if let Ok(ip) = http.host.parse::<std::net::IpAddr>() {
            return Ok(std::net::SocketAddr::new(ip, http.port));
        }

        // Fall back to async DNS/hostname resolution for non-IP host values.
        tokio::net::lookup_host((http.host.as_str(), http.port))
            .await
            .map_err(|e| {
                tracing::error!("Invalid socket address in reloaded config: {}", e);
                ReloadError::from(crate::error::ConfigError::InvalidValue {
                    key: "http.host:http.port".to_string(),
                    message: format!("Failed to parse or resolve socket address: {}", e),
                })
            })?
            .next()
            .ok_or_else(|| {
                ReloadError::from(crate::error::ConfigError::InvalidValue {
                    key: "http.host:http.port".to_string(),
                    message: "No socket addresses resolved".to_string(),
                })
            })
    }

    async fn maybe_restart_listener(
        &self,
        new_addr: std::net::SocketAddr,
    ) -> Result<(), ReloadError> {
        let Some(ref controller) = self.listener_controller else {
            return Ok(());
        };

        let old_addr = controller.current_addr().await;
        if old_addr == new_addr {
            tracing::debug!("HTTP listener address unchanged, skipping restart");
            return Ok(());
        }

        tracing::info!("Restarting HTTP listener: {} -> {}", old_addr, new_addr);
        controller.restart_with_addr(new_addr).await.map_err(|e| {
            tracing::error!("Listener restart failed: {}", e);
            e
        })?;
        tracing::info!("HTTP listener restarted on {}", new_addr);

        Ok(())
    }

    async fn update_channel_secrets(&self, http: &crate::config::HttpConfig) {
        use secrecy::{ExposeSecret, SecretString};
        let new_secret = http
            .webhook_secret
            .as_ref()
            .map(|s| SecretString::from(s.expose_secret().to_string()));

        for updater in &self.secret_updaters {
            updater.update_secret(new_secret.clone()).await;
        }
    }
}

#[cfg(test)]
mod tests;
