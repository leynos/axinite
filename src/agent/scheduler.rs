//! Job scheduler for parallel execution.
//!
//! This module holds the `Scheduler` type and its lightweight queries;
//! behaviour is split across sibling modules:
//!
//! - `dispatch` - Job creation, persistence, and worker scheduling
//! - `subtasks` - Lightweight sub-task spawning and tool execution
//! - `shutdown` - Stopping jobs, cancellation persistence, and cleanup

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{RwLock, mpsc};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::agent::task::TaskOutput;
use crate::channels::web::types::SseEvent;
use crate::config::AgentConfig;
use crate::context::ContextManager;
use crate::db::Database;
use crate::error::{Error, JobError};
use crate::hooks::HookRegistry;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;

mod dispatch;
mod shutdown;
mod subtasks;

/// Message to send to a worker.
#[derive(Debug)]
pub enum WorkerMessage {
    /// Start working on the job.
    Start,
    /// Stop the job.
    Stop,
    /// Check health.
    Ping,
    /// Inject a follow-up user message into the worker's reasoning context.
    UserMessage(String),
}

/// Status of a scheduled job.
#[derive(Debug)]
pub struct ScheduledJob {
    pub handle: JoinHandle<()>,
    pub tx: mpsc::Sender<WorkerMessage>,
    pub pending_cancel_persist: bool,
}

/// Status of a scheduled sub-task.
struct ScheduledSubtask {
    handle: JoinHandle<Result<TaskOutput, Error>>,
}

/// Schedules and manages parallel job execution.
pub struct Scheduler {
    config: AgentConfig,
    context_manager: Arc<ContextManager>,
    llm: Arc<dyn LlmProvider>,
    safety: Arc<SafetyLayer>,
    tools: Arc<ToolRegistry>,
    store: Option<Arc<dyn Database>>,
    hooks: Arc<HookRegistry>,
    /// SSE broadcast sender for live job event streaming.
    sse_tx: Option<tokio::sync::broadcast::Sender<SseEvent>>,
    /// HTTP interceptor for trace recording/replay (propagated to workers).
    http_interceptor: Option<Arc<dyn crate::llm::recording::HttpInterceptor>>,
    /// Running jobs (main LLM-driven jobs).
    jobs: Arc<RwLock<HashMap<Uuid, ScheduledJob>>>,
    /// Running sub-tasks (tool executions, background tasks).
    subtasks: Arc<RwLock<HashMap<Uuid, ScheduledSubtask>>>,
}

impl Scheduler {
    /// Create a new scheduler.
    pub fn new(
        config: AgentConfig,
        context_manager: Arc<ContextManager>,
        llm: Arc<dyn LlmProvider>,
        safety: Arc<SafetyLayer>,
        tools: Arc<ToolRegistry>,
        store: Option<Arc<dyn Database>>,
        hooks: Arc<HookRegistry>,
    ) -> Self {
        Self {
            config,
            context_manager,
            llm,
            safety,
            tools,
            store,
            hooks,
            sse_tx: None,
            http_interceptor: None,
            jobs: Arc::new(RwLock::new(HashMap::new())),
            subtasks: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set the SSE broadcast sender for live job event streaming.
    pub fn set_sse_sender(&mut self, tx: tokio::sync::broadcast::Sender<SseEvent>) {
        self.sse_tx = Some(tx);
    }

    /// Set the HTTP interceptor for trace recording/replay.
    pub fn set_http_interceptor(
        &mut self,
        interceptor: Arc<dyn crate::llm::recording::HttpInterceptor>,
    ) {
        self.http_interceptor = Some(interceptor);
    }

    /// Send a follow-up user message to a running job.
    ///
    /// Returns `Ok(())` if the message was queued, `Err` if the job is not running.
    pub async fn send_message(&self, job_id: Uuid, content: String) -> Result<(), JobError> {
        // Clone the sender while holding the lock, then release before the
        // async send to avoid blocking scheduler writes during backpressure.
        let tx = {
            let jobs = self.jobs.read().await;
            let scheduled = jobs.get(&job_id).ok_or(JobError::NotFound { id: job_id })?;
            scheduled.tx.clone()
        };
        tx.send(WorkerMessage::UserMessage(content))
            .await
            .map_err(|_| JobError::Failed {
                id: job_id,
                reason: "Worker channel closed".to_string(),
            })?;
        Ok(())
    }

    /// Check if a job is running.
    pub async fn is_running(&self, job_id: Uuid) -> bool {
        self.jobs.read().await.contains_key(&job_id)
    }

    /// Get count of running jobs.
    pub async fn running_count(&self) -> usize {
        self.jobs.read().await.len()
    }

    /// Get count of running subtasks.
    pub async fn subtask_count(&self) -> usize {
        self.subtasks.read().await.len()
    }

    /// Get all running job IDs.
    pub async fn running_jobs(&self) -> Vec<Uuid> {
        self.jobs.read().await.keys().cloned().collect()
    }

    /// Get access to the tools registry.
    pub fn tools(&self) -> &Arc<ToolRegistry> {
        &self.tools
    }

    /// Get access to the context manager.
    pub fn context_manager(&self) -> &Arc<ContextManager> {
        &self.context_manager
    }
}

#[cfg(test)]
#[path = "scheduler/tests.rs"]
mod tests;
