//! Core agent logic.
//!
//! The agent orchestrates:
//! - Message routing from channels
//! - Job scheduling and execution
//! - Tool invocation with safety
//! - Self-repair for stuck jobs
//! - Proactive heartbeat execution
//! - Routine-based scheduled and reactive jobs
//! - Turn-based session management with undo
//! - Context compaction for long conversations

use std::sync::Arc;

use crate::db::Database;
use crate::hooks::HookRegistry;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::skills::{SkillRegistry, catalog::SkillCatalog};
use crate::tools::ToolRegistry;
use crate::workspace::Workspace;

mod agent_loop;
pub mod agentic_loop;
mod attachments;
mod commands;
pub mod compaction;
pub mod context_monitor;
pub mod cost_guard;
mod dispatcher;
mod heartbeat;
pub mod job_monitor;
mod router;
pub mod routine;
pub mod routine_engine;
pub(crate) mod scheduler;
mod self_repair;
pub mod session;
mod session_manager;
pub mod submission;
pub mod task;
mod thread_ops;
pub mod undo;

pub use crate::worker::{Worker, WorkerDeps};
pub use agent_loop::{Agent, AgentDeps};
pub use compaction::{CompactionResult, ContextCompactor};
pub use context_monitor::{CompactionStrategy, ContextBreakdown, ContextMonitor};
pub(crate) use dispatcher::truncate_for_preview;
pub use heartbeat::{HeartbeatConfig, HeartbeatResult, HeartbeatRunner, spawn_heartbeat};
pub use router::{MessageIntent, Router};
pub use routine::{Routine, RoutineAction, RoutineRun, Trigger};
pub use routine_engine::RoutineEngine;
pub use scheduler::Scheduler;
pub use self_repair::{BrokenTool, RepairResult, RepairTask, SelfRepair, StuckJob};
pub use session::{PendingApproval, PendingAuth, Session, Thread, ThreadState, Turn, TurnState};
pub use session_manager::SessionManager;
pub use submission::{Submission, SubmissionParser, SubmissionResult};
pub use task::{Task, TaskContext, TaskHandler, TaskOutput};
pub use undo::{Checkpoint, UndoManager};

impl Agent {
    /// Set the routine engine slot for exposing the engine to the gateway.
    pub fn set_routine_engine_slot(
        &mut self,
        slot: Arc<tokio::sync::RwLock<Option<Arc<crate::agent::routine_engine::RoutineEngine>>>>,
    ) {
        self.routine_engine_slot = Some(slot);
    }

    /// Get the scheduler (for external wiring, e.g. CreateJobTool).
    pub fn scheduler(&self) -> Arc<Scheduler> {
        Arc::clone(&self.scheduler)
    }

    pub(super) fn store(&self) -> Option<&Arc<dyn Database>> {
        self.deps.store.as_ref()
    }

    pub(super) fn llm(&self) -> &Arc<dyn LlmProvider> {
        &self.deps.llm
    }

    /// Get the cheap/fast LLM provider, falling back to the main one.
    pub(super) fn cheap_llm(&self) -> &Arc<dyn LlmProvider> {
        self.deps.cheap_llm.as_ref().unwrap_or(&self.deps.llm)
    }

    pub(super) fn safety(&self) -> &Arc<SafetyLayer> {
        &self.deps.safety
    }

    pub(super) fn tools(&self) -> &Arc<ToolRegistry> {
        &self.deps.tools
    }

    pub(super) fn workspace(&self) -> Option<&Arc<Workspace>> {
        self.deps.workspace.as_ref()
    }

    pub(super) fn hooks(&self) -> &Arc<HookRegistry> {
        &self.deps.hooks
    }

    pub(super) fn cost_guard(&self) -> &Arc<crate::agent::cost_guard::CostGuard> {
        &self.deps.cost_guard
    }

    pub(super) fn skill_registry(&self) -> Option<&Arc<std::sync::RwLock<SkillRegistry>>> {
        self.deps.skill_registry.as_ref()
    }

    pub(super) fn skill_catalog(&self) -> Option<&Arc<SkillCatalog>> {
        self.deps.skill_catalog.as_ref()
    }
}
