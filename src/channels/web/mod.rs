//! Web gateway channel for browser-based access to Axinite.
//!
//! Provides a single-page web UI with:
//! - Chat with the agent (via REST + SSE)
//! - Workspace/memory browsing
//! - Job management
//!
//! ```text
//! Browser ─── POST /api/chat/send ──► Agent Loop
//!         ◄── GET  /api/chat/events ── SSE stream
//!         ─── GET  /api/chat/ws ─────► WebSocket (bidirectional)
//!         ─── GET  /api/memory/* ────► Workspace
//!         ─── GET  /api/jobs/* ──────► Database
//!         ◄── GET  / ───────────────── Static HTML/CSS/JS
//! ```

pub mod auth;
pub(crate) mod handlers;
pub mod log_layer;
mod native_channel;
pub mod openai_compat;
pub mod server;
pub mod sse;
pub mod types;
pub(crate) mod util;
pub mod ws;

/// Test helpers for gateway integration tests.
///
/// Always compiled (not behind `#[cfg(test)]`) so that integration tests in
/// `tests/` -- which import this crate as a regular dependency -- can use
/// [`TestGatewayBuilder`](test_helpers::TestGatewayBuilder).
pub mod test_helpers;

use std::sync::Arc;

use crate::agent::SessionManager;
use crate::config::GatewayConfig;
use crate::db::Database;
use crate::extensions::ExtensionManager;
use crate::orchestrator::job_manager::ContainerJobManager;
use crate::skills::catalog::SkillCatalog;
use crate::skills::registry::SkillRegistry;
use crate::tools::ToolRegistry;
use crate::workspace::Workspace;

use self::log_layer::{LogBroadcaster, LogLevelHandle};

use self::server::GatewayState;
use self::sse::SseManager;

/// Web gateway channel implementing the Channel trait.
pub struct GatewayChannel {
    config: GatewayConfig,
    state: Arc<GatewayState>,
    /// The actual auth token in use (generated or from config).
    auth_token: String,
}

impl GatewayChannel {
    /// Create a new gateway channel.
    ///
    /// If no auth token is configured, generates a random one and prints it.
    pub fn new(config: GatewayConfig) -> Self {
        let auth_token = config.auth_token.clone().unwrap_or_else(|| {
            use rand::RngCore;
            use rand::rngs::OsRng;
            let mut bytes = [0u8; 32];
            OsRng.fill_bytes(&mut bytes);
            bytes.iter().map(|b| format!("{b:02x}")).collect()
        });

        let state = Arc::new(GatewayState {
            msg_tx: tokio::sync::RwLock::new(None),
            sse: SseManager::new(),
            workspace: None,
            session_manager: None,
            log_broadcaster: None,
            log_level_handle: None,
            extension_manager: None,
            tool_registry: None,
            store: None,
            job_manager: None,
            prompt_queue: None,
            scheduler: None,
            user_id: config.user_id.clone(),
            shutdown_tx: tokio::sync::RwLock::new(None),
            ws_tracker: Some(Arc::new(ws::WsConnectionTracker::new())),
            llm_provider: None,
            skill_registry: None,
            skill_catalog: None,
            chat_rate_limiter: server::RateLimiter::new(30, 60),
            oauth_rate_limiter: server::RateLimiter::new(10, 60),
            registry_entries: Vec::new(),
            cost_guard: None,
            routine_engine: Arc::new(tokio::sync::RwLock::new(None)),
            startup_time: std::time::Instant::now(),
        });

        Self {
            config,
            state,
            auth_token,
        }
    }

    /// Helper to rebuild state, copying existing fields and applying a mutation.
    fn rebuild_state(&mut self, mutate: impl FnOnce(&mut GatewayState)) {
        let mut new_state = GatewayState {
            msg_tx: tokio::sync::RwLock::new(None),
            // Preserve the existing broadcast channel so sender handles remain valid.
            sse: SseManager::from_sender(self.state.sse.sender()),
            workspace: self.state.workspace.clone(),
            session_manager: self.state.session_manager.clone(),
            log_broadcaster: self.state.log_broadcaster.clone(),
            log_level_handle: self.state.log_level_handle.clone(),
            extension_manager: self.state.extension_manager.clone(),
            tool_registry: self.state.tool_registry.clone(),
            store: self.state.store.clone(),
            job_manager: self.state.job_manager.clone(),
            prompt_queue: self.state.prompt_queue.clone(),
            scheduler: self.state.scheduler.clone(),
            user_id: self.state.user_id.clone(),
            shutdown_tx: tokio::sync::RwLock::new(None),
            ws_tracker: self.state.ws_tracker.clone(),
            llm_provider: self.state.llm_provider.clone(),
            skill_registry: self.state.skill_registry.clone(),
            skill_catalog: self.state.skill_catalog.clone(),
            chat_rate_limiter: server::RateLimiter::new(30, 60),
            oauth_rate_limiter: server::RateLimiter::new(10, 60),
            registry_entries: self.state.registry_entries.clone(),
            cost_guard: self.state.cost_guard.clone(),
            routine_engine: Arc::clone(&self.state.routine_engine),
            startup_time: self.state.startup_time,
        };
        mutate(&mut new_state);
        self.state = Arc::new(new_state);
    }

    /// Inject the workspace reference for the memory API.
    pub fn with_workspace(mut self, workspace: Arc<Workspace>) -> Self {
        self.rebuild_state(|s| s.workspace = Some(workspace));
        self
    }

    /// Inject the session manager for thread/session info.
    pub fn with_session_manager(mut self, sm: Arc<SessionManager>) -> Self {
        self.rebuild_state(|s| s.session_manager = Some(sm));
        self
    }

    /// Inject the log broadcaster for the logs SSE endpoint.
    pub fn with_log_broadcaster(mut self, lb: Arc<LogBroadcaster>) -> Self {
        self.rebuild_state(|s| s.log_broadcaster = Some(lb));
        self
    }

    /// Inject the log level handle for runtime log level control.
    pub fn with_log_level_handle(mut self, h: Arc<LogLevelHandle>) -> Self {
        self.rebuild_state(|s| s.log_level_handle = Some(h));
        self
    }

    /// Inject the extension manager for the extensions API.
    pub fn with_extension_manager(mut self, em: Arc<ExtensionManager>) -> Self {
        self.rebuild_state(|s| s.extension_manager = Some(em));
        self
    }

    /// Inject the tool registry for the extensions API.
    pub fn with_tool_registry(mut self, tr: Arc<ToolRegistry>) -> Self {
        self.rebuild_state(|s| s.tool_registry = Some(tr));
        self
    }

    /// Inject the database store for sandbox job persistence.
    pub fn with_store(mut self, store: Arc<dyn Database>) -> Self {
        self.rebuild_state(|s| s.store = Some(store));
        self
    }

    /// Inject the container job manager for sandbox operations.
    pub fn with_job_manager(mut self, jm: Arc<ContainerJobManager>) -> Self {
        self.rebuild_state(|s| s.job_manager = Some(jm));
        self
    }

    /// Inject the prompt queue for Claude Code follow-up prompts.
    pub fn with_prompt_queue(
        mut self,
        pq: Arc<
            tokio::sync::Mutex<
                std::collections::HashMap<
                    uuid::Uuid,
                    std::collections::VecDeque<crate::orchestrator::api::PendingPrompt>,
                >,
            >,
        >,
    ) -> Self {
        self.rebuild_state(|s| s.prompt_queue = Some(pq));
        self
    }

    /// Inject the scheduler for sending follow-up messages to agent jobs.
    pub fn with_scheduler(mut self, slot: crate::tools::builtin::SchedulerSlot) -> Self {
        self.rebuild_state(|s| s.scheduler = Some(slot));
        self
    }

    /// Inject the skill registry for skill management API.
    pub fn with_skill_registry(mut self, sr: Arc<std::sync::RwLock<SkillRegistry>>) -> Self {
        self.rebuild_state(|s| s.skill_registry = Some(sr));
        self
    }

    /// Inject the skill catalogue for skill search API.
    pub fn with_skill_catalog(mut self, sc: Arc<SkillCatalog>) -> Self {
        self.rebuild_state(|s| s.skill_catalog = Some(sc));
        self
    }

    /// Inject the LLM provider for OpenAI-compatible API proxy.
    pub fn with_llm_provider(mut self, llm: Arc<dyn crate::llm::LlmProvider>) -> Self {
        self.rebuild_state(|s| s.llm_provider = Some(llm));
        self
    }

    /// Inject registry catalogue entries for the available extensions API.
    pub fn with_registry_entries(mut self, entries: Vec<crate::extensions::RegistryEntry>) -> Self {
        self.rebuild_state(|s| s.registry_entries = entries);
        self
    }

    /// Inject the cost guard for token/cost tracking in the status popover.
    pub fn with_cost_guard(mut self, cg: Arc<crate::agent::cost_guard::CostGuard>) -> Self {
        self.rebuild_state(|s| s.cost_guard = Some(cg));
        self
    }

    /// Get the auth token (for printing to console on startup).
    pub fn auth_token(&self) -> &str {
        &self.auth_token
    }

    /// Get a reference to the shared gateway state (for the agent to push SSE events).
    pub fn state(&self) -> &Arc<GatewayState> {
        &self.state
    }
}
