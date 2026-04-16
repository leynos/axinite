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
    extension_manager: &Option<Arc<ironclaw::extensions::ExtensionManager>>,
    wasm_channel_runtime_state: &mut Option<WasmChannelRuntimeState>,
    loaded_wasm_channel_names: &mut [String],
    channels: &Arc<ChannelManager>,
    sse_sender: &Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    wasm_channel_owner_ids: &std::collections::HashMap<String, i64>,
) {
    if let Some(ext_mgr) = extension_manager
        && let Some((rt, ps, router)) = wasm_channel_runtime_state.take()
    {
        let active_at_startup: std::collections::HashSet<String> =
            loaded_wasm_channel_names.iter().cloned().collect();
        ext_mgr
            .set_active_channels(loaded_wasm_channel_names.to_owned())
            .await;
        ext_mgr
            .set_channel_runtime(
                Arc::clone(channels),
                rt,
                ps,
                router,
                wasm_channel_owner_ids.clone(),
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

    if let Some(ext_mgr) = extension_manager {
        ext_mgr
            .set_relay_channel_manager(Arc::clone(channels))
            .await;
        ext_mgr.restore_relay_channels().await;
    }

    if let Some(ext_mgr) = extension_manager
        && let Some(sender) = sse_sender
    {
        ext_mgr.set_sse_sender(sender.clone()).await;
    }
}
