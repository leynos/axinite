//! Extension bootstrap helpers shared by the application builder and tests.
//!
//! Keeps extension-manager assembly and startup loading out of `src/app.rs`
//! so application bootstrap can stay focused on phase orchestration.

mod mcp_servers;
mod wasm_tools;

use std::sync::Arc;

use anyhow::Context;

use crate::config::Config;
use crate::db::Database;
use crate::extensions::manager::LiveWasmChannelSharedState;
use crate::extensions::{
    ExtensionManager, ExtensionManagerConfig, LiveMcpActivation, LiveMcpActivationConfig,
    LiveWasmChannelActivation, LiveWasmChannelActivationConfig, LiveWasmToolActivation,
    LiveWasmToolActivationConfig, McpClientsMap, OnlineDiscovery, RegistryEntry,
};
use crate::hooks::HookRegistry;
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;
use crate::tools::mcp::{McpProcessManager, McpSessionManager};
use crate::tools::wasm::WasmToolRuntime;

pub use mcp_servers::LoadMcpServersParams;
use mcp_servers::load_and_register_mcp_servers;
use wasm_tools::load_wasm_tools;

/// Return type for extension bootstrap.
pub type ExtensionInitResult = (
    Arc<McpSessionManager>,
    Arc<McpProcessManager>,
    Option<Arc<WasmToolRuntime>>,
    Option<Arc<ExtensionManager>>,
    Vec<RegistryEntry>,
    Vec<String>,
);

/// Inputs needed to build the extension manager.
pub struct BuildExtensionManagerParams<'a> {
    pub config: &'a Config,
    pub db: Option<Arc<dyn Database>>,
    pub tools: &'a Arc<ToolRegistry>,
    pub hooks: &'a Arc<HookRegistry>,
    pub mcp_session_manager: &'a Arc<McpSessionManager>,
    pub mcp_process_manager: &'a Arc<McpProcessManager>,
    pub ext_secrets: Arc<dyn SecretsStore + Send + Sync>,
    pub wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
    pub catalog_entries: Vec<RegistryEntry>,
    pub mcp_clients: McpClientsMap,
    pub relay_config: Option<crate::config::RelayConfig>,
    pub gateway_token: Option<String>,
}

/// Inputs needed to initialise all extension runtime components.
pub struct BuildExtensionsParams<'a> {
    pub config: &'a Config,
    pub db: Option<Arc<dyn Database>>,
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    pub tools: &'a Arc<ToolRegistry>,
    pub hooks: &'a Arc<HookRegistry>,
    pub relay_config: Option<crate::config::RelayConfig>,
    pub gateway_token: Option<String>,
}

fn build_mcp_activation(
    config: LiveMcpActivationConfig,
) -> Arc<dyn crate::extensions::McpActivationPort> {
    Arc::new(LiveMcpActivation::new(config))
}

fn build_wasm_tool_activation(
    config: LiveWasmToolActivationConfig,
) -> Arc<dyn crate::extensions::WasmToolActivationPort> {
    Arc::new(LiveWasmToolActivation::new(config))
}

fn build_wasm_channel_activation(
    config: LiveWasmChannelActivationConfig,
) -> Arc<dyn crate::extensions::WasmChannelActivationPort> {
    Arc::new(LiveWasmChannelActivation::new(config))
}

fn mk_mcp_activation_config(params: &BuildExtensionManagerParams<'_>) -> LiveMcpActivationConfig {
    LiveMcpActivationConfig {
        mcp_session_manager: Arc::clone(params.mcp_session_manager),
        mcp_process_manager: Arc::clone(params.mcp_process_manager),
        mcp_clients: Arc::clone(&params.mcp_clients),
        secrets: Arc::clone(&params.ext_secrets),
        tool_registry: Arc::clone(params.tools),
        user_id: "default".to_string(),
        store: params.db.clone(),
    }
}

fn mk_wasm_tool_activation_config(
    params: &BuildExtensionManagerParams<'_>,
) -> LiveWasmToolActivationConfig {
    LiveWasmToolActivationConfig {
        wasm_tool_runtime: params.wasm_tool_runtime.clone(),
        wasm_tools_dir: params.config.wasm.tools_dir.clone(),
        tool_registry: Arc::clone(params.tools),
        secrets: Arc::clone(&params.ext_secrets),
        hooks: Some(Arc::clone(params.hooks)),
    }
}

fn mk_wasm_channel_activation_config(
    params: &BuildExtensionManagerParams<'_>,
    shared_state: &LiveWasmChannelSharedState,
) -> LiveWasmChannelActivationConfig {
    LiveWasmChannelActivationConfig {
        active_channel_names: Arc::clone(&shared_state.active_channel_names),
        activation_errors: Arc::clone(&shared_state.activation_errors),
        sse_sender: Arc::clone(&shared_state.sse_sender),
        wasm_channels_dir: params.config.channels.wasm_channels_dir.clone(),
        secrets: Arc::clone(&params.ext_secrets),
        channel_runtime: Arc::clone(&shared_state.channel_runtime),
        relay_channel_manager: Arc::clone(&shared_state.relay_channel_manager),
        tunnel_url: params.config.tunnel.public_url.clone(),
        user_id: "default".to_string(),
        store: params.db.clone(),
        relay_config: params.relay_config.clone(),
        gateway_token: params.gateway_token.clone(),
        installed_relay_extensions: Arc::clone(&shared_state.installed_relay_extensions),
    }
}

pub async fn build_extension_manager(
    params: BuildExtensionManagerParams<'_>,
) -> Arc<ExtensionManager> {
    let shared_state = LiveWasmChannelSharedState::default();

    let mcp_activation = build_mcp_activation(mk_mcp_activation_config(&params));
    let wasm_tool_activation = build_wasm_tool_activation(mk_wasm_tool_activation_config(&params));
    let wasm_channel_activation =
        build_wasm_channel_activation(mk_wasm_channel_activation_config(&params, &shared_state));

    let manager = Arc::new(ExtensionManager::new(ExtensionManagerConfig {
        shared_state,
        discovery: Arc::new(OnlineDiscovery::new()),
        relay_config: params.relay_config.clone(),
        gateway_token: params.gateway_token.clone(),
        mcp_activation,
        wasm_tool_activation,
        wasm_channel_activation,
        mcp_clients: params.mcp_clients.clone(),
        secrets: params.ext_secrets.clone(),
        tool_registry: Arc::clone(params.tools),
        hooks: Some(Arc::clone(params.hooks)),
        wasm_tools_dir: params.config.wasm.tools_dir.clone(),
        wasm_channels_dir: params.config.channels.wasm_channels_dir.clone(),
        tunnel_url: params.config.tunnel.public_url.clone(),
        user_id: "default".to_string(),
        store: params.db.clone(),
        catalog_entries: params.catalog_entries.clone(),
    }));

    params.tools.register_extension_tools(Arc::clone(&manager));
    tracing::debug!("Extension manager initialised with in-chat discovery tools");
    manager
}

fn load_catalog_entries() -> Vec<RegistryEntry> {
    let mut entries = match crate::registry::RegistryCatalog::load_or_embedded() {
        Ok(catalog) => {
            let entries: Vec<_> = catalog
                .all()
                .iter()
                .map(|manifest| manifest.to_registry_entry())
                .collect();
            tracing::debug!(
                count = entries.len(),
                "Loaded registry catalog entries for extension discovery"
            );
            entries
        }
        Err(e) => {
            tracing::warn!("Failed to load registry catalog: {}", e);
            Vec::new()
        }
    };
    for entry in crate::extensions::registry::builtin_entries() {
        if !entries.iter().any(|existing| existing.name == entry.name) {
            entries.push(entry);
        }
    }
    entries
}

fn build_ext_secrets(
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
) -> Result<Arc<dyn SecretsStore + Send + Sync>, anyhow::Error> {
    if let Some(secrets_store) = secrets_store {
        return Ok(secrets_store);
    }

    use crate::secrets::{InMemorySecretsStore, SecretsCrypto};

    let ephemeral_key =
        secrecy::SecretString::from(crate::secrets::keychain::generate_master_key_hex());
    let crypto = Arc::new(
        SecretsCrypto::new(ephemeral_key)
            .context("failed to create ephemeral crypto for extension manager")?,
    );
    tracing::debug!("Using ephemeral in-memory secrets store for extension manager");
    Ok(Arc::new(InMemorySecretsStore::new(crypto)))
}

fn init_managers_and_clients() -> (
    Arc<McpSessionManager>,
    Arc<McpProcessManager>,
    McpClientsMap,
) {
    (
        Arc::new(McpSessionManager::new()),
        Arc::new(McpProcessManager::new()),
        McpClientsMap::default(),
    )
}

fn init_wasm_runtime(config: &Config) -> Option<Arc<WasmToolRuntime>> {
    if config.wasm.enabled {
        WasmToolRuntime::new(config.wasm.to_runtime_config())
            .map(Arc::new)
            .map_err(|e| tracing::warn!("Failed to initialize WASM runtime: {}", e))
            .ok()
    } else {
        None
    }
}

fn maybe_register_dev_tools(config: &Config, tools: &Arc<ToolRegistry>) {
    let builder_registered_dev_tools =
        config.builder.enabled && (config.agent.allow_local_tools || !config.sandbox.enabled);
    if config.agent.allow_local_tools && !builder_registered_dev_tools {
        tools.register_dev_tools();
    }
}

pub async fn build_extensions(
    params: BuildExtensionsParams<'_>,
) -> Result<ExtensionInitResult, anyhow::Error> {
    let BuildExtensionsParams {
        config,
        db,
        secrets_store,
        tools,
        hooks,
        relay_config,
        gateway_token,
    } = params;
    let (mcp_session_manager, mcp_process_manager, mcp_clients) = init_managers_and_clients();

    // Create WASM tool runtime eagerly so extensions installed after startup
    // (e.g. via the web UI) can still be activated.
    let wasm_tool_runtime = init_wasm_runtime(config);

    let (dev_loaded_tool_names, _) = tokio::join!(
        load_wasm_tools(
            config,
            secrets_store.clone(),
            tools,
            wasm_tool_runtime.clone(),
        ),
        load_and_register_mcp_servers(LoadMcpServersParams {
            db: db.clone(),
            secrets_store: secrets_store.clone(),
            tools,
            mcp_session_manager: &mcp_session_manager,
            mcp_process_manager: &mcp_process_manager,
            mcp_clients: Arc::clone(&mcp_clients),
        }),
    );

    let catalog_entries = load_catalog_entries();
    let ext_secrets = build_ext_secrets(secrets_store.clone())?;

    let extension_manager = Some(
        build_extension_manager(BuildExtensionManagerParams {
            config,
            db: db.clone(),
            tools,
            hooks,
            mcp_session_manager: &mcp_session_manager,
            mcp_process_manager: &mcp_process_manager,
            ext_secrets,
            wasm_tool_runtime: wasm_tool_runtime.clone(),
            catalog_entries: catalog_entries.clone(),
            mcp_clients: Arc::clone(&mcp_clients),
            relay_config,
            gateway_token,
        })
        .await,
    );

    // register_builder_tool() already calls register_dev_tools() internally,
    // so only register them here when the builder didn't already do it.
    maybe_register_dev_tools(config, tools);

    Ok((
        mcp_session_manager,
        mcp_process_manager,
        wasm_tool_runtime,
        extension_manager,
        catalog_entries,
        dev_loaded_tool_names,
    ))
}
