//! Job worker execution via the shared `AgenticLoop`.
//!
//! Replaces `src/agent/worker.rs` with a `JobDelegate` that implements
//! `LoopDelegate`. The `Worker` struct and `WorkerDeps` remain as the
//! public API consumed by `scheduler.rs`.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::agent::agentic_loop::{
    AgenticLoopConfig, LoopOutcome, LoopSignal, NativeLoopDelegate, TextAction, run_agentic_loop,
    truncate_for_preview,
};
use crate::agent::scheduler::WorkerMessage;
use crate::agent::task::TaskOutput;
use crate::channels::web::types::SseEvent;
use crate::context::{ContextManager, JobState};
use crate::db::Database;
use crate::error::Error;
use crate::hooks::HookRegistry;
use crate::llm::{
    ActionPlan, ChatMessage, LlmProvider, Reasoning, ReasoningContext, RespondResult, ToolCall,
    ToolSelection,
};
use crate::safety::SafetyLayer;
use crate::tools::execute::process_tool_result;
use crate::tools::rate_limiter::RateLimitResult;
use crate::tools::{ApprovalContext, ToolRegistry, redact_params};

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
pub struct Worker {
    job_id: Uuid,
    deps: WorkerDeps,
}

/// Result of a tool execution with metadata for context building.
struct ToolExecResult {
    result: Result<String, Error>,
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
    fn context_manager(&self) -> &Arc<ContextManager> {
        &self.deps.context_manager
    }

    fn llm(&self) -> &Arc<dyn LlmProvider> {
        &self.deps.llm
    }

    #[allow(dead_code)]
    fn safety(&self) -> &Arc<SafetyLayer> {
        &self.deps.safety
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

    /// Persist a terminal job status before returning to the caller.
    async fn persist_status(&self, status: JobState, reason: Option<String>) -> Result<(), Error> {
        if let Some(store) = self.store() {
            let job_id = self.job_id;
            store
                .update_job_status(job_id, status, reason.as_deref())
                .await
                .map_err(|e| crate::error::JobError::PersistenceError {
                    id: job_id,
                    reason: e.to_string(),
                })?;
        }
        Ok(())
    }

    /// Fire-and-forget persistence and SSE broadcast for non-terminal job
    /// events only.
    ///
    /// `log_event` spawns the database write and does not await persistence.
    /// Terminal events must use `log_terminal_result_event`, which awaits
    /// persistence before broadcasting.
    fn log_event(&self, event_type: &str, data: serde_json::Value) {
        let job_id = self.job_id;

        // Persist to DB
        if let Some(store) = self.store() {
            let store = store.clone();
            let et = event_type.to_string();
            let d = data.clone();
            tokio::spawn(async move {
                if let Err(e) = store
                    .save_job_event(job_id, crate::db::SandboxEventType::from(et), &d)
                    .await
                {
                    tracing::warn!("Failed to persist event for job {}: {}", job_id, e);
                }
            });
        }

        self.broadcast_event(event_type, &data);
    }

    /// Persist a terminal result event before returning to the caller.
    async fn log_terminal_result_event(
        &self,
        event_type: &str,
        data: serde_json::Value,
    ) -> Result<(), Error> {
        let job_id = self.job_id;
        if let Some(store) = self.store() {
            store
                .save_job_event(job_id, crate::db::SandboxEventType::from(event_type), &data)
                .await
                .map_err(|e| crate::error::JobError::PersistenceError {
                    id: job_id,
                    reason: e.to_string(),
                })?;
        }

        self.broadcast_event(event_type, &data);
        Ok(())
    }

    fn broadcast_event(&self, event_type: &str, data: &serde_json::Value) {
        // Broadcast SSE for live web UI updates
        if let Some(ref tx) = self.deps.sse_tx {
            let job_id_str = self.job_id.to_string();
            let event = match event_type {
                "message" => Some(SseEvent::JobMessage {
                    job_id: job_id_str,
                    role: data
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("assistant")
                        .to_string(),
                    content: data
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "tool_use" => Some(SseEvent::JobToolUse {
                    job_id: job_id_str,
                    tool_name: data
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    input: data
                        .get("input")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null),
                }),
                "tool_result" => Some(SseEvent::JobToolResult {
                    job_id: job_id_str,
                    tool_name: data
                        .get("tool_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string(),
                    output: data
                        .get("output")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "status" => Some(SseEvent::JobStatus {
                    job_id: job_id_str,
                    message: data
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string(),
                }),
                "result" => Some(SseEvent::JobResult {
                    job_id: job_id_str,
                    status: data
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("completed")
                        .to_string(),
                    session_id: data
                        .get("session_id")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string()),
                }),
                _ => None,
            };
            if let Some(event) = event {
                let _ = tx.send(event);
            }
        }
    }

    /// Run the worker until the job is complete or stopped.
    pub async fn run(self, mut rx: mpsc::Receiver<WorkerMessage>) -> Result<(), Error> {
        tracing::info!("Worker starting for job {}", self.job_id);

        // Wait for start signal
        match rx.recv().await {
            Some(WorkerMessage::Start) => {}
            Some(WorkerMessage::Stop) | None => {
                tracing::debug!("Worker for job {} stopped before starting", self.job_id);
                return Ok(());
            }
            Some(WorkerMessage::Ping) | Some(WorkerMessage::UserMessage(_)) => {}
        }

        // Get job context
        let job_ctx = self.context_manager().get_context(self.job_id).await?;

        // Create reasoning engine
        let reasoning =
            Reasoning::new(self.llm().clone()).with_model_name(self.llm().active_model_name());

        // Build initial reasoning context (tool definitions refreshed each iteration in execution_loop)
        let mut reason_ctx = ReasoningContext::new().with_job(&job_ctx.description);

        // Add system message
        reason_ctx.messages.push(ChatMessage::system(format!(
            r#"You are an autonomous agent working on a job.

Job: {}
Description: {}

You have access to tools to complete this job. Plan your approach and execute tools as needed.
You may request multiple tools at once if they can be executed in parallel.
Report when the job is complete or if you encounter issues you cannot resolve."#,
            job_ctx.title, job_ctx.description
        )));

        // Main execution loop with timeout
        let result = tokio::time::timeout(self.timeout(), async {
            self.execution_loop(&mut rx, &reasoning, &mut reason_ctx)
                .await
        })
        .await;

        match result {
            Ok(Ok(WorkerLoopOutcome::Completed)) => {
                tracing::info!("Worker for job {} completed successfully", self.job_id);
                // Only mark completed if still in an active, non-stuck state.
                let current_state = self
                    .context_manager()
                    .get_context(self.job_id)
                    .await
                    .map(|ctx| ctx.state);
                match current_state {
                    Ok(state) if state.is_terminal() => {}
                    Ok(JobState::Completed) => {}
                    Ok(JobState::Stuck) => {
                        tracing::info!(
                            "Job {} returned Ok but is Stuck — leaving for self-repair",
                            self.job_id
                        );
                    }
                    Ok(_) => {
                        self.mark_completed().await?;
                    }
                    Err(e) => {
                        tracing::warn!(
                            job_id = %self.job_id,
                            "Failed to get job context, cannot mark as completed: {}", e
                        );
                    }
                }
            }
            Ok(Ok(WorkerLoopOutcome::Exited)) => {}
            Ok(Ok(WorkerLoopOutcome::ContinueDirectSelection)) => {
                unreachable!("execution_loop should not return ContinueDirectSelection");
            }
            Ok(Err(e)) => {
                tracing::error!("Worker for job {} failed: {}", self.job_id, e);
                let reason = match e {
                    Error::Job(crate::error::JobError::Failed { reason, .. }) => reason,
                    other => other.to_string(),
                };
                self.mark_failed(&reason).await?;
            }
            Err(_) => {
                tracing::warn!("Worker for job {} timed out", self.job_id);
                self.mark_stuck("Execution timeout").await?;
            }
        }

        Ok(())
    }

    /// Generate an execution plan when planning is enabled.
    ///
    /// Returns `None` when planning is disabled or when the planner fails (in
    /// which case a warning is logged and direct tool selection is used
    /// instead).
    async fn generate_plan(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Option<ActionPlan> {
        if !self.use_planning() {
            return None;
        }
        match reasoning.plan(reason_ctx).await {
            Ok(p) => {
                tracing::info!(
                    "Created plan for job {}: {} actions, {:.0}% confidence",
                    self.job_id,
                    p.actions.len(),
                    p.confidence * 100.0
                );
                reason_ctx.messages.push(ChatMessage::assistant(format!(
                    "I've created a plan to accomplish this goal: {}\n\nSteps:\n{}",
                    p.goal,
                    p.actions
                        .iter()
                        .enumerate()
                        .map(|(i, a)| format!("{}. {} - {}", i + 1, a.tool_name, a.reasoning))
                        .collect::<Vec<_>>()
                        .join("\n")
                )));
                self.log_event(
                    "message",
                    serde_json::json!({
                        "role": "assistant",
                        "content": format!(
                            "Plan: {}\n\n{}",
                            p.goal,
                            p.actions
                                .iter()
                                .enumerate()
                                .map(|(i, a)| format!("{}. {} - {}", i + 1, a.tool_name, a.reasoning))
                                .collect::<Vec<_>>()
                                .join("\n")
                        ),
                    }),
                );
                Some(p)
            }
            Err(e) => {
                tracing::warn!(
                    "Planning failed for job {}, falling back to direct selection: {}",
                    self.job_id,
                    e
                );
                None
            }
        }
    }

    /// Run the planning phase and, if a plan is produced, execute it.
    ///
    /// Returns `Ok(Some(outcome))` when the caller should terminate with
    /// `outcome`, or `Ok(None)` when the loop should continue with direct tool
    /// selection.
    async fn maybe_plan_and_execute(
        &self,
        rx: &mut mpsc::Receiver<WorkerMessage>,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<Option<WorkerLoopOutcome>, Error> {
        let Some(plan) = self.generate_plan(reasoning, reason_ctx).await else {
            return Ok(None);
        };

        match self.execute_plan(rx, reasoning, reason_ctx, &plan).await? {
            WorkerLoopOutcome::Completed => return Ok(Some(WorkerLoopOutcome::Completed)),
            WorkerLoopOutcome::Exited => return Ok(Some(WorkerLoopOutcome::Exited)),
            WorkerLoopOutcome::ContinueDirectSelection => {}
        }

        if let Ok(ctx) = self.context_manager().get_context(self.job_id).await
            && (ctx.state.is_terminal()
                || ctx.state == JobState::Stuck
                || ctx.state == JobState::Completed)
        {
            return Ok(Some(WorkerLoopOutcome::Exited));
        }

        Ok(None)
    }

    async fn execution_loop(
        &self,
        rx: &mut mpsc::Receiver<WorkerMessage>,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<WorkerLoopOutcome, Error> {
        const MAX_WORKER_ITERATIONS: usize = 500;
        let max_iterations = self
            .context_manager()
            .get_context(self.job_id)
            .await
            .ok()
            .and_then(|ctx| ctx.metadata.get("max_iterations").and_then(|v| v.as_u64()))
            .unwrap_or(50) as usize;
        let max_iterations = max_iterations.min(MAX_WORKER_ITERATIONS);

        // Initial tool definitions for planning (will be refreshed in loop)
        reason_ctx.available_tools = self.tools().tool_definitions().await;

        if let Some(outcome) = self
            .maybe_plan_and_execute(rx, reasoning, reason_ctx)
            .await?
        {
            return Ok(outcome);
        }

        // Build the delegate and run the shared agentic loop
        let delegate = JobDelegate {
            worker: self,
            rx: tokio::sync::Mutex::new(rx),
            consecutive_rate_limits: std::sync::atomic::AtomicUsize::new(0),
        };

        let config = AgenticLoopConfig {
            max_iterations,
            enable_tool_intent_nudge: true,
            max_tool_intent_nudges: 2,
        };

        let outcome = run_agentic_loop(&delegate, reasoning, reason_ctx, &config).await?;

        match outcome {
            LoopOutcome::Response(_) => Ok(WorkerLoopOutcome::Completed),
            LoopOutcome::MaxIterations => Err(crate::error::JobError::Failed {
                id: self.job_id,
                reason: "Maximum iterations exceeded: job hit the iteration cap".to_string(),
            }
            .into()),
            LoopOutcome::Stopped | LoopOutcome::NeedApproval(_) => Ok(WorkerLoopOutcome::Exited),
        }
    }

    /// Execute multiple tools in parallel using a JoinSet.
    ///
    /// Each task is tagged with its original index so results are returned
    /// in the same order as `selections`, regardless of completion order.
    async fn execute_tools_parallel(&self, selections: &[ToolSelection]) -> Vec<ToolExecResult> {
        let count = selections.len();

        // Short-circuit for single tool: execute directly without JoinSet overhead
        if count <= 1 {
            let mut results = Vec::with_capacity(count);
            for selection in selections {
                let result = Self::execute_tool_inner(
                    &self.deps,
                    self.job_id,
                    &selection.tool_name,
                    &selection.parameters,
                )
                .await;
                results.push(ToolExecResult { result });
            }
            return results;
        }

        let mut join_set = JoinSet::new();

        for (idx, selection) in selections.iter().enumerate() {
            let deps = self.deps.clone();
            let job_id = self.job_id;
            let tool_name = selection.tool_name.clone();
            let params = selection.parameters.clone();
            join_set.spawn(async move {
                let result = Self::execute_tool_inner(&deps, job_id, &tool_name, &params).await;
                (idx, ToolExecResult { result })
            });
        }

        // Collect and reorder by original index
        let mut results: Vec<Option<ToolExecResult>> = (0..count).map(|_| None).collect();
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((idx, exec_result)) => results[idx] = Some(exec_result),
                Err(e) => {
                    if e.is_panic() {
                        tracing::error!("Tool execution task panicked: {}", e);
                    } else {
                        tracing::error!("Tool execution task cancelled: {}", e);
                    }
                }
            }
        }

        // Fill any panicked slots with error results
        results
            .into_iter()
            .enumerate()
            .map(|(i, opt)| {
                opt.unwrap_or_else(|| ToolExecResult {
                    result: Err(crate::error::ToolError::ExecutionFailed {
                        name: selections[i].tool_name.clone(),
                        reason: "Task failed during execution".to_string(),
                    }
                    .into()),
                })
            })
            .collect()
    }

    /// Inner tool execution logic that can be called from both single and parallel paths.
    async fn execute_tool_inner(
        deps: &WorkerDeps,
        job_id: Uuid,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, Error> {
        let tool =
            deps.tools
                .get(tool_name)
                .await
                .ok_or_else(|| crate::error::ToolError::NotFound {
                    name: tool_name.to_string(),
                })?;

        // Check approval: use context-aware check if available, else block all non-Never tools
        let requirement = tool.requires_approval(params);
        let blocked =
            ApprovalContext::is_blocked_or_default(&deps.approval_context, tool_name, requirement);
        if blocked {
            return Err(crate::error::ToolError::AuthRequired {
                name: tool_name.to_string(),
            }
            .into());
        }

        // Fetch job context early so we have the real user_id for hooks and rate limiting
        let mut job_ctx = deps.context_manager.get_context(job_id).await?;
        // Propagate http_interceptor for trace recording/replay
        if job_ctx.http_interceptor.is_none() {
            job_ctx.http_interceptor = deps.http_interceptor.clone();
        }

        // Check per-tool rate limit before running hooks or executing (cheaper check first)
        if let Some(config) = tool.rate_limit_config()
            && let RateLimitResult::Limited { retry_after, .. } = deps
                .tools
                .rate_limiter()
                .check_and_record(&job_ctx.user_id, tool_name, &config)
                .await
        {
            return Err(crate::error::ToolError::RateLimited {
                name: tool_name.to_string(),
                retry_after: Some(retry_after),
            }
            .into());
        }

        // Run BeforeToolCall hook
        let params = {
            use crate::hooks::{HookError, HookEvent, HookOutcome};
            let hook_params = redact_params(params, tool.sensitive_params());
            let event = HookEvent::ToolCall {
                tool_name: tool_name.to_string(),
                parameters: hook_params,
                user_id: job_ctx.user_id.clone(),
                context: format!("job:{}", job_id),
            };
            match deps.hooks.run(&event).await {
                Err(HookError::Rejected { reason }) => {
                    return Err(crate::error::ToolError::ExecutionFailed {
                        name: tool_name.to_string(),
                        reason: format!("Blocked by hook: {}", reason),
                    }
                    .into());
                }
                Err(err) => {
                    return Err(crate::error::ToolError::ExecutionFailed {
                        name: tool_name.to_string(),
                        reason: format!("Blocked by hook failure mode: {}", err),
                    }
                    .into());
                }
                Ok(HookOutcome::Continue {
                    modified: Some(new_params),
                }) => serde_json::from_str(&new_params).unwrap_or_else(|e| {
                    tracing::warn!(
                        tool = %tool_name,
                        "Hook returned non-JSON modification for ToolCall, ignoring: {}",
                        e
                    );
                    params.clone()
                }),
                _ => params.clone(),
            }
        };
        if job_ctx.state == JobState::Cancelled {
            return Err(crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: "Job is cancelled".to_string(),
            }
            .into());
        }

        // Validate tool parameters
        let validation = deps.safety.validator().validate_tool_params(&params);
        if !validation.is_valid {
            let details = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(crate::error::ToolError::InvalidParameters {
                name: tool_name.to_string(),
                reason: format!("Invalid tool parameters: {}", details),
            }
            .into());
        }

        // Redact sensitive parameter values before they touch any observability or audit path.
        let safe_params = redact_params(&params, tool.sensitive_params());
        tracing::debug!(
            tool = %tool_name,
            params = %safe_params,
            job = %job_id,
            "Tool call started"
        );

        // Execute with per-tool timeout and timing
        let tool_timeout = tool.execution_timeout();
        let start = std::time::Instant::now();
        let result = tokio::time::timeout(tool_timeout, async {
            tool.execute(params.clone(), &job_ctx).await
        })
        .await;
        let elapsed = start.elapsed();

        match &result {
            Ok(Ok(output)) => {
                let result_size = serde_json::to_string(&output.result)
                    .map(|s| s.len())
                    .unwrap_or(0);
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    result_size_bytes = result_size,
                    "Tool call succeeded"
                );
            }
            Ok(Err(e)) => {
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    error = %e,
                    "Tool call failed"
                );
            }
            Err(_) => {
                tracing::debug!(
                    tool = %tool_name,
                    elapsed_ms = elapsed.as_millis() as u64,
                    timeout_secs = tool_timeout.as_secs(),
                    "Tool call timed out"
                );
            }
        }

        // Record action in memory and get the ActionRecord for persistence
        let action = match &result {
            Ok(Ok(output)) => {
                let output_str = serde_json::to_string_pretty(&output.result)
                    .ok()
                    .map(|s| deps.safety.sanitize_tool_output(tool_name, &s).content);
                match deps
                    .context_manager
                    .update_memory(job_id, |mem| {
                        let rec = mem.create_action(tool_name, safe_params.clone()).succeed(
                            output_str.clone(),
                            output.result.clone(),
                            elapsed,
                        );
                        mem.record_action(rec.clone());
                        rec
                    })
                    .await
                {
                    Ok(rec) => Some(rec),
                    Err(e) => {
                        tracing::warn!(job_id = %job_id, tool = tool_name, "Failed to record action in memory: {e}");
                        None
                    }
                }
            }
            Ok(Err(e)) => {
                match deps
                    .context_manager
                    .update_memory(job_id, |mem| {
                        let rec = mem
                            .create_action(tool_name, safe_params.clone())
                            .fail(e.to_string(), elapsed);
                        mem.record_action(rec.clone());
                        rec
                    })
                    .await
                {
                    Ok(rec) => Some(rec),
                    Err(e) => {
                        tracing::warn!(job_id = %job_id, tool = tool_name, "Failed to record action in memory: {e}");
                        None
                    }
                }
            }
            Err(_) => {
                match deps
                    .context_manager
                    .update_memory(job_id, |mem| {
                        let rec = mem
                            .create_action(tool_name, safe_params.clone())
                            .fail("Execution timeout", elapsed);
                        mem.record_action(rec.clone());
                        rec
                    })
                    .await
                {
                    Ok(rec) => Some(rec),
                    Err(e) => {
                        tracing::warn!(job_id = %job_id, tool = tool_name, "Failed to record action in memory: {e}");
                        None
                    }
                }
            }
        };

        // Persist action to database (fire-and-forget)
        if let (Some(action), Some(store)) = (action, deps.store.clone()) {
            tokio::spawn(async move {
                if let Err(e) = store.save_action(job_id, &action).await {
                    tracing::warn!("Failed to persist action for job {}: {}", job_id, e);
                }
            });
        }

        // Handle the result
        let output = result
            .map_err(|_| crate::error::ToolError::Timeout {
                name: tool_name.to_string(),
                timeout: tool_timeout,
            })?
            .map_err(|e| crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: e.to_string(),
            })?;

        // Return result as string
        serde_json::to_string_pretty(&output.result).map_err(|e| {
            crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: format!("Failed to serialize result: {}", e),
            }
            .into()
        })
    }

    /// Process a tool execution result and add it to the reasoning context.
    async fn process_tool_result_job(
        &self,
        reason_ctx: &mut ReasoningContext,
        selection: &ToolSelection,
        result: Result<String, Error>,
    ) -> Result<(), Error> {
        self.log_event(
            "tool_use",
            serde_json::json!({
                "tool_name": selection.tool_name,
                "input": truncate_for_preview(
                    &selection.parameters.to_string(), 500),
            }),
        );

        // Use shared result processing for sanitize → wrap → ChatMessage.
        // The wrapped content (XML tags) goes into reason_ctx for the LLM.
        // The raw sanitized content goes into events/SSE for human-readable UI.
        let (_wrapped, message) = process_tool_result(
            &self.deps.safety,
            &selection.tool_name,
            &selection.tool_call_id,
            &result,
        );
        reason_ctx.messages.push(message);

        match &result {
            Ok(raw_output) => {
                let sanitized = self
                    .deps
                    .safety
                    .sanitize_tool_output(&selection.tool_name, raw_output);
                self.log_event(
                    "tool_result",
                    serde_json::json!({
                        "tool_name": selection.tool_name,
                        "success": true,
                        "output": truncate_for_preview(&sanitized.content, 500),
                    }),
                );
                Ok(())
            }
            Err(e) => {
                tracing::warn!(
                    "Tool {} failed for job {}: {}",
                    selection.tool_name,
                    self.job_id,
                    e
                );

                // Record failure for self-repair tracking
                if let Some(store) = self.store() {
                    let store = store.clone();
                    let tool_name = selection.tool_name.clone();
                    let error_msg = e.to_string();
                    tokio::spawn(async move {
                        if let Err(db_err) = store.record_tool_failure(&tool_name, &error_msg).await
                        {
                            tracing::warn!("Failed to record tool failure: {}", db_err);
                        }
                    });
                }

                self.log_event(
                    "tool_result",
                    serde_json::json!({
                        "tool_name": selection.tool_name,
                        "success": false,
                        "output": truncate_for_preview(&format!("Error: {}", e), 500),
                    }),
                );

                Ok(())
            }
        }
    }

    /// Execute a pre-generated plan.
    async fn execute_plan(
        &self,
        rx: &mut mpsc::Receiver<WorkerMessage>,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
        plan: &ActionPlan,
    ) -> Result<WorkerLoopOutcome, Error> {
        for (i, action) in plan.actions.iter().enumerate() {
            // Check for stop signal and injected user messages
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMessage::Stop => {
                        tracing::debug!(
                            "Worker for job {} received stop signal during plan execution",
                            self.job_id
                        );
                        return Ok(WorkerLoopOutcome::Exited);
                    }
                    WorkerMessage::Ping => {
                        tracing::trace!("Worker for job {} received ping", self.job_id);
                    }
                    WorkerMessage::Start => {}
                    WorkerMessage::UserMessage(content) => {
                        tracing::info!(
                            job_id = %self.job_id,
                            "User message received during plan execution, abandoning plan"
                        );
                        reason_ctx.messages.push(ChatMessage::user(&content));
                        self.log_event(
                            "message",
                            serde_json::json!({
                                "role": "user",
                                "content": content,
                            }),
                        );
                        self.log_event(
                            "status",
                            serde_json::json!({
                                "message": "Plan interrupted by user message, re-evaluating...",
                            }),
                        );
                        return Ok(WorkerLoopOutcome::ContinueDirectSelection);
                    }
                }
            }

            tracing::debug!(
                "Job {} executing planned action {}/{}: {} - {}",
                self.job_id,
                i + 1,
                plan.actions.len(),
                action.tool_name,
                action.reasoning
            );

            let selection = ToolSelection {
                tool_name: action.tool_name.clone(),
                parameters: action.parameters.clone(),
                reasoning: action.reasoning.clone(),
                alternatives: vec![],
                tool_call_id: format!("plan_{}_{}", self.job_id, i),
            };

            reason_ctx
                .messages
                .push(ChatMessage::assistant_with_tool_calls(
                    None,
                    vec![ToolCall {
                        id: selection.tool_call_id.clone(),
                        name: selection.tool_name.clone(),
                        arguments: selection.parameters.clone(),
                    }],
                ));

            let result = self
                .execute_tool(&action.tool_name, &action.parameters)
                .await;

            self.process_tool_result_job(reason_ctx, &selection, result)
                .await?;

            tokio::time::sleep(Duration::from_millis(100)).await;
        }

        // Plan completed, check with LLM if job is done
        reason_ctx.messages.push(ChatMessage::user(
            "All planned actions have been executed. Is the job complete? If not, what else needs to be done?",
        ));

        let response = reasoning.respond(reason_ctx).await?;
        reason_ctx.messages.push(ChatMessage::assistant(&response));

        if crate::util::llm_signals_completion(&response) {
            return Ok(WorkerLoopOutcome::Completed);
        } else {
            tracing::info!(
                "Job {} plan completed but work remains, falling back to direct selection",
                self.job_id
            );
            self.log_event(
                "status",
                serde_json::json!({
                    "message": "Plan completed but job needs more work, continuing...",
                }),
            );
        }

        Ok(WorkerLoopOutcome::ContinueDirectSelection)
    }

    async fn execute_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, Error> {
        Self::execute_tool_inner(&self.deps, self.job_id, tool_name, params).await
    }

    async fn mark_completed(&self) -> Result<(), Error> {
        self.context_manager()
            .update_context(self.job_id, |ctx| {
                ctx.transition_to(
                    JobState::Completed,
                    Some("Job completed successfully".to_string()),
                )
            })
            .await?
            .map_err(|s| crate::error::JobError::ContextError {
                id: self.job_id,
                reason: s,
            })?;

        self.log_terminal_result_event(
            "result",
            serde_json::json!({
                "status": "completed",
                "success": true,
                "message": "Job completed successfully",
            }),
        )
        .await?;
        self.persist_status(
            JobState::Completed,
            Some("Job completed successfully".to_string()),
        )
        .await?;
        Ok(())
    }

    async fn mark_failed(&self, reason: &str) -> Result<(), Error> {
        self.context_manager()
            .update_context(self.job_id, |ctx| {
                ctx.transition_to(JobState::Failed, Some(reason.to_string()))
            })
            .await?
            .map_err(|s| crate::error::JobError::ContextError {
                id: self.job_id,
                reason: s,
            })?;

        self.log_terminal_result_event(
            "result",
            serde_json::json!({
                "status": "failed",
                "success": false,
                "message": format!("Execution failed: {}", reason),
            }),
        )
        .await?;
        self.persist_status(JobState::Failed, Some(reason.to_string()))
            .await?;
        Ok(())
    }

    async fn mark_stuck(&self, reason: &str) -> Result<(), Error> {
        self.context_manager()
            .update_context(self.job_id, |ctx| ctx.mark_stuck(reason))
            .await?
            .map_err(|s| crate::error::JobError::ContextError {
                id: self.job_id,
                reason: s,
            })?;

        self.log_terminal_result_event(
            "result",
            serde_json::json!({
                "status": "stuck",
                "success": false,
                "message": format!("Job stuck: {}", reason),
            }),
        )
        .await?;
        self.persist_status(JobState::Stuck, Some(reason.to_string()))
            .await?;
        Ok(())
    }
}

/// Job delegate: implements `LoopDelegate` for the background job context.
///
/// Handles: signal channel (stop/ping/user messages), cancellation checks,
/// rate-limit retry, parallel tool execution, DB persistence, SSE broadcasting.
struct JobDelegate<'a> {
    worker: &'a Worker,
    rx: tokio::sync::Mutex<&'a mut mpsc::Receiver<WorkerMessage>>,
    /// Tracks consecutive rate-limit errors to fail fast instead of burning iterations.
    consecutive_rate_limits: std::sync::atomic::AtomicUsize,
}

impl<'a> JobDelegate<'a> {
    const MAX_CONSECUTIVE_RATE_LIMITS: usize = 10;

    /// Handle a rate-limit error: back off, increment counter, and fail fast
    /// if the provider remains rate-limited for too many consecutive attempts.
    async fn handle_rate_limit(
        &self,
        retry_after: Option<Duration>,
        context: &str,
    ) -> Result<crate::llm::RespondOutput, crate::error::Error> {
        use std::sync::atomic::Ordering::Relaxed;

        let count = self.consecutive_rate_limits.fetch_add(1, Relaxed) + 1;
        let wait = retry_after.unwrap_or(Duration::from_secs(5));
        tracing::warn!(
            job_id = %self.worker.job_id,
            wait_secs = wait.as_secs(),
            attempt = count,
            "LLM rate limited during {}, backing off",
            context,
        );

        if count >= Self::MAX_CONSECUTIVE_RATE_LIMITS {
            return Err(crate::error::JobError::Failed {
                id: self.worker.job_id,
                reason: "Persistent rate limiting: exceeded retry limit".to_string(),
            }
            .into());
        }

        self.worker.log_event(
            "status",
            serde_json::json!({
                "message": format!(
                    "Rate limited, retrying in {}s... ({}/{})",
                    wait.as_secs(), count, Self::MAX_CONSECUTIVE_RATE_LIMITS
                ),
            }),
        );
        tokio::time::sleep(wait).await;

        Ok(crate::llm::RespondOutput {
            result: RespondResult::Text(String::new()),
            usage: crate::llm::TokenUsage::default(),
        })
    }
}

impl<'a> NativeLoopDelegate for JobDelegate<'a> {
    async fn check_signals(&self) -> LoopSignal {
        // Drain the entire message channel, prioritizing Stop over user messages.
        // Scope the lock so it's dropped before any .await below.
        let mut stop_requested = false;
        let mut first_user_message: Option<String> = None;
        {
            let mut rx = self.rx.lock().await;
            while let Ok(msg) = rx.try_recv() {
                match msg {
                    WorkerMessage::Stop => {
                        tracing::debug!(
                            "Worker for job {} received stop signal",
                            self.worker.job_id
                        );
                        stop_requested = true;
                    }
                    WorkerMessage::Ping => {
                        tracing::trace!("Worker for job {} received ping", self.worker.job_id);
                    }
                    WorkerMessage::Start => {}
                    WorkerMessage::UserMessage(content) => {
                        tracing::info!(
                            job_id = %self.worker.job_id,
                            "Worker received follow-up user message"
                        );
                        self.worker.log_event(
                            "message",
                            serde_json::json!({
                                "role": "user",
                                "content": content,
                            }),
                        );
                        // Keep only the first user message; subsequent ones will be
                        // picked up on the next iteration's drain.
                        if first_user_message.is_none() {
                            first_user_message = Some(content);
                        }
                    }
                }
            }
        } // MutexGuard dropped here, before the cancellation .await

        // Stop takes priority over user messages
        if stop_requested {
            return LoopSignal::Stop;
        }

        if let Some(content) = first_user_message {
            return LoopSignal::InjectMessage(content);
        }

        // Check for terminal or non-progressing state. The loop should stop when the
        // job has been cancelled, failed, stuck, or already completed — not just the
        // three states that `is_terminal()` covers (Accepted/Failed/Cancelled).
        if let Ok(ctx) = self
            .worker
            .context_manager()
            .get_context(self.worker.job_id)
            .await
            && matches!(
                ctx.state,
                JobState::Cancelled
                    | JobState::Failed
                    | JobState::Stuck
                    | JobState::Completed
                    | JobState::Submitted
                    | JobState::Accepted
            )
        {
            tracing::info!(
                "Worker for job {} detected terminal state {:?}",
                self.worker.job_id,
                ctx.state,
            );
            return LoopSignal::Stop;
        }

        LoopSignal::Continue
    }

    async fn before_llm_call(
        &self,
        reason_ctx: &mut ReasoningContext,
        _iteration: usize,
    ) -> Option<LoopOutcome> {
        // Refresh tool definitions so newly built tools become visible
        reason_ctx.available_tools = self.worker.tools().tool_definitions().await;
        None
    }

    async fn call_llm(
        &self,
        reasoning: &Reasoning,
        reason_ctx: &mut ReasoningContext,
        _iteration: usize,
    ) -> Result<crate::llm::RespondOutput, crate::error::Error> {
        // Try select_tools first, fall back to respond_with_tools
        match reasoning.select_tools(reason_ctx).await {
            Ok(s) if !s.is_empty() => {
                // Reset counter after a successful LLM call
                self.consecutive_rate_limits
                    .store(0, std::sync::atomic::Ordering::Relaxed);
                let tool_calls: Vec<ToolCall> = selections_to_tool_calls(&s);
                return Ok(crate::llm::RespondOutput {
                    result: RespondResult::ToolCalls {
                        tool_calls,
                        content: None,
                    },
                    usage: crate::llm::TokenUsage::default(),
                });
            }
            Ok(_) => {} // empty selections, fall through
            Err(crate::error::LlmError::RateLimited { retry_after, .. }) => {
                return self.handle_rate_limit(retry_after, "tool selection").await;
            }
            Err(e) => return Err(e.into()),
        };

        // Fall back to respond_with_tools
        match reasoning.respond_with_tools(reason_ctx).await {
            Ok(output) => {
                // Reset counter after a successful LLM call
                self.consecutive_rate_limits
                    .store(0, std::sync::atomic::Ordering::Relaxed);

                // Track token usage against the job budget.
                // NOTE: select_tools() also makes LLM calls but doesn't expose
                // TokenUsage; only respond_with_tools() usage is tracked here.
                let total_tokens = output.usage.total() as u64;
                if total_tokens > 0
                    && let Err(msg) = self
                        .worker
                        .context_manager()
                        .update_context(self.worker.job_id, |ctx| ctx.add_tokens(total_tokens))
                        .await?
                {
                    return Err(crate::error::JobError::Failed {
                        id: self.worker.job_id,
                        reason: msg,
                    }
                    .into());
                }

                Ok(output)
            }
            Err(crate::error::LlmError::RateLimited { retry_after, .. }) => {
                self.handle_rate_limit(retry_after, "respond_with_tools")
                    .await
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn handle_text_response(
        &self,
        text: &str,
        reason_ctx: &mut ReasoningContext,
    ) -> TextAction {
        // Empty text from rate-limit backoff retry — skip processing and let the
        // loop proceed to the next iteration which will re-call the LLM.
        if text.is_empty() {
            return TextAction::Continue;
        }

        // Check for explicit completion
        if crate::util::llm_signals_completion(text) {
            return TextAction::Return(LoopOutcome::Response(text.to_string()));
        }

        // Add assistant response to context
        reason_ctx.messages.push(ChatMessage::assistant(text));

        self.worker.log_event(
            "message",
            serde_json::json!({
                "role": "assistant",
                "content": text,
            }),
        );

        TextAction::Continue
    }

    async fn execute_tool_calls(
        &self,
        tool_calls: Vec<crate::llm::ToolCall>,
        content: Option<String>,
        reason_ctx: &mut ReasoningContext,
    ) -> Result<Option<LoopOutcome>, crate::error::Error> {
        if let Some(ref text) = content {
            self.worker.log_event(
                "message",
                serde_json::json!({
                    "role": "assistant",
                    "content": text,
                }),
            );
        }

        // Add assistant message with tool_calls (OpenAI protocol)
        reason_ctx
            .messages
            .push(ChatMessage::assistant_with_tool_calls(
                content,
                tool_calls.clone(),
            ));

        // Convert to ToolSelections
        let selections: Vec<ToolSelection> = tool_calls
            .iter()
            .map(|tc| ToolSelection {
                tool_name: tc.name.clone(),
                parameters: tc.arguments.clone(),
                reasoning: String::new(),
                alternatives: vec![],
                tool_call_id: tc.id.clone(),
            })
            .collect();

        // Execute tools (parallel for multiple, direct for single)
        if selections.len() == 1 {
            let selection = &selections[0];
            let result = self
                .worker
                .execute_tool(&selection.tool_name, &selection.parameters)
                .await;
            self.worker
                .process_tool_result_job(reason_ctx, selection, result)
                .await?;
        } else {
            let results = self.worker.execute_tools_parallel(&selections).await;
            for (selection, result) in selections.iter().zip(results) {
                self.worker
                    .process_tool_result_job(reason_ctx, selection, result.result)
                    .await?;
            }
        }

        Ok(None)
    }

    async fn on_tool_intent_nudge(&self, text: &str, _reason_ctx: &mut ReasoningContext) {
        self.worker.log_event(
            "message",
            serde_json::json!({
                "role": "assistant",
                "content": truncate_for_preview(text, 2000),
                "nudge": true,
            }),
        );
    }

    async fn after_iteration(&self, _iteration: usize) {
        // Small delay between iterations
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// Convert `ToolSelection`s to `ToolCall`s.
fn selections_to_tool_calls(selections: &[ToolSelection]) -> Vec<ToolCall> {
    selections
        .iter()
        .map(|s| ToolCall {
            id: s.tool_call_id.clone(),
            name: s.tool_name.clone(),
            arguments: s.parameters.clone(),
        })
        .collect()
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
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::config::SafetyConfig;
    use crate::context::JobContext;
    use crate::llm::ToolSelection;
    use crate::llm::{
        CompletionRequest, CompletionResponse, ToolCompletionRequest, ToolCompletionResponse,
    };
    use crate::safety::SafetyLayer;
    use crate::tools::{NativeTool, Tool, ToolError as ToolExecError, ToolOutput};
    use tokio::sync::Mutex;

    /// A test tool that sleeps for a configurable duration before returning.
    struct SlowTool {
        tool_name: String,
        delay: Duration,
        current_active: Arc<AtomicUsize>,
        max_active: Arc<AtomicUsize>,
    }

    impl NativeTool for SlowTool {
        fn name(&self) -> &str {
            &self.tool_name
        }
        fn description(&self) -> &str {
            "Test tool with configurable delay"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &JobContext,
        ) -> Result<ToolOutput, ToolExecError> {
            let active = self.current_active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            let start = std::time::Instant::now();
            tokio::time::sleep(self.delay).await;
            self.current_active.fetch_sub(1, Ordering::SeqCst);
            Ok(ToolOutput::text(
                format!("done_{}", self.tool_name),
                start.elapsed(),
            ))
        }
        fn requires_sanitization(&self) -> bool {
            false
        }
    }

    /// Stub LLM provider (never called in these tests).
    struct StubLlm;

    impl crate::llm::NativeLlmProvider for StubLlm {
        fn model_name(&self) -> &str {
            "stub"
        }
        fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
            (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
        }
        async fn complete(
            &self,
            _req: CompletionRequest,
        ) -> Result<CompletionResponse, crate::error::LlmError> {
            unimplemented!("stub")
        }
        async fn complete_with_tools(
            &self,
            _req: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, crate::error::LlmError> {
            unimplemented!("stub")
        }
    }

    async fn build_registry(tools: Vec<Arc<dyn Tool>>) -> ToolRegistry {
        let registry = ToolRegistry::new();
        for tool in tools {
            registry.register(tool).await;
        }
        registry
    }

    fn base_deps(
        cm: Arc<crate::context::ContextManager>,
        tools: Arc<ToolRegistry>,
        store: Option<Arc<dyn crate::db::Database>>,
        approval_context: Option<crate::tools::ApprovalContext>,
    ) -> WorkerDeps {
        WorkerDeps {
            context_manager: cm,
            llm: Arc::new(StubLlm),
            safety: Arc::new(SafetyLayer::new(&SafetyConfig {
                max_output_length: 100_000,
                injection_check_enabled: false,
            })),
            tools,
            store,
            hooks: Arc::new(crate::hooks::HookRegistry::new()),
            timeout: Duration::from_secs(30),
            use_planning: false,
            sse_tx: None,
            approval_context,
            http_interceptor: None,
        }
    }

    /// Build a Worker wired to a ToolRegistry containing the given tools.
    async fn make_worker(tools: Vec<Arc<dyn Tool>>) -> Worker {
        let registry = Arc::new(build_registry(tools).await);
        let cm = Arc::new(crate::context::ContextManager::new(5));
        let job_id = cm.create_job("test", "test job").await.unwrap();
        let deps = base_deps(cm, registry, None, None);

        Worker::new(job_id, deps)
    }

    #[cfg(feature = "libsql")]
    async fn make_worker_with_store(
        tools: Vec<Arc<dyn Tool>>,
    ) -> (Worker, Arc<dyn Database>, tempfile::TempDir) {
        use crate::db::libsql::LibSqlBackend;
        use tempfile::tempdir;

        let registry = Arc::new(build_registry(tools).await);
        let cm = Arc::new(crate::context::ContextManager::new(5));
        let job_id = cm
            .create_job("test", "test job")
            .await
            .expect("failed to create job");
        let dir = tempdir().expect("failed to create tempdir");
        let path = dir.path().join("worker-test.db");
        let backend = LibSqlBackend::new_local(&path)
            .await
            .expect("failed to open libsql backend");
        backend
            .run_migrations()
            .await
            .expect("failed to run migrations");
        let store: Arc<dyn Database> = Arc::new(backend);
        let ctx = cm.get_context(job_id).await.expect("failed to get context");
        store.save_job(&ctx).await.expect("failed to save job");
        let deps = base_deps(cm, registry, Some(store.clone()), None);

        (Worker::new(job_id, deps), store, dir)
    }

    #[test]
    fn test_tool_selection_preserves_call_id() {
        let selection = ToolSelection {
            tool_name: "memory_search".to_string(),
            parameters: serde_json::json!({"query": "test"}),
            reasoning: "Need to search memory".to_string(),
            alternatives: vec![],
            tool_call_id: "call_abc123".to_string(),
        };

        assert_eq!(selection.tool_call_id, "call_abc123");
        assert_ne!(
            selection.tool_call_id, "tool_call_id",
            "tool_call_id must not be the hardcoded placeholder string"
        );
    }

    // Completion detection tests live in src/util.rs (the canonical location).
    // See: test_completion_signals, test_completion_negative, etc.

    #[tokio::test]
    async fn test_parallel_speedup() {
        let current_active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let tools: Vec<Arc<dyn Tool>> = (0..3)
            .map(|i| {
                Arc::new(SlowTool {
                    tool_name: format!("slow_{}", i),
                    delay: Duration::from_millis(200),
                    current_active: Arc::clone(&current_active),
                    max_active: Arc::clone(&max_active),
                }) as Arc<dyn Tool>
            })
            .collect();

        let worker = make_worker(tools).await;

        let selections: Vec<ToolSelection> = (0..3)
            .map(|i| ToolSelection {
                tool_name: format!("slow_{}", i),
                parameters: serde_json::json!({}),
                reasoning: String::new(),
                alternatives: vec![],
                tool_call_id: format!("call_{}", i),
            })
            .collect();

        let results = worker.execute_tools_parallel(&selections).await;

        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.result.is_ok(), "Tool should succeed");
        }
        assert!(
            max_active.load(Ordering::SeqCst) > 1,
            "Expected parallel tool execution to overlap, but max concurrency was {}",
            max_active.load(Ordering::SeqCst)
        );
    }

    #[tokio::test]
    async fn test_result_ordering_preserved() {
        let current_active = Arc::new(AtomicUsize::new(0));
        let max_active = Arc::new(AtomicUsize::new(0));
        let tools: Vec<Arc<dyn Tool>> = vec![
            Arc::new(SlowTool {
                tool_name: "tool_a".into(),
                delay: Duration::from_millis(300),
                current_active: Arc::clone(&current_active),
                max_active: Arc::clone(&max_active),
            }),
            Arc::new(SlowTool {
                tool_name: "tool_b".into(),
                delay: Duration::from_millis(100),
                current_active: Arc::clone(&current_active),
                max_active: Arc::clone(&max_active),
            }),
            Arc::new(SlowTool {
                tool_name: "tool_c".into(),
                delay: Duration::from_millis(200),
                current_active: Arc::clone(&current_active),
                max_active: Arc::clone(&max_active),
            }),
        ];

        let worker = make_worker(tools).await;

        let selections = vec![
            ToolSelection {
                tool_name: "tool_a".into(),
                parameters: serde_json::json!({}),
                reasoning: String::new(),
                alternatives: vec![],
                tool_call_id: "call_a".into(),
            },
            ToolSelection {
                tool_name: "tool_b".into(),
                parameters: serde_json::json!({}),
                reasoning: String::new(),
                alternatives: vec![],
                tool_call_id: "call_b".into(),
            },
            ToolSelection {
                tool_name: "tool_c".into(),
                parameters: serde_json::json!({}),
                reasoning: String::new(),
                alternatives: vec![],
                tool_call_id: "call_c".into(),
            },
        ];

        let results = worker.execute_tools_parallel(&selections).await;

        assert!(results[0].result.as_ref().unwrap().contains("done_tool_a"));
        assert!(results[1].result.as_ref().unwrap().contains("done_tool_b"));
        assert!(results[2].result.as_ref().unwrap().contains("done_tool_c"));
    }

    #[tokio::test]
    async fn test_missing_tool_produces_error_not_panic() {
        let worker = make_worker(vec![]).await;

        let selections = vec![ToolSelection {
            tool_name: "nonexistent_tool".into(),
            parameters: serde_json::json!({}),
            reasoning: String::new(),
            alternatives: vec![],
            tool_call_id: "call_x".into(),
        }];

        let results = worker.execute_tools_parallel(&selections).await;
        assert_eq!(results.len(), 1);
        assert!(
            results[0].result.is_err(),
            "Missing tool should produce an error, not a panic"
        );
    }

    #[tokio::test]
    async fn test_mark_completed_twice_returns_error() {
        let worker = make_worker(vec![]).await;

        worker
            .context_manager()
            .update_context(worker.job_id, |ctx| {
                ctx.transition_to(JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();

        worker.mark_completed().await.unwrap();

        let ctx = worker
            .context_manager()
            .get_context(worker.job_id)
            .await
            .unwrap();
        assert_eq!(ctx.state, JobState::Completed);

        let result = worker.mark_completed().await;
        assert!(
            result.is_err(),
            "Completed → Completed transition should be rejected by state machine"
        );
    }

    #[cfg(feature = "libsql")]
    #[tokio::test]
    async fn test_mark_completed_persists_result_before_returning() {
        let (worker, store, _dir) = make_worker_with_store(vec![]).await;

        worker
            .context_manager()
            .update_context(worker.job_id, |ctx| {
                ctx.transition_to(JobState::InProgress, None)
            })
            .await
            .expect("failed to update context")
            .expect("failed to transition to in-progress");

        worker
            .mark_completed()
            .await
            .expect("failed to mark job completed");

        let job = store
            .get_job(worker.job_id)
            .await
            .expect("failed to load job")
            .expect("job should exist");
        assert_eq!(job.state, JobState::Completed);

        let events = store
            .list_job_events(worker.job_id, None, None)
            .await
            .expect("failed to list job events");
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "result");
        assert_eq!(events[0].data["status"], "completed");
    }

    /// Build a Worker with the given approval context.
    async fn make_worker_with_approval(
        tools: Vec<Arc<dyn Tool>>,
        approval_context: Option<crate::tools::ApprovalContext>,
    ) -> Worker {
        let registry = Arc::new(build_registry(tools).await);
        let cm = Arc::new(crate::context::ContextManager::new(5));
        let job_id = cm.create_job("test", "test job").await.unwrap();
        let deps = base_deps(cm, registry, None, approval_context);

        Worker::new(job_id, deps)
    }

    /// A tool that requires approval (UnlessAutoApproved).
    struct ApprovalTool;

    impl NativeTool for ApprovalTool {
        fn name(&self) -> &str {
            "needs_approval"
        }
        fn description(&self) -> &str {
            "Tool requiring approval"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &crate::context::JobContext,
        ) -> Result<ToolOutput, crate::tools::ToolError> {
            Ok(ToolOutput::text(
                "approved",
                std::time::Instant::now().elapsed(),
            ))
        }
        fn requires_approval(
            &self,
            _params: &serde_json::Value,
        ) -> crate::tools::ApprovalRequirement {
            crate::tools::ApprovalRequirement::UnlessAutoApproved
        }
        fn requires_sanitization(&self) -> bool {
            false
        }
    }

    /// A tool that always requires approval.
    struct AlwaysApprovalTool;

    impl NativeTool for AlwaysApprovalTool {
        fn name(&self) -> &str {
            "always_approval"
        }
        fn description(&self) -> &str {
            "Tool always requiring approval"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &crate::context::JobContext,
        ) -> Result<ToolOutput, crate::tools::ToolError> {
            Ok(ToolOutput::text(
                "always",
                std::time::Instant::now().elapsed(),
            ))
        }
        fn requires_approval(
            &self,
            _params: &serde_json::Value,
        ) -> crate::tools::ApprovalRequirement {
            crate::tools::ApprovalRequirement::Always
        }
        fn requires_sanitization(&self) -> bool {
            false
        }
    }

    #[tokio::test]
    async fn test_approval_context_unblocks_unless_auto_approved() {
        let worker_blocked = make_worker_with_approval(vec![Arc::new(ApprovalTool)], None).await;
        let result = worker_blocked
            .execute_tool("needs_approval", &serde_json::json!({}))
            .await;
        assert!(
            result.is_err(),
            "Should be blocked without approval context"
        );

        let worker_allowed = make_worker_with_approval(
            vec![Arc::new(ApprovalTool)],
            Some(crate::tools::ApprovalContext::autonomous()),
        )
        .await;
        let result = worker_allowed
            .execute_tool("needs_approval", &serde_json::json!({}))
            .await;
        assert!(result.is_ok(), "Should be allowed with autonomous context");
    }

    #[tokio::test]
    async fn test_approval_context_blocks_always_unless_permitted() {
        let worker_blocked = make_worker_with_approval(
            vec![Arc::new(AlwaysApprovalTool)],
            Some(crate::tools::ApprovalContext::autonomous()),
        )
        .await;
        let result = worker_blocked
            .execute_tool("always_approval", &serde_json::json!({}))
            .await;
        assert!(
            result.is_err(),
            "Always tool should be blocked without permission"
        );

        let worker_allowed = make_worker_with_approval(
            vec![Arc::new(AlwaysApprovalTool)],
            Some(crate::tools::ApprovalContext::autonomous_with_tools([
                "always_approval".to_string(),
            ])),
        )
        .await;
        let result = worker_allowed
            .execute_tool("always_approval", &serde_json::json!({}))
            .await;
        assert!(
            result.is_ok(),
            "Always tool should be allowed with permission"
        );
    }

    #[tokio::test]
    async fn test_token_budget_exceeded_fails_job() {
        let worker = make_worker(vec![]).await;

        // Transition to InProgress (required for mark_failed)
        worker
            .context_manager()
            .update_context(worker.job_id, |ctx| {
                ctx.transition_to(JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();

        // Set a token budget
        worker
            .context_manager()
            .update_context(worker.job_id, |ctx| {
                ctx.max_tokens = 100;
            })
            .await
            .unwrap();

        // Simulate adding tokens that exceed the budget
        let budget_result = worker
            .context_manager()
            .update_context(worker.job_id, |ctx| ctx.add_tokens(200))
            .await
            .unwrap();

        assert!(
            budget_result.is_err(),
            "Should return error when token budget exceeded"
        );

        // Verify that mark_failed transitions job to Failed
        worker
            .mark_failed(&budget_result.unwrap_err())
            .await
            .unwrap();
        let ctx = worker
            .context_manager()
            .get_context(worker.job_id)
            .await
            .unwrap();
        assert_eq!(ctx.state, JobState::Failed);
    }

    #[tokio::test]
    async fn test_iteration_cap_marks_failed_not_stuck() {
        let worker = make_worker(vec![]).await;

        // Transition to InProgress (required for mark_failed)
        worker
            .context_manager()
            .update_context(worker.job_id, |ctx| {
                ctx.transition_to(JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();

        // Simulate what the execution loop does when max_iterations is exceeded
        worker
            .mark_failed("Maximum iterations exceeded: job hit the iteration cap")
            .await
            .unwrap();

        let ctx = worker
            .context_manager()
            .get_context(worker.job_id)
            .await
            .unwrap();
        assert_eq!(
            ctx.state,
            JobState::Failed,
            "Iteration cap should transition to Failed, not Stuck"
        );
    }

    // -----------------------------------------------------------------------
    // Terminal job-state persistence characterisation tests
    // -----------------------------------------------------------------------

    /// Captured call types for the mock database.
    #[derive(Debug, Clone)]
    enum CapturedCall {
        UpdateJobStatus {
            _job_id: Uuid,
            status: JobState,
            reason: Option<String>,
        },
        SaveJobEvent {
            _job_id: Uuid,
            event_type: String,
            data: serde_json::Value,
        },
    }

    /// Mock database that captures calls for characterisation testing.
    #[derive(Debug, Default)]
    struct CapturingStore {
        calls: Arc<Mutex<Vec<CapturedCall>>>,
    }

    impl CapturingStore {
        fn new() -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        async fn captured_calls(&self) -> Vec<CapturedCall> {
            self.calls.lock().await.clone()
        }

        fn doc_not_found(doc_type: &str) -> crate::error::WorkspaceError {
            crate::error::WorkspaceError::DocumentNotFound {
                doc_type: doc_type.to_string(),
                user_id: "test".to_string(),
            }
        }
    }

    impl crate::db::NativeDatabase for CapturingStore {
        async fn run_migrations(&self) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
    }

    impl crate::db::NativeJobStore for CapturingStore {
        async fn save_job(
            &self,
            _ctx: &crate::context::JobContext,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }

        async fn get_job(
            &self,
            _id: Uuid,
        ) -> Result<Option<crate::context::JobContext>, crate::error::DatabaseError> {
            Ok(None)
        }

        async fn update_job_status(
            &self,
            id: Uuid,
            status: JobState,
            failure_reason: Option<&str>,
        ) -> Result<(), crate::error::DatabaseError> {
            self.calls.lock().await.push(CapturedCall::UpdateJobStatus {
                _job_id: id,
                status,
                reason: failure_reason.map(|s| s.to_string()),
            });
            Ok(())
        }

        async fn mark_job_stuck(&self, _id: Uuid) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }

        async fn get_stuck_jobs(&self) -> Result<Vec<Uuid>, crate::error::DatabaseError> {
            Ok(vec![])
        }

        async fn list_agent_jobs(
            &self,
        ) -> Result<Vec<crate::history::AgentJobRecord>, crate::error::DatabaseError> {
            Ok(vec![])
        }

        async fn agent_job_summary(
            &self,
        ) -> Result<crate::history::AgentJobSummary, crate::error::DatabaseError> {
            Ok(crate::history::AgentJobSummary::default())
        }

        async fn get_agent_job_failure_reason(
            &self,
            _id: Uuid,
        ) -> Result<Option<String>, crate::error::DatabaseError> {
            Ok(None)
        }

        async fn save_action(
            &self,
            _job_id: Uuid,
            _action: &crate::context::ActionRecord,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }

        async fn get_job_actions(
            &self,
            _job_id: Uuid,
        ) -> Result<Vec<crate::context::ActionRecord>, crate::error::DatabaseError> {
            Ok(vec![])
        }

        async fn record_llm_call(
            &self,
            _record: &crate::history::LlmCallRecord<'_>,
        ) -> Result<Uuid, crate::error::DatabaseError> {
            Ok(Uuid::new_v4())
        }

        async fn save_estimation_snapshot(
            &self,
            _params: crate::db::EstimationSnapshotParams<'_>,
        ) -> Result<Uuid, crate::error::DatabaseError> {
            Ok(Uuid::new_v4())
        }

        async fn update_estimation_actuals(
            &self,
            _params: crate::db::EstimationActualsParams,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
    }

    impl crate::db::NativeSandboxStore for CapturingStore {
        async fn save_sandbox_job(
            &self,
            _job: &crate::history::SandboxJobRecord,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }

        async fn get_sandbox_job(
            &self,
            _id: Uuid,
        ) -> Result<Option<crate::history::SandboxJobRecord>, crate::error::DatabaseError> {
            Ok(None)
        }

        async fn list_sandbox_jobs(
            &self,
        ) -> Result<Vec<crate::history::SandboxJobRecord>, crate::error::DatabaseError> {
            Ok(vec![])
        }

        async fn update_sandbox_job_status(
            &self,
            _params: crate::db::SandboxJobStatusUpdate<'_>,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }

        async fn cleanup_stale_sandbox_jobs(&self) -> Result<u64, crate::error::DatabaseError> {
            Ok(0)
        }

        async fn sandbox_job_summary(
            &self,
        ) -> Result<crate::history::SandboxJobSummary, crate::error::DatabaseError> {
            Ok(crate::history::SandboxJobSummary::default())
        }

        async fn list_sandbox_jobs_for_user(
            &self,
            _user_id: crate::db::UserId,
        ) -> Result<Vec<crate::history::SandboxJobRecord>, crate::error::DatabaseError> {
            Ok(vec![])
        }

        async fn sandbox_job_summary_for_user(
            &self,
            _user_id: crate::db::UserId,
        ) -> Result<crate::history::SandboxJobSummary, crate::error::DatabaseError> {
            Ok(crate::history::SandboxJobSummary::default())
        }

        async fn sandbox_job_belongs_to_user(
            &self,
            _job_id: Uuid,
            _user_id: crate::db::UserId,
        ) -> Result<bool, crate::error::DatabaseError> {
            Ok(false)
        }

        async fn update_sandbox_job_mode(
            &self,
            _id: Uuid,
            _mode: crate::db::SandboxMode,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }

        async fn get_sandbox_job_mode(
            &self,
            _id: Uuid,
        ) -> Result<Option<crate::db::SandboxMode>, crate::error::DatabaseError> {
            Ok(None)
        }

        async fn save_job_event(
            &self,
            job_id: Uuid,
            event_type: crate::db::SandboxEventType,
            data: &serde_json::Value,
        ) -> Result<(), crate::error::DatabaseError> {
            self.calls.lock().await.push(CapturedCall::SaveJobEvent {
                _job_id: job_id,
                event_type: event_type.as_str().to_string(),
                data: data.clone(),
            });
            Ok(())
        }

        async fn list_job_events(
            &self,
            _job_id: Uuid,
            _before_id: Option<i64>,
            _limit: Option<i64>,
        ) -> Result<Vec<crate::history::JobEventRecord>, crate::error::DatabaseError> {
            Ok(vec![])
        }
    }

    // Stub implementations for remaining traits
    impl crate::db::NativeConversationStore for CapturingStore {
        async fn create_conversation(
            &self,
            _channel: &str,
            _user_id: &str,
            _thread_id: Option<&str>,
        ) -> Result<Uuid, crate::error::DatabaseError> {
            Ok(Uuid::new_v4())
        }
        async fn touch_conversation(&self, _id: Uuid) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn add_conversation_message(
            &self,
            _conversation_id: Uuid,
            _role: &str,
            _content: &str,
        ) -> Result<Uuid, crate::error::DatabaseError> {
            Ok(Uuid::new_v4())
        }
        async fn ensure_conversation(
            &self,
            _params: crate::db::EnsureConversationParams<'_>,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn list_conversations_with_preview(
            &self,
            _user_id: &str,
            _channel: &str,
            _limit: usize,
        ) -> Result<Vec<crate::history::ConversationSummary>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn list_conversations_all_channels(
            &self,
            _user_id: &str,
            _limit: usize,
        ) -> Result<Vec<crate::history::ConversationSummary>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn get_or_create_routine_conversation(
            &self,
            _routine_id: Uuid,
            _routine_name: &str,
            _user_id: &str,
        ) -> Result<Uuid, crate::error::DatabaseError> {
            Ok(Uuid::new_v4())
        }
        async fn get_or_create_heartbeat_conversation(
            &self,
            _user_id: &str,
        ) -> Result<Uuid, crate::error::DatabaseError> {
            Ok(Uuid::new_v4())
        }
        async fn get_or_create_assistant_conversation(
            &self,
            _user_id: &str,
            _channel: &str,
        ) -> Result<Uuid, crate::error::DatabaseError> {
            Ok(Uuid::new_v4())
        }
        async fn create_conversation_with_metadata(
            &self,
            _channel: &str,
            _user_id: &str,
            _metadata: &serde_json::Value,
        ) -> Result<Uuid, crate::error::DatabaseError> {
            Ok(Uuid::new_v4())
        }
        async fn list_conversation_messages_paginated(
            &self,
            _conversation_id: Uuid,
            _before: Option<(chrono::DateTime<chrono::Utc>, Uuid)>,
            _limit: usize,
        ) -> Result<(Vec<crate::history::ConversationMessage>, bool), crate::error::DatabaseError>
        {
            Ok((vec![], false))
        }
        async fn list_conversation_messages(
            &self,
            _conversation_id: Uuid,
        ) -> Result<Vec<crate::history::ConversationMessage>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn conversation_belongs_to_user(
            &self,
            _conversation_id: Uuid,
            _user_id: &str,
        ) -> Result<bool, crate::error::DatabaseError> {
            Ok(false)
        }
        async fn update_conversation_metadata_field(
            &self,
            _id: Uuid,
            _key: &str,
            _value: &serde_json::Value,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn get_conversation_metadata(
            &self,
            _id: Uuid,
        ) -> Result<Option<serde_json::Value>, crate::error::DatabaseError> {
            Ok(None)
        }
    }

    impl crate::db::NativeRoutineStore for CapturingStore {
        async fn create_routine(
            &self,
            _routine: &crate::agent::Routine,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn get_routine(
            &self,
            _id: Uuid,
        ) -> Result<Option<crate::agent::Routine>, crate::error::DatabaseError> {
            Ok(None)
        }
        async fn get_routine_by_name(
            &self,
            _user_id: &str,
            _name: &str,
        ) -> Result<Option<crate::agent::Routine>, crate::error::DatabaseError> {
            Ok(None)
        }
        async fn list_routines(
            &self,
            _user_id: &str,
        ) -> Result<Vec<crate::agent::Routine>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn list_all_routines(
            &self,
        ) -> Result<Vec<crate::agent::Routine>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn list_event_routines(
            &self,
        ) -> Result<Vec<crate::agent::Routine>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn list_due_cron_routines(
            &self,
        ) -> Result<Vec<crate::agent::Routine>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn update_routine(
            &self,
            _routine: &crate::agent::Routine,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn update_routine_runtime(
            &self,
            _params: crate::db::RoutineRuntimeUpdate<'_>,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn delete_routine(&self, _id: Uuid) -> Result<bool, crate::error::DatabaseError> {
            Ok(false)
        }
        async fn create_routine_run(
            &self,
            _run: &crate::agent::RoutineRun,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn complete_routine_run(
            &self,
            _params: crate::db::RoutineRunCompletion<'_>,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn list_routine_runs(
            &self,
            _routine_id: Uuid,
            _limit: i64,
        ) -> Result<Vec<crate::agent::RoutineRun>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn count_running_routine_runs(
            &self,
            _routine_id: Uuid,
        ) -> Result<i64, crate::error::DatabaseError> {
            Ok(0)
        }
        async fn link_routine_run_to_job(
            &self,
            _run_id: Uuid,
            _job_id: Uuid,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
    }

    impl crate::db::NativeToolFailureStore for CapturingStore {
        async fn record_tool_failure(
            &self,
            _tool_name: &str,
            _error_message: &str,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn get_broken_tools(
            &self,
            _threshold: i32,
        ) -> Result<Vec<crate::agent::BrokenTool>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn mark_tool_repaired(
            &self,
            _tool_name: &str,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn increment_repair_attempts(
            &self,
            _tool_name: &str,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
    }

    impl crate::db::NativeSettingsStore for CapturingStore {
        async fn get_setting(
            &self,
            _user_id: crate::db::UserId,
            _key: crate::db::SettingKey,
        ) -> Result<Option<serde_json::Value>, crate::error::DatabaseError> {
            Ok(None)
        }
        async fn get_setting_full(
            &self,
            _user_id: crate::db::UserId,
            _key: crate::db::SettingKey,
        ) -> Result<Option<crate::history::SettingRow>, crate::error::DatabaseError> {
            Ok(None)
        }
        async fn set_setting(
            &self,
            _user_id: crate::db::UserId,
            _key: crate::db::SettingKey,
            _value: &serde_json::Value,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn delete_setting(
            &self,
            _user_id: crate::db::UserId,
            _key: crate::db::SettingKey,
        ) -> Result<bool, crate::error::DatabaseError> {
            Ok(false)
        }
        async fn list_settings(
            &self,
            _user_id: crate::db::UserId,
        ) -> Result<Vec<crate::history::SettingRow>, crate::error::DatabaseError> {
            Ok(vec![])
        }
        async fn get_all_settings(
            &self,
            _user_id: crate::db::UserId,
        ) -> Result<std::collections::HashMap<String, serde_json::Value>, crate::error::DatabaseError>
        {
            Ok(std::collections::HashMap::new())
        }
        async fn set_all_settings(
            &self,
            _user_id: crate::db::UserId,
            _settings: &std::collections::HashMap<String, serde_json::Value>,
        ) -> Result<(), crate::error::DatabaseError> {
            Ok(())
        }
        async fn has_settings(
            &self,
            _user_id: crate::db::UserId,
        ) -> Result<bool, crate::error::DatabaseError> {
            Ok(false)
        }
    }

    impl crate::db::NativeWorkspaceStore for CapturingStore {
        async fn get_document_by_path(
            &self,
            _user_id: &str,
            _agent_id: Option<Uuid>,
            _path: &str,
        ) -> Result<crate::workspace::MemoryDocument, crate::error::WorkspaceError> {
            Err(Self::doc_not_found("file"))
        }
        async fn get_document_by_id(
            &self,
            _id: Uuid,
        ) -> Result<crate::workspace::MemoryDocument, crate::error::WorkspaceError> {
            Err(Self::doc_not_found("id"))
        }
        async fn get_or_create_document_by_path(
            &self,
            _user_id: &str,
            _agent_id: Option<Uuid>,
            _path: &str,
        ) -> Result<crate::workspace::MemoryDocument, crate::error::WorkspaceError> {
            Err(Self::doc_not_found("file"))
        }
        async fn update_document(
            &self,
            _id: Uuid,
            _content: &str,
        ) -> Result<(), crate::error::WorkspaceError> {
            Ok(())
        }
        async fn delete_document_by_path(
            &self,
            _user_id: &str,
            _agent_id: Option<Uuid>,
            _path: &str,
        ) -> Result<(), crate::error::WorkspaceError> {
            Ok(())
        }
        async fn list_directory(
            &self,
            _user_id: &str,
            _agent_id: Option<Uuid>,
            _directory: &str,
        ) -> Result<Vec<crate::workspace::WorkspaceEntry>, crate::error::WorkspaceError> {
            Ok(vec![])
        }
        async fn list_all_paths(
            &self,
            _user_id: &str,
            _agent_id: Option<Uuid>,
        ) -> Result<Vec<String>, crate::error::WorkspaceError> {
            Ok(vec![])
        }
        async fn list_documents(
            &self,
            _user_id: &str,
            _agent_id: Option<Uuid>,
        ) -> Result<Vec<crate::workspace::MemoryDocument>, crate::error::WorkspaceError> {
            Ok(vec![])
        }
        async fn delete_chunks(
            &self,
            _document_id: Uuid,
        ) -> Result<(), crate::error::WorkspaceError> {
            Ok(())
        }
        async fn insert_chunk(
            &self,
            _params: crate::db::InsertChunkParams<'_>,
        ) -> Result<Uuid, crate::error::WorkspaceError> {
            Ok(Uuid::new_v4())
        }
        async fn update_chunk_embedding(
            &self,
            _chunk_id: Uuid,
            _embedding: &[f32],
        ) -> Result<(), crate::error::WorkspaceError> {
            Ok(())
        }
        async fn get_chunks_without_embeddings(
            &self,
            _user_id: &str,
            _agent_id: Option<Uuid>,
            _limit: usize,
        ) -> Result<Vec<crate::workspace::MemoryChunk>, crate::error::WorkspaceError> {
            Ok(vec![])
        }
        async fn hybrid_search(
            &self,
            _params: crate::db::HybridSearchParams<'_>,
        ) -> Result<Vec<crate::workspace::SearchResult>, crate::error::WorkspaceError> {
            Ok(vec![])
        }
    }

    /// Build a Worker with a capturing store for characterisation tests.
    async fn make_worker_with_capturing_store(
        tools: Vec<Arc<dyn Tool>>,
    ) -> (Worker, Arc<CapturingStore>) {
        let registry = Arc::new(build_registry(tools).await);
        let cm = Arc::new(crate::context::ContextManager::new(5));
        let job_id = cm.create_job("test", "test job").await.unwrap();

        let store = Arc::new(CapturingStore::new());
        let store_dyn: Arc<dyn crate::db::Database> = store.clone();
        let deps = base_deps(cm, registry, Some(store_dyn), None);

        (Worker::new(job_id, deps), store)
    }

    async fn assert_terminal_persistence(
        store: &CapturingStore,
        expected_state: JobState,
        expected_status_str: &str,
        expected_reason: Option<&str>,
    ) {
        tokio::time::sleep(Duration::from_millis(100)).await;

        let calls = store.captured_calls().await;
        let update_calls: Vec<_> = calls
            .iter()
            .filter(|call| matches!(call, CapturedCall::UpdateJobStatus { .. }))
            .cloned()
            .collect();
        let event_calls: Vec<_> = calls
            .iter()
            .filter(|call| matches!(call, CapturedCall::SaveJobEvent { .. }))
            .cloned()
            .collect();

        assert_eq!(
            update_calls.len(),
            1,
            "Expected exactly one update_job_status call"
        );
        assert_eq!(
            event_calls.len(),
            1,
            "Expected exactly one save_job_event call"
        );

        if let CapturedCall::UpdateJobStatus { status, reason, .. } = &update_calls[0] {
            assert_eq!(*status, expected_state);
            if let Some(expected_reason) = expected_reason {
                assert_eq!(reason.as_deref(), Some(expected_reason));
            }
        }

        if let CapturedCall::SaveJobEvent {
            event_type, data, ..
        } = &event_calls[0]
        {
            assert_eq!(event_type, "result");
            assert_eq!(data["status"], expected_status_str);
        }
    }

    async fn transition_to_in_progress(worker: &Worker) {
        worker
            .context_manager()
            .update_context(worker.job_id, |ctx| {
                ctx.transition_to(JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn test_mark_completed_characterises_terminal_persistence() {
        let (worker, store) = make_worker_with_capturing_store(vec![]).await;

        // Transition to InProgress first
        transition_to_in_progress(&worker).await;

        // Call mark_completed
        worker.mark_completed().await.unwrap();

        // Verify state in ContextManager
        let ctx = worker
            .context_manager()
            .get_context(worker.job_id)
            .await
            .unwrap();
        assert_eq!(ctx.state, JobState::Completed);

        assert_terminal_persistence(&store, JobState::Completed, "completed", None).await;
    }

    #[tokio::test]
    async fn test_mark_failed_characterises_terminal_persistence() {
        let (worker, store) = make_worker_with_capturing_store(vec![]).await;

        // Transition to InProgress first
        transition_to_in_progress(&worker).await;

        // Call mark_failed
        worker.mark_failed("budget exceeded").await.unwrap();

        // Verify state in ContextManager
        let ctx = worker
            .context_manager()
            .get_context(worker.job_id)
            .await
            .unwrap();
        assert_eq!(ctx.state, JobState::Failed);

        assert_terminal_persistence(&store, JobState::Failed, "failed", Some("budget exceeded"))
            .await;
    }

    #[tokio::test]
    async fn test_mark_stuck_characterises_terminal_persistence() {
        let (worker, store) = make_worker_with_capturing_store(vec![]).await;

        // Transition to InProgress first
        transition_to_in_progress(&worker).await;

        // Call mark_stuck
        worker.mark_stuck("timeout").await.unwrap();

        // Verify state in ContextManager
        let ctx = worker
            .context_manager()
            .get_context(worker.job_id)
            .await
            .unwrap();
        assert_eq!(ctx.state, JobState::Stuck);

        assert_terminal_persistence(&store, JobState::Stuck, "stuck", Some("timeout")).await;
    }

    #[tokio::test]
    async fn test_double_completed_transition_rejected() {
        let (worker, store) = make_worker_with_capturing_store(vec![]).await;

        // Transition to InProgress first
        transition_to_in_progress(&worker).await;

        // First call succeeds
        worker.mark_completed().await.unwrap();

        // Second call should fail
        let result = worker.mark_completed().await;
        assert!(
            result.is_err(),
            "Double transition to Completed should be rejected"
        );

        assert_terminal_persistence(&store, JobState::Completed, "completed", None).await;
    }
}
