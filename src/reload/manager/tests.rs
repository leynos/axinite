//! Tests for the hot-reload manager.

use std::net::SocketAddr;

use secrecy::SecretString;
use tempfile::TempDir;

use super::*;
use crate::config::HttpConfig;
use crate::reload::test_stubs::{
    SpySecretUpdater, StubConfigLoader, StubListenerController, StubSecretInjector,
};
use rstest::rstest;

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
async fn test_config_with_http(http: Option<HttpConfig>) -> (TempDir, crate::config::Config) {
    let temp_dir = tempfile::tempdir().expect("Failed to create temp dir");
    let temp_db = temp_dir.path().join("test_reload.db");
    let skills_dir = temp_dir.path().join("skills");
    let installed_skills_dir = temp_dir.path().join("installed_skills");
    let mut config = crate::config::Config::for_testing(temp_db, skills_dir, installed_skills_dir)
        .await
        .expect("test config should build");
    config.channels.http = http;
    (temp_dir, config)
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
    let injector = Arc::new(StubSecretInjector::new());
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

    let (_temp_dir, config) = test_config_with_http(Some(http_config)).await;
    let loader = Arc::new(StubConfigLoader::new_success(config));

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
    // For IPv4 and IPv6, assert full address equality
    // For hostname, only assert port (since resolved IP may vary)
    if test_case.host == "localhost" {
        assert_eq!(
            restarts[0].port(),
            test_case.expected_addr.port(),
            "Listener should restart on new port for {}",
            test_case.description
        );
    } else {
        assert_eq!(
            restarts[0], test_case.expected_addr,
            "Listener should restart on correct address for {}",
            test_case.description
        );
    }

    // Verify channel secret updater was called
    assert_eq!(
        spy_clone.call_count().await,
        1,
        "ChannelSecretUpdater should be called once for {}",
        test_case.description
    );
    assert_eq!(
        spy_clone.last_secret().await,
        Some(Some("new_secret".to_string())),
        "ChannelSecretUpdater should receive the reloaded webhook secret for {}",
        test_case.description
    );
}

#[rstest]
#[case::ipv4("127.0.0.1:8080")]
#[case::ipv6("[::1]:8080")]
#[tokio::test]
async fn address_unchanged_skips_listener_restart(#[case] addr_str: &str) {
    let injector = Arc::new(StubSecretInjector::new());

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

    let (_temp_dir, config) = test_config_with_http(Some(http_config)).await;
    let loader = Arc::new(StubConfigLoader::new_success(config));

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
    assert_eq!(
        spy_clone.last_secret().await,
        Some(Some("secret".to_string())),
        "ChannelSecretUpdater should receive the current webhook secret"
    );
}

#[tokio::test]
async fn config_load_failure_prevents_listener_restart() {
    let injector = Arc::new(StubSecretInjector::new());
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
    let injector = Arc::new(StubSecretInjector::new());

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

    let (_temp_dir, config) = test_config_with_http(Some(http_config)).await;
    let loader = Arc::new(StubConfigLoader::new_success(config));

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

#[tokio::test]
async fn http_config_removed_shuts_down_listener_and_clears_secrets() {
    let injector = Arc::new(StubSecretInjector::new());

    // Start with a listener on a known address
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let controller_clone = Arc::clone(&controller);

    // Config with no HTTP channel
    let (_temp_dir, config) = test_config_with_http(None).await;
    let loader = Arc::new(StubConfigLoader::new_success(config));

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
        "Reload should succeed when HTTP config is removed"
    );

    // Verify listener was shut down (no restart calls, but shutdown was called)
    let restarts = controller_clone.restart_calls().await;
    assert_eq!(
        restarts.len(),
        0,
        "Listener should not be restarted when HTTP config is removed"
    );
    assert_eq!(
        controller_clone.shutdown_count(),
        1,
        "Listener should be shut down when HTTP config is removed"
    );

    // Verify channel secret updater was called with None to clear secrets
    assert_eq!(
        spy_clone.call_count().await,
        1,
        "ChannelSecretUpdater should be called to clear secrets when HTTP config is removed"
    );
    assert_eq!(
        spy_clone.last_secret().await,
        Some(None),
        "ChannelSecretUpdater should clear the webhook secret when HTTP config is removed"
    );
}

#[tokio::test]
async fn readding_same_listener_address_restarts_after_shutdown() {
    let injector = Arc::new(StubSecretInjector::new());
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let controller_clone = Arc::clone(&controller);
    let spy = Arc::new(SpySecretUpdater::new());

    let (_remove_temp_dir, remove_config) = test_config_with_http(None).await;
    let remove_loader = Arc::new(StubConfigLoader::new_success(remove_config));
    let remove_manager = HotReloadManager::new(
        remove_loader as Arc<dyn ConfigLoader>,
        Some(Arc::clone(&controller) as Arc<dyn ListenerController>),
        Some(Arc::clone(&injector) as Arc<dyn SecretInjector>),
        vec![Arc::clone(&spy) as Arc<dyn crate::channels::ChannelSecretUpdater>],
    );

    remove_manager
        .perform_reload()
        .await
        .expect("removing HTTP config should shut the listener down");
    assert_eq!(
        controller_clone.shutdown_count(),
        1,
        "the listener should have been shut down before re-adding the config"
    );

    let readd_http = HttpConfig {
        host: "127.0.0.1".to_string(),
        port: 8080,
        user_id: "test_user".to_string(),
        webhook_secret: Some(SecretString::from("restored-secret".to_string())),
    };
    let (_readd_temp_dir, readd_config) = test_config_with_http(Some(readd_http)).await;
    let readd_loader = Arc::new(StubConfigLoader::new_success(readd_config));
    let readd_manager = HotReloadManager::new(
        readd_loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        vec![spy as Arc<dyn crate::channels::ChannelSecretUpdater>],
    );

    readd_manager
        .perform_reload()
        .await
        .expect("re-adding the same address should restart the stopped listener");

    let restarts = controller_clone.restart_calls().await;
    assert_eq!(
        restarts,
        vec![current_addr],
        "the stopped listener should restart even when the address matches the previous one"
    );
}

#[tokio::test]
async fn secret_injector_failure_does_not_block_config_reload() {
    let injector = Arc::new(StubSecretInjector::new_failure());
    let injector_clone = Arc::clone(&injector);
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let controller_clone = Arc::clone(&controller);
    let spy = Arc::new(SpySecretUpdater::new());
    let spy_clone = Arc::clone(&spy);

    let http_config = HttpConfig {
        host: "127.0.0.1".to_string(),
        port: 8081,
        user_id: "test_user".to_string(),
        webhook_secret: Some(SecretString::from("rotated-secret".to_string())),
    };
    let (_temp_dir, config) = test_config_with_http(Some(http_config)).await;
    let loader = Arc::new(StubConfigLoader::new_success(config));

    let manager = HotReloadManager::new(
        loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        vec![spy as Arc<dyn crate::channels::ChannelSecretUpdater>],
    );

    let result = manager.perform_reload().await;
    assert!(
        result.is_ok(),
        "HotReloadManager::perform_reload should keep applying config when the injector fails"
    );
    assert!(
        injector_clone.was_called().await,
        "StubSecretInjector should still be invoked during reload"
    );
    assert_eq!(
        controller_clone.restart_calls().await,
        vec![
            "127.0.0.1:8081"
                .parse()
                .expect("valid socket address for restarted listener")
        ],
        "the listener should still restart onto the new address"
    );
    assert_eq!(
        spy_clone.last_secret().await,
        Some(Some("rotated-secret".to_string())),
        "channel secret updates should still propagate the new webhook secret"
    );
}
