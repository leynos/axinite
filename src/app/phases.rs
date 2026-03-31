//! Initialization phases for the application builder.
//!
//! This module contains the mechanical init phases that orchestrate
//! component construction in a specific order.

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
use crate::tools::{ToolRegistry, VisionToolsRegistration};
use crate::workspace::{EmbeddingProvider, Workspace};

use super::AppBuilder;

impl AppBuilder {
    /// Phase 1: Initialize database backend.
    ///
    /// Creates the database connection, runs migrations, reloads config
    /// from DB, attaches DB to session manager, and cleans up stale jobs.
    pub(super) async fn init_database(&mut self) -> Result<(), anyhow::Error> {
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

        self.db = Some(db);
        Ok(())
    }

    /// Phase 2: Create secrets store.
    ///
    /// Requires a master key and a backend-specific DB handle. After creating
    /// the store, injects any encrypted LLM API keys into the config overlay
    /// and re-resolves config.
    pub(super) async fn init_secrets(&mut self) -> Result<(), anyhow::Error> {
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
    pub(super) async fn init_llm(
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
    pub(super) async fn init_tools(
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

    /// Phase 5: Load WASM tools, MCP servers, and create extension manager.
    pub(super) async fn init_extensions(
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
        use crate::tools::mcp::config::load_mcp_servers_from_db;
        use crate::tools::wasm::{WasmToolLoader, load_dev_tools};

        let mcp_session_manager = Arc::new(McpSessionManager::new());
        let mcp_process_manager = Arc::new(McpProcessManager::new());

        // Create WASM tool runtime eagerly so extensions installed after startup
        // (e.g. via the web UI) can still be activated. The tools directory is only
        // needed when loading modules, not for engine initialisation.
        let wasm_tool_runtime: Option<Arc<WasmToolRuntime>> = if self.config.wasm.enabled {
            WasmToolRuntime::new(self.config.wasm.to_runtime_config())
                .map(Arc::new)
                .map_err(|e| tracing::warn!("Failed to initialize WASM runtime: {}", e))
                .ok()
        } else {
            None
        };

        // Load WASM tools and MCP servers concurrently
        let wasm_tools_future = {
            let wasm_tool_runtime = wasm_tool_runtime.clone();
            let secrets_store = self.secrets_store.clone();
            let tools = Arc::clone(tools);
            let wasm_config = self.config.wasm.clone();
            async move {
                let mut dev_loaded_tool_names: Vec<String> = Vec::new();

                if let Some(ref runtime) = wasm_tool_runtime {
                    let mut loader = WasmToolLoader::new(Arc::clone(runtime), Arc::clone(&tools));
                    if let Some(ref secrets) = secrets_store {
                        loader = loader.with_secrets_store(Arc::clone(secrets));
                    }

                    match loader.load_from_dir(&wasm_config.tools_dir).await {
                        Ok(results) => {
                            if !results.loaded.is_empty() {
                                tracing::debug!(
                                    "Loaded {} WASM tools from {}",
                                    results.loaded.len(),
                                    wasm_config.tools_dir.display()
                                );
                            }
                            for (path, err) in &results.errors {
                                tracing::warn!(
                                    "Failed to load WASM tool {}: {}",
                                    path.display(),
                                    err
                                );
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Failed to scan WASM tools directory: {}", e);
                        }
                    }

                    match load_dev_tools(&loader, &wasm_config.tools_dir).await {
                        Ok(results) => {
                            dev_loaded_tool_names.extend(results.loaded.iter().cloned());
                            if !dev_loaded_tool_names.is_empty() {
                                tracing::debug!(
                                    "Loaded {} dev WASM tools from build artifacts",
                                    dev_loaded_tool_names.len()
                                );
                            }
                        }
                        Err(e) => {
                            tracing::debug!("No dev WASM tools found: {}", e);
                        }
                    }
                }

                dev_loaded_tool_names
            }
        };

        let mcp_servers_future = {
            let secrets_store = self.secrets_store.clone();
            let db = self.db.clone();
            let tools = Arc::clone(tools);
            let mcp_sm = Arc::clone(&mcp_session_manager);
            let pm = Arc::clone(&mcp_process_manager);
            async move {
                let servers_result = if let Some(ref d) = db {
                    load_mcp_servers_from_db(d.as_ref(), "default").await
                } else {
                    crate::tools::mcp::config::load_mcp_servers().await
                };
                match servers_result {
                    Ok(servers) => {
                        let enabled: Vec<_> = servers.enabled_servers().cloned().collect();
                        if !enabled.is_empty() {
                            tracing::debug!(
                                "Loading {} configured MCP server(s)...",
                                enabled.len()
                            );
                        }

                        let mut join_set = tokio::task::JoinSet::new();
                        for server in enabled {
                            let mcp_sm = Arc::clone(&mcp_sm);
                            let secrets = secrets_store.clone();
                            let tools = Arc::clone(&tools);
                            let pm = Arc::clone(&pm);

                            join_set.spawn(async move {
                                let server_name = server.name.clone();

                                let client = match crate::tools::mcp::create_client_from_config(
                                    server,
                                    &mcp_sm,
                                    &pm,
                                    secrets,
                                    "default",
                                )
                                .await
                                {
                                    Ok(c) => c,
                                    Err(e) => {
                                        tracing::warn!(
                                            "Failed to create MCP client for '{}': {}",
                                            server_name,
                                            e
                                        );
                                        return;
                                    }
                                };

                                match client.list_tools().await {
                                    Ok(mcp_tools) => {
                                        let tool_count = mcp_tools.len();
                                        match client.create_tools().await {
                                            Ok(tool_impls) => {
                                                for tool in tool_impls {
                                                    tools.register(tool).await;
                                                }
                                                tracing::debug!(
                                                    "Loaded {} tools from MCP server '{}'",
                                                    tool_count,
                                                    server_name
                                                );
                                            }
                                            Err(e) => {
                                                tracing::warn!(
                                                    "Failed to create tools from MCP server '{}': {}",
                                                    server_name,
                                                    e
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let err_str = e.to_string();
                                        if err_str.contains("401")
                                            || err_str.contains("authentication")
                                        {
                                            tracing::warn!(
                                                "MCP server '{}' requires authentication. \
                                                 Run: ironclaw mcp auth {}",
                                                server_name,
                                                server_name
                                            );
                                        } else {
                                            tracing::warn!(
                                                "Failed to connect to MCP server '{}': {}",
                                                server_name,
                                                e
                                            );
                                        }
                                    }
                                }
                            });
                        }

                        while let Some(result) = join_set.join_next().await {
                            if let Err(e) = result {
                                tracing::warn!("MCP server loading task panicked: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        tracing::debug!("No MCP servers configured ({})", e);
                    }
                }
            }
        };

        let (dev_loaded_tool_names, _) = tokio::join!(wasm_tools_future, mcp_servers_future);

        // Load registry catalog entries for extension discovery
        let mut catalog_entries = match crate::registry::RegistryCatalog::load_or_embedded() {
            Ok(catalog) => {
                let entries: Vec<_> = catalog
                    .all()
                    .iter()
                    .map(|m| m.to_registry_entry())
                    .collect();
                tracing::debug!(
                    count = entries.len(),
                    "Loaded registry catalog entries for extension discovery"
                );
                entries
            }
            Err(e) => {
                tracing::warn!("Failed to load registry catalog: {}", e);
                Vec::new()
            }
        };

        // Append builtin entries (e.g. channel-relay integrations) so they appear
        // in the web UI's available extensions list.
        let builtin = crate::extensions::registry::builtin_entries();
        for entry in builtin {
            if !catalog_entries.iter().any(|e| e.name == entry.name) {
                catalog_entries.push(entry);
            }
        }

        // Create extension manager. Use ephemeral in-memory secrets if no
        // persistent store is configured (listing/install/activate still work).
        let ext_secrets: Arc<dyn crate::secrets::SecretsStore + Send + Sync> = if let Some(ref s) =
            self.secrets_store
        {
            Arc::clone(s)
        } else {
            use crate::secrets::{InMemorySecretsStore, SecretsCrypto};
            let ephemeral_key =
                secrecy::SecretString::from(crate::secrets::keychain::generate_master_key_hex());
            let crypto = Arc::new(SecretsCrypto::new(ephemeral_key).expect("ephemeral crypto"));
            tracing::debug!("Using ephemeral in-memory secrets store for extension manager");
            Arc::new(InMemorySecretsStore::new(crypto))
        };
        let extension_manager = {
            let manager = Arc::new(ExtensionManager::new(
                Arc::clone(&mcp_session_manager),
                Arc::clone(&mcp_process_manager),
                ext_secrets,
                Arc::clone(tools),
                Some(Arc::clone(hooks)),
                wasm_tool_runtime.clone(),
                self.config.wasm.tools_dir.clone(),
                self.config.channels.wasm_channels_dir.clone(),
                self.config.tunnel.public_url.clone(),
                "default".to_string(),
                self.db.clone(),
                catalog_entries.clone(),
            ));
            tools.register_extension_tools(Arc::clone(&manager));
            tracing::debug!("Extension manager initialized with in-chat discovery tools");
            Some(manager)
        };

        // register_builder_tool() already calls register_dev_tools() internally,
        // so only register them here when the builder didn't already do it.
        let builder_registered_dev_tools = self.config.builder.enabled
            && (self.config.agent.allow_local_tools || !self.config.sandbox.enabled);
        if self.config.agent.allow_local_tools && !builder_registered_dev_tools {
            tools.register_dev_tools();
        }

        Ok((
            mcp_session_manager,
            mcp_process_manager,
            wasm_tool_runtime,
            extension_manager,
            catalog_entries,
            dev_loaded_tool_names,
        ))
    }

    /// Phase 6: Discover and register skills.
    ///
    /// Returns `(None, None)` when the skills feature is disabled.
    pub(super) async fn init_skills(
        &self,
        tools: &Arc<ToolRegistry>,
    ) -> (
        Option<Arc<std::sync::RwLock<SkillRegistry>>>,
        Option<Arc<SkillCatalog>>,
    ) {
        if !self.config.skills.enabled {
            return (None, None);
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
        (Some(registry), Some(catalog))
    }

    /// Phase 7: Construct runtime resource guards.
    ///
    /// Creates the `ContextManager` (parallel-job limiter) and `CostGuard`
    /// (per-day/per-hour spend limiter) from resolved config.
    pub(super) fn init_runtime_context(
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
}
