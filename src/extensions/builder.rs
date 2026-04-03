//! Extension bootstrap helpers shared by the application builder and tests.
//!
//! Keeps extension-manager assembly and startup loading out of `src/app.rs`
//! so application bootstrap can stay focused on phase orchestration.

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
    db: Option<Arc<dyn Database>>,
    mcp_session_manager: &Arc<McpSessionManager>,
    mcp_process_manager: &Arc<McpProcessManager>,
    mcp_clients: &McpClientsMap,
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    tools: &Arc<ToolRegistry>,
) -> Arc<dyn crate::extensions::McpActivationPort> {
    Arc::new(LiveMcpActivation::new(LiveMcpActivationConfig {
        mcp_session_manager: Arc::clone(mcp_session_manager),
        mcp_process_manager: Arc::clone(mcp_process_manager),
        mcp_clients: Arc::clone(mcp_clients),
        secrets: Arc::clone(secrets),
        tool_registry: Arc::clone(tools),
        user_id: "default".to_string(),
        store: db,
    }))
}

fn build_wasm_tool_activation(
    config: &Config,
    wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    tools: &Arc<ToolRegistry>,
    hooks: &Arc<HookRegistry>,
) -> Arc<dyn crate::extensions::WasmToolActivationPort> {
    Arc::new(LiveWasmToolActivation::new(LiveWasmToolActivationConfig {
        wasm_tool_runtime,
        wasm_tools_dir: config.wasm.tools_dir.clone(),
        tool_registry: Arc::clone(tools),
        secrets: Arc::clone(secrets),
        hooks: Some(Arc::clone(hooks)),
    }))
}

fn build_wasm_channel_activation(
    config: &Config,
    db: Option<Arc<dyn Database>>,
    shared_state: &LiveWasmChannelSharedState,
    secrets: &Arc<dyn SecretsStore + Send + Sync>,
    relay_config: Option<crate::config::RelayConfig>,
    gateway_token: Option<String>,
) -> Arc<dyn crate::extensions::WasmChannelActivationPort> {
    Arc::new(LiveWasmChannelActivation::new(
        LiveWasmChannelActivationConfig {
            active_channel_names: Arc::clone(&shared_state.active_channel_names),
            activation_errors: Arc::clone(&shared_state.activation_errors),
            sse_sender: Arc::clone(&shared_state.sse_sender),
            wasm_channels_dir: config.channels.wasm_channels_dir.clone(),
            secrets: Arc::clone(secrets),
            channel_runtime: Arc::clone(&shared_state.channel_runtime),
            relay_channel_manager: Arc::clone(&shared_state.relay_channel_manager),
            tunnel_url: config.tunnel.public_url.clone(),
            user_id: "default".to_string(),
            store: db,
            relay_config,
            gateway_token,
            installed_relay_extensions: Arc::clone(&shared_state.installed_relay_extensions),
        },
    ))
}

pub async fn build_extension_manager(
    params: BuildExtensionManagerParams<'_>,
) -> Arc<ExtensionManager> {
    let BuildExtensionManagerParams {
        config,
        db,
        tools,
        hooks,
        mcp_session_manager,
        mcp_process_manager,
        ext_secrets,
        wasm_tool_runtime,
        catalog_entries,
        mcp_clients,
        relay_config,
        gateway_token,
    } = params;
    let discovery = Arc::new(OnlineDiscovery::new());
    let shared_state = LiveWasmChannelSharedState::default();

    let mcp_activation = build_mcp_activation(
        db.clone(),
        mcp_session_manager,
        mcp_process_manager,
        &mcp_clients,
        &ext_secrets,
        tools,
    );
    let wasm_tool_activation =
        build_wasm_tool_activation(config, wasm_tool_runtime, &ext_secrets, tools, hooks);
    let wasm_channel_activation = build_wasm_channel_activation(
        config,
        db.clone(),
        &shared_state,
        &ext_secrets,
        relay_config.clone(),
        gateway_token.clone(),
    );

    let manager = Arc::new(ExtensionManager::new(ExtensionManagerConfig {
        shared_state,
        discovery,
        relay_config,
        gateway_token,
        mcp_activation,
        wasm_tool_activation,
        wasm_channel_activation,
        mcp_clients,
        secrets: ext_secrets,
        tool_registry: Arc::clone(tools),
        hooks: Some(Arc::clone(hooks)),
        wasm_tools_dir: config.wasm.tools_dir.clone(),
        wasm_channels_dir: config.channels.wasm_channels_dir.clone(),
        tunnel_url: config.tunnel.public_url.clone(),
        user_id: "default".to_string(),
        store: db,
        catalog_entries,
    }));

    tools.register_extension_tools(Arc::clone(&manager));
    tracing::debug!("Extension manager initialized with in-chat discovery tools");
    manager
}

async fn load_wasm_tools(
    config: &Config,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    tools: &Arc<ToolRegistry>,
    wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
) -> Vec<String> {
    use crate::tools::wasm::{WasmToolLoader, load_dev_tools};

    let mut dev_loaded_tool_names: Vec<String> = Vec::new();
    let Some(ref runtime) = wasm_tool_runtime else {
        return dev_loaded_tool_names;
    };

    let mut loader = WasmToolLoader::new(Arc::clone(runtime), Arc::clone(tools));
    if let Some(ref secrets) = secrets_store {
        loader = loader.with_secrets_store(Arc::clone(secrets));
    }

    match loader.load_from_dir(&config.wasm.tools_dir).await {
        Ok(results) => {
            if !results.loaded.is_empty() {
                tracing::debug!(
                    "Loaded {} WASM tools from {}",
                    results.loaded.len(),
                    config.wasm.tools_dir.display()
                );
            }
            for (path, err) in &results.errors {
                tracing::warn!("Failed to load WASM tool {}: {}", path.display(), err);
            }
        }
        Err(e) => tracing::warn!("Failed to scan WASM tools directory: {}", e),
    }

    match load_dev_tools(&loader, &config.wasm.tools_dir).await {
        Ok(results) => {
            dev_loaded_tool_names.extend(results.loaded.iter().cloned());
            if !dev_loaded_tool_names.is_empty() {
                tracing::debug!(
                    "Loaded {} dev WASM tools from build artifacts",
                    dev_loaded_tool_names.len()
                );
            }
        }
        Err(e) => tracing::debug!("No dev WASM tools found: {}", e),
    }

    dev_loaded_tool_names
}

async fn load_and_register_mcp_servers(
    db: Option<Arc<dyn Database>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    tools: &Arc<ToolRegistry>,
    mcp_session_manager: &Arc<McpSessionManager>,
    mcp_process_manager: &Arc<McpProcessManager>,
    mcp_clients: McpClientsMap,
) {
    use crate::tools::mcp::config::load_mcp_servers_from_db;

    let servers_result = if let Some(ref db) = db {
        load_mcp_servers_from_db(db.as_ref(), "default").await
    } else {
        crate::tools::mcp::config::load_mcp_servers().await
    };

    let servers = match servers_result {
        Ok(s) => s,
        Err(e) => {
            tracing::debug!("No MCP servers configured ({})", e);
            return;
        }
    };

    let enabled: Vec<_> = servers.enabled_servers().cloned().collect();
    if !enabled.is_empty() {
        tracing::debug!("Loading {} configured MCP server(s)...", enabled.len());
    }

    let mut join_set = tokio::task::JoinSet::new();
    for server in enabled {
        let mcp_sm = Arc::clone(mcp_session_manager);
        let pm = Arc::clone(mcp_process_manager);
        let secrets = secrets_store.clone();
        let tools = Arc::clone(tools);
        let mcp_clients = Arc::clone(&mcp_clients);

        join_set.spawn(async move {
            let server_name = server.name.clone();
            let client = match crate::tools::mcp::create_client_from_config(
                server, &mcp_sm, &pm, secrets, "default",
            )
            .await
            {
                Ok(c) => Arc::new(c),
                Err(e) => {
                    tracing::warn!("Failed to create MCP client for '{}': {}", server_name, e);
                    return;
                }
            };

            match client.list_tools().await {
                Ok(mcp_tools) => {
                    let tool_count = mcp_tools.len();
                    match client.create_tools().await {
                        Ok(tool_impls) => {
                            for tool in tool_impls {
                                tools.register(tool).await;
                            }
                            let cell = Arc::new(tokio::sync::OnceCell::new());
                            let _ = cell.set(Arc::clone(&client));
                            mcp_clients.write().await.insert(server_name.clone(), cell);
                            tracing::debug!(
                                "Loaded {} tools from MCP server '{}'",
                                tool_count,
                                server_name
                            );
                        }
                        Err(e) => tracing::warn!(
                            "Failed to create tools from MCP server '{}': {}",
                            server_name,
                            e
                        ),
                    }
                }
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("401") || err_str.contains("authentication") {
                        tracing::warn!(
                            "MCP server '{}' requires authentication. \
                             Run: ironclaw mcp auth {}",
                            server_name,
                            server_name
                        );
                    } else {
                        tracing::warn!("Failed to connect to MCP server '{}': {}", server_name, e);
                    }
                }
            }
        });
    }

    while let Some(result) = join_set.join_next().await {
        if let Err(e) = result {
            tracing::warn!("MCP server loading task panicked: {}", e);
        }
    }
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
    let mcp_session_manager = Arc::new(McpSessionManager::new());
    let mcp_process_manager = Arc::new(McpProcessManager::new());
    let mcp_clients = McpClientsMap::default();

    // Create WASM tool runtime eagerly so extensions installed after startup
    // (e.g. via the web UI) can still be activated.
    let wasm_tool_runtime: Option<Arc<WasmToolRuntime>> = if config.wasm.enabled {
        WasmToolRuntime::new(config.wasm.to_runtime_config())
            .map(Arc::new)
            .map_err(|e| tracing::warn!("Failed to initialize WASM runtime: {}", e))
            .ok()
    } else {
        None
    };

    let (dev_loaded_tool_names, _) = tokio::join!(
        load_wasm_tools(
            config,
            secrets_store.clone(),
            tools,
            wasm_tool_runtime.clone(),
        ),
        load_and_register_mcp_servers(
            db.clone(),
            secrets_store.clone(),
            tools,
            &mcp_session_manager,
            &mcp_process_manager,
            Arc::clone(&mcp_clients),
        ),
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
    let builder_registered_dev_tools =
        config.builder.enabled && (config.agent.allow_local_tools || !config.sandbox.enabled);
    if config.agent.allow_local_tools && !builder_registered_dev_tools {
        tools.register_dev_tools();
    }

    Ok((
        mcp_session_manager,
        mcp_process_manager,
        wasm_tool_runtime,
        extension_manager,
        catalog_entries,
        dev_loaded_tool_names,
    ))
}
