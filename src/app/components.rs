//! Fully initialized application component bundle produced by the builder.

use std::sync::Arc;

use crate::channels::web::log_layer::LogBroadcaster;
use crate::config::Config;
use crate::context::ContextManager;
use crate::db::Database;
use crate::extensions::ExtensionManager;
use crate::hooks::HookRegistry;
use crate::llm::{LlmProvider, RecordingLlm, SessionManager};
use crate::safety::SafetyLayer;
use crate::secrets::SecretsStore;
use crate::skills::SkillRegistry;
use crate::skills::catalog::SkillCatalog;
use crate::tools::ToolRegistry;
use crate::tools::mcp::{McpProcessManager, McpSessionManager};
use crate::tools::wasm::WasmToolRuntime;
use crate::workspace::{EmbeddingProvider, Workspace};

/// Fully initialized application components, ready for channel wiring
/// and agent construction.
pub struct AppComponents {
    /// The (potentially mutated) config after DB reload and secret injection.
    pub config: Config,
    /// Database handle; `None` when no database is configured.
    pub db: Option<Arc<dyn Database>>,
    /// Encrypted secrets store; `None` when no master key or backend handle is available.
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    /// Primary LLM provider used for agent turns.
    pub llm: Arc<dyn LlmProvider>,
    /// Cheaper provider for lightweight calls; `None` falls back to `llm`.
    pub cheap_llm: Option<Arc<dyn LlmProvider>>,
    /// Safety layer that sanitizes and validates tool inputs and outputs.
    pub safety: Arc<SafetyLayer>,
    /// Shared registry of built-in, extension and dynamically loaded tools.
    pub tools: Arc<ToolRegistry>,
    /// Embedding provider for semantic workspace search; `None` when none is configured.
    pub embeddings: Option<Arc<dyn EmbeddingProvider>>,
    /// Workspace document store; `None` when no database is available.
    pub workspace: Option<Arc<Workspace>>,
    /// Extension lifecycle manager for installing, activating and removing extensions.
    pub extension_manager: Option<Arc<ExtensionManager>>,
    /// Tracks live MCP server sessions across all configured servers.
    pub mcp_session_manager: Arc<McpSessionManager>,
    /// Owns the child processes of stdio MCP servers.
    pub mcp_process_manager: Arc<McpProcessManager>,
    /// WASM tool runtime; `None` when WASM is disabled or the runtime failed to start.
    pub wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
    /// Broadcasts log records to web gateway log subscribers.
    pub log_broadcaster: Arc<LogBroadcaster>,
    /// Tracks the execution context of every concurrent job.
    pub context_manager: Arc<ContextManager>,
    /// Lifecycle hook registry; extensions register hooks into it at runtime.
    pub hooks: Arc<HookRegistry>,
    /// Installed skills behind a read-write lock; `None` when skills are disabled.
    pub skill_registry: Option<Arc<std::sync::RwLock<SkillRegistry>>>,
    /// Skill catalogue for search and install; `None` when skills are disabled.
    pub skill_catalog: Option<Arc<SkillCatalog>>,
    /// Spend and action-rate guardrails shared across the agent.
    pub cost_guard: Arc<crate::agent::cost_guard::CostGuard>,
    /// Trace recorder wrapping `llm`; `Some` only when trace recording is enabled.
    pub recording_handle: Option<Arc<RecordingLlm>>,
    /// NEAR AI session-token manager shared by the LLM providers.
    pub session: Arc<SessionManager>,
    /// Registry catalogue entries used for extension discovery and installation.
    pub catalog_entries: Vec<crate::extensions::RegistryEntry>,
    /// Names of WASM tools loaded from local build artefacts (the dev override), not the
    /// bundled set. Hook bootstrap uses them to tell the two sources apart.
    pub dev_loaded_tool_names: Vec<String>,
}
