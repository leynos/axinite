//! Agent-callable tools for managing extensions (MCP servers and WASM tools).
//!
//! These six tools let the LLM search, install, authenticate, activate, list,
//! and remove extensions entirely through conversation.

use std::sync::Arc;

use async_trait::async_trait;

use crate::context::JobContext;
use crate::extensions::{ExtensionKind, ExtensionManager};
use crate::tools::tool::{ApprovalRequirement, Tool, ToolError, ToolOutput, require_str};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExtensionToolKind {
    Search,
    Install,
    Auth,
    Activate,
    List,
    Remove,
    Upgrade,
    Info,
}

impl ExtensionToolKind {
    pub const ALL: [Self; 8] = [
        Self::Search,
        Self::Install,
        Self::Auth,
        Self::Activate,
        Self::List,
        Self::Remove,
        Self::Upgrade,
        Self::Info,
    ];

    pub const HOSTED_WORKER_PROXY_SAFE: [Self; 4] =
        [Self::Search, Self::Activate, Self::List, Self::Info];

    pub fn name(self) -> &'static str {
        match self {
            Self::Search => "tool_search",
            Self::Install => "tool_install",
            Self::Auth => "tool_auth",
            Self::Activate => "tool_activate",
            Self::List => "tool_list",
            Self::Remove => "tool_remove",
            Self::Upgrade => "tool_upgrade",
            Self::Info => "extension_info",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Search => {
                "Search for available extensions to add new capabilities. Extensions include \
                 channels (Telegram, Slack, Discord — for messaging), tools, and MCP servers. \
                 Use discover:true to search online if the built-in registry has no results."
            }
            Self::Install => {
                "Install an extension (channel, tool, or MCP server). \
                 Use the name from tool_search results, or provide an explicit URL."
            }
            Self::Auth => {
                "Initiate authentication for an extension. For OAuth, returns a URL. \
                 For manual auth, returns instructions. The user provides their token \
                 through a secure channel, never through this tool."
            }
            Self::Activate => {
                "Activate an installed extension — starts channels, loads tools, or connects to MCP servers."
            }
            Self::List => {
                "List extensions with their authentication and activation status. \
                 Set include_available:true to also show registry entries not yet installed."
            }
            Self::Remove => {
                "Permanently remove an installed extension (channel, tool, or MCP server) from disk. \
                 This action cannot be undone - the WASM binary and configuration files will be deleted."
            }
            Self::Upgrade => {
                "Upgrade installed WASM extensions (channels and tools) to match the current \
                 host WIT version. If name is omitted, checks and upgrades all installed WASM \
                 extensions. Authentication and secrets are preserved."
            }
            Self::Info => {
                "Show detailed information about an installed extension, including version \
                 and WIT version compatibility."
            }
        }
    }

    pub fn parameters_schema(self) -> serde_json::Value {
        match self {
            Self::Search => serde_json::json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "Search query (name, keyword, or description fragment)"
                    },
                    "discover": {
                        "type": "boolean",
                        "description": "If true, also search online (slower, 5-15s). Try without first.",
                        "default": false
                    }
                },
                "required": ["query"]
            }),
            Self::Install => serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Extension name (from search results or custom)"
                    },
                    "url": {
                        "type": "string",
                        "description": "Explicit URL (for extensions not in the registry)"
                    },
                    "kind": {
                        "type": "string",
                        "enum": ["mcp_server", "wasm_tool", "wasm_channel"],
                        "description": "Extension type (auto-detected if omitted)"
                    }
                },
                "required": ["name"]
            }),
            Self::Auth | Self::Activate | Self::Remove | Self::Info => serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": match self {
                            Self::Auth => "Extension name to authenticate",
                            Self::Activate => "Extension name to activate",
                            Self::Remove => "Extension name to remove",
                            Self::Info => "Extension name to get info about",
                            _ => unreachable!(),
                        }
                    }
                },
                "required": ["name"]
            }),
            Self::List => serde_json::json!({
                "type": "object",
                "properties": {
                    "kind": {
                        "type": "string",
                        "enum": ["mcp_server", "wasm_tool", "wasm_channel"],
                        "description": "Filter by extension type (omit to list all)"
                    },
                    "include_available": {
                        "type": "boolean",
                        "description": "If true, also include registry entries that are not yet installed",
                        "default": false
                    }
                }
            }),
            Self::Upgrade => serde_json::json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "Extension name to upgrade (omit to upgrade all)"
                    }
                }
            }),
        }
    }

    pub fn approval_requirement(self) -> ApprovalRequirement {
        match self {
            Self::Search | Self::Activate | Self::List | Self::Info => ApprovalRequirement::Never,
            Self::Install | Self::Auth | Self::Upgrade => ApprovalRequirement::UnlessAutoApproved,
            Self::Remove => ApprovalRequirement::Always,
        }
    }
}

// ── tool_search ──────────────────────────────────────────────────────────

pub struct ToolSearchTool {
    manager: Arc<ExtensionManager>,
}

impl ToolSearchTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &str {
        ExtensionToolKind::Search.name()
    }

    fn description(&self) -> &str {
        ExtensionToolKind::Search.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        ExtensionToolKind::Search.parameters_schema()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let query = params.get("query").and_then(|v| v.as_str()).unwrap_or("");
        let discover = params
            .get("discover")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let results = self
            .manager
            .search(query, discover)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let output = serde_json::json!({
            "results": results,
            "count": results.len(),
            "searched_online": discover,
        });

        Ok(ToolOutput::success(output, start.elapsed()))
    }
}

// ── tool_install ─────────────────────────────────────────────────────────

pub struct ToolInstallTool {
    manager: Arc<ExtensionManager>,
}

impl ToolInstallTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for ToolInstallTool {
    fn name(&self) -> &str {
        ExtensionToolKind::Install.name()
    }

    fn description(&self) -> &str {
        ExtensionToolKind::Install.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        ExtensionToolKind::Install.parameters_schema()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = require_str(&params, "name")?;

        let url = params.get("url").and_then(|v| v.as_str());

        let kind_hint = params
            .get("kind")
            .and_then(|v| v.as_str())
            .and_then(|k| match k {
                "mcp_server" => Some(ExtensionKind::McpServer),
                "wasm_tool" => Some(ExtensionKind::WasmTool),
                "wasm_channel" => Some(ExtensionKind::WasmChannel),
                _ => None,
            });

        let result = self
            .manager
            .install(name, url, kind_hint)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let output = serde_json::to_value(&result)
            .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));

        Ok(ToolOutput::success(output, start.elapsed()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ExtensionToolKind::Install.approval_requirement()
    }
}

// ── tool_auth ────────────────────────────────────────────────────────────

pub struct ToolAuthTool {
    manager: Arc<ExtensionManager>,
}

impl ToolAuthTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for ToolAuthTool {
    fn name(&self) -> &str {
        ExtensionToolKind::Auth.name()
    }

    fn description(&self) -> &str {
        ExtensionToolKind::Auth.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        ExtensionToolKind::Auth.parameters_schema()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = require_str(&params, "name")?;

        let result = self
            .manager
            .auth(name, None)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        // Auto-activate after successful auth so tools are available immediately
        if result.is_authenticated() {
            match self.manager.activate(name).await {
                Ok(activate_result) => {
                    let output = serde_json::json!({
                        "status": "authenticated_and_activated",
                        "name": name,
                        "tools_loaded": activate_result.tools_loaded,
                        "message": activate_result.message,
                    });
                    return Ok(ToolOutput::success(output, start.elapsed()));
                }
                Err(e) => {
                    tracing::warn!(
                        "Extension '{}' authenticated but activation failed: {}",
                        name,
                        e
                    );
                    let output = serde_json::json!({
                        "status": "authenticated",
                        "name": name,
                        "activation_error": e.to_string(),
                        "message": format!(
                            "Authenticated but activation failed: {}. Try tool_activate.",
                            e
                        ),
                    });
                    return Ok(ToolOutput::success(output, start.elapsed()));
                }
            }
        }

        let output = serde_json::to_value(&result)
            .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));

        Ok(ToolOutput::success(output, start.elapsed()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ExtensionToolKind::Auth.approval_requirement()
    }
}

// ── tool_activate ────────────────────────────────────────────────────────

pub struct ToolActivateTool {
    manager: Arc<ExtensionManager>,
}

impl ToolActivateTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for ToolActivateTool {
    fn name(&self) -> &str {
        ExtensionToolKind::Activate.name()
    }

    fn description(&self) -> &str {
        ExtensionToolKind::Activate.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        ExtensionToolKind::Activate.parameters_schema()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = require_str(&params, "name")?;

        match self.manager.activate(name).await {
            Ok(result) => {
                let output = serde_json::to_value(&result)
                    .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));
                Ok(ToolOutput::success(output, start.elapsed()))
            }
            Err(activate_err) => {
                let err_str = activate_err.to_string();
                let needs_auth = err_str.contains("authentication")
                    || err_str.contains("401")
                    || err_str.contains("Unauthorized")
                    || err_str.contains("not authenticated");

                if !needs_auth {
                    return Err(ToolError::ExecutionFailed(err_str));
                }

                // Activation failed due to missing auth; initiate auth flow
                // so the agent loop can show the auth card.
                match self.manager.auth(name, None).await {
                    Ok(auth_result) if auth_result.is_authenticated() => {
                        // Auth succeeded (e.g. env var was set); retry activation.
                        let result = self
                            .manager
                            .activate(name)
                            .await
                            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                        let output = serde_json::to_value(&result).unwrap_or_else(
                            |_| serde_json::json!({"error": "serialization failed"}),
                        );
                        Ok(ToolOutput::success(output, start.elapsed()))
                    }
                    Ok(auth_result) => {
                        // Auth needs user input (awaiting_token). Return the auth
                        // result so detect_auth_awaiting picks it up.
                        let output = serde_json::to_value(&auth_result).unwrap_or_else(
                            |_| serde_json::json!({"error": "serialization failed"}),
                        );
                        Ok(ToolOutput::success(output, start.elapsed()))
                    }
                    Err(auth_err) => Err(ToolError::ExecutionFailed(format!(
                        "Activation failed ({}), and authentication also failed: {}",
                        err_str, auth_err
                    ))),
                }
            }
        }
    }
}

// ── tool_list ────────────────────────────────────────────────────────────

pub struct ToolListTool {
    manager: Arc<ExtensionManager>,
}

impl ToolListTool {
    pub fn new(manager: Arc<ExtensionManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl Tool for ToolListTool {
    fn name(&self) -> &str {
        ExtensionToolKind::List.name()
    }

    fn description(&self) -> &str {
        ExtensionToolKind::List.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        ExtensionToolKind::List.parameters_schema()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let kind_filter = params
            .get("kind")
            .and_then(|v| v.as_str())
            .and_then(|k| match k {
                "mcp_server" => Some(ExtensionKind::McpServer),
                "wasm_tool" => Some(ExtensionKind::WasmTool),
                "wasm_channel" => Some(ExtensionKind::WasmChannel),
                _ => None,
            });

        let include_available = params
            .get("include_available")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

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

#[async_trait]
impl Tool for ToolRemoveTool {
    fn name(&self) -> &str {
        ExtensionToolKind::Remove.name()
    }

    fn description(&self) -> &str {
        ExtensionToolKind::Remove.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        ExtensionToolKind::Remove.parameters_schema()
    }

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

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ExtensionToolKind::Remove.approval_requirement()
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

#[async_trait]
impl Tool for ToolUpgradeTool {
    fn name(&self) -> &str {
        ExtensionToolKind::Upgrade.name()
    }

    fn description(&self) -> &str {
        ExtensionToolKind::Upgrade.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        ExtensionToolKind::Upgrade.parameters_schema()
    }

    async fn execute(
        &self,
        params: serde_json::Value,
        _ctx: &JobContext,
    ) -> Result<ToolOutput, ToolError> {
        let start = std::time::Instant::now();

        let name = params.get("name").and_then(|v| v.as_str());

        let result = self
            .manager
            .upgrade(name)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let output = serde_json::to_value(&result)
            .unwrap_or_else(|_| serde_json::json!({"error": "serialization failed"}));

        Ok(ToolOutput::success(output, start.elapsed()))
    }

    fn requires_approval(&self, _params: &serde_json::Value) -> ApprovalRequirement {
        ExtensionToolKind::Upgrade.approval_requirement()
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

#[async_trait]
impl Tool for ExtensionInfoTool {
    fn name(&self) -> &str {
        ExtensionToolKind::Info.name()
    }

    fn description(&self) -> &str {
        ExtensionToolKind::Info.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        ExtensionToolKind::Info.parameters_schema()
    }

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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use crate::context::JobContext;
    use crate::extensions::{AuthHint, ExtensionSource, RegistryEntry};
    use crate::tools::tool::ToolError;

    #[test]
    fn test_tool_search_schema() {
        let tool = ToolSearchTool {
            manager: test_manager_stub(),
        };
        assert_eq!(tool.name(), "tool_search");
        let schema = tool.parameters_schema();
        assert!(schema.get("properties").is_some());
        assert!(schema["properties"].get("query").is_some());
    }

    #[test]
    fn test_tool_install_schema() {
        use crate::tools::tool::ApprovalRequirement;
        let tool = ToolInstallTool {
            manager: test_manager_stub(),
        };
        assert_eq!(tool.name(), "tool_install");
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::UnlessAutoApproved
        );
        let schema = tool.parameters_schema();
        assert!(schema["properties"].get("name").is_some());
        assert!(schema["properties"].get("url").is_some());
    }

    #[test]
    fn test_tool_auth_schema() {
        use crate::tools::tool::ApprovalRequirement;
        let tool = ToolAuthTool {
            manager: test_manager_stub(),
        };
        assert_eq!(tool.name(), "tool_auth");
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::UnlessAutoApproved
        );
        let schema = tool.parameters_schema();
        assert!(schema["properties"].get("name").is_some());
        assert!(
            schema["properties"].get("token").is_none(),
            "tool_auth must not have a token parameter"
        );
    }

    #[test]
    fn test_tool_activate_schema() {
        use crate::tools::tool::ApprovalRequirement;
        let tool = ToolActivateTool {
            manager: test_manager_stub(),
        };
        assert_eq!(tool.name(), "tool_activate");
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::Never
        );
    }

    #[test]
    fn test_tool_list_schema() {
        use crate::tools::tool::ApprovalRequirement;
        let tool = ToolListTool {
            manager: test_manager_stub(),
        };
        assert_eq!(tool.name(), "tool_list");
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::Never
        );
        let schema = tool.parameters_schema();
        assert!(schema["properties"].get("kind").is_some());
    }

    #[test]
    fn test_tool_remove_schema() {
        use crate::tools::tool::ApprovalRequirement;
        let tool = ToolRemoveTool {
            manager: test_manager_stub(),
        };
        assert_eq!(tool.name(), "tool_remove");
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::Always
        );
    }

    #[test]
    fn tool_remove_always_requires_approval_regardless_of_params() {
        use crate::tools::tool::ApprovalRequirement;
        let tool = ToolRemoveTool {
            manager: test_manager_stub(),
        };

        let test_cases = vec![
            ("no params", serde_json::json!({})),
            ("empty name", serde_json::json!({"name": ""})),
            ("slack", serde_json::json!({"name": "slack"})),
            ("github-cli", serde_json::json!({"name": "github-cli"})),
            (
                "with extra fields",
                serde_json::json!({"name": "tool", "extra": "field"}),
            ),
        ];

        for (case_name, params) in test_cases {
            assert_eq!(
                tool.requires_approval(&params),
                ApprovalRequirement::Always,
                "tool_remove must always require approval for case: {}",
                case_name
            );
        }
    }

    #[test]
    fn test_tool_upgrade_schema() {
        use crate::tools::tool::ApprovalRequirement;
        let tool = ToolUpgradeTool {
            manager: test_manager_stub(),
        };
        assert_eq!(tool.name(), "tool_upgrade");
        assert_eq!(
            tool.requires_approval(&serde_json::json!({})),
            ApprovalRequirement::UnlessAutoApproved
        );
        let schema = tool.parameters_schema();
        assert!(schema["properties"].get("name").is_some());
        assert!(
            schema.get("required").is_none(),
            "tool_upgrade should have no required params"
        );
    }

    #[test]
    fn test_extension_info_schema() {
        let tool = ExtensionInfoTool {
            manager: test_manager_stub(),
        };
        assert_eq!(tool.name(), "extension_info");
        let schema = tool.parameters_schema();
        assert!(schema["properties"].get("name").is_some());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("name")));
    }

    #[tokio::test]
    async fn test_tool_search_execute_returns_registry_results() {
        let (_temp_dir, manager) = test_manager_with_catalog(vec![test_registry_entry(
            "quasar-sync",
            ExtensionKind::WasmTool,
            vec!["quasar".into(), "sync".into()],
        )]);
        let tool = ToolSearchTool::new(manager);
        let ctx = JobContext::with_user("test", "chat", "test-session");

        let output = tool
            .execute(serde_json::json!({ "query": "quasar" }), &ctx)
            .await
            .expect("search should succeed");

        assert_eq!(output.result["searched_online"], serde_json::json!(false));
        let results = output.result["results"]
            .as_array()
            .expect("results should be an array");
        assert!(
            results.iter().any(|entry| {
                entry.get("name").and_then(|value| value.as_str()) == Some("quasar-sync")
                    && entry.get("source").and_then(|value| value.as_str()) == Some("registry")
            }),
            "expected quasar-sync registry result, got: {results:?}"
        );
    }

    #[tokio::test]
    async fn test_tool_list_execute_includes_available_registry_entries() {
        let (_temp_dir, manager) = test_manager_with_catalog(vec![test_registry_entry(
            "quasar-sync",
            ExtensionKind::WasmTool,
            vec!["quasar".into(), "sync".into()],
        )]);
        let tool = ToolListTool::new(manager);
        let ctx = JobContext::with_user("test", "chat", "test-session");

        let output = tool
            .execute(
                serde_json::json!({
                    "kind": "wasm_tool",
                    "include_available": true
                }),
                &ctx,
            )
            .await
            .expect("list should succeed");

        let extensions = output.result["extensions"]
            .as_array()
            .expect("extensions should be an array");
        let entry = extensions
            .iter()
            .find(|extension| {
                extension.get("name").and_then(|value| value.as_str()) == Some("quasar-sync")
            })
            .expect("quasar-sync should be included");

        assert_eq!(entry["kind"], serde_json::json!("wasm_tool"));
        assert_eq!(entry["installed"], serde_json::json!(false));
        assert_eq!(entry["display_name"], serde_json::json!("Quasar Sync"));
        assert_eq!(output.result["count"], serde_json::json!(extensions.len()));
    }

    #[tokio::test]
    async fn test_tool_auth_execute_requires_name_param() {
        let tool = ToolAuthTool::new(test_manager_stub());
        let ctx = JobContext::with_user("test", "chat", "test-session");

        let err = tool
            .execute(serde_json::json!({}), &ctx)
            .await
            .expect_err("tool_auth should reject missing name");

        assert!(matches!(err, ToolError::InvalidParameters(_)));
    }

    #[tokio::test]
    async fn test_tool_activate_execute_requires_name_param() {
        let tool = ToolActivateTool::new(test_manager_stub());
        let ctx = JobContext::with_user("test", "chat", "test-session");

        let err = tool
            .execute(serde_json::json!({}), &ctx)
            .await
            .expect_err("tool_activate should reject missing name");

        assert!(matches!(err, ToolError::InvalidParameters(_)));
    }

    fn test_manager_stub() -> Arc<ExtensionManager> {
        test_manager_with_catalog(Vec::new()).1
    }

    fn test_manager_with_catalog(
        catalog_entries: Vec<RegistryEntry>,
    ) -> (TempDir, Arc<ExtensionManager>) {
        use crate::secrets::{InMemorySecretsStore, SecretsCrypto};
        use crate::tools::ToolRegistry;
        use crate::tools::mcp::session::McpSessionManager;

        let temp_dir = tempfile::tempdir().expect("temp dir");
        let tools_dir = temp_dir.path().join("tools");
        let channels_dir = temp_dir.path().join("channels");
        std::fs::create_dir_all(&tools_dir).expect("create tools dir");
        std::fs::create_dir_all(&channels_dir).expect("create channels dir");

        let master_key =
            secrecy::SecretString::from("0123456789abcdef0123456789abcdef".to_string());
        let crypto = Arc::new(SecretsCrypto::new(master_key).unwrap());

        (
            temp_dir,
            Arc::new(ExtensionManager::new(
                Arc::new(McpSessionManager::new()),
                Arc::new(crate::tools::mcp::process::McpProcessManager::new()),
                Arc::new(InMemorySecretsStore::new(crypto)),
                Arc::new(ToolRegistry::new()),
                None,
                None,
                tools_dir,
                channels_dir,
                None,
                "test".to_string(),
                None,
                catalog_entries,
            )),
        )
    }

    fn test_registry_entry(
        name: &str,
        kind: ExtensionKind,
        keywords: Vec<String>,
    ) -> RegistryEntry {
        RegistryEntry {
            name: name.to_string(),
            display_name: "Quasar Sync".to_string(),
            kind,
            description: "Synthetic extension used for deterministic tool tests".to_string(),
            keywords,
            source: ExtensionSource::Discovered {
                url: format!("https://example.com/{name}"),
            },
            fallback_source: None,
            auth_hint: AuthHint::None,
            version: Some("1.2.3".to_string()),
        }
    }
}
