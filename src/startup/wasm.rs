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
pub(crate) struct WasmWiringContext<'a, Manager = ironclaw::extensions::ExtensionManager>
where
    Manager: WasmRuntimeWiringPort + ?Sized,
{
    /// Extension manager used to inject the WASM channel runtime.
    pub(crate) extension_manager: &'a Option<Arc<Manager>>,
    /// Live channel manager for relay-channel and runtime wiring.
    pub(crate) channels: &'a Arc<ChannelManager>,
    /// Optional SSE sender registered with the extension manager after wiring.
    pub(crate) sse_sender:
        &'a Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    /// Map from WASM channel name to its owner account ID.
    pub(crate) wasm_channel_owner_ids: &'a std::collections::HashMap<String, i64>,
}

/// Minimal runtime-wiring surface used during startup.
pub(crate) trait WasmRuntimeWiringPort: Send + Sync {
    async fn set_active_channels(&self, names: Vec<String>);
    async fn set_channel_runtime(
        &self,
        channel_manager: Arc<ChannelManager>,
        wasm_channel_runtime: Arc<WasmChannelRuntime>,
        pairing_store: Arc<PairingStore>,
        wasm_channel_router: Arc<WasmChannelRouter>,
        wasm_channel_owner_ids: std::collections::HashMap<String, i64>,
    );
    async fn load_persisted_active_channels(&self) -> Vec<String>;
    async fn is_relay_channel(&self, name: &str) -> bool;
    async fn activate(
        &self,
        name: &str,
    ) -> Result<ironclaw::extensions::ActivateResult, ironclaw::extensions::ExtensionError>;
    async fn set_relay_channel_manager(&self, channel_manager: Arc<ChannelManager>);
    async fn restore_relay_channels(&self);
    async fn set_sse_sender(
        &self,
        sender: tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>,
    );
}

impl WasmRuntimeWiringPort for ironclaw::extensions::ExtensionManager {
    async fn set_active_channels(&self, names: Vec<String>) {
        self.set_active_channels(names).await;
    }

    async fn set_channel_runtime(
        &self,
        channel_manager: Arc<ChannelManager>,
        wasm_channel_runtime: Arc<WasmChannelRuntime>,
        pairing_store: Arc<PairingStore>,
        wasm_channel_router: Arc<WasmChannelRouter>,
        wasm_channel_owner_ids: std::collections::HashMap<String, i64>,
    ) {
        self.set_channel_runtime(
            channel_manager,
            wasm_channel_runtime,
            pairing_store,
            wasm_channel_router,
            wasm_channel_owner_ids,
        )
        .await;
    }

    async fn load_persisted_active_channels(&self) -> Vec<String> {
        self.load_persisted_active_channels().await
    }

    async fn is_relay_channel(&self, name: &str) -> bool {
        self.is_relay_channel(name).await
    }

    async fn activate(
        &self,
        name: &str,
    ) -> Result<ironclaw::extensions::ActivateResult, ironclaw::extensions::ExtensionError> {
        self.activate(name).await
    }

    async fn set_relay_channel_manager(&self, channel_manager: Arc<ChannelManager>) {
        self.set_relay_channel_manager(channel_manager).await;
    }

    async fn restore_relay_channels(&self) {
        self.restore_relay_channels().await;
    }

    async fn set_sse_sender(
        &self,
        sender: tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>,
    ) {
        self.set_sse_sender(sender).await;
    }
}

/// Result of [`init_wasm_channels`]: the list of loaded channel names and the
/// optional runtime state that must later be wired via
/// [`wire_wasm_channel_runtime`].
pub(crate) struct WasmChannelsInit {
    /// Names of WASM channels that were successfully loaded.
    pub(crate) loaded_wasm_channel_names: Vec<String>,
    /// Runtime state to be wired into the extension manager; `None` when no
    /// channels loaded.
    pub(crate) runtime_state: Option<WasmChannelRuntimeState>,
}

fn empty_wasm_channels_init() -> WasmChannelsInit {
    WasmChannelsInit {
        loaded_wasm_channel_names: vec![],
        runtime_state: None,
    }
}

async fn validate_wasm_channels_dir(config: &Config) -> bool {
    let metadata = match tokio::fs::metadata(&config.channels.wasm_channels_dir).await {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tracing::warn!(
                path = %config.channels.wasm_channels_dir.display(),
                "WASM channels are enabled, but the channel directory does not exist"
            );
            return false;
        }
        Err(error) => {
            tracing::error!(
                path = %config.channels.wasm_channels_dir.display(),
                error = %error,
                "Failed to inspect WASM channel directory"
            );
            return false;
        }
    };

    if !metadata.is_dir() {
        tracing::warn!(
            path = %config.channels.wasm_channels_dir.display(),
            "WASM channels are enabled, but the channel directory path is not a directory"
        );
        return false;
    }

    true
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
    if !config.channels.wasm_channels_enabled || !validate_wasm_channels_dir(config).await {
        return empty_wasm_channels_init();
    }

    let Some(result) = ironclaw::channels::wasm::setup_wasm_channels(
        config,
        &components.secrets_store,
        components.extension_manager.as_ref(),
        components.db.as_ref(),
    )
    .await
    else {
        return empty_wasm_channels_init();
    };
    let loaded_wasm_channel_names = result.channel_names;
    let runtime_state = Some((
        result.wasm_channel_runtime,
        result.pairing_store,
        result.wasm_channel_router,
    ));
    for (name, channel) in result.channels {
        reg.channel_names.push(name);
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

/// Auto-activates any persisted WASM channels that were not active at startup
/// and are not relay channels.
async fn auto_activate_persisted_channels<Manager: WasmRuntimeWiringPort + ?Sized>(
    ext_mgr: &Manager,
    active_at_startup: &std::collections::HashSet<String>,
    persisted: &[String],
) {
    for name in persisted {
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

/// Reconnects the extension manager to the live relay-channel registry and
/// restores persisted relay channels.
async fn wire_relay_channels<Manager: WasmRuntimeWiringPort + ?Sized>(
    ext_mgr: &Manager,
    channels: &Arc<ChannelManager>,
) {
    ext_mgr
        .set_relay_channel_manager(Arc::clone(channels))
        .await;
    ext_mgr.restore_relay_channels().await;
}

/// Registers the gateway SSE sender with the extension manager after runtime
/// wiring.
async fn register_sse_sender<Manager: WasmRuntimeWiringPort + ?Sized>(
    ext_mgr: &Manager,
    sender: &tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>,
) {
    ext_mgr.set_sse_sender(sender.clone()).await;
}

/// Transfers ownership of the WASM runtime state into the extension manager and
/// activates any channels that were not already active at startup.
///
/// Also configures relay-channel management and registers the SSE sender with
/// the extension manager when one is provided.
pub(crate) async fn wire_wasm_channel_runtime<Manager: WasmRuntimeWiringPort + ?Sized>(
    wiring: &WasmWiringContext<'_, Manager>,
    wasm_channel_runtime_state: &mut Option<WasmChannelRuntimeState>,
    loaded_wasm_channel_names: &[String],
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
        auto_activate_persisted_channels(ext_mgr.as_ref(), &active_at_startup, &persisted).await;
    }

    if let Some(ext_mgr) = wiring.extension_manager {
        wire_relay_channels(ext_mgr.as_ref(), wiring.channels).await;
    }

    if let Some(ext_mgr) = wiring.extension_manager
        && let Some(sender) = wiring.sse_sender
    {
        register_sse_sender(ext_mgr.as_ref(), sender).await;
    }
}

#[cfg(test)]
mod tests;
