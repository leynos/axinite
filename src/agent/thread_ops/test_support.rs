//! Shared test fixtures for thread operation modules.
//!
//! These helpers keep the turn-pipeline tests focused on orchestration logic
//! rather than repeating `Agent` construction and temporary database setup.

use std::sync::Arc;

use anyhow::Result;
use rstest::fixture;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
use crate::agent::session::Session;
use crate::agent::{Agent, AgentDeps, SessionManager};
use crate::channels::{ChannelManager, IncomingMessage};
use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
use crate::db::Database;
#[cfg(feature = "libsql")]
use crate::db::NativeDatabase;
#[cfg(feature = "libsql")]
use crate::db::libsql::LibSqlBackend;
use crate::hooks::HookRegistry;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::testing::StubLlm;
use crate::tools::ToolRegistry;

#[fixture]
pub(crate) fn incoming_message() -> IncomingMessage {
    IncomingMessage::new("web", "user-1", "hello")
}

#[fixture]
pub(crate) fn session_manager() -> Arc<SessionManager> {
    Arc::new(SessionManager::new())
}

#[fixture]
pub(crate) fn fresh_session_thread() -> (Arc<Mutex<Session>>, Uuid) {
    let mut session = Session::new("user-1");
    let thread_id = session.create_thread().id;
    (Arc::new(Mutex::new(session)), thread_id)
}

pub(crate) fn make_agent(
    store: Option<Arc<dyn Database>>,
    llm: Arc<dyn LlmProvider>,
    session_manager: Arc<SessionManager>,
) -> Agent {
    let deps = AgentDeps {
        store,
        llm,
        cheap_llm: None,
        safety: Arc::new(SafetyLayer::new(&SafetyConfig {
            max_output_length: 100_000,
            injection_check_enabled: false,
        })),
        tools: Arc::new(ToolRegistry::new()),
        workspace: None,
        extension_manager: None,
        skill_registry: None,
        skill_catalog: None,
        skills_config: SkillsConfig::default(),
        hooks: Arc::new(HookRegistry::new()),
        cost_guard: Arc::new(CostGuard::new(CostGuardConfig::default())),
        sse_tx: None,
        http_interceptor: None,
        transcription: None,
        document_extraction: None,
    };

    Agent::new(
        AgentConfig::for_testing(),
        deps,
        Arc::new(ChannelManager::new()),
        None,
        None,
        None,
        None,
        Some(session_manager),
    )
}

#[fixture]
pub(crate) fn bare_agent(session_manager: Arc<SessionManager>) -> Agent {
    make_agent(
        None,
        Arc::new(StubLlm::new("ok")) as Arc<dyn LlmProvider>,
        session_manager,
    )
}

#[cfg(feature = "libsql")]
pub(crate) async fn local_backend() -> Result<(Arc<LibSqlBackend>, tempfile::TempDir)> {
    let tempdir = tempfile::tempdir()?;
    let db_path = tempdir.path().join("thread-ops-test.db");
    let backend = LibSqlBackend::new_local(&db_path).await?;
    NativeDatabase::run_migrations(&backend).await?;
    Ok((Arc::new(backend), tempdir))
}
