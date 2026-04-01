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
mod tests {
    use super::*;
    use crate::config::HttpConfig;
    use crate::reload::test_stubs::{
        SpySecretUpdater, StubConfigLoader, StubListenerController, StubSecretInjector,
    };
    use rstest::rstest;
    use secrecy::SecretString;
    use std::net::SocketAddr;

    /// Test case for address resolution scenarios.
    struct AddrTestCase {
        /// Host string to use in HttpConfig
        host: &'static str,
        /// Port to use in HttpConfig
        port: u16,
        /// Expected socket address for verification
        expected_addr: SocketAddr,
        /// Description for test output
        description: &'static str,
    }

    /// Helper to create a minimal test config with the given HTTP config.
    fn test_config_with_http(http: Option<HttpConfig>) -> crate::config::Config {
        let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
        let temp_db = temp_dir.path().join("test_reload.db");
        let skills_dir = temp_dir.path().join("skills");
        let installed_skills_dir = temp_dir.path().join("installed_skills");
        let mut config =
            crate::config::Config::for_testing(temp_db, skills_dir, installed_skills_dir);
        config.channels.http = http;
        config
    }

    #[rstest]
    #[case::ipv4(AddrTestCase {
        host: "127.0.0.1",
        port: 8081,
        expected_addr: "127.0.0.1:8081".parse().expect("failed to parse address 127.0.0.1:8081"),
        description: "IPv4 address",
    })]
    #[case::ipv6(AddrTestCase {
        host: "::1",
        port: 8081,
        expected_addr: "[::1]:8081".parse().expect("failed to parse address [::1]:8081"),
        description: "IPv6 address",
    })]
    #[case::hostname_localhost(AddrTestCase {
        host: "localhost",
        port: 8081,
        // localhost typically resolves to 127.0.0.1 or ::1; we verify the port matches
        expected_addr: "127.0.0.1:8081".parse().expect("failed to parse address 127.0.0.1:8081"),
        description: "localhost hostname",
    })]
    #[tokio::test]
    async fn successful_reload_invokes_all_dependencies(#[case] test_case: AddrTestCase) {
        let injector = Arc::new(StubSecretInjector::new(false));
        let injector_clone = Arc::clone(&injector);

        // Current address is different from new address to trigger restart
        let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
        let controller = Arc::new(StubListenerController::new(current_addr));
        let controller_clone = Arc::clone(&controller);

        let http_config = HttpConfig {
            host: test_case.host.to_string(),
            port: test_case.port,
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
            result.is_ok(),
            "Reload should succeed for {}",
            test_case.description
        );

        // Verify injector was called
        assert!(
            injector_clone.was_called().await,
            "SecretInjector should be invoked for {}",
            test_case.description
        );

        // Verify listener restart was called with correct address
        let restarts = controller_clone.restart_calls().await;
        assert_eq!(
            restarts.len(),
            1,
            "Listener should be restarted once for {}",
            test_case.description
        );
        assert_eq!(
            restarts[0].port(),
            test_case.expected_addr.port(),
            "Listener should restart on new port for {}",
            test_case.description
        );

        // Verify channel secret updater was called
        assert_eq!(
            spy_clone.call_count().await,
            1,
            "ChannelSecretUpdater should be called once for {}",
            test_case.description
        );
    }

    #[rstest]
    #[case::ipv4("127.0.0.1:8080")]
    #[case::ipv6("[::1]:8080")]
    #[tokio::test]
    async fn address_unchanged_skips_listener_restart(#[case] addr_str: &str) {
        let injector = Arc::new(StubSecretInjector::new(false));

        // Current address matches new address - should skip restart
        let addr: SocketAddr = addr_str.parse().expect("valid socket address");
        let controller = Arc::new(StubListenerController::new(addr));
        let controller_clone = Arc::clone(&controller);

        // Parse host and port from the address for the config
        let (host, port) = match addr {
            SocketAddr::V4(v4) => (v4.ip().to_string(), v4.port()),
            SocketAddr::V6(v6) => (v6.ip().to_string(), v6.port()),
        };

        let http_config = HttpConfig {
            host,
            port,
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
        assert!(result.is_ok(), "Reload should succeed for {}", addr_str);

        // Verify listener was not restarted
        let restarts = controller_clone.restart_calls().await;
        assert_eq!(
            restarts.len(),
            0,
            "Listener should not be restarted when {} is unchanged",
            addr_str
        );

        // Verify channel secret updater was still called
        assert_eq!(
            spy_clone.call_count().await,
            1,
            "ChannelSecretUpdater should be called even when address is unchanged"
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

    #[rstest]
    #[case::ipv4("127.0.0.1:8080")]
    #[case::ipv6("[::1]:8080")]
    #[tokio::test]
    async fn listener_restart_failure_prevents_secret_update(#[case] addr_str: &str) {
        let injector = Arc::new(StubSecretInjector::new(false));

        let addr: SocketAddr = addr_str.parse().expect("valid socket address");
        let controller = Arc::new(StubListenerController::new_with_restart_failure(addr));

        // Parse host from the address for the config
        let host = match addr {
            SocketAddr::V4(v4) => v4.ip().to_string(),
            SocketAddr::V6(v6) => v6.ip().to_string(),
        };

        let http_config = HttpConfig {
            host,
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
            "Reload should fail when listener restart fails for {}",
            addr_str
        );

        // Verify channel secret updater was not called after listener failure
        assert!(
            spy_clone.was_not_called().await,
            "ChannelSecretUpdater should not be called after listener restart failure"
        );
    }
}
