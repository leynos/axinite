//! Request sending, authentication, token refresh, and the MCP protocol
//! operations (initialize, list tools, call tool).

use std::collections::HashMap;
use std::sync::Arc;

use crate::tools::mcp::auth::refresh_access_token;
use crate::tools::mcp::protocol::{
    CallToolResult, InitializeResult, ListToolsResult, McpRequest, McpResponse, McpTool,
};
use crate::tools::tool::{Tool, ToolError};

use super::core::McpClient;
use super::wrapper::McpToolWrapper;

impl McpClient {
    /// Get the access token for this server (if authenticated).
    async fn get_access_token(&self) -> Result<Option<String>, ToolError> {
        let Some(ref secrets) = self.secrets else {
            return Ok(None);
        };
        let Some(ref config) = self.server_config else {
            return Ok(None);
        };
        match secrets
            .get_decrypted(&self.user_id, &config.token_secret_name())
            .await
        {
            Ok(token) => Ok(Some(token.expose().to_string())),
            Err(crate::secrets::SecretError::NotFound(_)) => Ok(None),
            Err(e) => Err(ToolError::ExternalService(format!(
                "Failed to get access token: {}",
                e
            ))),
        }
    }

    /// Build the headers map for a request (auth, session-id, custom headers).
    async fn build_request_headers(&self) -> Result<HashMap<String, String>, ToolError> {
        let mut headers = self.custom_headers.clone();
        if let Some(token) = self.get_access_token().await? {
            headers.insert("Authorization".to_string(), format!("Bearer {}", token));
        }
        if let Some(ref session_manager) = self.session_manager
            && let Some(session_id) = session_manager.get_session_id(&self.server_name).await
        {
            headers.insert("Mcp-Session-Id".to_string(), session_id);
        }
        Ok(headers)
    }

    /// Send the request once with freshly built auth and session headers.
    async fn send_with_headers(&self, request: &McpRequest) -> Result<McpResponse, ToolError> {
        let headers = self.build_request_headers().await?;
        self.transport.send(request, &headers).await
    }

    /// Whether an error indicates the server rejected our credentials (401).
    fn is_unauthorized_error(error: &ToolError) -> bool {
        matches!(
            error,
            ToolError::ExternalService(msg) if msg.contains("401") || msg.contains("Unauthorized")
        )
    }

    /// Error advising the user to (re-)authenticate against this server.
    fn auth_required_error(&self) -> ToolError {
        ToolError::ExternalService(format!(
            "MCP server '{}' requires authentication. Run: axinite mcp auth {}",
            self.server_name, self.server_name
        ))
    }

    /// Send a request to the MCP server with auth and session headers.
    /// Automatically attempts token refresh on 401 errors (HTTP transports only).
    async fn send_request(&self, request: McpRequest) -> Result<McpResponse, ToolError> {
        // For non-HTTP transports, just send directly without retry logic
        if !self.transport.supports_http_features() {
            return self.send_with_headers(&request).await;
        }

        // HTTP transport: first attempt, then one retry after a token refresh.
        let result = self.send_with_headers(&request).await;
        let Err(error) = result else {
            return result;
        };
        if !Self::is_unauthorized_error(&error) {
            return Err(error);
        }
        if !self.refresh_expired_token().await {
            return Err(self.auth_required_error());
        }

        match self.send_with_headers(&request).await {
            Err(retry_error) if Self::is_unauthorized_error(&retry_error) => {
                Err(self.auth_required_error())
            }
            other => other,
        }
    }

    /// Attempt to refresh the MCP OAuth token after a 401 response.
    ///
    /// Returns `true` when the refresh succeeded and the request should be
    /// retried; returns `false` (logging any failure) when no secrets store
    /// or server config is available, or the refresh fails.
    async fn refresh_expired_token(&self) -> bool {
        let Some(ref secrets) = self.secrets else {
            return false;
        };
        let Some(ref config) = self.server_config else {
            return false;
        };
        tracing::debug!(
            "MCP token expired, attempting refresh for '{}'",
            self.server_name
        );
        match refresh_access_token(config, secrets, &self.user_id).await {
            Ok(_) => {
                tracing::info!("MCP token refreshed for '{}'", self.server_name);
                true
            }
            Err(e) => {
                tracing::debug!("Token refresh failed for '{}': {}", self.server_name, e);
                false
            }
        }
    }

    /// Initialize the connection to the MCP server.
    pub async fn initialize(&self) -> Result<InitializeResult, ToolError> {
        if let Some(ref session_manager) = self.session_manager
            && session_manager.is_initialized(&self.server_name).await
        {
            return Ok(InitializeResult::default());
        }
        if let Some(ref session_manager) = self.session_manager {
            session_manager
                .get_or_create(&self.server_name, &self.server_url)
                .await;
        }

        let request = McpRequest::initialize(self.next_request_id());
        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(ToolError::ExternalService(format!(
                "MCP initialization error: {} (code {})",
                error.message, error.code
            )));
        }

        let result: InitializeResult = response
            .result
            .ok_or_else(|| {
                ToolError::ExternalService("No result in initialize response".to_string())
            })
            .and_then(|r| {
                serde_json::from_value(r).map_err(|e| {
                    ToolError::ExternalService(format!("Invalid initialize result: {}", e))
                })
            })?;

        if let Some(ref session_manager) = self.session_manager {
            session_manager.mark_initialized(&self.server_name).await;
        }

        let notification = McpRequest::initialized_notification();
        let _ = self.send_request(notification).await;

        Ok(result)
    }

    /// List available tools from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<McpTool>, ToolError> {
        if let Some(tools) = self.tools_cache.read().await.as_ref() {
            return Ok(tools.clone());
        }
        if self.session_manager.is_some() {
            self.initialize().await?;
        }

        let request = McpRequest::list_tools(self.next_request_id());
        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(ToolError::ExternalService(format!(
                "MCP error: {} (code {})",
                error.message, error.code
            )));
        }

        let result: ListToolsResult = response
            .result
            .ok_or_else(|| ToolError::ExternalService("No result in MCP response".to_string()))
            .and_then(|r| {
                serde_json::from_value(r)
                    .map_err(|e| ToolError::ExternalService(format!("Invalid tools list: {}", e)))
            })?;

        *self.tools_cache.write().await = Some(result.tools.clone());
        Ok(result.tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &self,
        name: &str,
        arguments: serde_json::Value,
    ) -> Result<CallToolResult, ToolError> {
        if self.session_manager.is_some() {
            self.initialize().await?;
        }

        let request = McpRequest::call_tool(self.next_request_id(), name, arguments);
        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(ToolError::ExecutionFailed(format!(
                "MCP tool error: {} (code {})",
                error.message, error.code
            )));
        }

        response
            .result
            .ok_or_else(|| ToolError::ExternalService("No result in MCP response".to_string()))
            .and_then(|r| {
                serde_json::from_value(r)
                    .map_err(|e| ToolError::ExternalService(format!("Invalid tool result: {}", e)))
            })
    }

    /// Clear the tools cache.
    pub async fn clear_cache(&self) {
        *self.tools_cache.write().await = None;
    }

    /// Create Tool implementations for all MCP tools.
    pub async fn create_tools(&self) -> Result<Vec<Arc<dyn Tool>>, ToolError> {
        let mcp_tools = self.list_tools().await?;
        let client = Arc::new(self.clone());
        Ok(mcp_tools
            .into_iter()
            .map(|t| {
                let prefixed_name = format!("{}_{}", self.server_name, t.name);
                Arc::new(McpToolWrapper {
                    tool: t,
                    prefixed_name,
                    client: client.clone(),
                }) as Arc<dyn Tool>
            })
            .collect())
    }

    /// Test the connection to the MCP server.
    pub async fn test_connection(&self) -> Result<(), ToolError> {
        self.initialize().await?;
        self.list_tools().await?;
        Ok(())
    }
}
