//! Job worker execution via the shared `AgenticLoop`.
//!
//! Replaces `src/agent/worker.rs` with a `JobDelegate` that implements
//! `LoopDelegate`. The `Worker` struct and `WorkerDeps` remain as the
//! public API consumed by `scheduler.rs`.

mod delegate;
mod events;
mod planning;
mod run_loop;
mod terminal;
mod tool_exec;
mod tool_pipeline;

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::agent::task::TaskOutput;
use crate::channels::web::types::SseEvent;
use crate::context::ContextManager;
use crate::db::Database;
use crate::error::Error;
use crate::hooks::HookRegistry;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::tools::{ApprovalContext, ToolRegistry};

/// Shared dependencies for worker execution.
///
/// This bundles the dependencies that are shared across all workers,
/// reducing the number of arguments to `Worker::new`.
#[derive(Clone)]
pub struct WorkerDeps {
    pub context_manager: Arc<ContextManager>,
    pub llm: Arc<dyn LlmProvider>,
    pub safety: Arc<SafetyLayer>,
    pub tools: Arc<ToolRegistry>,
    pub store: Option<Arc<dyn Database>>,
    pub hooks: Arc<HookRegistry>,
    pub timeout: Duration,
    pub use_planning: bool,
    /// SSE broadcast sender for live job event streaming to the web gateway.
    pub sse_tx: Option<tokio::sync::broadcast::Sender<SseEvent>>,
    /// Approval context for tool execution. When `None`, all non-`Never` tools are
    /// blocked (legacy behavior). When `Some`, the context determines which tools
    /// are pre-approved for autonomous execution.
    pub approval_context: Option<ApprovalContext>,
    /// HTTP interceptor for trace recording/replay (propagated to JobContext).
    pub http_interceptor: Option<Arc<dyn crate::llm::recording::HttpInterceptor>>,
}

/// Worker that executes a single job.
///
/// The scheduler and worker-oriented unit tests own this type. It coordinates
/// in-memory job state, tool execution, and terminal persistence for one job.
pub struct Worker {
    /// Stable job identifier exposed to internal callers and unit tests.
    ///
    /// Callers use this to correlate scheduler state, context-manager lookups,
    /// and persistence assertions. Reading this field has no side effects and
    /// does not itself make any state durable.
    pub(crate) job_id: Uuid,
    deps: WorkerDeps,
}

enum WorkerLoopOutcome {
    Completed,
    ContinueDirectSelection,
    Exited,
}

impl Worker {
    /// Create a new worker for a specific job.
    pub fn new(job_id: Uuid, deps: WorkerDeps) -> Self {
        Self { job_id, deps }
    }

    // Convenience accessors to avoid deps.field everywhere
    /// Return the shared context manager for this worker's job.
    ///
    /// Internal crates and unit tests use this accessor to inspect or prepare
    /// the in-memory job state before driving the worker. This is a pure
    /// accessor: it does not persist anything and requires no rollback by the
    /// caller.
    pub(crate) fn context_manager(&self) -> &Arc<ContextManager> {
        &self.deps.context_manager
    }

    fn llm(&self) -> &Arc<dyn LlmProvider> {
        &self.deps.llm
    }

    fn tools(&self) -> &Arc<ToolRegistry> {
        &self.deps.tools
    }

    fn store(&self) -> Option<&Arc<dyn Database>> {
        self.deps.store.as_ref()
    }

    fn timeout(&self) -> Duration {
        self.deps.timeout
    }

    fn use_planning(&self) -> bool {
        self.deps.use_planning
    }
}

/// Convert a TaskOutput to a string result for tool execution.
impl From<TaskOutput> for Result<String, Error> {
    fn from(output: TaskOutput) -> Self {
        serde_json::to_string_pretty(&output.result).map_err(|e| {
            crate::error::ToolError::ExecutionFailed {
                name: "task".to_string(),
                reason: format!("Failed to serialize result: {}", e),
            }
            .into()
        })
    }
}

#[cfg(test)]
mod tests;
