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
            return Err(e.into());
        }

        // Step 2: Load new configuration
        let new_config = match self.config_loader.load().await {
            Ok(c) => c,
            Err(e) => {
                tracing::error!("Config reload failed: {}", e);
                return Err(e.into());
            }
        };

        // Step 3: Parse HTTP config and restart listener if address changed
        let new_http = match new_config.channels.http {
            Some(c) => c,
            None => {
                tracing::warn!("HTTP channel no longer configured, skipping listener restart");
                // No HTTP config means no listener to restart or secrets to update
                return Ok(());
            }
        };

        let new_addr: std::net::SocketAddr =
            match format!("{}:{}", new_http.host, new_http.port).parse() {
                Ok(a) => a,
                Err(e) => {
                    tracing::error!("Invalid socket address in reloaded config: {}", e);
                    return Err(crate::error::ConfigError::InvalidValue {
                        key: "http.host:http.port".to_string(),
                        message: format!("Failed to parse socket address: {}", e),
                    }
                    .into());
                }
            };

        // Check if listener restart is needed
        let restart_needed = if let Some(ref controller) = self.listener_controller {
            let old_addr = controller.current_addr();
            old_addr != new_addr
        } else {
            false
        };

        if restart_needed {
            if let Some(ref controller) = self.listener_controller {
                let old_addr = controller.current_addr();
                tracing::info!("Restarting HTTP listener: {} -> {}", old_addr, new_addr);
                if let Err(e) = controller.restart_with_addr(new_addr).await {
                    tracing::error!("Listener restart failed: {}", e);
                    return Err(e.into());
                }
                tracing::info!("HTTP listener restarted on {}", new_addr);
            }
        } else {
            tracing::debug!("HTTP listener address unchanged, skipping restart");
        }

        // Step 4: Update channel secrets
        use secrecy::{ExposeSecret, SecretString};
        let new_secret = new_http
            .webhook_secret
            .as_ref()
            .map(|s| SecretString::from(s.expose_secret().to_string()));

        for updater in &self.secret_updaters {
            updater.update_secret(new_secret.clone()).await;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HttpConfig;
    use crate::reload::test_stubs::{StubConfigLoader, StubListenerController, StubSecretInjector};
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

        let addr1: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let addr2: SocketAddr = "127.0.0.1:8081".parse().unwrap();

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

        let manager = HotReloadManager::new(
            loader as Arc<dyn ConfigLoader>,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![],
        );

        let result = manager.perform_reload().await;
        assert!(result.is_ok(), "Reload should succeed");

        // Verify injector was called
        assert!(
            injector_clone.was_called(),
            "SecretInjector should be invoked"
        );

        // Verify listener restart was called with new address
        let restarts = controller_clone.restart_calls();
        assert_eq!(restarts.len(), 1, "Listener should be restarted once");
        assert_eq!(restarts[0], addr2, "Listener should restart on new address");
    }

    #[tokio::test]
    async fn config_load_failure_prevents_listener_restart() {
        let injector = Arc::new(StubSecretInjector::new(false));
        let controller = Arc::new(StubListenerController::new(
            "127.0.0.1:8080".parse().unwrap(),
        ));
        let controller_clone = Arc::clone(&controller);

        let loader = Arc::new(StubConfigLoader::new_error(
            crate::error::ConfigError::MissingEnvVar("TEST".to_string()),
        ));

        let manager = HotReloadManager::new(
            loader as Arc<dyn ConfigLoader>,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![],
        );

        let result = manager.perform_reload().await;
        assert!(result.is_err(), "Reload should fail on config error");

        // Verify listener was never restarted
        let restarts = controller_clone.restart_calls();
        assert_eq!(
            restarts.len(),
            0,
            "Listener should not be restarted after config load failure"
        );
    }

    #[tokio::test]
    async fn listener_restart_failure_prevents_secret_update() {
        let injector = Arc::new(StubSecretInjector::new(false));

        let addr1: SocketAddr = "127.0.0.1:8080".parse().unwrap();
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

        let manager = HotReloadManager::new(
            loader as Arc<dyn ConfigLoader>,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![],
        );

        let result = manager.perform_reload().await;
        assert!(
            result.is_err(),
            "Reload should fail when listener restart fails"
        );
    }

    #[tokio::test]
    async fn address_unchanged_skips_listener_restart() {
        let injector = Arc::new(StubSecretInjector::new(false));

        let addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
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

        let manager = HotReloadManager::new(
            loader as Arc<dyn ConfigLoader>,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![],
        );

        let result = manager.perform_reload().await;
        assert!(result.is_ok(), "Reload should succeed");

        // Verify listener was not restarted
        let restarts = controller_clone.restart_calls();
        assert_eq!(
            restarts.len(),
            0,
            "Listener should not be restarted when address is unchanged"
        );
    }
}
