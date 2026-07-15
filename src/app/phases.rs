//! The mechanical init phases of `AppBuilder`: database, secrets, LLM,
//! tools, skills, extensions, and runtime metering.

use std::sync::Arc;

use crate::config::Config;
use crate::context::ContextManager;
use crate::extensions::ExtensionManager;
use crate::hooks::HookRegistry;
use crate::llm::{LlmProvider, RecordingLlm};
use crate::safety::SafetyLayer;
use crate::skills::SkillRegistry;
use crate::skills::catalog::SkillCatalog;
use crate::tools::mcp::{McpProcessManager, McpSessionManager};
use crate::tools::wasm::SharedCredentialRegistry;
use crate::tools::wasm::WasmToolRuntime;
use crate::tools::{ImageToolsRegistration, ToolRegistry, VisionToolsRegistration};
use crate::workspace::{EmbeddingProvider, Workspace};

use super::builder::AppBuilder;

impl AppBuilder {
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

    /// Re-resolve only the LLM section of the config, logging (not
    /// propagating) failures with the supplied context label.
    async fn re_resolve_llm_config(&mut self, context: &str) {
        let store: Option<&(dyn crate::db::SettingsStore + Sync)> =
            self.db.as_ref().map(|db| db.as_ref() as _);
        let toml_path = self.toml_path.as_deref();
        if let Err(e) = self
            .config
            .re_resolve_llm(store, "default", toml_path)
            .await
        {
            tracing::warn!("Failed to re-resolve LLM config after {context}: {e}");
        }
    }

    /// Fallback for `init_secrets` when no master key is configured: loads
    /// tokens from OS credential stores (e.g., Anthropic OAuth via Claude
    /// Code's macOS Keychain / Linux ~/.claude/.credentials.json) instead.
    async fn init_secrets_from_os_credentials(&mut self) {
        crate::config::inject_os_credentials();

        // Consume unused handles
        self.handles.take();

        // Re-resolve only the LLM config with OS credentials.
        self.re_resolve_llm_config("OS credential injection").await;
    }

    /// Phase 2: Create secrets store.
    ///
    /// Requires a master key and a backend-specific DB handle. After creating
    /// the store, injects any encrypted LLM API keys into the config overlay
    /// and re-resolves config.
    pub async fn init_secrets(&mut self) -> Result<(), anyhow::Error> {
        let Some(master_key) = self.config.secrets.master_key() else {
            self.init_secrets_from_os_credentials().await;
            return Ok(());
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
            crate::config::inject_llm_keys_from_secrets(secrets.as_ref(), "default").await?;

            // Re-resolve only the LLM config with newly available keys.
            self.re_resolve_llm_config("secret injection").await;
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

        let tools = self.build_tool_registry()?;

        // Create embeddings provider using the unified method
        let embeddings = self
            .config
            .embeddings
            .create_provider(&self.config.llm.nearai.base_url, self.session.clone());

        let workspace = self.init_workspace(&tools, &embeddings);

        // Register image/vision tools if we have a workspace and LLM API credentials
        if workspace.is_some() {
            self.register_media_tools(&tools);
        }

        // Register builder tool if enabled
        if self.builder_tool_permitted() {
            tools
                .register_builder_tool(llm.clone(), Some(self.config.builder.to_builder_config()))
                .await?;
            tracing::debug!("Builder mode enabled");
        }

        Ok((safety, tools, embeddings, workspace))
    }

    /// Create the tool registry with credential injection support, register
    /// the built-in tools, and (when a secrets store exists) secrets tools.
    fn build_tool_registry(&self) -> Result<Arc<ToolRegistry>, anyhow::Error> {
        let credential_registry = Arc::new(SharedCredentialRegistry::new());
        let tools = if let Some(ref ss) = self.secrets_store {
            Arc::new(
                ToolRegistry::new()
                    .with_credentials(Arc::clone(&credential_registry), Arc::clone(ss)),
            )
        } else {
            Arc::new(ToolRegistry::new())
        };
        tools.register_builtin_tools()?;

        if let Some(ref ss) = self.secrets_store {
            tools.register_secrets_tools(Arc::clone(ss));
        }
        Ok(tools)
    }

    /// Create the workspace and register memory tools when a database is
    /// available; returns `None` otherwise.
    fn init_workspace(
        &self,
        tools: &Arc<ToolRegistry>,
        embeddings: &Option<Arc<dyn EmbeddingProvider>>,
    ) -> Option<Arc<Workspace>> {
        let db = self.db.as_ref()?;
        let mut ws = Workspace::new_with_db("default", db.clone());
        if let Some(emb) = embeddings {
            ws = ws.with_embeddings(emb.clone());
        }
        let ws = Arc::new(ws);
        tools.register_memory_tools(Arc::clone(&ws));
        Some(ws)
    }

    /// Resolve the API base URL and (optional) key used for image and vision
    /// tools, preferring the explicit provider config over NEAR AI defaults.
    fn media_api_credentials(&self) -> (String, Option<String>) {
        use secrecy::ExposeSecret;
        if let Some(ref provider) = self.config.llm.provider {
            (
                provider.base_url.clone(),
                provider
                    .api_key
                    .as_ref()
                    .map(|s| s.expose_secret().to_string()),
            )
        } else {
            (
                self.config.llm.nearai.base_url.clone(),
                self.config
                    .llm
                    .nearai
                    .api_key
                    .as_ref()
                    .map(|s| s.expose_secret().to_string()),
            )
        }
    }

    /// Register image generation and vision tools when API credentials are
    /// available.
    fn register_media_tools(&self, tools: &Arc<ToolRegistry>) {
        let (api_base, api_key_opt) = self.media_api_credentials();
        let Some(api_key) = api_key_opt else {
            return;
        };

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
        tools.register_image_tools(ImageToolsRegistration::new(
            api_base.clone(),
            api_key.clone(),
            gen_model,
        ));

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

    /// Return `true` when the builder tool is enabled and local tool
    /// execution is available (allowed explicitly, or implied by a
    /// disabled sandbox).
    fn builder_tool_permitted(&self) -> bool {
        let local_tools_available =
            self.config.agent.allow_local_tools || !self.config.sandbox.enabled;
        self.config.builder.enabled && local_tools_available
    }

    /// Phase 5: Initialize the skills system.
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

    /// Phase 6: Load WASM tools, MCP servers, and create extension manager.
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

    /// Phase 7: Initialize runtime metering (context manager and cost guard).
    pub(super) fn init_metering(
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
    pub(super) fn validate_llm_config(&self) -> Result<(), anyhow::Error> {
        if self.config.llm.backend != "nearai" && self.config.llm.provider.is_none() {
            let backend = &self.config.llm.backend;
            anyhow::bail!(
                "LLM_BACKEND={backend} is configured but no credentials were found. \
                 Set the appropriate API key environment variable or run the setup wizard."
            );
        }
        Ok(())
    }
}
