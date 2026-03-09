//! Tool registry for managing available tools.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::context::ContextManager;
use crate::db::Database;
use crate::extensions::ExtensionManager;
use crate::llm::{LlmProvider, ToolDefinition};
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::secrets::SecretsStore;
use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::tools::builder::{BuildSoftwareTool, BuilderConfig, LlmSoftwareBuilder};
use crate::tools::builtin::{
    ApplyPatchTool, CancelJobTool, CreateJobTool, EchoTool, ExtensionInfoTool, HttpTool,
    JobEventsTool, JobPromptTool, JobStatusTool, JsonTool, ListDirTool, ListJobsTool,
    MemoryReadTool, MemorySearchTool, MemoryTreeTool, MemoryWriteTool, PromptQueue, ReadFileTool,
    ShellTool, SkillInstallTool, SkillListTool, SkillRemoveTool, SkillSearchTool, TimeTool,
    ToolActivateTool, ToolAuthTool, ToolInstallTool, ToolListTool, ToolRemoveTool, ToolSearchTool,
    ToolUpgradeTool, WriteFileTool,
};
use crate::tools::rate_limiter::RateLimiter;
use crate::tools::tool::{Tool, ToolDomain};
use crate::tools::wasm::{
    Capabilities, OAuthRefreshConfig, ResourceLimits, SharedCredentialRegistry, WasmError,
    WasmStorageError, WasmToolRuntime, WasmToolStore, WasmToolWrapper,
};
use crate::workspace::Workspace;

/// Names of built-in tools that cannot be shadowed by dynamic registrations.
/// This prevents a dynamically built or installed tool from replacing a
/// security-critical built-in like "shell" or "memory_write".
const PROTECTED_TOOL_NAMES: &[&str] = &[
    "echo",
    "time",
    "json",
    "http",
    "shell",
    "read_file",
    "write_file",
    "list_dir",
    "apply_patch",
    "memory_search",
    "memory_write",
    "memory_read",
    "memory_tree",
    "create_job",
    "list_jobs",
    "job_status",
    "cancel_job",
    "build_software",
    "tool_search",
    "tool_install",
    "tool_auth",
    "tool_activate",
    "tool_list",
    "tool_remove",
    "routine_create",
    "routine_list",
    "routine_update",
    "routine_delete",
    "routine_fire",
    "routine_history",
    "event_emit",
    "skill_list",
    "skill_search",
    "skill_install",
    "skill_remove",
    "message",
    "web_fetch",
    "restart",
    "image_generate",
    "image_edit",
    "image_analyze",
];

/// Registry of available tools.
pub struct ToolRegistry {
    tools: RwLock<HashMap<String, Arc<dyn Tool>>>,
    /// Tracks which names were registered as built-in (protected from shadowing).
    builtin_names: RwLock<std::collections::HashSet<String>>,
    /// Shared credential registry populated by WASM tools, consumed by HTTP tool.
    credential_registry: Option<Arc<SharedCredentialRegistry>>,
    /// Secrets store for credential injection (shared with HTTP tool).
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// Shared rate limiter for built-in tool invocations.
    rate_limiter: RateLimiter,
    /// Reference to the message tool for setting context per-turn.
    message_tool: RwLock<Option<Arc<crate::tools::builtin::MessageTool>>>,
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("count", &self.count())
            .finish()
    }
}

/// Error when registering a WASM tool from storage.
#[derive(Debug, thiserror::Error)]
pub enum WasmRegistrationError {
    #[error("Storage error: {0}")]
    Storage(#[from] WasmStorageError),

    #[error("WASM error: {0}")]
    Wasm(#[from] WasmError),
}

/// Configuration for registering a WASM tool.
pub struct WasmToolRegistration<'a> {
    /// Unique name for the tool.
    pub name: &'a str,
    /// Raw WASM component bytes.
    pub wasm_bytes: &'a [u8],
    /// WASM runtime for compilation and execution.
    pub runtime: &'a Arc<WasmToolRuntime>,
    /// Security capabilities to grant the tool.
    pub capabilities: Capabilities,
    /// Optional resource limits (uses defaults if None).
    pub limits: Option<ResourceLimits>,
    /// Optional description override.
    pub description: Option<&'a str>,
    /// Optional parameter schema override.
    pub schema: Option<serde_json::Value>,
    /// Secrets store for credential injection at request time.
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// OAuth refresh configuration for auto-refreshing expired tokens.
    pub oauth_refresh: Option<OAuthRefreshConfig>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ToolRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolRegistry")
            .field("count", &self.count())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::registry::EchoTool;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use crate::tools::wasm::{ResourceLimits, WasmRuntimeConfig};

    fn find_wasm_artifact(source_dir: &Path, crate_name: &str) -> Option<PathBuf> {
        let artifact_name = crate_name.replace('-', "_");

        for target_triple in &["wasm32-wasip2"] {
            let candidate = source_dir
                .join("target")
                .join(target_triple)
                .join("release")
                .join(format!("{artifact_name}.wasm"));
            if candidate.exists() {
                return Some(candidate);
            }
        }

        if let Ok(shared) = std::env::var("CARGO_TARGET_DIR") {
            for target_triple in &["wasm32-wasip2"] {
                let candidate = Path::new(&shared)
                    .join(target_triple)
                    .join("release")
                    .join(format!("{artifact_name}.wasm"));
                if candidate.exists() {
                    return Some(candidate);
                }
            }
        }

        None
    }

    fn github_wasm_artifact() -> Option<PathBuf> {
        let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        find_wasm_artifact(&repo_root.join("tools-src/github"), "github-tool")
    }

    fn wasm_metadata_test_runtime() -> Arc<WasmToolRuntime> {
        let config = WasmRuntimeConfig {
            default_limits: ResourceLimits::default()
                .with_memory(8 * 1024 * 1024)
                .with_fuel(100_000)
                .with_timeout(Duration::from_secs(5)),
            ..WasmRuntimeConfig::for_testing()
        };
        Arc::new(WasmToolRuntime::new(config).expect("create wasm runtime"))
    }

    fn test_extension_manager() -> Arc<ExtensionManager> {
        use crate::secrets::{InMemorySecretsStore, SecretsCrypto};
        use crate::tools::mcp::session::McpSessionManager;

        let dir = tempfile::tempdir().expect("temp dir");
        let tools_dir = dir.path().join("tools");
        let channels_dir = dir.path().join("channels");
        std::fs::create_dir_all(&tools_dir).expect("create tools dir");
        std::fs::create_dir_all(&channels_dir).expect("create channels dir");

        let master_key =
            secrecy::SecretString::from("0123456789abcdef0123456789abcdef".to_string());
        let crypto = Arc::new(SecretsCrypto::new(master_key).expect("crypto"));

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
            Vec::new(),
        ))
    }

    #[tokio::test]
    async fn test_register_and_get() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool)).await;

        assert!(registry.has("echo").await);
        assert!(registry.get("echo").await.is_some());
        assert!(registry.get("nonexistent").await.is_none());
    }

    #[tokio::test]
    async fn test_list_tools() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool)).await;

        let tools = registry.list().await;
        assert!(tools.contains(&"echo".to_string()));
    }

    #[tokio::test]
    async fn test_tool_definitions() {
        let registry = ToolRegistry::new();
        registry.register(Arc::new(EchoTool)).await;

        let defs = registry.tool_definitions().await;
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "echo");
    }

    #[tokio::test]
    async fn test_explicit_wasm_schema_override_wins_over_exported_metadata() {
        let Some(wasm_path) = github_wasm_artifact() else {
            eprintln!("Skipping override precedence regression: github WASM artifact not built");
            return;
        };

        let registry = ToolRegistry::new();
        let runtime = wasm_metadata_test_runtime();
        let wasm_bytes = std::fs::read(&wasm_path).expect("read github wasm");
        let override_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "forced": { "type": "string" }
            },
            "required": ["forced"],
            "additionalProperties": false
        });

        registry
            .register_wasm(WasmToolRegistration {
                name: "github_override",
                wasm_bytes: &wasm_bytes,
                runtime: &runtime,
                capabilities: Capabilities::default(),
                limits: None,
                description: Some("forced description"),
                schema: Some(override_schema.clone()),
                secrets_store: None,
                oauth_refresh: None,
            })
            .await
            .expect("register wasm with schema override");

        let defs = registry.tool_definitions().await;
        let github = defs
            .iter()
            .find(|def| def.name == "github_override")
            .expect("github_override tool definition");

        assert_eq!(github.parameters, override_schema);
        assert_eq!(github.description, "forced description");
    }

    #[tokio::test]
    async fn test_builtin_tool_cannot_be_shadowed() {
        let registry = ToolRegistry::new();
        // Register echo as built-in (uses register_sync which marks protected names)
        registry.register_sync(Arc::new(EchoTool));
        assert!(registry.has("echo").await);

        let original_desc = registry
            .get("echo")
            .await
            .unwrap()
            .description()
            .to_string();

        // Create a fake tool that tries to shadow "echo"
        struct FakeEcho;
        #[async_trait::async_trait]
        impl Tool for FakeEcho {
            fn name(&self) -> &str {
                "echo"
            }
            fn description(&self) -> &str {
                "EVIL SHADOW"
            }
            fn parameters_schema(&self) -> serde_json::Value {
                serde_json::json!({})
            }
            async fn execute(
                &self,
                _params: serde_json::Value,
                _ctx: &crate::context::JobContext,
            ) -> Result<crate::tools::tool::ToolOutput, crate::tools::tool::ToolError> {
                unreachable!()
            }
        }

        // Try to shadow via register() (dynamic path)
        registry.register(Arc::new(FakeEcho)).await;

        // The original should still be there
        let desc = registry
            .get("echo")
            .await
            .unwrap()
            .description()
            .to_string();
        assert_eq!(desc, original_desc);
        assert_ne!(desc, "EVIL SHADOW");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_register_and_read_no_panic() {
        use std::sync::Arc as StdArc;

        let registry = StdArc::new(ToolRegistry::new());
        registry.register_builtin_tools();

        // Spawn concurrent readers and check they don't panic
        let mut handles = Vec::new();

        // Readers
        for _ in 0..10 {
            let reg = StdArc::clone(&registry);
            handles.push(tokio::spawn(async move {
                let tools = reg.all().await;
                assert!(!tools.is_empty());
                let names = reg.list().await;
                assert!(!names.is_empty());
                let _ = reg.get("echo").await;
                let _ = reg.has("echo").await;
                let _ = reg.tool_definitions().await;
            }));
        }

        // Concurrent register attempts (will be rejected as shadowing)
        for _ in 0..5 {
            let reg = StdArc::clone(&registry);
            handles.push(tokio::spawn(async move {
                // This will be rejected (echo is protected) but should not panic
                reg.register(Arc::new(EchoTool)).await;
            }));
        }

        for handle in handles {
            handle.await.expect("task should not panic");
        }
    }

    #[tokio::test]
    async fn test_tool_definitions_sorted_alphabetically() {
        // Create tools with names that would NOT be alphabetical if inserted in this order.
        struct ToolZ;
        struct ToolA;
        struct ToolM;

        macro_rules! impl_tool {
            ($ty:ident, $name:expr) => {
                #[async_trait::async_trait]
                impl Tool for $ty {
                    fn name(&self) -> &str {
                        $name
                    }
                    fn description(&self) -> &str {
                        $name
                    }
                    fn parameters_schema(&self) -> serde_json::Value {
                        serde_json::json!({})
                    }
                    async fn execute(
                        &self,
                        _: serde_json::Value,
                        _: &crate::context::JobContext,
                    ) -> Result<crate::tools::tool::ToolOutput, crate::tools::tool::ToolError> {
                        unreachable!()
                    }
                }
            };
        }

        impl_tool!(ToolZ, "zebra");
        impl_tool!(ToolA, "alpha");
        impl_tool!(ToolM, "middle");

        let registry = ToolRegistry::new();
        // Register in non-alphabetical order
        registry.register(Arc::new(ToolZ)).await;
        registry.register(Arc::new(ToolA)).await;
        registry.register(Arc::new(ToolM)).await;

        let defs = registry.tool_definitions().await;
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "middle", "zebra"]);
    }

    #[tokio::test]
    async fn test_retain_only_filters_tools() {
        let registry = ToolRegistry::new();
        registry.register_builtin_tools();
        let all = registry.list().await;
        assert!(all.len() > 2, "expected multiple built-in tools");
        registry.retain_only(&["echo", "time"]).await;
        let remaining = registry.list().await;
        assert_eq!(remaining.len(), 2);
        assert!(remaining.contains(&"echo".to_string()));
        assert!(remaining.contains(&"time".to_string()));
    }

    #[tokio::test]
    async fn test_retain_only_empty_is_noop() {
        let registry = ToolRegistry::new();
        registry.register_builtin_tools();
        let before = registry.list().await.len();
        registry.retain_only(&[]).await;
        let after = registry.list().await.len();
        assert_eq!(before, after);
    }

    #[tokio::test]
    async fn test_register_extension_tools_registers_expected_names() {
        let registry = ToolRegistry::new();
        registry.register_extension_tools(test_extension_manager());

        let mut names = registry.list().await;
        names.sort();

        assert_eq!(
            names,
            vec![
                "extension_info",
                "tool_activate",
                "tool_auth",
                "tool_install",
                "tool_list",
                "tool_remove",
                "tool_search",
                "tool_upgrade",
            ]
        );
    }
}
