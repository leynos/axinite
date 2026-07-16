//! Worker entry point and the main agentic execution loop.
//!
//! `Worker::run` waits for the start signal, builds the reasoning context,
//! and drives the shared `AgenticLoop` via `JobDelegate` until the job
//! completes, fails, or times out.

use tokio::sync::mpsc;

use crate::agent::agentic_loop::{AgenticLoopConfig, LoopOutcome, run_agentic_loop};
use crate::agent::scheduler::WorkerMessage;
use crate::context::JobState;
use crate::error::Error;
use crate::llm::{ChatMessage, Reasoning, ReasoningContext};

use super::delegate::JobDelegate;
use super::{Worker, WorkerLoopOutcome};

impl Worker {
    /// Run the worker until the job is complete or stopped.
    pub async fn run(self, mut rx: mpsc::Receiver<WorkerMessage>) -> Result<(), Error> {
        tracing::info!("Worker starting for job {}", self.job_id);

        if !self.wait_for_start(&mut rx).await {
            return Ok(());
        }

        // Get job context
        let job_ctx = self.context_manager().get_context(self.job_id).await?;

        // Create reasoning engine
        let reasoning =
            Reasoning::new(self.llm().clone()).with_model_name(self.llm().active_model_name());

        let mut reason_ctx = Self::build_reasoning_context(&job_ctx);

        // Main execution loop with timeout
        let result = tokio::time::timeout(self.timeout(), async {
            self.execution_loop(&mut rx, &reasoning, &mut reason_ctx)
                .await
        })
        .await;

        self.settle_run_result(result).await
    }

    /// Wait for the start signal.
    ///
    /// Returns `false` when the worker was stopped (or the channel closed)
    /// before starting; `Ping` and `UserMessage` are treated as a start.
    async fn wait_for_start(&self, rx: &mut mpsc::Receiver<WorkerMessage>) -> bool {
        match rx.recv().await {
            Some(WorkerMessage::Stop) | None => {
                tracing::debug!("Worker for job {} stopped before starting", self.job_id);
                false
            }
            Some(WorkerMessage::Start)
            | Some(WorkerMessage::Ping)
            | Some(WorkerMessage::UserMessage(_)) => true,
        }
    }

    /// Build the initial reasoning context with the job's system message.
    ///
    /// Tool definitions are refreshed each iteration in `execution_loop`.
    fn build_reasoning_context(job_ctx: &crate::context::JobContext) -> ReasoningContext {
        let mut reason_ctx = ReasoningContext::new().with_job(&job_ctx.description);
        reason_ctx.messages.push(ChatMessage::system(format!(
            r#"You are an autonomous agent working on a job.

Job: {}
Description: {}

You have access to tools to complete this job. Plan your approach and execute tools as needed.
You may request multiple tools at once if they can be executed in parallel.
Report when the job is complete or if you encounter issues you cannot resolve."#,
            job_ctx.title, job_ctx.description
        )));
        reason_ctx
    }

    /// Translate the (possibly timed-out) loop result into a terminal
    /// job-state transition.
    async fn settle_run_result(
        &self,
        result: Result<Result<WorkerLoopOutcome, Error>, tokio::time::error::Elapsed>,
    ) -> Result<(), Error> {
        match result {
            Ok(Ok(WorkerLoopOutcome::Completed)) => {
                tracing::info!("Worker for job {} completed successfully", self.job_id);
                self.mark_completed_if_active().await?;
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

    /// Mark the job completed only if it is still in an active, non-stuck
    /// state; terminal and stuck states are left untouched.
    async fn mark_completed_if_active(&self) -> Result<(), Error> {
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
        Ok(())
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
}
