//! `Tool` wrapper that exposes a remote MCP tool through the tool registry.

use std::sync::Arc;

use crate::context::JobContext;
use crate::tools::mcp::protocol::McpTool;
use crate::tools::tool::{
    ApprovalRequirement, HostedToolCatalogSource, HostedToolEligibility, NativeTool, ToolError,
    ToolOutput,
};

use super::core::McpClient;

/// Wrapper that implements Tool for an MCP tool.
pub(in crate::tools::mcp) struct McpToolWrapper {
    pub(in crate::tools::mcp) tool: McpTool,
    pub(in crate::tools::mcp) prefixed_name: String,
    pub(in crate::tools::mcp) client: Arc<McpClient>,
}

impl NativeTool for McpToolWrapper {
    fn name(&self) -> &str {
        &self.prefixed_name
    }
    fn description(&self) -> &str {
        &self.tool.description
    }
    fn parameters_schema(&self) -> serde_json::Value {
        self.tool.input_schema.clone()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        // Strip top-level null values before forwarding — LLMs often emit
        // `"field": null` for optional params, but many MCP servers reject
        // explicit nulls for fields that should simply be absent.
        let params = strip_top_level_nulls(params);

        let result = self.client.call_tool(&self.tool.name, params).await?;
        let content: String = result
            .content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("\n");
        if result.is_error {
            return Err(ToolError::ExecutionFailed(content));
        }
        Ok(ToolOutput::text(content, start.elapsed()))
    }

    fn requires_sanitization(&self) -> bool {
        true
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        if self.tool.requires_approval() {
            ApprovalRequirement::UnlessAutoApproved
        } else {
            ApprovalRequirement::Never
        }
    }

    fn hosted_tool_eligibility(&self) -> HostedToolEligibility {
        if self.tool.requires_approval() {
            HostedToolEligibility::ApprovalGated
        } else {
            HostedToolEligibility::Eligible
        }
    }

    fn hosted_tool_catalog_source(&self) -> Option<HostedToolCatalogSource> {
        Some(HostedToolCatalogSource::Mcp)
    }
}

/// Remove top-level keys whose value is JSON null from an object.
///
/// LLMs frequently emit `"field": null` for optional parameters.  Many MCP
/// servers (e.g. Notion) treat an explicit `null` as an invalid value for
/// optional fields that should simply be absent.  Stripping these before
/// forwarding avoids 400-class rejections from strict servers.
pub(super) fn strip_top_level_nulls(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let filtered = map.into_iter().filter(|(_, v)| !v.is_null()).collect();
            serde_json::Value::Object(filtered)
        }
        other => other,
    }
}
