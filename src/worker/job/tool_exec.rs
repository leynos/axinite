//! Tool execution for job workers: approval, hooks, rate limits, timeouts.
//!
//! `execute_tool_inner` performs the full per-tool pipeline (approval check,
//! rate limiting, hooks, validation, timed execution, memory recording,
//! persistence) via the stage helpers in `tool_pipeline`;
//! `execute_tools_parallel` fans multiple selections out over a `JoinSet`
//! while preserving result order.

use tokio::task::JoinSet;
use uuid::Uuid;

use crate::context::JobState;
use crate::error::Error;
use crate::llm::ToolSelection;
use crate::tools::redact_params;

use super::tool_pipeline;
use super::{Worker, WorkerDeps};

/// Result of a tool execution with metadata for context building.
pub(super) struct ToolExecResult {
    pub(super) result: Result<String, Error>,
}

impl Worker {
    /// Execute multiple tools in parallel using a JoinSet.
    ///
    /// Each task is tagged with its original index so results are returned
    /// in the same order as `selections`, regardless of completion order.
    pub(super) async fn execute_tools_parallel(
        &self,
        selections: &[ToolSelection],
    ) -> Vec<ToolExecResult> {
        // Short-circuit for single tool: execute directly without JoinSet overhead
        if selections.len() <= 1 {
            return self.execute_selections_sequentially(selections).await;
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

        Self::collect_ordered_results(join_set, selections).await
    }

    /// Execute the given selections one at a time in order.
    async fn execute_selections_sequentially(
        &self,
        selections: &[ToolSelection],
    ) -> Vec<ToolExecResult> {
        let mut results = Vec::with_capacity(selections.len());
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
        results
    }

    /// Drain a JoinSet of indexed tool tasks, restoring the original order
    /// and substituting error results for panicked or cancelled tasks.
    async fn collect_ordered_results(
        mut join_set: JoinSet<(usize, ToolExecResult)>,
        selections: &[ToolSelection],
    ) -> Vec<ToolExecResult> {
        let mut results: Vec<Option<ToolExecResult>> =
            (0..selections.len()).map(|_| None).collect();
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((idx, exec_result)) => results[idx] = Some(exec_result),
                Err(e) if e.is_panic() => {
                    tracing::error!("Tool execution task panicked: {}", e);
                }
                Err(e) => {
                    tracing::error!("Tool execution task cancelled: {}", e);
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
    ///
    /// Pipeline: approval check, rate limiting, `BeforeToolCall` hook,
    /// cancellation check, parameter validation, timed execution, memory
    /// recording, fire-and-forget persistence, and result serialization.
    async fn execute_tool_inner(
        deps: &WorkerDeps,
        job_id: Uuid,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, Error> {
        let tool = tool_pipeline::resolve_approved_tool(deps, tool_name, params).await?;

        // Fetch job context early so we have the real user_id for hooks and rate limiting
        let job_ctx = tool_pipeline::job_context_with_interceptor(deps, job_id).await?;

        tool_pipeline::check_tool_rate_limit(deps, tool.as_ref(), &job_ctx, tool_name).await?;

        let params = tool_pipeline::apply_tool_call_hook(tool_pipeline::ToolCallHookArgs {
            deps,
            tool: tool.as_ref(),
            tool_name,
            params,
            job_ctx: &job_ctx,
            job_id,
        })
        .await?;

        if job_ctx.state == JobState::Cancelled {
            return Err(crate::error::ToolError::ExecutionFailed {
                name: tool_name.to_string(),
                reason: "Job is cancelled".to_string(),
            }
            .into());
        }

        tool_pipeline::validate_tool_params(deps, tool_name, &params)?;

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

        tool_pipeline::log_tool_outcome(tool_name, &result, elapsed, tool_timeout);

        // Record action in memory and persist it (fire-and-forget)
        let outcome = tool_pipeline::summarize_outcome(deps, tool_name, &result);
        let action = tool_pipeline::record_action_in_memory(tool_pipeline::RecordActionArgs {
            deps,
            job_id,
            tool_name,
            safe_params: &safe_params,
            outcome,
            elapsed,
        })
        .await;
        if let (Some(action), Some(store)) = (action, deps.store.clone()) {
            tokio::spawn(async move {
                if let Err(e) = store.save_action(job_id, &action).await {
                    tracing::warn!("Failed to persist action for job {}: {}", job_id, e);
                }
            });
        }

        tool_pipeline::finalize_output(tool_name, result, tool_timeout)
    }

    pub(super) async fn execute_tool(
        &self,
        tool_name: &str,
        params: &serde_json::Value,
    ) -> Result<String, Error> {
        Self::execute_tool_inner(&self.deps, self.job_id, tool_name, params).await
    }
}
