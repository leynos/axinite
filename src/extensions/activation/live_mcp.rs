//! Live MCP server activation adapter.
//!
//! Holds the concrete MCP infrastructure references (session manager, process
//! manager, secrets, tool registry, shared client map) and implements the
//! [`McpActivationPort`] by connecting to the named server, listing
//! tools, registering them, and caching the client.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::extensions::activation::{ActivationFuture, McpActivationPort};
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;
use crate::tools::mcp::McpClient;
use crate::tools::mcp::config::McpServerConfig;
use crate::tools::mcp::session::McpSessionManager;

/// Configuration for [`LiveMcpActivation`].
pub struct LiveMcpActivationConfig {
    pub mcp_session_manager: Arc<McpSessionManager>,
    pub mcp_process_manager: Arc<crate::tools::mcp::process::McpProcessManager>,
    /// Shared with [`ExtensionManager`] so both see the same set of active
    /// MCP clients.
    pub mcp_clients: Arc<RwLock<HashMap<String, Arc<McpClient>>>>,
    pub secrets: Arc<dyn SecretsStore + Send + Sync>,
    pub tool_registry: Arc<ToolRegistry>,
    pub user_id: String,
    pub store: Option<Arc<dyn crate::db::Database>>,
}

/// Live adapter wiring MCP activation to the real MCP client infrastructure.
pub struct LiveMcpActivation {
    mcp_session_manager: Arc<McpSessionManager>,
    mcp_process_manager: Arc<crate::tools::mcp::process::McpProcessManager>,
    /// Shared with [`ExtensionManager`] so both see the same set of active
    /// MCP clients.
    mcp_clients: Arc<RwLock<HashMap<String, Arc<McpClient>>>>,
    secrets: Arc<dyn SecretsStore + Send + Sync>,
    tool_registry: Arc<ToolRegistry>,
    user_id: String,
    store: Option<Arc<dyn crate::db::Database>>,
}

impl LiveMcpActivation {
    pub fn new(config: LiveMcpActivationConfig) -> Self {
        Self {
            mcp_session_manager: config.mcp_session_manager,
            mcp_process_manager: config.mcp_process_manager,
            mcp_clients: config.mcp_clients,
            secrets: config.secrets,
            tool_registry: config.tool_registry,
            user_id: config.user_id,
            store: config.store,
        }
    }

    /// Load MCP server configuration from DB or filesystem.
    async fn get_mcp_server(
        &self,
        name: &str,
    ) -> Result<McpServerConfig, crate::tools::mcp::config::ConfigError> {
        let servers = if let Some(ref store) = self.store {
            crate::tools::mcp::config::load_mcp_servers_from_db(store.as_ref(), &self.user_id)
                .await?
        } else {
            crate::tools::mcp::config::load_mcp_servers().await?
        };
        servers.get(name).cloned().ok_or_else(|| {
            crate::tools::mcp::config::ConfigError::ServerNotFound {
                name: name.to_string(),
            }
        })
    }
}

impl McpActivationPort for LiveMcpActivation {
    fn activate_mcp<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        Box::pin(async move { self.activate_mcp_inner(name).await })
    }
}

impl LiveMcpActivation {
    async fn activate_mcp_inner<'a>(
        &'a self,
        name: &'a str,
    ) -> Result<ActivateResult, ExtensionError> {
        // Check if already activated
        {
            let clients = self.mcp_clients.read().await;
            if clients.contains_key(name) {
                let tools: Vec<String> = self
                    .tool_registry
                    .list()
                    .await
                    .into_iter()
                    .filter(|t| t.starts_with(&format!("{}_", name)))
                    .collect();

                return Ok(ActivateResult {
                    name: name.to_string(),
                    kind: ExtensionKind::McpServer,
                    tools_loaded: tools,
                    message: format!("MCP server '{}' already active", name),
                });
            }
        }

        let server = self
            .get_mcp_server(name)
            .await
            .map_err(|e| ExtensionError::NotInstalled(e.to_string()))?;

        let client = crate::tools::mcp::create_client_from_config(
            server.clone(),
            &self.mcp_session_manager,
            &self.mcp_process_manager,
            Some(Arc::clone(&self.secrets)),
            &self.user_id,
        )
        .await
        .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        let mcp_tools = client
            .list_tools()
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        let tool_impls = client
            .create_tools()
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        let tool_names: Vec<String> = mcp_tools
            .iter()
            .map(|t| format!("{}_{}", name, t.name))
            .collect();

        for tool in tool_impls {
            self.tool_registry.register(tool).await;
        }

        self.mcp_clients
            .write()
            .await
            .insert(name.to_string(), Arc::new(client));

        tracing::info!(
            "Activated MCP server '{}' with {} tools",
            name,
            tool_names.len()
        );

        Ok(ActivateResult {
            name: name.to_string(),
            kind: ExtensionKind::McpServer,
            tools_loaded: tool_names,
            message: format!("Connected to '{}' and loaded tools", name),
        })
    }
}
