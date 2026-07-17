//! Loading and registration of configured MCP servers at startup.

use std::sync::Arc;

use crate::db::Database;
use crate::extensions::McpClientsMap;
use crate::secrets::SecretsStore;
use crate::tools::ToolRegistry;
use crate::tools::mcp::{McpProcessManager, McpSessionManager};

/// Inputs needed to load and register MCP servers.
pub struct LoadMcpServersParams<'a> {
    pub db: Option<Arc<dyn Database>>,
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    pub tools: &'a Arc<ToolRegistry>,
    pub mcp_session_manager: &'a Arc<McpSessionManager>,
    pub mcp_process_manager: &'a Arc<McpProcessManager>,
    pub mcp_clients: McpClientsMap,
}

struct McpLoadCtx {
    user_id: String,
    tools: Arc<ToolRegistry>,
    session: Arc<McpSessionManager>,
    process: Arc<McpProcessManager>,
    secrets: Option<Arc<dyn SecretsStore + Send + Sync>>,
    clients: McpClientsMap,
    db: Option<Arc<dyn Database>>,
}

async fn fetch_enabled_servers(
    db: Option<&Arc<dyn Database>>,
) -> Vec<crate::tools::mcp::config::McpServerConfig> {
    use crate::tools::mcp::config::load_mcp_servers_from_db;

    let servers_result = if let Some(db) = db {
        load_mcp_servers_from_db(db.as_ref(), "default").await
    } else {
        crate::tools::mcp::config::load_mcp_servers().await
    };

    match servers_result {
        Ok(s) => s.enabled_servers().cloned().collect(),
        Err(e) => {
            tracing::debug!("No MCP servers configured ({})", e);
            Vec::new()
        }
    }
}

fn is_auth_error(err_str: &str) -> bool {
    err_str.contains("401") || err_str.contains("authentication")
}

async fn register_tools_from_client(
    client: &Arc<crate::tools::mcp::McpClient>,
    tools: &Arc<ToolRegistry>,
    server_name: &str,
) {
    match client.list_tools().await {
        Ok(mcp_tools) => {
            let tool_count = mcp_tools.len();
            match client.create_tools().await {
                Ok(tool_impls) => {
                    for tool in tool_impls {
                        tools.register(tool).await;
                    }
                    tracing::debug!(
                        "Loaded {} tools from MCP server '{}'",
                        tool_count,
                        server_name
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to create tools from MCP server '{}': {}",
                        server_name,
                        e
                    );
                }
            }
        }
        Err(e) => {
            let err_str = e.to_string();
            if is_auth_error(&err_str) {
                tracing::warn!(
                    "MCP server '{}' requires authentication. \
                     Run: axinite mcp auth {}",
                    server_name,
                    server_name
                );
            } else {
                tracing::warn!("Failed to connect to MCP server '{}': {}", server_name, e);
            }
        }
    }
}

async fn spawn_server_task(
    ctx: Arc<McpLoadCtx>,
    server: crate::tools::mcp::config::McpServerConfig,
) {
    let server_name = server.name.clone();
    let client = match crate::tools::mcp::create_client_from_config(
        server,
        &ctx.session,
        &ctx.process,
        ctx.secrets.clone(),
        &ctx.user_id,
    )
    .await
    {
        Ok(c) => Arc::new(c),
        Err(e) => {
            tracing::warn!("Failed to create MCP client for '{}': {}", server_name, e);
            return;
        }
    };

    let cell = Arc::new(tokio::sync::OnceCell::new());
    let _ = cell.set(Arc::clone(&client));
    ctx.clients.write().await.insert(server_name.clone(), cell);

    register_tools_from_client(&client, &ctx.tools, &server_name).await;
}

pub(super) async fn load_and_register_mcp_servers(params: LoadMcpServersParams<'_>) {
    let LoadMcpServersParams {
        db,
        secrets_store,
        tools,
        mcp_session_manager,
        mcp_process_manager,
        mcp_clients,
    } = params;

    let ctx = Arc::new(McpLoadCtx {
        user_id: "default".to_string(),
        tools: Arc::clone(tools),
        session: Arc::clone(mcp_session_manager),
        process: Arc::clone(mcp_process_manager),
        secrets: secrets_store,
        clients: Arc::clone(&mcp_clients),
        db,
    });

    let enabled = fetch_enabled_servers(ctx.db.as_ref()).await;
    if enabled.is_empty() {
        return;
    }
    tracing::debug!("Loading {} configured MCP server(s)...", enabled.len());

    let mut join_set = tokio::task::JoinSet::new();
    for server in enabled {
        let ctx = Arc::clone(&ctx);
        join_set.spawn(async move { spawn_server_task(ctx, server).await });
    }

    while let Some(result) = join_set.join_next().await {
        if let Err(e) = result {
            tracing::warn!("MCP server loading task panicked: {}", e);
        }
    }
}
