//! Request building and response-failure handling for [`HttpMcpTransport`].

use std::collections::HashMap;

use crate::tools::mcp::protocol::McpRequest;
use crate::tools::tool::ToolError;

use super::{HttpMcpTransport, sanitize_error_body};

impl HttpMcpTransport {
    /// Build the HTTP request. Surface any deferred client construction
    /// failure as a transport error instead of a constructor panic.
    pub(super) fn build_request(
        &self,
        request: &McpRequest,
        headers: &HashMap<String, String>,
    ) -> Result<reqwest::RequestBuilder, ToolError> {
        let http_client = self.http_client.as_ref().map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "[{}] Failed to create HTTP client: {e}",
                self.server_name
            ))
        })?;
        let mut req_builder = http_client
            .post(&self.server_url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(request);

        // Apply custom headers configured on the transport, then per-request
        // headers (e.g. Authorization, Mcp-Session-Id).
        for (key, value) in self.custom_headers.iter().chain(headers) {
            req_builder = req_builder.header(key.as_str(), value.as_str());
        }
        Ok(req_builder)
    }

    /// Describe a failed send, following the error's source chain.
    pub(super) fn request_failure(&self, error: &reqwest::Error) -> ToolError {
        let mut chain = format!("[{}] MCP HTTP request failed: {}", self.server_name, error);
        let mut source = std::error::Error::source(error);
        while let Some(cause) = source {
            chain.push_str(&format!(" -> {}", cause));
            source = cause.source();
        }
        ToolError::ExternalService(chain)
    }

    /// Extract the session ID from response headers before the body is consumed.
    pub(super) async fn record_session_id(&self, response: &reqwest::Response) {
        if let Some(ref session_manager) = self.session_manager
            && let Some(session_id) = response
                .headers()
                .get("Mcp-Session-Id")
                .and_then(|v| v.to_str().ok())
        {
            session_manager
                .update_session_id(&self.server_name, Some(session_id.to_string()))
                .await;
        }
    }

    /// Build the error for a non-success response, with its sanitized body.
    pub(super) async fn status_failure(&self, response: reqwest::Response) -> ToolError {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let sanitized = sanitize_error_body(&body);
        ToolError::ExternalService(format!(
            "[{}] MCP server returned status: {} - {}",
            self.server_name, status, sanitized
        ))
    }
}
