//! WebAssembly channel bootstrap and runtime hot-wiring.

use std::sync::Arc;

use ironclaw::{
    app::AppComponents,
    channels::{
        ChannelManager,
        wasm::{WasmChannelRouter, WasmChannelRuntime},
    },
    config::Config,
    pairing::PairingStore,
};

use crate::startup::channels::ChannelRegistrar;

/// Shared runtime components for the loaded WASM channel subsystem.
pub(crate) type WasmChannelRuntimeState = (
    Arc<WasmChannelRuntime>,
    Arc<PairingStore>,
    Arc<WasmChannelRouter>,
);

/// Shared dependencies needed to wire the loaded WASM runtime back into the
/// extension manager and live channel registry.
pub(crate) struct WasmWiringContext<'a> {
    pub(crate) extension_manager: &'a Option<Arc<ironclaw::extensions::ExtensionManager>>,
    pub(crate) channels: &'a Arc<ChannelManager>,
    pub(crate) sse_sender:
        &'a Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    pub(crate) wasm_channel_owner_ids: &'a std::collections::HashMap<String, i64>,
}

/// Result of [`init_wasm_channels`]: the list of loaded channel names and the
/// optional runtime state that must later be wired via
/// [`wire_wasm_channel_runtime`].
pub(crate) struct WasmChannelsInit {
    pub(crate) loaded_wasm_channel_names: Vec<String>,
    pub(crate) runtime_state: Option<WasmChannelRuntimeState>,
}

/// Initialises WASM channels from the configured directory.
///
/// Returns an empty [`WasmChannelsInit`] (with `runtime_state: None`) when WASM
/// channels are disabled or the configured directory does not exist.
pub(crate) async fn init_wasm_channels(
    config: &Config,
    components: &AppComponents,
    reg: &mut ChannelRegistrar<'_>,
) -> WasmChannelsInit {
    if !config.channels.wasm_channels_enabled {
        return WasmChannelsInit {
            loaded_wasm_channel_names: vec![],
            runtime_state: None,
        };
    }
    if !config.channels.wasm_channels_dir.exists() {
        tracing::warn!(
            path = %config.channels.wasm_channels_dir.display(),
            "WASM channels are enabled, but the channel directory does not exist"
        );
        return WasmChannelsInit {
            loaded_wasm_channel_names: vec![],
            runtime_state: None,
        };
    }
    let Some(result) = ironclaw::channels::wasm::setup_wasm_channels(
        config,
        &components.secrets_store,
        components.extension_manager.as_ref(),
        components.db.as_ref(),
    )
    .await
    else {
        return WasmChannelsInit {
            loaded_wasm_channel_names: vec![],
            runtime_state: None,
        };
    };
    let loaded_wasm_channel_names = result.channel_names;
    let runtime_state = Some((
        result.wasm_channel_runtime,
        result.pairing_store,
        result.wasm_channel_router,
    ));
    for (name, channel) in result.channels {
        reg.channel_names.push(name.clone());
        reg.channels.add(channel).await;
    }
    if let Some(routes) = result.webhook_routes {
        reg.webhook_routes.push(routes);
    }
    WasmChannelsInit {
        loaded_wasm_channel_names,
        runtime_state,
    }
}

/// Transfers ownership of the WASM runtime state into the extension manager and
/// activates any channels that were not already active at startup.
///
/// Also configures relay-channel management and registers the SSE sender with
/// the extension manager when one is provided.
pub(crate) async fn wire_wasm_channel_runtime(
    wiring: &WasmWiringContext<'_>,
    wasm_channel_runtime_state: &mut Option<WasmChannelRuntimeState>,
    loaded_wasm_channel_names: &mut [String],
) {
    if let Some(ext_mgr) = wiring.extension_manager
        && let Some((rt, ps, router)) = wasm_channel_runtime_state.take()
    {
        let active_at_startup: std::collections::HashSet<String> =
            loaded_wasm_channel_names.iter().cloned().collect();
        ext_mgr
            .set_active_channels(loaded_wasm_channel_names.to_owned())
            .await;
        ext_mgr
            .set_channel_runtime(
                Arc::clone(wiring.channels),
                rt,
                ps,
                router,
                wiring.wasm_channel_owner_ids.clone(),
            )
            .await;
        tracing::debug!("Channel runtime wired into extension manager for hot-activation");

        let persisted = ext_mgr.load_persisted_active_channels().await;
        for name in &persisted {
            if active_at_startup.contains(name) || ext_mgr.is_relay_channel(name).await {
                continue;
            }
            match ext_mgr.activate(name).await {
                Ok(result) => {
                    tracing::debug!(
                        channel = %name,
                        message = %result.message,
                        "Auto-activated persisted WASM channel"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        channel = %name,
                        error = %e,
                        "Failed to auto-activate persisted WASM channel"
                    );
                }
            }
        }
    }

    if let Some(ext_mgr) = wiring.extension_manager {
        wire_relay_channels(ext_mgr, wiring.channels).await;
    }

    if let Some(ext_mgr) = wiring.extension_manager
        && let Some(sender) = wiring.sse_sender
    {
        register_sse_sender(ext_mgr, sender).await;
    }
}

async fn wire_relay_channels(
    ext_mgr: &ironclaw::extensions::ExtensionManager,
    channels: &Arc<ChannelManager>,
) {
    ext_mgr
        .set_relay_channel_manager(Arc::clone(channels))
        .await;
    ext_mgr.restore_relay_channels().await;
}

async fn register_sse_sender(
    ext_mgr: &ironclaw::extensions::ExtensionManager,
    sender: &tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>,
) {
    ext_mgr.set_sse_sender(sender.clone()).await;
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ironclaw::{
        app::{AppBuilder, AppBuilderFlags, AppComponents},
        channels::web::log_layer::LogBroadcaster,
        config::Config,
        llm::create_session_manager,
    };
    use tracing_test::traced_test;

    use crate::startup::channels::ChannelRegistrar;

    use super::init_wasm_channels;

    async fn build_test_components(config: Config) -> AppComponents {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let session = create_session_manager(config.llm.session.clone()).await;
        let log_broadcaster = Arc::new(LogBroadcaster::new());
        let (components, _side_effects) = AppBuilder::new(
            config,
            AppBuilderFlags {
                no_db: true,
                ..Default::default()
            },
            Some(tempdir.path().join("test.toml")),
            session,
            log_broadcaster,
        )
        .build_components()
        .await
        .expect("test components should build");
        components
    }

    #[tokio::test]
    async fn init_wasm_channels_skips_when_disabled() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let mut config = Config::for_testing(
            tempdir.path().join("test.db"),
            tempdir.path().join("skills"),
            tempdir.path().join("installed-skills"),
        )
        .await
        .expect("test config should be built");
        config.channels.wasm_channels_enabled = false;

        let components = build_test_components(config.clone()).await;

        let mut channel_names: Vec<String> = Vec::new();
        let mut webhook_routes: Vec<axum::Router> = Vec::new();
        let channels = ironclaw::channels::ChannelManager::new();
        let mut reg = ChannelRegistrar {
            channels: &channels,
            channel_names: &mut channel_names,
            webhook_routes: &mut webhook_routes,
        };

        let result = init_wasm_channels(&config, &components, &mut reg).await;

        assert!(result.loaded_wasm_channel_names.is_empty());
        assert!(result.runtime_state.is_none());
    }

    #[tokio::test]
    #[traced_test]
    async fn init_wasm_channels_warns_when_directory_missing() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let mut config = Config::for_testing(
            tempdir.path().join("test.db"),
            tempdir.path().join("skills"),
            tempdir.path().join("installed-skills"),
        )
        .await
        .expect("test config should be built");
        config.channels.wasm_channels_enabled = true;
        config.channels.wasm_channels_dir = tempdir.path().join("nonexistent");

        let components = build_test_components(config.clone()).await;

        let mut channel_names: Vec<String> = Vec::new();
        let mut webhook_routes: Vec<axum::Router> = Vec::new();
        let channels = ironclaw::channels::ChannelManager::new();
        let mut reg = ChannelRegistrar {
            channels: &channels,
            channel_names: &mut channel_names,
            webhook_routes: &mut webhook_routes,
        };

        let result = init_wasm_channels(&config, &components, &mut reg).await;

        assert!(result.loaded_wasm_channel_names.is_empty());
        assert!(result.runtime_state.is_none());
        assert!(logs_contain(
            "WASM channels are enabled, but the channel directory does not exist"
        ));
    }
}
