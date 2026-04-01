//! Job scheduler for parallel execution.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{RwLock, mpsc, oneshot};
use tokio::task::JoinHandle;
use uuid::Uuid;

use crate::agent::task::{Task, TaskContext, TaskOutput};
use crate::channels::web::types::SseEvent;
use crate::config::AgentConfig;
use crate::context::{ContextManager, JobContext, JobState};
use crate::db::Database;
use crate::error::{Error, JobError};
use crate::hooks::HookRegistry;
use crate::llm::LlmProvider;
use crate::safety::SafetyLayer;
use crate::tools::{ApprovalContext, ToolRegistry};
use crate::worker::job::{Worker, WorkerDeps};

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

const STOP_GRACE_PERIOD: Duration = Duration::from_millis(500);

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

    /// Create, persist, and schedule a job in one shot.
    ///
    /// This is the preferred entry point for dispatching new jobs. It:
    /// 1. Creates the job context via `ContextManager`
    /// 2. Optionally applies metadata (e.g. `max_iterations`)
    /// 3. Persists the job to the database (so FK references from
    ///    `job_actions` / `llm_calls` work immediately)
    /// 4. Schedules the job for worker execution
    ///
    /// Returns the new job ID.
    pub async fn dispatch_job(
        &self,
        user_id: &str,
        title: &str,
        description: &str,
        metadata: Option<serde_json::Value>,
    ) -> Result<Uuid, JobError> {
        self.dispatch_job_inner(user_id, title, description, metadata, None)
            .await
    }

    /// Dispatch a job with an explicit approval context for autonomous execution.
    ///
    /// Same as `dispatch_job`, but the worker will use the given `ApprovalContext`
    /// to determine which tools are pre-approved (instead of blocking all non-`Never` tools).
    pub async fn dispatch_job_with_context(
        &self,
        user_id: &str,
        title: &str,
        description: &str,
        metadata: Option<serde_json::Value>,
        approval_context: ApprovalContext,
    ) -> Result<Uuid, JobError> {
        self.dispatch_job_inner(
            user_id,
            title,
            description,
            metadata,
            Some(approval_context),
        )
        .await
    }

    /// Shared implementation for `dispatch_job` and `dispatch_job_with_context`.
    async fn dispatch_job_inner(
        &self,
        user_id: &str,
        title: &str,
        description: &str,
        metadata: Option<serde_json::Value>,
        approval_context: Option<ApprovalContext>,
    ) -> Result<Uuid, JobError> {
        let job_id = self
            .context_manager
            .create_job_for_user(user_id, title, description)
            .await?;

        // Apply metadata and token budget in a single atomic update.
        // This prevents concurrent workers from observing partial state.
        // Cap user-supplied max_tokens at the configured limit (Issue #815).
        let user_max_tokens = metadata
            .as_ref()
            .and_then(|m| m.get("max_tokens"))
            .and_then(|v| v.as_u64());

        let max_tokens = user_max_tokens
            .map(|user_val| {
                if self.config.max_tokens_per_job == 0 {
                    // Config is "unlimited": use the user-supplied value directly.
                    user_val
                } else {
                    std::cmp::min(user_val, self.config.max_tokens_per_job)
                }
            })
            .unwrap_or(self.config.max_tokens_per_job);

        // Apply both metadata and token budget in one closure (Issue #813: atomic update)
        if let Some(meta) = metadata {
            self.context_manager
                .update_context(job_id, |ctx| {
                    ctx.metadata = meta;
                    if max_tokens > 0 {
                        ctx.max_tokens = max_tokens;
                    }
                })
                .await?;
        } else if max_tokens > 0 {
            self.context_manager
                .update_context(job_id, |ctx| {
                    ctx.max_tokens = max_tokens;
                })
                .await?;
        }

        // Persist to DB before scheduling so the worker's FK references are valid
        if let Some(ref store) = self.store {
            let ctx = self.context_manager.get_context(job_id).await?;
            store.save_job(&ctx).await.map_err(|e| JobError::Failed {
                id: job_id,
                reason: format!("failed to persist job: {e}"),
            })?;
        }

        self.schedule_with_context(job_id, approval_context).await?;
        Ok(job_id)
    }

    /// Schedule a job for execution.
    pub async fn schedule(&self, job_id: Uuid) -> Result<(), JobError> {
        self.schedule_with_context(job_id, None).await
    }

    /// Schedule a job with an optional approval context.
    async fn schedule_with_context(
        &self,
        job_id: Uuid,
        approval_context: Option<ApprovalContext>,
    ) -> Result<(), JobError> {
        // Hold write lock for the entire check-insert sequence to prevent
        // TOCTOU races where two concurrent calls both pass the checks.
        {
            let mut jobs = self.jobs.write().await;

            if jobs.contains_key(&job_id) {
                return Ok(());
            }

            if jobs.len() >= self.config.max_parallel_jobs {
                return Err(JobError::MaxJobsExceeded {
                    max: self.config.max_parallel_jobs,
                });
            }

            // Transition job to in_progress
            self.context_manager
                .update_context(job_id, |ctx| {
                    ctx.transition_to(
                        JobState::InProgress,
                        Some("Scheduled for execution".to_string()),
                    )
                })
                .await?
                .map_err(|s| JobError::ContextError {
                    id: job_id,
                    reason: s,
                })?;

            // Create worker channel
            let (tx, rx) = mpsc::channel(16);

            // Create worker with shared dependencies
            let deps = WorkerDeps {
                context_manager: self.context_manager.clone(),
                llm: self.llm.clone(),
                safety: self.safety.clone(),
                tools: self.tools.clone(),
                store: self.store.clone(),
                hooks: self.hooks.clone(),
                timeout: self.config.job_timeout,
                use_planning: self.config.use_planning,
                sse_tx: self.sse_tx.clone(),
                approval_context,
                http_interceptor: self.http_interceptor.clone(),
            };
            let worker = Worker::new(job_id, deps);

            // Spawn worker task
            let handle = tokio::spawn(async move {
                if let Err(e) = worker.run(rx).await {
                    tracing::error!("Worker for job {} failed: {}", job_id, e);
                }
            });

            // Start the worker
            if tx.send(WorkerMessage::Start).await.is_err() {
                tracing::error!(job_id = %job_id, "Worker died before receiving Start message");
            }

            // Insert while still holding the write lock
            jobs.insert(job_id, ScheduledJob { handle, tx });
        }

        // Cleanup task for this job to avoid capacity leaks
        let jobs = Arc::clone(&self.jobs);
        tokio::spawn(async move {
            loop {
                let finished = {
                    let jobs_read = jobs.read().await;
                    match jobs_read.get(&job_id) {
                        Some(scheduled) => scheduled.handle.is_finished(),
                        None => true,
                    }
                };

                if finished {
                    jobs.write().await.remove(&job_id);
                    break;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        tracing::info!("Scheduled job {} for execution", job_id);
        Ok(())
    }

    /// Schedule a sub-task from within a worker.
    ///
    /// Sub-tasks are lightweight tasks that don't go through the full job lifecycle.
    /// They're used for parallel tool execution and background computations.
    ///
    /// Returns a oneshot receiver to get the result.
    pub async fn spawn_subtask(
        &self,
        parent_id: Uuid,
        task: Task,
    ) -> Result<oneshot::Receiver<Result<TaskOutput, Error>>, JobError> {
        let task_id = Uuid::new_v4();
        let (result_tx, result_rx) = oneshot::channel();

        let handle = match task {
            Task::Job { .. } => {
                // Jobs should go through schedule(), not spawn_subtask
                return Err(JobError::ContextError {
                    id: parent_id,
                    reason: "Use schedule() for Job tasks, not spawn_subtask()".to_string(),
                });
            }

            Task::ToolExec {
                parent_id: tool_parent_id,
                tool_name,
                params,
            } => {
                let tools = self.tools.clone();
                let context_manager = self.context_manager.clone();
                let safety = self.safety.clone();

                // TODO: propagate parent job's ApprovalContext here when subtasks
                // are used in autonomous/routine paths (currently only used in tests).
                tokio::spawn(async move {
                    let result = Self::execute_tool_task(
                        tools,
                        context_manager,
                        safety,
                        None,
                        tool_parent_id,
                        &tool_name,
                        params,
                    )
                    .await;

                    // Send result (ignore if receiver dropped)
                    let _ = result_tx.send(result);
                })
            }

            Task::Background { id: _, handler } => {
                let ctx = TaskContext::new(task_id).with_parent(parent_id);

                tokio::spawn(async move {
                    let result = handler.run(ctx).await;
                    let _ = result_tx.send(result);
                })
            }
        };

        // Track the subtask
        self.subtasks.write().await.insert(
            task_id,
            ScheduledSubtask {
                handle: tokio::spawn(async move {
                    // Wrap the handle to get its result
                    match handle.await {
                        Ok(()) => Err(Error::Job(JobError::ContextError {
                            id: task_id,
                            reason: "Subtask completed but result not captured".to_string(),
                        })),
                        Err(e) => Err(Error::Job(JobError::ContextError {
                            id: task_id,
                            reason: format!("Subtask panicked: {}", e),
                        })),
                    }
                }),
            },
        );

        // Cleanup task for subtask tracking
        let subtasks = Arc::clone(&self.subtasks);
        tokio::spawn(async move {
            loop {
                let finished = {
                    let subtasks_read = subtasks.read().await;
                    match subtasks_read.get(&task_id) {
                        Some(scheduled) => scheduled.handle.is_finished(),
                        None => true,
                    }
                };

                if finished {
                    subtasks.write().await.remove(&task_id);
                    break;
                }

                tokio::time::sleep(Duration::from_secs(1)).await;
            }
        });

        tracing::debug!(
            parent_id = %parent_id,
            task_id = %task_id,
            "Spawned subtask"
        );

        Ok(result_rx)
    }

    /// Schedule multiple tasks in parallel and wait for all to complete.
    ///
    /// Returns results in the same order as the input tasks.
    pub async fn spawn_batch(
        &self,
        parent_id: Uuid,
        tasks: Vec<Task>,
    ) -> Vec<Result<TaskOutput, Error>> {
        if tasks.is_empty() {
            return Vec::new();
        }

        let mut receivers = Vec::with_capacity(tasks.len());

        // Spawn all tasks
        for task in tasks {
            match self.spawn_subtask(parent_id, task).await {
                Ok(rx) => receivers.push(Some(rx)),
                Err(e) => {
                    // Store the error directly
                    receivers.push(None);
                    tracing::warn!(
                        parent_id = %parent_id,
                        error = %e,
                        "Failed to spawn subtask in batch"
                    );
                }
            }
        }

        // Collect results
        let mut results = Vec::with_capacity(receivers.len());
        for rx in receivers {
            let result = match rx {
                Some(receiver) => match receiver.await {
                    Ok(task_result) => task_result,
                    Err(_) => Err(Error::Job(JobError::ContextError {
                        id: parent_id,
                        reason: "Subtask channel closed unexpectedly".to_string(),
                    })),
                },
                None => Err(Error::Job(JobError::ContextError {
                    id: parent_id,
                    reason: "Subtask failed to spawn".to_string(),
                })),
            };
            results.push(result);
        }

        results
    }

    /// Execute a single tool as a subtask.
    ///
    /// Performs scheduler-specific checks (approval, cancellation) then
    /// delegates to the shared `execute_tool_with_safety` pipeline.
    async fn execute_tool_task(
        tools: Arc<ToolRegistry>,
        context_manager: Arc<ContextManager>,
        safety: Arc<SafetyLayer>,
        approval_context: Option<ApprovalContext>,
        job_id: Uuid,
        tool_name: &str,
        params: serde_json::Value,
    ) -> Result<TaskOutput, Error> {
        let start = std::time::Instant::now();

        // Get the tool for approval check
        let tool = tools.get(tool_name).await.ok_or_else(|| {
            Error::Tool(crate::error::ToolError::NotFound {
                name: tool_name.to_string(),
            })
        })?;

        // Get job context
        let job_ctx: JobContext = context_manager.get_context(job_id).await?;
        if job_ctx.state == JobState::Cancelled {
            return Err(crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: "Job is cancelled".to_string(),
            }
            .into());
        }

        // Scheduler-specific approval check
        let requirement = tool.requires_approval(&params);
        let blocked =
            ApprovalContext::is_blocked_or_default(&approval_context, tool_name, requirement);
        if blocked {
            return Err(crate::error::ToolError::AuthRequired {
                name: tool_name.to_string(),
            }
            .into());
        }

        // Delegate to shared tool execution pipeline
        let output_str = crate::tools::execute::execute_tool_with_safety(
            &tools, &safety, tool_name, &params, &job_ctx,
        )
        .await?;

        // Parse back to Value for TaskOutput; this should be infallible given
        // `execute_tool_with_safety` uses `serde_json::to_string_pretty`, but if it
        // ever fails we surface a clear error instead of silently changing types.
        let result_value: serde_json::Value = serde_json::from_str(&output_str).map_err(|e| {
            Error::Tool(crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: format!("Failed to parse tool output as JSON: {}", e),
            })
        })?;

        Ok(TaskOutput::new(result_value, start.elapsed()))
    }

    async fn stop_in_memory(&self, job_id: Uuid, reason: &str) -> Result<(), JobError> {
        let tx = {
            let jobs = self.jobs.read().await;
            match jobs.get(&job_id) {
                Some(scheduled) => scheduled.tx.clone(),
                None => return Err(JobError::NotFound { id: job_id }),
            }
        };

        self.send_stop_signal(job_id, tx).await;

        // Give the worker a bounded window to observe the stop signal and
        // finish in-flight cleanup before we transition its state.
        tokio::time::sleep(STOP_GRACE_PERIOD).await;

        self.transition_to_cancelled(job_id, reason).await
    }

    async fn send_stop_signal(&self, job_id: Uuid, tx: mpsc::Sender<WorkerMessage>) {
        match tokio::time::timeout(
            tokio::time::Duration::from_secs(5),
            tx.send(WorkerMessage::Stop),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(error)) => {
                tracing::warn!(
                    job_id = %job_id,
                    reason = %error,
                    "Failed to send stop signal to worker"
                );
            }
            Err(_) => {
                tracing::warn!(
                    job_id = %job_id,
                    timeout_seconds = 5_u64,
                    "Timed out sending stop signal to worker"
                );
            }
        }
    }

    async fn transition_to_cancelled(&self, job_id: Uuid, reason: &str) -> Result<(), JobError> {
        self.context_manager
            .update_context(job_id, |ctx| {
                let current_state = ctx.state;
                if current_state == JobState::Cancelled {
                    return Ok(());
                }
                ctx.transition_to(JobState::Cancelled, Some(reason.to_string()))
                    .map_err(|_| current_state)
            })
            .await?
            .map_err(|from_state| JobError::InvalidTransition {
                id: job_id,
                from_state,
                target: JobState::Cancelled,
            })?;

        Ok(())
    }

    async fn persist_cancelled_status(&self, job_id: Uuid, reason: &str) -> Result<(), JobError> {
        if let Some(ref store) = self.store {
            store
                .update_job_status(job_id, JobState::Cancelled, Some(reason))
                .await
                .map_err(|e| JobError::PersistenceError {
                    id: job_id,
                    reason: e.to_string(),
                })?;
        }

        Ok(())
    }

    fn should_persist_cancelled_after_timeout(state: JobState) -> bool {
        state == JobState::Cancelled || state.can_transition_to(JobState::Cancelled)
    }

    /// Handle the case where a graceful in-memory stop timed out during
    /// `stop_all`.
    ///
    /// Attempts to force the job to `Cancelled` via
    /// `transition_to_cancelled`, then persists and finalises if the
    /// transition succeeds. Logs an appropriate warning for each failure
    /// mode.
    async fn handle_stop_timeout(
        &self,
        job_id: Uuid,
        reason: &str,
        stop_timeout: tokio::time::Duration,
    ) {
        tracing::warn!(
            job_id = %job_id,
            timeout_seconds = stop_timeout.as_secs(),
            "Timed out stopping job during shutdown"
        );
        match self.transition_to_cancelled(job_id, reason).await {
            Ok(()) => {
                if let Err(error) = self.persist_cancelled_status(job_id, reason).await {
                    tracing::warn!(
                        job_id = %job_id,
                        %error,
                        "Failed to persist cancellation after shutdown timeout"
                    );
                } else {
                    self.finalize_stop(job_id).await;
                }
            }
            Err(JobError::InvalidTransition { from_state, .. })
                if !Self::should_persist_cancelled_after_timeout(from_state) =>
            {
                tracing::warn!(
                    job_id = %job_id,
                    state = %from_state,
                    "Skipping cancellation persistence after shutdown timeout because the job state no longer permits cancellation"
                );
            }
            Err(error) => {
                tracing::warn!(
                    job_id = %job_id,
                    %error,
                    "Failed to cancel job after shutdown timeout"
                );
            }
        }
    }

    async fn finalize_stop(&self, job_id: Uuid) {
        let mut jobs = self.jobs.write().await;
        if let Some(scheduled) = jobs.get(&job_id)
            && !scheduled.handle.is_finished()
        {
            scheduled.handle.abort();
        }
        jobs.remove(&job_id);
        tracing::info!("Stopped job {}", job_id);
    }

    /// Stop a running job.
    pub async fn stop(&self, job_id: Uuid, reason: &str) -> Result<(), JobError> {
        self.stop_in_memory(job_id, reason).await?;
        self.persist_cancelled_status(job_id, reason).await?;
        self.finalize_stop(job_id).await;

        Ok(())
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

    /// Clean up finished jobs and subtasks.
    pub async fn cleanup_finished(&self) {
        // Clean up jobs
        {
            let mut jobs = self.jobs.write().await;
            let mut finished = Vec::new();

            for (id, scheduled) in jobs.iter() {
                if scheduled.handle.is_finished() {
                    finished.push(*id);
                }
            }

            for id in finished {
                jobs.remove(&id);
                tracing::debug!("Cleaned up finished job {}", id);
            }
        }

        // Clean up subtasks
        {
            let mut subtasks = self.subtasks.write().await;
            let mut finished = Vec::new();

            for (id, scheduled) in subtasks.iter() {
                if scheduled.handle.is_finished() {
                    finished.push(*id);
                }
            }

            for id in finished {
                subtasks.remove(&id);
                tracing::trace!("Cleaned up finished subtask {}", id);
            }
        }
    }

    /// Stop all jobs.
    pub async fn stop_all(&self) {
        let job_ids: Vec<Uuid> = self.jobs.read().await.keys().cloned().collect();
        let stop_timeout = tokio::time::Duration::from_secs(5);
        let stop_reason = "Stopped by scheduler";
        let stop_futures = job_ids.into_iter().map(|job_id| async move {
            (
                job_id,
                tokio::time::timeout(stop_timeout, self.stop_in_memory(job_id, stop_reason)).await,
            )
        });

        for (job_id, result) in futures::future::join_all(stop_futures).await {
            match result {
                Ok(Ok(())) => {
                    if let Err(error) = self.persist_cancelled_status(job_id, stop_reason).await {
                        tracing::warn!(
                            job_id = %job_id,
                            %error,
                            "Failed to persist cancellation during shutdown"
                        );
                    } else {
                        self.finalize_stop(job_id).await;
                    }
                }
                Ok(Err(error)) => {
                    tracing::warn!(job_id = %job_id, %error, "Failed to stop job during shutdown");
                }
                Err(_) => {
                    self.handle_stop_timeout(job_id, stop_reason, stop_timeout)
                        .await;
                }
            }
        }

        // Abort all subtasks
        let mut subtasks = self.subtasks.write().await;
        for (_, scheduled) in subtasks.drain() {
            scheduled.handle.abort();
        }
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
