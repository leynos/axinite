//! Live MCP server activation adapter.
//!
//! Holds the concrete MCP infrastructure references (session manager, process
//! manager, secrets, tool registry, shared client map) and implements the
//! [`McpActivationPort`] by connecting to the named server, listing
//! tools, registering them, and caching the client.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{OnceCell, RwLock};

use crate::extensions::activation::{ActivationFuture, McpActivationPort};
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;
use crate::tools::mcp::McpClient;
use crate::tools::mcp::config::McpServerConfig;
use crate::tools::mcp::session::McpSessionManager;

/// Type alias for the MCP client cache entry using OnceCell to serialize
/// concurrent activations without holding a write lock across I/O.
pub type McpClientCell = OnceCell<Arc<McpClient>>;

/// Type alias for the shared MCP clients map.
pub type McpClientsMap = Arc<RwLock<HashMap<String, Arc<McpClientCell>>>>;

/// Configuration for [`LiveMcpActivation`].
pub struct LiveMcpActivationConfig {
    pub mcp_session_manager: Arc<McpSessionManager>,
    pub mcp_process_manager: Arc<crate::tools::mcp::process::McpProcessManager>,
    /// Shared with [`ExtensionManager`] so both see the same set of active
    /// MCP clients.
    pub mcp_clients: McpClientsMap,
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
    mcp_clients: McpClientsMap,
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
        // Step 1: Acquire write lock briefly to get or insert the OnceCell for this server.
        // Release the lock immediately before any await.
        let cell = {
            let mut clients = self.mcp_clients.write().await;
            clients
                .entry(name.to_string())
                .or_insert_with(|| Arc::new(OnceCell::new()))
                .clone()
        };

        // Step 2: Use OnceCell to perform the expensive async work exactly once.
        // On error, the cell remains uninitialized so a later retry can succeed.
        let _client = cell
            .get_or_try_init(|| async { self.do_activate_mcp(name).await.map(Arc::new) })
            .await
            .map_err(|e: ExtensionError| e)?;

        // Step 3: Build the result with tool names from the registry.
        let tools: Vec<String> = self
            .tool_registry
            .list()
            .await
            .into_iter()
            .filter(|t| t.starts_with(&format!("{}_", name)))
            .collect();

        Ok(ActivateResult {
            name: name.to_string(),
            kind: ExtensionKind::McpServer,
            tools_loaded: tools.clone(),
            message: if tools.is_empty() {
                format!("MCP server '{}' activated with no tools", name)
            } else {
                format!("MCP server '{}' activated with {} tools", name, tools.len())
            },
        })
    }

    /// Performs the actual MCP activation work (config load, client creation,
    /// tool listing/registration). This is called by OnceCell to ensure it
    /// executes exactly once per server name.
    async fn do_activate_mcp(&self, name: &str) -> Result<McpClient, ExtensionError> {
        let server = self.get_mcp_server(name).await.map_err(|e| match e {
            crate::tools::mcp::config::ConfigError::ServerNotFound { .. } => {
                ExtensionError::NotInstalled(name.to_string())
            }
            other => ExtensionError::Config(other.to_string()),
        })?;

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

        for tool in tool_impls {
            self.tool_registry.register(tool).await;
        }

        tracing::info!(
            "Activated MCP server '{}' with {} tools",
            name,
            mcp_tools.len()
        );

        Ok(client)
    }
}
