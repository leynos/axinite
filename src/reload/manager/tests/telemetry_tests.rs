//! Counter assertions for the hot-reload manager's production outcomes.
//!
//! Each test drives `perform_reload` through one outcome branch and asserts
//! the exact counter name and fixed `outcome` label.

#![cfg(feature = "metrics")]

use std::net::SocketAddr;
use std::sync::Arc;

use super::super::*;
use super::common::{http_config, test_config_with_http};
use crate::config::EnvContext;
use crate::error::ConfigError;
use crate::reload::test_stubs::{StubConfigLoader, StubListenerController, StubSecretInjector};
use crate::testing::metrics;

fn addr(value: &str) -> SocketAddr {
    value.parse().expect("valid socket address")
}

#[tokio::test]
async fn successful_reload_records_success_outcome() -> Result<(), Box<dyn std::error::Error>> {
    let (_temp_dir, config) =
        test_config_with_http(Some(http_config("127.0.0.1", 8081, Some("secret")))).await?;
    let loader = Arc::new(StubConfigLoader::new_success(config));
    let controller = Arc::new(StubListenerController::new(addr("127.0.0.1:8080")));

    let manager = HotReloadManager::new_with_context(HotReloadManagerDependencies {
        env_context: EnvContext::default(),
        config_loader: loader as Arc<dyn ConfigLoader>,
        listener_controller: Some(controller as Arc<dyn ListenerController>),
        secret_injector: Some(Arc::new(StubSecretInjector::new()) as Arc<dyn SecretInjector>),
        secret_updaters: Vec::new(),
    });

    let _guard = metrics::record();
    manager.perform_reload().await?;

    assert_eq!(metrics::counter("axinite.reload.outcomes", "success"), 1);
    Ok(())
}

#[tokio::test]
async fn config_failure_records_config_error_outcome() -> Result<(), Box<dyn std::error::Error>> {
    let loader = Arc::new(StubConfigLoader::new_error(ConfigError::MissingRequired {
        key: "DATABASE_BACKEND".to_string(),
        hint: "set DATABASE_BACKEND".to_string(),
    }));

    let manager = HotReloadManager::new_with_context(HotReloadManagerDependencies {
        env_context: EnvContext::default(),
        config_loader: loader as Arc<dyn ConfigLoader>,
        listener_controller: None,
        secret_injector: None,
        secret_updaters: Vec::new(),
    });

    let _guard = metrics::record();
    let result = manager.perform_reload().await;

    assert!(result.is_err(), "a loader failure must fail the reload");
    assert_eq!(
        metrics::counter("axinite.reload.outcomes", "config_error"),
        1
    );
    Ok(())
}

#[tokio::test]
async fn removed_http_channel_records_http_removed_outcome()
-> Result<(), Box<dyn std::error::Error>> {
    let (_temp_dir, config) = test_config_with_http(None).await?;
    let loader = Arc::new(StubConfigLoader::new_success(config));

    let manager = HotReloadManager::new_with_context(HotReloadManagerDependencies {
        env_context: EnvContext::default(),
        config_loader: loader as Arc<dyn ConfigLoader>,
        listener_controller: None,
        secret_injector: None,
        secret_updaters: Vec::new(),
    });

    let _guard = metrics::record();
    manager.perform_reload().await?;

    assert_eq!(
        metrics::counter("axinite.reload.outcomes", "http_removed"),
        1
    );
    Ok(())
}

#[tokio::test]
async fn listener_failure_records_listener_error_outcome() -> Result<(), Box<dyn std::error::Error>>
{
    let (_temp_dir, config) =
        test_config_with_http(Some(http_config("127.0.0.1", 8081, Some("secret")))).await?;
    let loader = Arc::new(StubConfigLoader::new_success(config));
    let controller = Arc::new(StubListenerController::new_with_restart_failure(addr(
        "127.0.0.1:8080",
    )));

    let manager = HotReloadManager::new_with_context(HotReloadManagerDependencies {
        env_context: EnvContext::default(),
        config_loader: loader as Arc<dyn ConfigLoader>,
        listener_controller: Some(controller as Arc<dyn ListenerController>),
        secret_injector: None,
        secret_updaters: Vec::new(),
    });

    let _guard = metrics::record();
    let result = manager.perform_reload().await;

    assert!(result.is_err(), "a listener failure must fail the reload");
    assert_eq!(
        metrics::counter("axinite.reload.outcomes", "listener_error"),
        1
    );
    Ok(())
}
