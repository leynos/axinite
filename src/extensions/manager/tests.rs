//! Unit tests for extension kind inference and install fallback logic.

mod fallback_logic;
mod relay;
mod sanitize_url;
mod storage_paths;
mod upgrade;

use std::sync::Arc;

use crate::extensions::ExtensionManager;
use crate::extensions::manager::ExtensionManagerConfig;

fn make_manager_with_temp_dirs() -> anyhow::Result<ExtensionManager> {
    use anyhow::Context as _;

    let dir = tempfile::tempdir().context("temp dir")?;
    Ok(make_manager_custom_dirs(
        dir.path().join("tools"),
        dir.path().join("channels"),
    ))
}

fn make_manager_custom_dirs(
    tools_dir: std::path::PathBuf,
    channels_dir: std::path::PathBuf,
) -> ExtensionManager {
    use crate::secrets::{InMemorySecretsStore, SecretsCrypto};
    use crate::testing::credentials::TEST_CRYPTO_KEY;
    use crate::tools::ToolRegistry;

    ambient_fs::create_dir_all(&tools_dir).ok();
    ambient_fs::create_dir_all(&channels_dir).ok();

    let master_key = secrecy::SecretString::from(TEST_CRYPTO_KEY.to_string());
    let crypto = Arc::new(SecretsCrypto::new(master_key).unwrap());
    let mcp_clients = crate::extensions::McpClientsMap::default();

    ExtensionManager::new(ExtensionManagerConfig {
        shared_state: crate::extensions::LiveWasmChannelSharedState::default(),
        discovery: Arc::new(crate::extensions::NoOpDiscovery),
        relay_config: None,
        gateway_token: None,
        mcp_activation: Arc::new(crate::extensions::NoOpMcpActivation),
        wasm_tool_activation: Arc::new(crate::extensions::NoOpWasmToolActivation),
        wasm_channel_activation: Arc::new(crate::extensions::NoOpWasmChannelActivation),
        mcp_clients,
        secrets: Arc::new(InMemorySecretsStore::new(crypto)),
        tool_registry: Arc::new(ToolRegistry::new()),
        hooks: None,
        wasm_tools_dir: tools_dir,
        wasm_channels_dir: channels_dir,
        tunnel_url: None,
        user_id: "test".to_string(),
        store: None,
        catalog_entries: Vec::new(),
    })
}
