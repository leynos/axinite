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
        if let Some(ref injector) = self.secret_injector
            && let Err(e) = injector.inject().await
        {
            tracing::error!("Secret injection failed during reload: {}", e);
            // Secret injection failures are non-fatal by design; continue reload.
        }

        // Step 2: Load new configuration
        let new_config = self.config_loader.load().await.map_err(|e| {
            tracing::error!("Config reload failed: {}", e);
            ReloadError::from(e)
        })?;

        // Step 3: Parse HTTP config and restart listener if address changed
        let Some(new_http) = new_config.channels.http else {
            tracing::warn!("HTTP channel no longer configured, skipping listener restart");
            // No HTTP config means no listener to restart or secrets to update
            return Ok(());
        };

        let new_addr = Self::parse_http_addr(&new_http)?;
        self.maybe_restart_listener(new_addr).await?;

        // Step 4: Update channel secrets
        self.update_channel_secrets(&new_http).await;

        Ok(())
    }

    fn parse_http_addr(
        http: &crate::config::HttpConfig,
    ) -> Result<std::net::SocketAddr, ReloadError> {
        // Prefer structured construction when host is a valid IP (handles IPv6 correctly).
        if let Ok(ip) = http.host.parse::<std::net::IpAddr>() {
            return Ok(std::net::SocketAddr::new(ip, http.port));
        }

        // Fall back to string-based parse for hostname-style values.
        format!("{}:{}", http.host, http.port).parse().map_err(|e| {
            tracing::error!("Invalid socket address in reloaded config: {}", e);
            crate::error::ConfigError::InvalidValue {
                key: "http.host:http.port".to_string(),
                message: format!("Failed to parse socket address: {}", e),
            }
            .into()
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
mod tests {
    use super::*;
    use crate::config::HttpConfig;
    use crate::reload::test_stubs::{
        SpySecretUpdater, StubConfigLoader, StubListenerController, StubSecretInjector,
    };
    use secrecy::SecretString;
    use std::net::SocketAddr;

    /// Helper to create a minimal test config with the given HTTP config.
    fn test_config_with_http(http: Option<HttpConfig>) -> crate::config::Config {
        let temp_db = std::path::PathBuf::from("/tmp/test_reload.db");
        let skills_dir = std::path::PathBuf::from("/tmp/skills");
        let installed_skills_dir = std::path::PathBuf::from("/tmp/installed_skills");
        let mut config =
            crate::config::Config::for_testing(temp_db, skills_dir, installed_skills_dir);
        config.channels.http = http;
        config
    }

    #[tokio::test]
    async fn successful_reload_invokes_all_dependencies() {
        let injector = Arc::new(StubSecretInjector::new(false));
        let injector_clone = Arc::clone(&injector);

        let addr1: SocketAddr = "127.0.0.1:8080".parse().expect("valid test socket address");
        let addr2: SocketAddr = "127.0.0.1:8081".parse().expect("valid test socket address");

        let controller = Arc::new(StubListenerController::new(addr1));
        let controller_clone = Arc::clone(&controller);

        let http_config = HttpConfig {
            host: "127.0.0.1".to_string(),
            port: 8081,
            user_id: "test_user".to_string(),
            webhook_secret: Some(SecretString::from("new_secret".to_string())),
        };

        let loader = Arc::new(StubConfigLoader::new_success(test_config_with_http(Some(
            http_config,
        ))));

        let spy = Arc::new(SpySecretUpdater::new());
        let spy_clone = Arc::clone(&spy);

        let manager = HotReloadManager::new(
            loader as Arc<dyn ConfigLoader>,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![spy as Arc<dyn crate::channels::ChannelSecretUpdater>],
        );

        let result = manager.perform_reload().await;
        assert!(result.is_ok(), "Reload should succeed");

        // Verify injector was called
        assert!(
            injector_clone.was_called().await,
            "SecretInjector should be invoked"
        );

        // Verify listener restart was called with new address
        let restarts = controller_clone.restart_calls().await;
        assert_eq!(restarts.len(), 1, "Listener should be restarted once");
        assert_eq!(restarts[0], addr2, "Listener should restart on new address");

        // Verify channel secret updater was called
        assert_eq!(
            spy_clone.call_count().await,
            1,
            "ChannelSecretUpdater should be called once on successful reload"
        );
    }

    #[tokio::test]
    async fn config_load_failure_prevents_listener_restart() {
        let injector = Arc::new(StubSecretInjector::new(false));
        let controller = Arc::new(StubListenerController::new(
            "127.0.0.1:8080".parse().expect("valid test socket address"),
        ));
        let controller_clone = Arc::clone(&controller);

        let loader = Arc::new(StubConfigLoader::new_error(
            crate::error::ConfigError::MissingEnvVar("TEST".to_string()),
        ));

        let spy = Arc::new(SpySecretUpdater::new());
        let spy_clone = Arc::clone(&spy);

        let manager = HotReloadManager::new(
            loader as Arc<dyn ConfigLoader>,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![spy as Arc<dyn crate::channels::ChannelSecretUpdater>],
        );

        let result = manager.perform_reload().await;
        assert!(result.is_err(), "Reload should fail on config error");

        // Verify listener was never restarted
        let restarts = controller_clone.restart_calls().await;
        assert_eq!(
            restarts.len(),
            0,
            "Listener should not be restarted after config load failure"
        );

        // Verify channel secret updater was not called
        assert!(
            spy_clone.was_not_called().await,
            "ChannelSecretUpdater should not be called after config load failure"
        );
    }

    #[tokio::test]
    async fn listener_restart_failure_prevents_secret_update() {
        let injector = Arc::new(StubSecretInjector::new(false));

        let addr1: SocketAddr = "127.0.0.1:8080".parse().expect("valid test socket address");
        let controller = Arc::new(StubListenerController::new_with_restart_failure(addr1));

        let http_config = HttpConfig {
            host: "127.0.0.1".to_string(),
            port: 8081,
            user_id: "test_user".to_string(),
            webhook_secret: Some(SecretString::from("new_secret".to_string())),
        };

        let loader = Arc::new(StubConfigLoader::new_success(test_config_with_http(Some(
            http_config,
        ))));

        let spy = Arc::new(SpySecretUpdater::new());
        let spy_clone = Arc::clone(&spy);

        let manager = HotReloadManager::new(
            loader as Arc<dyn ConfigLoader>,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![spy as Arc<dyn crate::channels::ChannelSecretUpdater>],
        );

        let result = manager.perform_reload().await;
        assert!(
            result.is_err(),
            "Reload should fail when listener restart fails"
        );

        // Verify channel secret updater was not called after listener failure
        assert!(
            spy_clone.was_not_called().await,
            "ChannelSecretUpdater should not be called after listener restart failure"
        );
    }

    #[tokio::test]
    async fn address_unchanged_skips_listener_restart() {
        let injector = Arc::new(StubSecretInjector::new(false));

        let addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid test socket address");
        let controller = Arc::new(StubListenerController::new(addr));
        let controller_clone = Arc::clone(&controller);

        let http_config = HttpConfig {
            host: "127.0.0.1".to_string(),
            port: 8080, // Same as current address
            user_id: "test_user".to_string(),
            webhook_secret: Some(SecretString::from("secret".to_string())),
        };

        let loader = Arc::new(StubConfigLoader::new_success(test_config_with_http(Some(
            http_config,
        ))));

        let spy = Arc::new(SpySecretUpdater::new());
        let spy_clone = Arc::clone(&spy);

        let manager = HotReloadManager::new(
            loader as Arc<dyn ConfigLoader>,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![spy as Arc<dyn crate::channels::ChannelSecretUpdater>],
        );

        let result = manager.perform_reload().await;
        assert!(result.is_ok(), "Reload should succeed");

        // Verify listener was not restarted
        let restarts = controller_clone.restart_calls().await;
        assert_eq!(
            restarts.len(),
            0,
            "Listener should not be restarted when address is unchanged"
        );

        // Verify channel secret updater was still called (secrets update even without restart)
        assert_eq!(
            spy_clone.call_count().await,
            1,
            "ChannelSecretUpdater should be called even when address is unchanged"
        );
    }
}
