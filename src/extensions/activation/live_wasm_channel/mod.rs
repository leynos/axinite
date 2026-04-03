//! Live WASM channel and channel-relay activation adapter.
//!
//! Contains the full implementation for activating WASM channels and
//! channel-relay extensions, decoupled from the [`ExtensionManager`] via
//! direct state injection.
//!
//! The port seam is in place so that tests can inject
//! [`NoOpWasmChannelActivation`](super::NoOpWasmChannelActivation) without
//! triggering real channel infrastructure.

mod auth_check;
mod channel_activation;
mod channel_refresh;
mod credentials;
mod relay_activation;
mod state;

pub(crate) use credentials::ToolAuthState;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};

use crate::channels::ChannelManager;
use crate::channels::wasm::WasmChannelRuntime;
use crate::extensions::activation::{ActivationFuture, WasmChannelActivationPort};
use crate::pairing::PairingStore;
use crate::secrets::SecretsStore;

/// Configuration bundle for [`LiveWasmChannelActivation`].
///
/// All fields are required for construction; production callers obtain these
/// from the same sources that previously constructed [`ExtensionManager`].
pub struct LiveWasmChannelActivationConfig {
    /// Names of WASM channels that are currently active.
    pub active_channel_names: Arc<RwLock<HashSet<String>>>,
    /// Last activation error for each WASM channel (ephemeral, cleared on success).
    pub activation_errors: Arc<RwLock<HashMap<String, String>>>,
    /// SSE broadcast sender for pushing extension status events to the web UI.
    pub sse_sender: Arc<RwLock<Option<broadcast::Sender<crate::channels::web::types::SseEvent>>>>,
    /// Directory containing installed WASM channels.
    pub wasm_channels_dir: PathBuf,
    /// Secrets store for credential injection.
    pub secrets: Arc<dyn SecretsStore + Send + Sync>,
    /// Channel runtime state for WASM channel activation (optional until set).
    pub(crate) channel_runtime: Arc<RwLock<Option<ChannelRuntimeState>>>,
    /// Channel manager for hot-adding relay channels (optional until set).
    pub relay_channel_manager: Arc<RwLock<Option<Arc<ChannelManager>>>>,
    /// Tunnel URL for webhook configuration and remote OAuth callbacks.
    pub tunnel_url: Option<String>,
    /// User identifier for namespacing secrets and configuration.
    pub user_id: String,
    /// Database store for persistence.
    pub store: Option<Arc<dyn crate::db::Database>>,
    /// Relay configuration for channel-relay extensions.
    pub relay_config: Option<crate::config::RelayConfig>,
    /// Gateway authentication token for platform token exchange proxy.
    pub gateway_token: Option<String>,
    /// Installed channel-relay extensions (no on-disk artifact, tracked in memory).
    pub installed_relay_extensions: Arc<RwLock<HashSet<String>>>,
}

/// Runtime state for WASM channel hot-activation.
///
/// This is a pub(crate) re-export from the manager module to allow
/// `LiveWasmChannelActivation` to access the channel runtime.
pub(crate) struct ChannelRuntimeState {
    pub(crate) channel_manager: Arc<ChannelManager>,
    pub(crate) wasm_channel_runtime: Arc<WasmChannelRuntime>,
    pub(crate) pairing_store: Arc<PairingStore>,
    pub(crate) wasm_channel_router: Arc<crate::channels::wasm::WasmChannelRouter>,
    pub(crate) wasm_channel_owner_ids: HashMap<String, i64>,
}

/// Live adapter that implements WASM channel and channel-relay activation.
///
/// State is injected at construction time via
/// [`LiveWasmChannelActivationConfig`], eliminating the need for
/// post-construction wiring or weak references.
pub struct LiveWasmChannelActivation {
    active_channel_names: Arc<RwLock<HashSet<String>>>,
    activation_errors: Arc<RwLock<HashMap<String, String>>>,
    sse_sender: Arc<RwLock<Option<broadcast::Sender<crate::channels::web::types::SseEvent>>>>,
    wasm_channels_dir: PathBuf,
    secrets: Arc<dyn SecretsStore + Send + Sync>,
    channel_runtime: Arc<RwLock<Option<ChannelRuntimeState>>>,
    relay_channel_manager: Arc<RwLock<Option<Arc<ChannelManager>>>>,
    tunnel_url: Option<String>,
    user_id: String,
    store: Option<Arc<dyn crate::db::Database>>,
    relay_config: Option<crate::config::RelayConfig>,
    #[expect(
        dead_code,
        reason = "FIXME: placeholder for gateway auth token plumbing"
    )]
    gateway_token: Option<String>,
    #[expect(dead_code, reason = "FIXME: placeholder for relay extension tracking")]
    installed_relay_extensions: Arc<RwLock<HashSet<String>>>,
}

impl LiveWasmChannelActivation {
    /// Create a new activation adapter with the provided configuration.
    pub fn new(config: LiveWasmChannelActivationConfig) -> Self {
        Self {
            active_channel_names: config.active_channel_names,
            activation_errors: config.activation_errors,
            sse_sender: config.sse_sender,
            wasm_channels_dir: config.wasm_channels_dir,
            secrets: config.secrets,
            channel_runtime: config.channel_runtime,
            relay_channel_manager: config.relay_channel_manager,
            tunnel_url: config.tunnel_url,
            user_id: config.user_id,
            store: config.store,
            relay_config: config.relay_config,
            gateway_token: config.gateway_token,
            installed_relay_extensions: config.installed_relay_extensions,
        }
    }
}

#[cfg(test)]
impl Default for LiveWasmChannelActivation {
    fn default() -> Self {
        // Create default empty configuration for testing
        use crate::secrets::{InMemorySecretsStore, SecretsCrypto};
        use secrecy::SecretString;

        let master_key = SecretString::from(crate::secrets::keychain::generate_master_key_hex());
        let crypto = Arc::new(SecretsCrypto::new(master_key).expect("ephemeral crypto"));

        Self {
            active_channel_names: Arc::new(RwLock::new(HashSet::new())),
            activation_errors: Arc::new(RwLock::new(HashMap::new())),
            sse_sender: Arc::new(RwLock::new(None)),
            wasm_channels_dir: PathBuf::new(),
            secrets: Arc::new(InMemorySecretsStore::new(crypto)),
            channel_runtime: Arc::new(RwLock::new(None)),
            relay_channel_manager: Arc::new(RwLock::new(None)),
            tunnel_url: None,
            user_id: "default".to_string(),
            store: None,
            relay_config: None,
            gateway_token: None,
            installed_relay_extensions: Arc::new(RwLock::new(HashSet::new())),
        }
    }
}

impl WasmChannelActivationPort for LiveWasmChannelActivation {
    fn activate_wasm_channel<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        let name = name.to_owned();
        Box::pin(async move { self.activate_wasm_channel_inner(&name).await })
    }

    fn activate_channel_relay<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        let name = name.to_owned();
        Box::pin(async move { self.activate_channel_relay_inner(&name).await })
    }
}
