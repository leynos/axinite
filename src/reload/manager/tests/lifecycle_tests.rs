use std::net::SocketAddr;
use std::sync::Arc;

use super::super::*;
use super::common::{http_config, test_config_with_http};
use crate::channels::ChannelSecretUpdater;
use crate::reload::test_stubs::{
    SpySecretUpdater, StubConfigLoader, StubListenerController, StubSecretInjector,
};

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
        vec![spy as Arc<dyn ChannelSecretUpdater>],
    );

    let result = manager.perform_reload().await;
    assert!(result.is_err(), "Reload should fail on config error");
    assert_eq!(
        controller_clone.restart_calls().await.len(),
        0,
        "Listener should not be restarted after config load failure"
    );
    assert!(
        spy_clone.was_not_called().await,
        "ChannelSecretUpdater should not be called after config load failure"
    );
}

#[tokio::test]
async fn http_config_removed_shuts_down_listener_and_clears_secrets() {
    let injector = Arc::new(StubSecretInjector::new());
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let controller_clone = Arc::clone(&controller);
    let (_temp_dir, config) = test_config_with_http(None).await;
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
        "Reload should succeed when HTTP config is removed"
    );
    assert_eq!(
        controller_clone.restart_calls().await.len(),
        0,
        "Listener should not be restarted when HTTP config is removed"
    );
    assert_eq!(
        controller_clone.shutdown_count(),
        1,
        "Listener should be shut down when HTTP config is removed"
    );
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
async fn secret_injector_failure_does_not_block_config_reload() {
    let injector = Arc::new(StubSecretInjector::new_failure());
    let injector_clone = Arc::clone(&injector);
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let controller = Arc::new(StubListenerController::new(current_addr));
    let controller_clone = Arc::clone(&controller);
    let spy = Arc::new(SpySecretUpdater::new());
    let spy_clone = Arc::clone(&spy);
    let (_temp_dir, config) =
        test_config_with_http(Some(http_config("127.0.0.1", 8081, Some("rotated-secret")))).await;
    let loader = Arc::new(StubConfigLoader::new_success(config));

    let manager = HotReloadManager::new(
        loader as Arc<dyn ConfigLoader>,
        Some(controller as Arc<dyn ListenerController>),
        Some(injector as Arc<dyn SecretInjector>),
        vec![spy as Arc<dyn ChannelSecretUpdater>],
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
