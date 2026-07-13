//! Central extension manager that dispatches operations by ExtensionKind.
//!
//! Holds references to channel runtime, WASM tool runtime, MCP infrastructure,
//! secrets store, and tool registry. All extension operations (search, install,
//! auth, activate, list, remove) flow through here.

mod auth_channel;
mod auth_mcp;
mod auth_wasm;
mod channel_state;
mod install;
mod listing;
mod oauth_flow;
mod ops;
mod removal;
mod setup_secrets;
mod upgrade;
mod wasm_install;

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::channels::ChannelManager;
use crate::extensions::activation::ChannelRuntimeState;
use crate::extensions::activation::McpClientsMap;
use crate::extensions::registry::ExtensionRegistry;
use crate::extensions::{
    ExtensionError, ExtensionKind, ExtensionSource, InstallResult, RegistryEntry,
};
use crate::hooks::HookRegistry;
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;

/// Pending OAuth authorization state.
struct PendingAuth {
    _name: String,
    _kind: ExtensionKind,
    created_at: std::time::Instant,
    /// Background task listening for the OAuth callback.
    /// Aborted when a new auth flow starts for the same extension.
    task_handle: Option<tokio::task::JoinHandle<()>>,
}

/// Shared mutable state between [`ExtensionManager`] and
/// [`LiveWasmChannelActivation`](crate::extensions::LiveWasmChannelActivation).
#[derive(Clone, Default)]
pub struct LiveWasmChannelSharedState {
    pub(crate) channel_runtime: Arc<RwLock<Option<ChannelRuntimeState>>>,
    pub(crate) relay_channel_manager: Arc<RwLock<Option<Arc<ChannelManager>>>>,
    pub(crate) active_channel_names: Arc<RwLock<HashSet<String>>>,
    pub(crate) installed_relay_extensions: Arc<RwLock<HashSet<String>>>,
    pub(crate) activation_errors: Arc<RwLock<HashMap<String, String>>>,
    pub(crate) sse_sender:
        Arc<RwLock<Option<tokio::sync::broadcast::Sender<crate::channels::web::types::SseEvent>>>>,
}

/// Result of saving setup secrets and attempting activation.
pub struct SetupResult {
    /// Human-readable status message.
    pub message: String,
    /// Whether the channel was successfully activated after saving secrets.
    pub activated: bool,
    /// OAuth authorization URL for the UI to open (if OAuth flow was started).
    pub auth_url: Option<String>,
}

/// Central manager for extension lifecycle operations.
pub struct ExtensionManager {
    registry: ExtensionRegistry,
    discovery: Arc<dyn crate::extensions::DiscoveryPort + Send + Sync>,

    // ── Activation ports (hexagonal seams) ──────────────────────────────
    mcp_activation: Arc<dyn crate::extensions::McpActivationPort>,
    wasm_tool_activation: Arc<dyn crate::extensions::WasmToolActivationPort>,
    wasm_channel_activation: Arc<dyn crate::extensions::WasmChannelActivationPort>,

    // MCP infrastructure
    /// Active MCP clients keyed by server name.
    ///
    /// Wrapped in `Arc` so the live MCP activation adapter can share this
    /// mutable registry with the manager.
    mcp_clients: McpClientsMap,

    // WASM tool infrastructure
    wasm_tools_dir: PathBuf,
    wasm_channels_dir: PathBuf,

    // WASM channel hot-activation infrastructure (set post-construction)
    channel_runtime: Arc<RwLock<Option<ChannelRuntimeState>>>,
    /// Channel manager for hot-adding relay channels (set independently of WASM runtime).
    relay_channel_manager: Arc<RwLock<Option<Arc<ChannelManager>>>>,

    // Shared
    secrets: Arc<dyn SecretsStore + Send + Sync>,
    tool_registry: Arc<ToolRegistry>,
    hooks: Option<Arc<HookRegistry>>,
    pending_auth: RwLock<HashMap<String, PendingAuth>>,
    /// Tunnel URL for webhook configuration and remote OAuth callbacks.
    tunnel_url: Option<String>,
    user_id: String,
    /// Optional database store for DB-backed MCP config.
    store: Option<Arc<dyn crate::db::Database>>,
    /// Names of WASM channels that were successfully loaded at startup.
    ///
    /// Wrapped in `Arc` so the live channel activation adapter can share
    /// this set with the manager.
    active_channel_names: Arc<RwLock<HashSet<String>>>,
    /// Installed channel-relay extensions (no on-disk artifact, tracked in memory).
    installed_relay_extensions: Arc<RwLock<HashSet<String>>>,
    /// Last activation error for each WASM channel (ephemeral, cleared on success).
    activation_errors: Arc<RwLock<HashMap<String, String>>>,
    /// SSE broadcast sender (set post-construction via `set_sse_sender()`).
    sse_sender:
        Arc<RwLock<Option<tokio::sync::broadcast::Sender<crate::channels::web::types::SseEvent>>>>,
    /// Shared registry of pending OAuth flows for gateway-routed callbacks.
    ///
    /// Keyed by CSRF `state` parameter. Populated in `start_wasm_oauth()`
    /// when running in gateway mode, consumed by the web gateway's
    /// `/oauth/callback` handler.
    pending_oauth_flows: crate::cli::oauth_defaults::PendingOAuthRegistry,
    /// Gateway auth token for authenticating with the platform token exchange proxy.
    /// Read once at construction from `GATEWAY_AUTH_TOKEN` env var.
    gateway_token: Option<String>,
    /// Relay config captured at startup. Used by `auth_channel_relay` and
    /// `activate_channel_relay` instead of re-reading env vars.
    relay_config: Option<crate::config::RelayConfig>,
}

/// Dependency bundle for [`ExtensionManager::new`].
///
/// All fields are required unless marked as optional. Production callers typically
/// supply live implementations of the activation ports (e.g., [`LiveMcpActivation`]),
/// while tests inject no-op stubs.
pub struct ExtensionManagerConfig {
    /// Shared mutable state for live WASM channel activation and manager state.
    pub shared_state: LiveWasmChannelSharedState,
    /// Discovery port for searching online extensions (required).
    pub discovery: Arc<dyn crate::extensions::DiscoveryPort + Send + Sync>,
    /// Relay configuration for channel-relay extensions (optional).
    pub relay_config: Option<crate::config::RelayConfig>,
    /// Gateway authentication token for platform OAuth proxy (optional).
    pub gateway_token: Option<String>,
    /// Activation port for MCP server extensions (required).
    pub mcp_activation: Arc<dyn crate::extensions::McpActivationPort>,
    /// Activation port for WASM tool extensions (required).
    pub wasm_tool_activation: Arc<dyn crate::extensions::WasmToolActivationPort>,
    /// Activation port for WASM channel and channel-relay extensions (required).
    pub wasm_channel_activation: Arc<dyn crate::extensions::WasmChannelActivationPort>,
    /// Shared map of active MCP clients, keyed by server name (required).
    ///
    /// This is shared with the live MCP activation adapter so both see the same
    /// set of active connections.
    pub mcp_clients: McpClientsMap,
    /// Secrets store for credential injection (required).
    pub secrets: Arc<dyn SecretsStore + Send + Sync>,
    /// Tool registry for registering activated tools (required).
    pub tool_registry: Arc<ToolRegistry>,
    /// Hook registry for plugin hooks (optional).
    pub hooks: Option<Arc<HookRegistry>>,
    /// Directory containing installed WASM tools (required).
    pub wasm_tools_dir: PathBuf,
    /// Directory containing installed WASM channels (required).
    pub wasm_channels_dir: PathBuf,
    /// Public tunnel URL for webhook configuration (optional).
    pub tunnel_url: Option<String>,
    /// User identifier for namespacing secrets and configuration (required).
    pub user_id: String,
    /// Database store for persistence (optional).
    pub store: Option<Arc<dyn crate::db::Database>>,
    /// Catalog entries for built-in and discovered extensions (required, may be empty).
    pub catalog_entries: Vec<RegistryEntry>,
}

/// Sanitize a URL for logging by removing query parameters and credentials.
/// Prevents accidental logging of API keys, OAuth tokens, or other sensitive data in URLs.
fn sanitize_url_for_logging(url: &str) -> String {
    // If URL is very short or doesn't look like a URL, just use as-is
    if url.len() < 10 || !url.contains("://") {
        return url.to_string();
    }

    // Try to parse and remove sensitive components
    if let Ok(mut parsed) = url::Url::parse(url) {
        // Remove query string and fragment
        parsed.set_query(None);
        parsed.set_fragment(None);

        // Remove userinfo (username and password) if present
        let _ = parsed.set_username("");
        let _ = parsed.set_password(None);

        parsed.to_string()
    } else {
        // Fallback: strip after ? or #
        url.split(['?', '#']).next().unwrap_or(url).to_string()
    }
}

impl ExtensionManager {
    /// Construct a new extension manager from a bundled configuration.
    ///
    /// # Configuration
    ///
    /// The constructor accepts a single [`ExtensionManagerConfig`] bundle. All
    /// activation ports are supplied as config fields rather than individual
    /// parameters.
    ///
    /// ## Required fields
    ///
    /// - `discovery` — Port for searching online extensions.
    /// - `mcp_activation` — Port for MCP server activation.
    /// - `wasm_tool_activation` — Port for WASM tool activation.
    /// - `wasm_channel_activation` — Port for WASM channel and channel-relay activation.
    /// - `shared_state` — Shared live WASM channel state used by both the
    ///   manager and the live activation adapter.
    /// - `mcp_clients` — Shared map of active MCP clients; must be shared with the
    ///   live MCP activation adapter so both see the same set of active connections.
    /// - `secrets` — Secrets store for credential injection.
    /// - `tool_registry` — Tool registry for registering activated tools.
    /// - `wasm_tools_dir` — Directory containing installed WASM tools.
    /// - `wasm_channels_dir` — Directory containing installed WASM channels.
    /// - `user_id` — User identifier for namespacing secrets and configuration.
    /// - `catalog_entries` — Catalog entries for built-in and discovered extensions
    ///   (may be empty).
    ///
    /// ## Optional fields
    ///
    /// - `relay_config` — Relay configuration for channel-relay extensions.
    /// - `gateway_token` — Gateway authentication token for platform OAuth proxy.
    /// - `hooks` — Hook registry for plugin hooks.
    /// - `tunnel_url` — Public tunnel URL for webhook configuration.
    /// - `store` — Database store for persistence.
    ///
    /// # Activation ports
    ///
    /// The three activation port fields (`mcp_activation`, `wasm_tool_activation`,
    /// `wasm_channel_activation`) decouple activation I/O from policy logic.
    /// Production callers supply [`LiveMcpActivation`], [`LiveWasmToolActivation`],
    /// and [`LiveWasmChannelActivation`]; tests inject no-op stubs.
    pub fn new(config: ExtensionManagerConfig) -> Self {
        let shared_state = config.shared_state.clone();
        Self::new_with_shared_state(config, shared_state)
    }

    pub(crate) fn new_with_shared_state(
        config: ExtensionManagerConfig,
        shared_state: LiveWasmChannelSharedState,
    ) -> Self {
        let registry = if config.catalog_entries.is_empty() {
            ExtensionRegistry::new()
        } else {
            ExtensionRegistry::new_with_catalog(config.catalog_entries)
        };
        Self {
            registry,
            discovery: config.discovery,
            mcp_activation: config.mcp_activation,
            wasm_tool_activation: config.wasm_tool_activation,
            wasm_channel_activation: config.wasm_channel_activation,
            mcp_clients: config.mcp_clients,
            wasm_tools_dir: config.wasm_tools_dir,
            wasm_channels_dir: config.wasm_channels_dir,
            channel_runtime: shared_state.channel_runtime,
            relay_channel_manager: shared_state.relay_channel_manager,
            secrets: config.secrets,
            tool_registry: config.tool_registry,
            hooks: config.hooks,
            pending_auth: RwLock::new(HashMap::new()),
            tunnel_url: config.tunnel_url,
            user_id: config.user_id,
            store: config.store,
            active_channel_names: shared_state.active_channel_names,
            installed_relay_extensions: shared_state.installed_relay_extensions,
            activation_errors: shared_state.activation_errors,
            sse_sender: shared_state.sse_sender,
            pending_oauth_flows: crate::cli::oauth_defaults::new_pending_oauth_registry(),
            gateway_token: config.gateway_token,
            relay_config: config.relay_config,
        }
    }
}

/// Infer the extension kind from a URL.
fn infer_kind_from_url(url: &str) -> ExtensionKind {
    if url.ends_with(".wasm") || url.ends_with(".tar.gz") {
        ExtensionKind::WasmTool
    } else {
        ExtensionKind::McpServer
    }
}

/// Decision from `fallback_decision`: should we try the fallback source or
/// return the primary result as-is?
enum FallbackDecision {
    /// Return the primary result directly (success or non-retriable error).
    Return,
    /// Primary failed with a retriable error and a fallback source is available.
    TryFallback,
}

/// Decide whether to attempt a fallback install based on the primary result
/// and the availability of a fallback source.
fn fallback_decision(
    primary_result: &Result<InstallResult, ExtensionError>,
    fallback_source: &Option<Box<ExtensionSource>>,
) -> FallbackDecision {
    match (primary_result, fallback_source) {
        // Success — no fallback needed
        (Ok(_), _) => FallbackDecision::Return,
        // AlreadyInstalled — don't try building from source
        (Err(ExtensionError::AlreadyInstalled(_)), _) => FallbackDecision::Return,
        // Failed with a fallback available — try it
        (Err(_), Some(_)) => FallbackDecision::TryFallback,
        // Failed with no fallback — return the error
        (Err(_), None) => FallbackDecision::Return,
    }
}

/// Combine primary and fallback errors into a single error.
///
/// Preserves `AlreadyInstalled` from the fallback directly; otherwise wraps
/// both errors into the structured `ExtensionError::FallbackFailed` variant.
fn combine_install_errors(
    primary_err: ExtensionError,
    fallback_err: ExtensionError,
) -> ExtensionError {
    if matches!(fallback_err, ExtensionError::AlreadyInstalled(_)) {
        return fallback_err;
    }
    ExtensionError::FallbackFailed {
        primary: Box::new(primary_err),
        fallback: Box::new(fallback_err),
    }
}

#[cfg(test)]
mod tests;
