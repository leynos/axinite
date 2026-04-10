//! Application builder for initializing core IronClaw components.
//!
//! Extracts the mechanical initialization phases from `main.rs` into a
//! reusable builder so that:
//!
//! - Tests can construct a full `AppComponents` without wiring channels
//! - Main stays focused on CLI dispatch and channel setup
//! - Each init phase is independently testable
//!
//! ## Two-phase bootstrap pattern
//!
//! This module follows a hexagonal architecture principle: **keep assembly
//! distinct from mechanism-heavy activation**. Construction of components
//! (the `AppBuilder`) is separated from fire-and-forget runtime side effects
//! (the `RuntimeSideEffects`).
//!
//! - Use `build_components()` when you need control over side-effect timing
//!   (e.g., in tests where I/O and background tasks should be avoided).
//! - Use `build_all()` as a convenience wrapper that constructs components
//!   and immediately starts side effects — suitable for production startup.
//!
//! The `RuntimeSideEffects::start()` method is fire-and-forget; callers need
//! not await unless ordering guarantees are required.

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
use crate::tools::mcp::{McpProcessManager, McpSessionManager};
use crate::tools::wasm::SharedCredentialRegistry;
use crate::tools::wasm::WasmToolRuntime;
use crate::tools::{ImageToolsRegistration, ToolRegistry, VisionToolsRegistration};
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

/// Deferred runtime side effects that should be started after component
/// construction is complete.
///
/// This struct encapsulates fire-and-forget background tasks (stale job cleanup,
/// workspace import/seeding, embedding backfill) that are activated separately
/// from pure construction. Following hexagonal architecture principles, this
/// separates assembly from activation.
pub struct RuntimeSideEffects {
    db: Option<Arc<dyn Database>>,
    workspace: Option<Arc<Workspace>>,
    workspace_import_dir: Option<std::path::PathBuf>,
    embeddings_available: bool,
}
/// Options that control optional init phases.
#[derive(Default)]
pub struct AppBuilderFlags {
    pub no_db: bool,
}

/// Builder that orchestrates the 5 mechanical init phases.
pub struct AppBuilder {
    config: Config,
    flags: AppBuilderFlags,
    toml_path: Option<std::path::PathBuf>,
    session: Arc<SessionManager>,
    log_broadcaster: Arc<LogBroadcaster>,

    // Accumulated state
    db: Option<Arc<dyn Database>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,

    // Test overrides
    llm_override: Option<Arc<dyn LlmProvider>>,

    // Backend-specific handles needed by secrets store
    handles: Option<crate::db::DatabaseHandles>,
    relay_config: Option<crate::config::RelayConfig>,
    gateway_token: Option<String>,
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

    fn register_image_tools_default(
        &self,
        tools: &ToolRegistry,
        api_base: String,
        api_key: String,
        gen_model: String,
    ) {
        tools.register_image_tools(ImageToolsRegistration::new(api_base, api_key, gen_model));
    }

    /// Phase 1: Initialize database backend.
    ///
    /// Creates the database connection, runs migrations, reloads config
    /// from DB, attaches DB to session manager, and cleans up stale jobs.
    pub async fn init_database(&mut self) -> Result<(), anyhow::Error> {
        if self.db.is_some() {
            tracing::debug!("Database already provided, skipping init_database()");
            return Ok(());
        }

        if self.flags.no_db {
            tracing::warn!("Running without database connection");
            return Ok(());
        }

        let (db, handles) = crate::db::connect_with_handles(&self.config.database)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        self.handles = Some(handles);

        // Post-init: migrate disk config, reload config from DB, attach session, cleanup
        if let Err(e) = crate::bootstrap::migrate_disk_to_db(db.as_ref(), "default").await {
            tracing::warn!("Disk-to-DB settings migration failed: {}", e);
        }

        let toml_path = self.toml_path.as_deref();
        match Config::from_db_with_toml(db.as_ref(), "default", toml_path).await {
            Ok(db_config) => {
                self.config = db_config;
                tracing::debug!("Configuration reloaded from database");
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to reload config from DB, keeping env-based config: {}",
                    e
                );
            }
        }

        self.session.attach_store(db.clone(), "default").await;

        // Note: stale job cleanup is now handled by RuntimeSideEffects::start()
        // to separate construction from activation side effects.

        self.db = Some(db);
        Ok(())
    }

    /// Phase 2: Create secrets store.
    ///
    /// Requires a master key and a backend-specific DB handle. After creating
    /// the store, injects any encrypted LLM API keys into the config overlay
    /// and re-resolves config.
    pub async fn init_secrets(&mut self) -> Result<(), anyhow::Error> {
        let master_key = match self.config.secrets.master_key() {
            Some(k) => k,
            None => {
                // No secrets DB available, but we can still load tokens from
                // OS credential stores (e.g., Anthropic OAuth via Claude Code's
                // macOS Keychain / Linux ~/.claude/.credentials.json).
                crate::config::inject_os_credentials();

                // Consume unused handles
                self.handles.take();

                // Re-resolve only the LLM config with OS credentials.
                let store: Option<&(dyn crate::db::SettingsStore + Sync)> =
                    self.db.as_ref().map(|db| db.as_ref() as _);
                let toml_path = self.toml_path.as_deref();
                if let Err(e) = self
                    .config
                    .re_resolve_llm(store, "default", toml_path)
                    .await
                {
                    tracing::warn!(
                        "Failed to re-resolve LLM config after OS credential injection: {e}"
                    );
                }

                return Ok(());
            }
        };

        let crypto = match crate::secrets::SecretsCrypto::new(master_key.clone()) {
            Ok(c) => Arc::new(c),
            Err(e) => {
                tracing::warn!("Failed to initialize secrets crypto: {}", e);
                self.handles.take();
                return Ok(());
            }
        };

        // Fallback covers the no-database path where `init_database` returned
        // early before populating `self.handles`.
        let empty_handles = crate::db::DatabaseHandles::default();
        let handles = self.handles.as_ref().unwrap_or(&empty_handles);
        let store = crate::secrets::create_secrets_store(crypto, handles);

        if let Some(ref secrets) = store {
            // Inject LLM API keys from encrypted storage
            crate::config::inject_llm_keys_from_secrets(secrets.as_ref(), "default").await;

            // Re-resolve only the LLM config with newly available keys.
            let store: Option<&(dyn crate::db::SettingsStore + Sync)> =
                self.db.as_ref().map(|db| db.as_ref() as _);
            let toml_path = self.toml_path.as_deref();
            if let Err(e) = self
                .config
                .re_resolve_llm(store, "default", toml_path)
                .await
            {
                tracing::warn!("Failed to re-resolve LLM config after secret injection: {e}");
            }
        }

        self.secrets_store = store;
        Ok(())
    }

    /// Phase 3: Initialize LLM provider chain.
    ///
    /// Delegates to `build_provider_chain` which applies all decorators
    /// (retry, smart routing, failover, circuit breaker, response cache).
    pub async fn init_llm(
        &self,
    ) -> Result<
        (
            Arc<dyn LlmProvider>,
            Option<Arc<dyn LlmProvider>>,
            Option<Arc<RecordingLlm>>,
        ),
        anyhow::Error,
    > {
        let (llm, cheap_llm, recording_handle) =
            crate::llm::build_provider_chain(&self.config.llm, self.session.clone()).await?;
        Ok((llm, cheap_llm, recording_handle))
    }

    /// Phase 4: Initialize safety, tools, embeddings, and workspace.
    pub async fn init_tools(
        &self,
        llm: &Arc<dyn LlmProvider>,
    ) -> Result<
        (
            Arc<SafetyLayer>,
            Arc<ToolRegistry>,
            Option<Arc<dyn EmbeddingProvider>>,
            Option<Arc<Workspace>>,
        ),
        anyhow::Error,
    > {
        let safety = Arc::new(SafetyLayer::new(&self.config.safety));
        tracing::debug!("Safety layer initialized");

        // Initialize tool registry with credential injection support
        let credential_registry = Arc::new(SharedCredentialRegistry::new());
        let tools = if let Some(ref ss) = self.secrets_store {
            Arc::new(
                ToolRegistry::new()
                    .with_credentials(Arc::clone(&credential_registry), Arc::clone(ss)),
            )
        } else {
            Arc::new(ToolRegistry::new())
        };
        tools.register_builtin_tools();

        if let Some(ref ss) = self.secrets_store {
            tools.register_secrets_tools(Arc::clone(ss));
        }

        // Create embeddings provider using the unified method
        let embeddings = self
            .config
            .embeddings
            .create_provider(&self.config.llm.nearai.base_url, self.session.clone());

        // Register memory tools if database is available
        let workspace = if let Some(ref db) = self.db {
            let mut ws = Workspace::new_with_db("default", db.clone());
            if let Some(ref emb) = embeddings {
                ws = ws.with_embeddings(emb.clone());
            }
            let ws = Arc::new(ws);
            tools.register_memory_tools(Arc::clone(&ws));
            Some(ws)
        } else {
            None
        };

        // Register image/vision tools if we have a workspace and LLM API credentials
        if workspace.is_some() {
            let (api_base, api_key_opt) = if let Some(ref provider) = self.config.llm.provider {
                (
                    provider.base_url.clone(),
                    provider.api_key.as_ref().map(|s| {
                        use secrecy::ExposeSecret;
                        s.expose_secret().to_string()
                    }),
                )
            } else {
                (
                    self.config.llm.nearai.base_url.clone(),
                    self.config.llm.nearai.api_key.as_ref().map(|s| {
                        use secrecy::ExposeSecret;
                        s.expose_secret().to_string()
                    }),
                )
            };

            if let Some(api_key) = api_key_opt {
                // Check for image generation models
                let model_name = self
                    .config
                    .llm
                    .provider
                    .as_ref()
                    .map(|p| p.model.clone())
                    .unwrap_or_else(|| self.config.llm.nearai.model.clone());
                let models = vec![model_name.clone()];
                let gen_model = crate::llm::image_models::suggest_image_model(&models)
                    .unwrap_or("flux-1.1-pro")
                    .to_string();
                self.register_image_tools_default(
                    &tools,
                    api_base.clone(),
                    api_key.clone(),
                    gen_model,
                );

                // Check for vision models
                let vision_model = crate::llm::vision_models::suggest_vision_model(&models)
                    .unwrap_or(&model_name)
                    .to_string();
                tools.register_vision_tools(VisionToolsRegistration {
                    api_base_url: api_base,
                    api_key,
                    vision_model,
                    base_dir: None,
                });
            }
        }

        // Register builder tool if enabled
        if self.config.builder.enabled
            && (self.config.agent.allow_local_tools || !self.config.sandbox.enabled)
        {
            tools
                .register_builder_tool(llm.clone(), Some(self.config.builder.to_builder_config()))
                .await?;
            tracing::debug!("Builder mode enabled");
        }

        Ok((safety, tools, embeddings, workspace))
    }

    /// Phase 6: Initialise the skills system.
    pub async fn init_skills(
        &self,
        tools: &Arc<ToolRegistry>,
    ) -> Result<
        (
            Option<Arc<std::sync::RwLock<SkillRegistry>>>,
            Option<Arc<SkillCatalog>>,
        ),
        anyhow::Error,
    > {
        if !self.config.skills.enabled {
            return Ok((None, None));
        }

        let mut registry = SkillRegistry::new(self.config.skills.local_dir.clone())
            .with_installed_dir(self.config.skills.installed_dir.clone());
        let loaded = registry.discover_all().await;
        if !loaded.is_empty() {
            tracing::debug!("Loaded {} skill(s): {}", loaded.len(), loaded.join(", "));
        }
        let registry = Arc::new(std::sync::RwLock::new(registry));
        let catalog = crate::skills::catalog::shared_catalog();
        tools.register_skill_tools(Arc::clone(&registry), Arc::clone(&catalog));
        Ok((Some(registry), Some(catalog)))
    }

    /// Phase 5: Load WASM tools, MCP servers, and create extension manager.
    pub async fn init_extensions(
        &self,
        tools: &Arc<ToolRegistry>,
        hooks: &Arc<HookRegistry>,
    ) -> Result<
        (
            Arc<McpSessionManager>,
            Arc<McpProcessManager>,
            Option<Arc<WasmToolRuntime>>,
            Option<Arc<ExtensionManager>>,
            Vec<crate::extensions::RegistryEntry>,
            Vec<String>,
        ),
        anyhow::Error,
    > {
        crate::extensions::build_extensions(crate::extensions::BuildExtensionsParams {
            config: &self.config,
            db: self.db.clone(),
            secrets_store: self.secrets_store.clone(),
            tools,
            hooks,
            relay_config: self.relay_config.clone(),
            gateway_token: self.gateway_token.clone(),
        })
        .await
    }

    /// Resolves the LLM provider: returns the injected override if present,
    /// otherwise delegates to `init_llm()`.
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
        self.init_llm().await
    }

    /// Phase 7: Initialise runtime metering (context manager and cost guard).
    fn init_metering(
        &self,
    ) -> (
        Arc<ContextManager>,
        Arc<crate::agent::cost_guard::CostGuard>,
    ) {
        let context_manager = Arc::new(ContextManager::new(self.config.agent.max_parallel_jobs));
        let cost_guard = Arc::new(crate::agent::cost_guard::CostGuard::new(
            crate::agent::cost_guard::CostGuardConfig {
                max_cost_per_day_cents: self.config.agent.max_cost_per_day_cents,
                max_actions_per_hour: self.config.agent.max_actions_per_hour,
            },
        ));
        (context_manager, cost_guard)
    }

    /// Validates that LLM credentials are configured for non-nearai backends.
    fn validate_llm_config(&self) -> Result<(), anyhow::Error> {
        if self.config.llm.backend != "nearai" && self.config.llm.provider.is_none() {
            let backend = &self.config.llm.backend;
            anyhow::bail!(
                "LLM_BACKEND={backend} is configured but no credentials were found. \
                 Set the appropriate API key environment variable or run the setup wizard."
            );
        }
        Ok(())
    }

    /// Run all init phases in order and return the assembled components
    /// along with deferred runtime side effects.
    ///
    /// This method performs pure construction without activating background
    /// tasks or I/O-heavy operations. Call `side_effects.start().await` to
    /// activate deferred work (workspace import, seeding, embedding backfill,
    /// stale job cleanup).
    pub async fn build_components(
        mut self,
    ) -> Result<(AppComponents, RuntimeSideEffects), anyhow::Error> {
        self.init_database().await?;
        self.init_secrets().await?;
        self.validate_llm_config()?;

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
        let workspace_import_dir = std::env::var("WORKSPACE_IMPORT_DIR")
            .ok()
            .map(std::path::PathBuf::from);

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
    /// This is equivalent to calling `build_components()` followed by
    /// `side_effects.start().await`. Use `build_components()` directly when
    /// you need control over side-effect timing (e.g., in tests).
    pub async fn build_all(self) -> Result<AppComponents, anyhow::Error> {
        let (components, side_effects) = self.build_components().await?;
        side_effects.start().await;
        Ok(components)
    }
}

impl RuntimeSideEffects {
    /// Create a new `RuntimeSideEffects` instance.
    pub fn new(
        db: Option<Arc<dyn Database>>,
        workspace: Option<Arc<Workspace>>,
        workspace_import_dir: Option<std::path::PathBuf>,
        embeddings_available: bool,
    ) -> Self {
        Self {
            db,
            workspace,
            workspace_import_dir,
            embeddings_available,
        }
    }

    /// Start all deferred runtime side effects.
    ///
    /// This method is fire-and-forget; it spawns background tasks and returns
    /// immediately. Callers need not await unless ordering guarantees are required
    /// (e.g., ensuring side effects start before accepting requests).
    ///
    /// Side effects include:
    /// - Stale sandbox job cleanup (via database)
    /// - Workspace import from disk (if `WORKSPACE_IMPORT_DIR` is set)
    /// - Workspace seeding (if workspace is empty)
    /// - Embedding backfill (spawns a background task)
    pub async fn start(self) {
        // Spawn stale sandbox cleanup task if database is available.
        if let Some(db) = self.db {
            tokio::spawn(async move {
                if let Err(e) = db.cleanup_stale_sandbox_jobs().await {
                    tracing::warn!("Failed to cleanup stale sandbox jobs: {}", e);
                }
            });
        }

        // Run workspace import, seeding, and embedding backfill if workspace is available.
        if let Some(ref ws) = self.workspace {
            // Import workspace files from disk FIRST if WORKSPACE_IMPORT_DIR is set.
            // This lets Docker images / deployment scripts ship customized workspace
            // templates that override generic seeds. Only imports files that don't
            // already exist in the database — never overwrites user edits.
            if let Some(import_dir) = self.workspace_import_dir {
                match ws.import_from_directory(&import_dir).await {
                    Ok(count) if count > 0 => {
                        tracing::debug!(
                            "Imported {} workspace file(s) from {}",
                            count,
                            import_dir.display()
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(
                            "Failed to import workspace files from {}: {}",
                            import_dir.display(),
                            e
                        );
                    }
                }
            }

            // Seed workspace with default content if empty.
            match ws.seed_if_empty().await {
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("Failed to seed workspace: {}", e);
                }
            }

            // Spawn embedding backfill in background if embeddings are configured.
            if self.embeddings_available {
                let ws_bg = Arc::clone(ws);
                tokio::spawn(async move {
                    match ws_bg.backfill_embeddings().await {
                        Ok(count) if count > 0 => {
                            tracing::debug!("Backfilled embeddings for {} chunks", count);
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!("Failed to backfill embeddings: {}", e);
                        }
                    }
                });
            }
        }
    }
}
