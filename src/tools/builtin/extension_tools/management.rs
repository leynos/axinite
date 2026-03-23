//! Extension-management tool implementations backed by `ExtensionManager`.

use super::*;

// ── tool_list ────────────────────────────────────────────────────────────

pub struct ToolListTool {
    manager: Arc<ExtensionManager>,
}

impl ToolListTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

impl NativeTool for ToolListTool {
    delegate_extension_tool_metadata!(ExtensionToolKind::List);

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let kind_filter = match params.get("kind") {
            Some(serde_json::Value::String(kind)) => Some(match kind.as_str() {
                "mcp_server" => ExtensionKind::McpServer,
                "wasm_tool" => ExtensionKind::WasmTool,
                "wasm_channel" => ExtensionKind::WasmChannel,
                other => {
                    return Err(ToolError::InvalidParameters(format!(
                        "invalid 'kind' parameter '{other}': expected mcp_server, wasm_tool, or wasm_channel"
                    )));
                }
            }),
            Some(_) => {
                return Err(ToolError::InvalidParameters(
                    "'kind' parameter must be a string".to_string(),
                ));
            }
            None => None,
        };

        let include_available = match params.get("include_available") {
            Some(serde_json::Value::Bool(include_available)) => *include_available,
            Some(_) => {
                return Err(ToolError::InvalidParameters(
                    "'include_available' parameter must be a boolean".to_string(),
                ));
            }
            None => false,
        };

        let extensions = self
            .manager
            .list(kind_filter, include_available)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let output = serde_json::json!({
            "extensions": extensions,
            "count": extensions.len(),
        });

        Ok(ToolOutput::success(output, start.elapsed()))
    }
}

// ── tool_remove ──────────────────────────────────────────────────────────

pub struct ToolRemoveTool {
    manager: Arc<ExtensionManager>,
}

impl ToolRemoveTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

impl NativeTool for ToolRemoveTool {
    delegate_extension_tool_metadata!(ExtensionToolKind::Remove);

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = require_str(&params, "name")?;

        let message = self
            .manager
            .remove(name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let output = serde_json::json!({
            "name": name,
            "message": message,
        });

        Ok(ToolOutput::success(output, start.elapsed()))
    }
}

// ── tool_upgrade ─────────────────────────────────────────────────────

pub struct ToolUpgradeTool {
    manager: Arc<ExtensionManager>,
}

impl ToolUpgradeTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

impl NativeTool for ToolUpgradeTool {
    delegate_extension_tool_metadata!(ExtensionToolKind::Upgrade);

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = match params.get("name") {
            Some(serde_json::Value::String(name)) => Some(name.as_str()),
            Some(_) => {
                return Err(ToolError::InvalidParameters(
                    "'name' parameter must be a string when provided".to_string(),
                ));
            }
            None => None,
        };

        let result = self
            .manager
            .upgrade(name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let output = serde_json::to_value(&result).map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to serialise upgrade result: {e}"))
        })?;

        Ok(ToolOutput::success(output, start.elapsed()))
    }
}

// ── extension_info ────────────────────────────────────────────────────

pub struct ExtensionInfoTool {
    manager: Arc<ExtensionManager>,
}

impl ExtensionInfoTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

impl NativeTool for ExtensionInfoTool {
    delegate_extension_tool_metadata!(ExtensionToolKind::Info);

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = require_str(&params, "name")?;

        let info = self
            .manager
            .extension_info(name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(ToolOutput::success(info, start.elapsed()))
    }
}
