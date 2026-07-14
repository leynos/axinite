//! The `AppBuilder` type, its construction, and top-level assembly flow.

use std::sync::Arc;

use crate::channels::web::log_layer::LogBroadcaster;
use crate::config::Config;
use crate::db::Database;
use crate::hooks::HookRegistry;
use crate::llm::{LlmProvider, RecordingLlm, SessionManager};
use crate::secrets::SecretsStore;

use super::components::AppComponents;
use super::side_effects::RuntimeSideEffects;

/// Options that control optional init phases.
#[derive(Default)]
pub struct AppBuilderFlags {
    pub no_db: bool,
    /// Workspace import directory (overrides WORKSPACE_IMPORT_DIR env var if set).
    pub workspace_import_dir: Option<std::path::PathBuf>,
}

/// Builder that orchestrates the 5 mechanical init phases.
pub struct AppBuilder {
    pub(super) config: Config,
    pub(super) flags: AppBuilderFlags,
    pub(super) toml_path: Option<std::path::PathBuf>,
    pub(super) session: Arc<SessionManager>,
    pub(super) log_broadcaster: Arc<LogBroadcaster>,

    // Accumulated state
    pub(super) db: Option<Arc<dyn Database>>,
    pub(super) secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,

    // Test overrides
    pub(super) llm_override: Option<Arc<dyn LlmProvider>>,

    // Backend-specific handles needed by secrets store
    pub(super) handles: Option<crate::db::DatabaseHandles>,
    pub(super) relay_config: Option<crate::config::RelayConfig>,
    pub(super) gateway_token: Option<String>,
}

impl AppBuilder {
    /// Create a new builder.
    ///
    /// The `session` and `log_broadcaster` are created before the builder
    /// because tracing must be initialized before any init phase runs,
    /// and the log broadcaster is part of the tracing layer.
    pub fn new(
        config: Config,
        flags: AppBuilderFlags,
        toml_path: Option<std::path::PathBuf>,
        session: Arc<SessionManager>,
        log_broadcaster: Arc<LogBroadcaster>,
    ) -> Self {
        Self {
            config,
            flags,
            toml_path,
            session,
            log_broadcaster,
            db: None,
            secrets_store: None,
            llm_override: None,
            handles: None,
            relay_config: crate::config::RelayConfig::from_env(),
            gateway_token: std::env::var("GATEWAY_AUTH_TOKEN").ok(),
        }
    }

    /// Inject a pre-created database, skipping `init_database()`.
    pub fn with_database(&mut self, db: Arc<dyn Database>) {
        self.db = Some(db);
    }

    /// Inject a pre-created LLM provider, skipping `init_llm()`.
    pub fn with_llm(&mut self, llm: Arc<dyn LlmProvider>) {
        self.llm_override = Some(llm);
    }

    /// Resolves the LLM provider: returns the injected override if present,
    /// otherwise validates credentials and delegates to `init_llm()`.
    async fn resolve_llm(
        &mut self,
    ) -> Result<
        (
            Arc<dyn LlmProvider>,
            Option<Arc<dyn LlmProvider>>,
            Option<Arc<RecordingLlm>>,
        ),
        anyhow::Error,
    > {
        if let Some(llm) = self.llm_override.take() {
            return Ok((llm, None, None));
        }
        // Validate credentials only when not using an override.
        self.validate_llm_config()?;
        self.init_llm().await
    }

    /// Run all init phases in order and return the assembled components
    /// along with deferred runtime side effects.
    ///
    /// This method performs pure construction without activating background
    /// tasks or I/O-heavy operations. Call `side_effects.start()` to
    /// activate deferred work (workspace import, seeding, embedding backfill,
    /// stale job cleanup).
    pub async fn build_components(
        mut self,
    ) -> Result<(AppComponents, RuntimeSideEffects), anyhow::Error> {
        self.init_database().await?;
        self.init_secrets().await?;

        let (llm, cheap_llm, recording_handle) = self.resolve_llm().await?;
        let (safety, tools, embeddings, workspace) = self.init_tools(&llm).await?;

        // Create hook registry early so runtime extension activation can register hooks.
        let hooks = Arc::new(HookRegistry::new());

        let (
            mcp_session_manager,
            mcp_process_manager,
            wasm_tool_runtime,
            extension_manager,
            catalog_entries,
            dev_loaded_tool_names,
        ) = self.init_extensions(&tools, &hooks).await?;

        // Capture workspace import directory for deferred side effects.
        let workspace_import_dir = self.flags.workspace_import_dir.clone();

        // Skills system
        let (skill_registry, skill_catalog) = self.init_skills(&tools).await?;

        let (context_manager, cost_guard) = self.init_metering();

        tracing::debug!(
            "Tool registry initialized with {} total tools",
            tools.count()
        );

        let embeddings_available = embeddings.is_some();
        let components = AppComponents {
            config: self.config,
            db: self.db.clone(),
            secrets_store: self.secrets_store,
            llm,
            cheap_llm,
            safety,
            tools,
            embeddings,
            workspace: workspace.clone(),
            extension_manager,
            mcp_session_manager,
            mcp_process_manager,
            wasm_tool_runtime,
            log_broadcaster: self.log_broadcaster,
            context_manager,
            hooks,
            skill_registry,
            skill_catalog,
            cost_guard,
            recording_handle,
            session: self.session,
            catalog_entries,
            dev_loaded_tool_names,
        };

        let side_effects = RuntimeSideEffects::new(
            self.db,
            workspace,
            workspace_import_dir,
            embeddings_available,
        );

        Ok((components, side_effects))
    }

    /// Convenience wrapper that builds components and immediately starts
    /// runtime side effects.
    ///
    /// This is equivalent to calling `build_components()`, then
    /// `side_effects.start()`, then awaiting workspace bootstrap completion.
    /// Use `build_components()` directly when you need control over
    /// side-effect timing (e.g., in tests).
    pub async fn build_all(self) -> Result<AppComponents, anyhow::Error> {
        let (components, side_effects) = self.build_components().await?;
        side_effects.start()?.wait_until_bootstrapped().await?;
        Ok(components)
    }
}
