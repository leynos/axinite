//! Hot-reload manager lifecycle and error-handling tests.
//!
//! These tests verify lifecycle behaviour including config load failures,
//! HTTP listener removal and shutdown, and secret injector error handling
//! during reload operations.

use std::net::SocketAddr;
use std::sync::Arc;

use rstest::rstest;

use super::super::*;
use super::common::{http_config, test_config_with_http};
use crate::channels::ChannelSecretUpdater;
use crate::error::ConfigError;
use crate::reload::test_stubs::{
    SpySecretUpdater, StubConfigLoader, StubListenerController, StubSecretInjector,
};

/// Test fixture providing assembled reload manager dependencies.
struct LifecycleFixture {
    manager: HotReloadManager,
    controller_clone: Arc<StubListenerController>,
    injector_clone: Arc<StubSecretInjector>,
    spy_clone: Arc<SpySecretUpdater>,
}

impl LifecycleFixture {
    async fn new(
        loader: Arc<dyn ConfigLoader>,
        injector: Arc<StubSecretInjector>,
        current_addr: SocketAddr,
    ) -> Self {
        let injector_clone = Arc::clone(&injector);
        let controller = Arc::new(StubListenerController::new(current_addr));
        let controller_clone = Arc::clone(&controller);
        let spy = Arc::new(SpySecretUpdater::new());
        let spy_clone = Arc::clone(&spy);

        let manager = HotReloadManager::new(
            loader,
            Some(controller as Arc<dyn ListenerController>),
            Some(injector as Arc<dyn SecretInjector>),
            vec![spy as Arc<dyn ChannelSecretUpdater>],
        );

        Self {
            manager,
            controller_clone,
            injector_clone,
            spy_clone,
        }
    }
}

/// Factory for creating a config loader, or None if loader is configured in test body.
type LoaderFactoryFn = fn() -> Arc<dyn ConfigLoader>;

/// Test scenario configuration and expected outcomes.
struct Scenario {
    description: &'static str,
    loader_factory: Option<LoaderFactoryFn>,
    injector_factory: fn() -> Arc<StubSecretInjector>,
    http_config_provider: fn() -> Option<(Option<crate::config::HttpConfig>, bool)>,
    expect_reload_ok: bool,
    expect_restart_count: usize,
    expect_shutdown_count: usize,
    expect_spy_call_count: usize,
    expect_spy_last_secret: Option<Option<String>>,
    expect_injector_called: Option<bool>,
}

#[rstest]
#[case::config_load_failure(Scenario {
    description: "config load failure prevents listener restart",
    loader_factory: Some(|| Arc::new(StubConfigLoader::new_error(
        ConfigError::MissingEnvVar("TEST".to_string()),
    ))),
    injector_factory: || Arc::new(StubSecretInjector::new()),
    http_config_provider: || None,
    expect_reload_ok: false,
    expect_restart_count: 0,
    expect_shutdown_count: 0,
    expect_spy_call_count: 0,
    expect_spy_last_secret: None,
    expect_injector_called: None,
})]
#[case::http_config_removed(Scenario {
    description: "http config removed shuts down listener and clears secrets",
    loader_factory: None,
    injector_factory: || Arc::new(StubSecretInjector::new()),
    http_config_provider: || Some((None, true)),
    expect_reload_ok: true,
    expect_restart_count: 0,
    expect_shutdown_count: 1,
    expect_spy_call_count: 1,
    expect_spy_last_secret: Some(None),
    expect_injector_called: None,
})]
#[case::secret_injector_failure(Scenario {
    description: "secret injector failure does not block config reload",
    loader_factory: None,
    injector_factory: || Arc::new(StubSecretInjector::new_failure()),
    http_config_provider: || Some((
        Some(http_config("127.0.0.1", 8081, Some("rotated-secret"))),
        true,
    )),
    expect_reload_ok: true,
    expect_restart_count: 1,
    expect_shutdown_count: 0,
    expect_spy_call_count: 1,
    expect_spy_last_secret: Some(Some("rotated-secret".to_string())),
    expect_injector_called: Some(true),
})]
#[tokio::test]
async fn lifecycle_scenarios(#[case] scenario: Scenario) -> Result<(), Box<dyn std::error::Error>> {
    let current_addr: SocketAddr = "127.0.0.1:8080".parse().expect("valid socket address");
    let injector = (scenario.injector_factory)();

    let loader: Arc<dyn ConfigLoader> = if let Some((http_cfg, _)) =
        (scenario.http_config_provider)()
    {
        let (_temp_dir, mut config) = test_config_with_http(None).await?;
        config.channels.http = http_cfg;
        Arc::new(StubConfigLoader::new_success(config))
    } else {
        scenario
            .loader_factory
            .expect("loader_factory must be Some when http_config_provider returns None")()
    };

    let fixture = LifecycleFixture::new(loader, injector, current_addr).await;

    let result = fixture.manager.perform_reload().await;

    if scenario.expect_reload_ok {
        assert!(
            result.is_ok(),
            "Reload should succeed for {}",
            scenario.description
        );
    } else {
        assert!(
            result.is_err(),
            "Reload should fail for {}",
            scenario.description
        );
    }

    assert_eq!(
        fixture.controller_clone.restart_calls().await.len(),
        scenario.expect_restart_count,
        "Listener restart count mismatch for {}",
        scenario.description
    );

    assert_eq!(
        fixture.controller_clone.shutdown_count(),
        scenario.expect_shutdown_count,
        "Listener shutdown count mismatch for {}",
        scenario.description
    );

    assert_eq!(
        fixture.spy_clone.call_count().await,
        scenario.expect_spy_call_count,
        "ChannelSecretUpdater call count mismatch for {}",
        scenario.description
    );

    if let Some(expected_secret) = scenario.expect_spy_last_secret {
        assert_eq!(
            fixture.spy_clone.last_secret().await,
            Some(expected_secret),
            "ChannelSecretUpdater last_secret mismatch for {}",
            scenario.description
        );
    } else if scenario.expect_spy_call_count == 0 {
        assert!(
            fixture.spy_clone.was_not_called().await,
            "ChannelSecretUpdater should not be called for {}",
            scenario.description
        );
    }

    if let Some(expect_called) = scenario.expect_injector_called
        && expect_called
    {
        assert!(
            fixture.injector_clone.was_called().await,
            "SecretInjector should be called for {}",
            scenario.description
        );
    }

    Ok(())
}
