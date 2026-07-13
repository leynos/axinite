//! Test harness assembly: fully wired `AgentDeps` with sensible defaults.

use std::sync::Arc;

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use anyhow::Result;
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use tempfile::TempDir;
use tokio::sync::mpsc;

use crate::agent::AgentDeps;
use crate::channels::{ChannelManager, IncomingMessage};
use crate::db::Database;
use crate::llm::LlmProvider;
use crate::tools::ToolRegistry;

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
use super::{StubChannel, StubLlm};

/// Create a libSQL-backed test database in a temporary directory.
///
/// Returns the database and a `TempDir` guard — the database file is
/// deleted when the guard is dropped.
///
/// # Errors
///
/// Returns an error when the temporary directory cannot be created, the
/// backend cannot be initialized, or the migrations fail to run.
#[cfg(all(feature = "libsql", feature = "test-helpers"))]
pub async fn test_db() -> Result<(Arc<dyn Database>, TempDir)> {
    use anyhow::Context as _;

    use crate::db::libsql::LibSqlBackend;
    use tempfile::tempdir;

    let dir = tempdir().context("failed to create temp dir")?;
    let path = dir.path().join("test.db");
    let backend = LibSqlBackend::new_local(&path)
        .await
        .context("failed to create test LibSqlBackend")?;
    backend
        .run_migrations()
        .await
        .context("failed to run migrations")?;
    Ok((Arc::new(backend) as Arc<dyn Database>, dir))
}

/// Assembled test components.
pub struct TestHarness {
    /// The agent dependencies, ready for use.
    pub deps: AgentDeps,
    /// Direct reference to the database (as `Arc<dyn Database>`).
    pub db: Arc<dyn Database>,
    /// Stub channel sender + manager, present if `with_stub_channel()` was called.
    pub channel: Option<(mpsc::Sender<IncomingMessage>, ChannelManager)>,
    /// Temp directory guard — keeps the test database alive. Dropped
    /// automatically when the harness goes out of scope. `None` when the
    /// caller supplied its own database, which needs no on-disk guard.
    #[cfg(all(feature = "libsql", feature = "test-helpers"))]
    _temp_dir: Option<TempDir>,
}

/// Builder for constructing a [`TestHarness`] with sensible defaults.
///
/// All defaults are designed to work without any external services:
/// - Database: libSQL in a temp directory (real SQL, FTS5, no network)
/// - LLM: `StubLlm` returning "OK"
/// - Safety: permissive config
/// - Tools: builtin tools registered
/// - Hooks: empty registry
/// - Cost guard: no limits
pub struct TestHarnessBuilder {
    db: Option<Arc<dyn Database>>,
    llm: Option<Arc<dyn LlmProvider>>,
    tools: Option<Arc<ToolRegistry>>,
    stub_channel: bool,
}

impl TestHarnessBuilder {
    /// Create a new builder with all defaults.
    pub fn new() -> Self {
        Self {
            db: None,
            llm: None,
            tools: None,
            stub_channel: false,
        }
    }

    /// Override the database backend.
    pub fn with_db(mut self, db: Arc<dyn Database>) -> Self {
        self.db = Some(db);
        self
    }

    /// Override the LLM provider.
    pub fn with_llm(mut self, llm: Arc<dyn LlmProvider>) -> Self {
        self.llm = Some(llm);
        self
    }

    /// Override the tool registry.
    pub fn with_tools(mut self, tools: Arc<ToolRegistry>) -> Self {
        self.tools = Some(tools);
        self
    }

    /// Include a `StubChannel` wired into a `ChannelManager`.
    ///
    /// The harness will expose the sender (for injecting messages) and
    /// the manager (for routing responses) via [`TestHarness::channel`].
    pub fn with_stub_channel(mut self) -> Self {
        self.stub_channel = true;
        self
    }

    /// Build the harness with defaults applied.
    ///
    /// # Errors
    ///
    /// Returns an error when the fallback test database cannot be
    /// provisioned.
    #[cfg(all(feature = "libsql", feature = "test-helpers"))]
    pub async fn build(self) -> Result<TestHarness> {
        use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
        use crate::config::{SafetyConfig, SkillsConfig};
        use crate::hooks::HookRegistry;
        use crate::safety::SafetyLayer;

        let (db, temp_dir) = if let Some(db) = self.db {
            // Caller provided a DB; no on-disk guard is needed.
            (db, None)
        } else {
            let (db, dir) = test_db().await?;
            (db, Some(dir))
        };

        let llm: Arc<dyn LlmProvider> = self.llm.unwrap_or_else(|| Arc::new(StubLlm::default()));

        let tools = match self.tools {
            Some(t) => t,
            None => {
                let t = Arc::new(ToolRegistry::new());
                t.register_builtin_tools()?;
                t
            }
        };

        let safety = Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: false,
        }));

        let hooks = Arc::new(HookRegistry::new());

        let cost_guard = Arc::new(CostGuard::new(CostGuardConfig {
            max_cost_per_day_cents: None,
            max_actions_per_hour: None,
        }));

        let channel = if self.stub_channel {
            let (stub, sender) = StubChannel::new("stub");
            let manager = ChannelManager::new();
            manager.add(Box::new(stub)).await;
            Some((sender, manager))
        } else {
            None
        };

        let deps = AgentDeps {
            store: Some(Arc::clone(&db)),
            llm,
            cheap_llm: None,
            safety,
            tools,
            workspace: None,
            extension_manager: None,
            skill_registry: None,
            skill_catalog: None,
            skills_config: SkillsConfig::default(),
            hooks,
            cost_guard,
            sse_tx: None,
            http_interceptor: None,
            transcription: None,
            document_extraction: None,
        };

        Ok(TestHarness {
            deps,
            db,
            channel,
            _temp_dir: temp_dir,
        })
    }
}

impl Default for TestHarnessBuilder {
    fn default() -> Self {
        Self::new()
    }
}
