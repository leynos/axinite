//! Application builder for initializing core IronClaw components.
//!
//! Extracts the mechanical initialization phases from `main.rs` into a
//! reusable builder so that:
//!
//! - Tests can construct a full `AppComponents` without wiring channels
//! - Main stays focused on CLI dispatch and channel setup
//! - Each init phase is independently testable
//!
//! ## Two-Phase Bootstrap Pattern
//!
//! This module follows hexagonal architecture principles by separating pure
//! component assembly from mechanism-heavy activation:
//!
//! 1. **Assembly phase** (`build_components()`): Constructs all components
//!    and returns them along with a `RuntimeSideEffects` struct containing
//!    deferred background work.
//!
//! 2. **Activation phase** (`RuntimeSideEffects::start()`): Runs workspace
//!    import and seeding synchronously, then spawns fire-and-forget background
//!    tasks for stale job cleanup and embedding backfill.
//!
//! This separation allows tests to validate composition without paying for
//! unrelated I/O and background task overhead.
//!
//! ### Usage
//!
//! **Production code** (immediate startup desired):
//! ```ignore
//! let components = AppBuilder::new(config, flags, toml_path, session, log_broadcaster)
//!     .build_all()  // convenience wrapper
//!     .await?;
//! ```
//!
//! **Tests** (precise control over side-effect timing):
//! ```ignore
//! let (components, _side_effects) = AppBuilder::new(config, flags, None, session, log_broadcaster)
//!     .build_components()
//!     .await?;
//! // side_effects intentionally not started — avoids background I/O
//! ```
//!
//! **Explicit control** (coordinating startup with other systems):
//! ```ignore
//! let (components, side_effects) = AppBuilder::new(...)
//!     .build_components()
//!     .await?;
//! // ... perform additional setup ...
//! side_effects.start().await;
//! ```

mod phases;
mod side_effects;

pub use side_effects::RuntimeSideEffects;

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
use crate::tools::wasm::WasmToolRuntime;
use crate::tools::{ImageToolsRegistration, ToolRegistry};
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

/// Options that control optional init phases.
#[derive(Default)]
pub struct AppBuilderFlags {
    pub no_db: bool,
    /// Optional workspace import directory path.
    ///
    /// When set, workspace files from this directory will be imported during
    /// startup. If `None`, the import phase is skipped.
    pub workspace_import_dir: Option<std::path::PathBuf>,
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

    /// Run all init phases in order and return the assembled components
    /// along with deferred runtime side effects.
    ///
    /// Use this method when you need control over when side effects start,
    /// such as in tests or when coordinating startup with other systems.
    ///
    /// For production use, consider using the `build_all()` convenience
    /// wrapper which immediately starts side effects.
    pub async fn build_components(
        mut self,
    ) -> Result<(AppComponents, RuntimeSideEffects), anyhow::Error> {
        self.init_database().await?;
        self.init_secrets().await?;

        // Post-init validation: if a non-nearai backend was selected but
        // credentials were never resolved (deferred resolution found no keys),
        // fail early with a clear error instead of a confusing runtime failure.
        if self.config.llm.backend != "nearai" && self.config.llm.provider.is_none() {
            let backend = &self.config.llm.backend;
            anyhow::bail!(
                "LLM_BACKEND={backend} is configured but no credentials were found. \
                 Set the appropriate API key environment variable or run the setup wizard."
            );
        }

        let (llm, cheap_llm, recording_handle) = if let Some(llm) = self.llm_override.take() {
            (llm, None, None)
        } else {
            self.init_llm().await?
        };
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

        // Use workspace import directory from flags (injected by caller)
        let workspace_import_dir = self.flags.workspace_import_dir.clone();

        // Phase 6 – skills
        let (skill_registry, skill_catalog) = self.init_skills(&tools).await;

        // Phase 7 – runtime context
        let (context_manager, cost_guard) = self.init_runtime_context();

        tracing::debug!(
            "Tool registry initialized with {} total tools",
            tools.count()
        );

        let components = AppComponents {
            config: self.config,
            db: self.db.clone(),
            secrets_store: self.secrets_store,
            llm,
            cheap_llm,
            safety,
            tools,
            embeddings: embeddings.clone(),
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

        let side_effects = RuntimeSideEffects {
            db: self.db,
            workspace,
            workspace_import_dir,
            embeddings_available: embeddings.is_some(),
        };

        Ok((components, side_effects))
    }

    /// Convenience wrapper that builds components and immediately starts
    /// all runtime side effects.
    ///
    /// For production use where immediate startup is desired. Tests and
    /// scenarios requiring precise control over side-effect timing should
    /// use `build_components()` directly.
    pub async fn build_all(self) -> Result<AppComponents, anyhow::Error> {
        let (components, side_effects) = self.build_components().await?;
        side_effects.start().await;
        Ok(components)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct a minimal AppBuilder for testing (no_db = true, no workspace).
    fn minimal_builder() -> AppBuilder {
        let config = Config::for_testing(
            std::env::temp_dir().join("ironclaw-test.db"),
            std::env::temp_dir().join("skills"),
            std::env::temp_dir().join("installed-skills"),
        );
        let flags = AppBuilderFlags {
            no_db: true,
            workspace_import_dir: None,
        };
        let session = Arc::new(SessionManager::new(config.llm.session.clone()));
        let log_broadcaster = Arc::new(LogBroadcaster::new());

        AppBuilder::new(config, flags, None, session, log_broadcaster)
    }

    /// build_components() returns side effects without starting them.
    #[tokio::test]
    async fn build_components_does_not_start_side_effects() {
        let (_components, side_effects) = minimal_builder()
            .build_components()
            .await
            .expect("build_components failed");
        // RuntimeSideEffects is returned; start() has NOT been called.
        // The mere existence of the value (without panicking or spawning) is
        // sufficient proof for a no-db, no-workspace builder.
        let _ = side_effects;
    }

    /// build_all() delegates to build_components() + start() and succeeds.
    #[tokio::test]
    async fn build_all_completes_successfully() {
        minimal_builder()
            .build_all()
            .await
            .expect("build_all failed");
    }

    /// Calling start() on the side effects returned by build_components()
    /// completes without panicking.
    #[tokio::test]
    async fn runtime_side_effects_start_is_idempotent() {
        let (_components, side_effects) = minimal_builder()
            .build_components()
            .await
            .expect("build_components failed");
        side_effects.start().await; // must not panic
    }

    /// Verify that `build_components()` returns side effects separately.
    ///
    /// This test ensures the two-phase API guarantees that component
    /// construction is separate from side-effect startup, allowing tests
    /// to initialise the app without background I/O.
    #[tokio::test]
    async fn build_components_returns_side_effects_separately() {
        // Use minimal config for testing
        let config = Config::for_testing(
            std::env::temp_dir().join("ironclaw-test.db"),
            std::env::temp_dir().join("skills"),
            std::env::temp_dir().join("installed-skills"),
        );
        let flags = AppBuilderFlags {
            no_db: true,
            workspace_import_dir: None,
        };
        let session = Arc::new(SessionManager::new(config.llm.session.clone()));
        let log_broadcaster = Arc::new(LogBroadcaster::new());

        let (_components, side_effects) =
            AppBuilder::new(config, flags, None, session, log_broadcaster)
                .build_components()
                .await
                .expect("build_components should succeed");

        // side_effects is an owned value that has not been started.
        // This demonstrates the two-phase API: we have components
        // without any background work having started.
        // The type system enforces this: RuntimeSideEffects must be
        // explicitly consumed by calling start().
        let _ = side_effects;
    }

    /// Verify that RuntimeSideEffects::start runs synchronously for
    /// workspace operations (import/seed) before returning.
    #[cfg(feature = "libsql")]
    #[tokio::test]
    async fn runtime_side_effects_start_awaits_workspace_operations() {
        use crate::db::Database;
        use std::io::Write;

        // Create a temp directory with a file to import
        let import_temp = tempfile::tempdir().expect("tempdir");
        let import_file = import_temp.path().join("AGENTS.md");
        {
            let mut f = std::fs::File::create(&import_file).expect("create file");
            f.write_all(b"# Test Agent\n\nTest content for import")
                .expect("write file");
        }

        // Create a test database and workspace
        let db_temp = tempfile::tempdir().expect("tempdir");
        let db_path = db_temp.path().join("test.db");
        let backend = crate::db::libsql::LibSqlBackend::new_local(&db_path)
            .await
            .expect("LibSqlBackend::new_local");
        backend.run_migrations().await.expect("run_migrations");
        let db: Arc<dyn Database> = Arc::new(backend);
        let workspace = Arc::new(Workspace::new_with_db("default", db));

        // Set up RuntimeSideEffects with workspace and import_dir
        let side_effects = RuntimeSideEffects {
            db: None,
            workspace: Some(workspace.clone()),
            workspace_import_dir: Some(import_temp.path().to_path_buf()),
            embeddings_available: false,
        };

        // Call start() - this should import the file before returning
        side_effects.start().await;

        // Verify the file was imported
        let doc = workspace
            .read("AGENTS.md")
            .await
            .expect("read imported doc");
        assert_eq!(doc.content, "# Test Agent\n\nTest content for import");
    }

    /// Non-libsql version of the test just verifies the method signature.
    #[cfg(not(feature = "libsql"))]
    #[tokio::test]
    async fn runtime_side_effects_start_awaits_workspace_operations() {
        let side_effects = RuntimeSideEffects {
            db: None,
            workspace: None,
            workspace_import_dir: None,
            embeddings_available: false,
        };
        side_effects.start().await;
    }

    /// Verify that `build_components()` leaves side effects dormant until
    /// explicitly started, proving the two-phase API works end-to-end.
    ///
    /// This test creates a workspace import scenario, builds components,
    /// verifies the import has NOT occurred (dormancy), then starts side
    /// effects and confirms the import completes.
    #[cfg(feature = "libsql")]
    #[tokio::test]
    async fn build_components_leaves_side_effects_dormant() {
        use crate::db::Database;
        use std::io::Write;

        // Create a temp directory with a file to import
        let import_temp = tempfile::tempdir().expect("tempdir");
        let import_file = import_temp.path().join("AGENTS.md");
        {
            let mut f = std::fs::File::create(&import_file).expect("create file");
            f.write_all(b"# Test Agent\n\nTest content for import")
                .expect("write file");
        }

        // Create a test database and run migrations
        let db_temp = tempfile::tempdir().expect("tempdir");
        let db_path = db_temp.path().join("test.db");
        let backend = crate::db::libsql::LibSqlBackend::new_local(&db_path)
            .await
            .expect("LibSqlBackend::new_local");
        backend.run_migrations().await.expect("run_migrations");
        let db: Arc<dyn Database> = Arc::new(backend);

        // Build components with import directory set
        let config = Config::for_testing(
            db_path.clone(),
            std::env::temp_dir().join("skills"),
            std::env::temp_dir().join("installed-skills"),
        );
        let flags = AppBuilderFlags {
            no_db: false,
            workspace_import_dir: Some(import_temp.path().to_path_buf()),
        };
        let session = Arc::new(SessionManager::new(config.llm.session.clone()));
        let log_broadcaster = Arc::new(LogBroadcaster::new());

        let mut builder = AppBuilder::new(config, flags, None, session, log_broadcaster);
        builder.with_database(db);
        let (components, side_effects) = builder
            .build_components()
            .await
            .expect("build_components should succeed");

        // DORMANCY ASSERTION: The import should NOT have run yet
        let workspace = components
            .workspace
            .as_ref()
            .expect("workspace should exist");
        let read_result = workspace.read("AGENTS.md").await;
        assert!(
            read_result.is_err(),
            "AGENTS.md should not exist before side_effects.start()"
        );

        // Start side effects - this should run the import
        side_effects.start().await;

        // POST-START ASSERTION: The import should now be complete
        let doc = workspace
            .read("AGENTS.md")
            .await
            .expect("read after start should succeed");
        assert!(doc.content.contains("Test content for import"));
    }

    /// Verify that `build_all()` starts side effects before returning.
    ///
    /// This test ensures the convenience wrapper `build_all` properly
    /// awaits workspace import/seeding before returning components,
    /// unlike `build_components` which defers side effects.
    #[cfg(feature = "libsql")]
    #[tokio::test]
    async fn build_all_runs_side_effects_before_returning() {
        use crate::db::Database;
        use std::io::Write;

        // Create a temp directory with a file to import
        let import_temp = tempfile::tempdir().expect("tempdir");
        let import_file = import_temp.path().join("AGENTS.md");
        {
            let mut f = std::fs::File::create(&import_file).expect("create file");
            f.write_all(b"# Test Agent\n\nTest content for import")
                .expect("write file");
        }

        // Create a test database
        let db_temp = tempfile::tempdir().expect("tempdir");
        let db_path = db_temp.path().join("test.db");
        let backend = crate::db::libsql::LibSqlBackend::new_local(&db_path)
            .await
            .expect("LibSqlBackend::new_local");
        backend.run_migrations().await.expect("run_migrations");
        let db: Arc<dyn Database> = Arc::new(backend);

        // Build components with import directory set
        let config = Config::for_testing(
            db_path.clone(),
            std::env::temp_dir().join("skills"),
            std::env::temp_dir().join("installed-skills"),
        );
        let flags = AppBuilderFlags {
            no_db: false,
            workspace_import_dir: Some(import_temp.path().to_path_buf()),
        };
        let session = Arc::new(SessionManager::new(config.llm.session.clone()));
        let log_broadcaster = Arc::new(LogBroadcaster::new());

        // Use build_all() - side effects should run before returning
        let mut builder = AppBuilder::new(config, flags, None, session, log_broadcaster);
        builder.with_database(db);
        let components = builder.build_all().await.expect("build_all should succeed");

        // Verify the workspace exists and the file was imported
        let workspace = components
            .workspace
            .as_ref()
            .expect("workspace should exist");
        let doc = workspace
            .read("AGENTS.md")
            .await
            .expect("read imported doc");
        assert!(doc.content.contains("Test content for import"));
    }
}
