use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use tokio::time::advance;

use super::super::*;
use super::common::{http_config, test_config_with_http};
use crate::error::ConfigError;
use crate::reload::test_stubs::{StubConfigLoader, StubListenerController, StubSecretInjector};

#[tokio::test]
async fn resolve_http_addrs_rejects_empty_lookup_results() {
    let error = HotReloadManager::resolve_http_addrs_with_lookup(
        &http_config("empty.example.test", 8081, None),
        |_host, _port| async { Ok::<Vec<SocketAddr>, std::io::Error>(Vec::new()) },
    )
    .await
    .expect_err("empty DNS lookups should be rejected");

    assert!(
        matches!(
            error,
            ReloadError::Config(ConfigError::InvalidValue { key, message })
                if key == "http.host:http.port" && message.contains("No socket addresses")
        ),
        "empty DNS lookups should surface an invalid socket-address config error"
    );
}

#[tokio::test(start_paused = true)]
async fn resolve_http_addrs_times_out_slow_dns_lookup() {
    let http_config = http_config("slow.example.test", 8081, None);
    let resolve = tokio::spawn(async move {
        HotReloadManager::resolve_http_addrs_with_lookup(&http_config, |_host, _port| {
            std::future::pending::<std::io::Result<Vec<SocketAddr>>>()
        })
        .await
    });

    tokio::task::yield_now().await;
    advance(Duration::from_secs(16)).await;

    let error = resolve
        .await
        .expect("lookup task should complete")
        .expect_err("slow lookup should time out");
    assert!(
        matches!(
            error,
            ReloadError::Config(ConfigError::InvalidValue { key, message })
                if key == "http.host:http.port"
                    && message.contains("Timed out resolving socket address")
        ),
        "timeout should be surfaced as an invalid socket-address reload error"
    );
}

#[tokio::test(start_paused = true)]
async fn perform_reload_returns_timeout_error_when_dns_resolution_stalls() {
    let injector = Arc::new(StubSecretInjector::new());
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let (_temp_dir, config) = test_config_with_http(Some(http_config(
        "slow.example.test",
        8081,
        Some("new-secret"),
    )))
    .await;
    let loader = Arc::new(StubConfigLoader::new_success(config));

    let manager = HotReloadManager::new(
        loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        Vec::new(),
    );

    let reload = tokio::spawn(async move { manager.perform_reload().await });

    tokio::task::yield_now().await;
    advance(Duration::from_secs(16)).await;

    let result = reload
        .await
        .expect("reload task should complete after timeout");
    assert!(
        matches!(
            result,
            Err(ReloadError::Config(ConfigError::InvalidValue { key, message }))
                if key == "http.host:http.port"
                    && message.contains("Timed out resolving socket address")
        ),
        "perform_reload should return a timeout-derived reload error"
    );
}

#[tokio::test(start_paused = true)]
async fn reload_lock_is_released_after_dns_timeout() {
    let injector = Arc::new(StubSecretInjector::new());
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let (_temp_dir, config) =
        test_config_with_http(Some(http_config("slow.example.test", 8081, None))).await;
    let loader = Arc::new(StubConfigLoader::new_success(config));

    let manager = Arc::new(HotReloadManager::new(
        loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        Vec::new(),
    ));

    let first_reload = tokio::spawn({
        let manager = Arc::clone(&manager);
        async move { manager.perform_reload().await }
    });

    tokio::task::yield_now().await;
    advance(Duration::from_secs(16)).await;
    first_reload
        .await
        .expect("first reload task should complete")
        .expect_err("first reload should time out");

    let second_reload = tokio::spawn({
        let manager = Arc::clone(&manager);
        async move { manager.perform_reload().await }
    });

    tokio::task::yield_now().await;
    advance(Duration::from_secs(16)).await;
    second_reload
        .await
        .expect("second reload task should complete")
        .expect_err("second reload should also time out cleanly");
}
