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
    pub db: Option<Arc<dyn Database>>,
    pub secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    pub llm: Arc<dyn LlmProvider>,
    pub cheap_llm: Option<Arc<dyn LlmProvider>>,
    pub safety: Arc<SafetyLayer>,
    pub tools: Arc<ToolRegistry>,
    pub embeddings: Option<Arc<dyn EmbeddingProvider>>,
    pub workspace: Option<Arc<Workspace>>,
    pub extension_manager: Option<Arc<ExtensionManager>>,
    pub mcp_session_manager: Arc<McpSessionManager>,
    pub mcp_process_manager: Arc<McpProcessManager>,
    pub wasm_tool_runtime: Option<Arc<WasmToolRuntime>>,
    pub log_broadcaster: Arc<LogBroadcaster>,
    pub context_manager: Arc<ContextManager>,
    pub hooks: Arc<HookRegistry>,
    pub skill_registry: Option<Arc<std::sync::RwLock<SkillRegistry>>>,
    pub skill_catalog: Option<Arc<SkillCatalog>>,
    pub cost_guard: Arc<crate::agent::cost_guard::CostGuard>,
    pub recording_handle: Option<Arc<RecordingLlm>>,
    pub session: Arc<SessionManager>,
    pub catalog_entries: Vec<crate::extensions::RegistryEntry>,
    pub dev_loaded_tool_names: Vec<String>,
}
