//! Listener restart and address resolution tests for the hot-reload manager.
//!
//! These tests verify correct listener restart behaviour across IPv4, IPv6,
//! and hostname resolution, including unchanged-address optimization,
//! restart-after-shutdown, and failure propagation.

use std::net::SocketAddr;
use std::sync::Arc;

use rstest::rstest;

use super::super::*;
use super::common::{AddrTestCase, http_config, test_config_with_http};
use crate::channels::ChannelSecretUpdater;
use crate::reload::test_stubs::{
    SpySecretUpdater, StubConfigLoader, StubListenerController, StubSecretInjector,
};

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
    expected_addr: "127.0.0.1:8081".parse().expect("failed to parse address 127.0.0.1:8081"),
    description: "localhost hostname",
})]
#[tokio::test]
async fn successful_reload_invokes_all_dependencies(
    #[case] test_case: AddrTestCase,
) -> Result<(), Box<dyn std::error::Error>> {
    let injector = Arc::new(StubSecretInjector::new());
    let injector_clone = Arc::clone(&injector);
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let controller_clone = Arc::clone(&controller);
    let (_temp_dir, config) = test_config_with_http(Some(http_config(
        test_case.host,
        test_case.port,
        Some("new_secret"),
    )))
    .await?;
    let loader = Arc::new(StubConfigLoader::new_success(config));
    let spy = Arc::new(SpySecretUpdater::new());
    let spy_clone = Arc::clone(&spy);

    let manager = HotReloadManager::new(
        loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        vec![spy as Arc<dyn ChannelSecretUpdater>],
    );

    let result = manager.perform_reload().await;
    assert!(
        result.is_ok(),
        "Reload should succeed for {}",
        test_case.description
    );
    assert!(
        injector_clone.was_called().await,
        "SecretInjector should be invoked for {}",
        test_case.description
    );

    let restarts = controller_clone.restart_calls().await;
    assert_eq!(
        restarts.len(),
        1,
        "Listener should be restarted once for {}",
        test_case.description
    );
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
    Ok(())
}

#[rstest]
#[case::ipv4("127.0.0.1:8080")]
#[case::ipv6("[::1]:8080")]
#[tokio::test]
async fn address_unchanged_skips_listener_restart(
    #[case] addr_str: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let injector = Arc::new(StubSecretInjector::new());
    let addr: SocketAddr = addr_str.parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(addr));
    let controller_clone = Arc::clone(&controller);
    let (host, port) = match addr {
        SocketAddr::V4(v4) => (v4.ip().to_string(), v4.port()),
        SocketAddr::V6(v6) => (v6.ip().to_string(), v6.port()),
    };
    let (_temp_dir, config) =
        test_config_with_http(Some(http_config(&host, port, Some("secret")))).await?;
    let loader = Arc::new(StubConfigLoader::new_success(config));
    let spy = Arc::new(SpySecretUpdater::new());
    let spy_clone = Arc::clone(&spy);

    let manager = HotReloadManager::new(
        loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        vec![spy as Arc<dyn ChannelSecretUpdater>],
    );

    let result = manager.perform_reload().await;
    assert!(result.is_ok(), "Reload should succeed for {}", addr_str);
    assert_eq!(
        controller_clone.restart_calls().await.len(),
        0,
        "Listener should not be restarted when {} is unchanged",
        addr_str
    );
    assert_eq!(
        spy_clone.last_secret().await,
        Some(Some("secret".to_string())),
        "ChannelSecretUpdater should receive the current webhook secret"
    );
    Ok(())
}

#[rstest]
#[case::ipv4("127.0.0.1:8080")]
#[case::ipv6("[::1]:8080")]
#[tokio::test]
async fn listener_restart_failure_prevents_secret_update(
    #[case] addr_str: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let injector = Arc::new(StubSecretInjector::new());
    let addr: SocketAddr = addr_str.parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new_with_restart_failure(addr));
    let host = match addr {
        SocketAddr::V4(v4) => v4.ip().to_string(),
        SocketAddr::V6(v6) => v6.ip().to_string(),
    };
    let (_temp_dir, config) =
        test_config_with_http(Some(http_config(&host, 8081, Some("new_secret")))).await?;
    let loader = Arc::new(StubConfigLoader::new_success(config));
    let spy = Arc::new(SpySecretUpdater::new());
    let spy_clone = Arc::clone(&spy);

    let manager = HotReloadManager::new(
        loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        vec![spy as Arc<dyn ChannelSecretUpdater>],
    );

    let result = manager.perform_reload().await;
    assert!(
        result.is_err(),
        "Reload should fail when listener restart fails for {}",
        addr_str
    );
    assert!(
        spy_clone.was_not_called().await,
        "ChannelSecretUpdater should not be called after listener restart failure"
    );
    Ok(())
}

#[tokio::test]
async fn readding_same_listener_address_restarts_after_shutdown()
-> Result<(), Box<dyn std::error::Error>> {
    let injector = Arc::new(StubSecretInjector::new());
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let controller_clone = Arc::clone(&controller);
    let spy = Arc::new(SpySecretUpdater::new());

    let (_remove_temp_dir, remove_config) = test_config_with_http(None).await?;
    let remove_loader = Arc::new(StubConfigLoader::new_success(remove_config));
    let remove_manager = HotReloadManager::new(
        remove_loader as Arc<dyn ConfigLoader>,
        Some(Arc::clone(&controller) as Arc<dyn ListenerController>),
        Some(Arc::clone(&injector) as Arc<dyn SecretInjector>),
        vec![Arc::clone(&spy) as Arc<dyn ChannelSecretUpdater>],
    );

    remove_manager
        .perform_reload()
        .await
        .expect("removing HTTP config should shut the listener down");
    assert_eq!(controller_clone.shutdown_count(), 1);

    let (_readd_temp_dir, readd_config) = test_config_with_http(Some(http_config(
        "127.0.0.1",
        8080,
        Some("restored-secret"),
    )))
    .await?;
    let readd_loader = Arc::new(StubConfigLoader::new_success(readd_config));
    let readd_manager = HotReloadManager::new(
        readd_loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        vec![spy as Arc<dyn ChannelSecretUpdater>],
    );

    readd_manager
        .perform_reload()
        .await
        .expect("re-adding the same address should restart the stopped listener");

    assert_eq!(
        controller_clone.restart_calls().await,
        vec![current_addr],
        "the stopped listener should restart even when the address matches the previous one"
    );
    Ok(())
}

#[tokio::test]
async fn maybe_restart_listener_skips_restart_when_current_addr_matches_non_first_resolved_addr()
-> Result<(), Box<dyn std::error::Error>> {
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let controller_clone = Arc::clone(&controller);

    // Use a hostname that resolves to multiple addresses including current_addr
    let http_cfg = http_config("localhost", 8080, None);

    let (_temp_dir, mut config) = test_config_with_http(Some(http_cfg.clone())).await?;
    // Ensure the HTTP config uses localhost for DNS resolution
    config.channels.http = Some(http_cfg.clone());
    let loader = Arc::new(StubConfigLoader::new_success(config));

    let manager = HotReloadManager::new(
        loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        None,
        Vec::new(),
    );

    // localhost typically resolves to both 127.0.0.1 and ::1
    // If current_addr (127.0.0.1:8080) is in the resolved list, restart should be skipped
    manager
        .maybe_restart_listener(&http_cfg)
        .await
        .expect("matching any resolved address should skip restart");

    assert_eq!(
        controller_clone.restart_calls().await.len(),
        0,
        "listener should not restart when its current address matches a non-first resolved address"
    );
    Ok(())
}
