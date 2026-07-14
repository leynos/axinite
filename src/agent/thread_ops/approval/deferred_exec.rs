//! Execution of runnable deferred tool calls: inline for a single call,
//! parallel via `JoinSet` for batches.

use tokio::task::JoinSet;

use crate::agent::Agent;
use crate::agent::dispatcher::{ChatToolRequest, execute_chat_tool_standalone};
use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::error::Error;

use super::context::MsgEnv;

/// Deferred execution environment.
#[derive(Clone)]
pub(super) struct DeferredEnv {
    pub(super) job_ctx: JobContext,
    pub(super) env: MsgEnv,
}

impl Agent {
    /// Run deferred tools inline (single or empty).
    async fn run_deferred_inline(
        &self,
        runnable: &[crate::llm::ToolCall],
        exec: &DeferredEnv,
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        let mut results = Vec::new();
        for tc in runnable {
            let _ = self
                .channels
                .send_status(
                    &exec.env.channel,
                    StatusUpdate::ToolStarted {
                        name: tc.name.clone(),
                    },
                    &exec.env.metadata,
                )
                .await;

            let result = self
                .execute_chat_tool(&tc.name, &tc.arguments, &exec.job_ctx)
                .await;

            let deferred_tool = self.tools().get(&tc.name).await;
            let _ = self
                .channels
                .send_status(
                    &exec.env.channel,
                    StatusUpdate::tool_completed(
                        tc.name.clone(),
                        &result,
                        &tc.arguments,
                        deferred_tool.as_deref(),
                    ),
                    &exec.env.metadata,
                )
                .await;

            results.push((tc.clone(), result));
        }
        results
    }

    /// Collect and reorder parallel results.
    async fn collect_and_reorder_parallel_results(
        &self,
        mut join_set: JoinSet<(usize, crate::llm::ToolCall, Result<String, Error>)>,
        runnable: &[crate::llm::ToolCall],
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        let mut ordered: Vec<Option<(crate::llm::ToolCall, Result<String, Error>)>> =
            (0..runnable.len()).map(|_| None).collect();
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((idx, tc, result)) => {
                    ordered[idx] = Some((tc, result));
                }
                Err(e) => {
                    if e.is_panic() {
                        tracing::error!("Deferred tool execution task panicked: {}", e);
                    } else {
                        tracing::error!("Deferred tool execution task cancelled: {}", e);
                    }
                }
            }
        }

        // Fill panicked slots with error results
        ordered
            .into_iter()
            .enumerate()
            .map(|(i, opt)| {
                opt.unwrap_or_else(|| {
                    let tc = runnable[i].clone();
                    let err: Error = crate::error::ToolError::ExecutionFailed {
                        name: tc.name.clone(),
                        reason: "Task failed during execution".to_string(),
                    }
                    .into();
                    (tc, Err(err))
                })
            })
            .collect()
    }

    /// Run deferred tools in parallel via JoinSet.
    async fn run_deferred_parallel(
        &self,
        runnable: &[crate::llm::ToolCall],
        exec: &DeferredEnv,
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        let mut join_set = JoinSet::new();

        for (idx, tc) in runnable.iter().cloned().enumerate() {
            let tools = self.tools().clone();
            let safety = self.safety().clone();
            let channels = self.channels.clone();
            let job_ctx = exec.job_ctx.clone();
            let env = exec.env.clone();
            join_set.spawn(async move {
                let _ = channels
                    .send_status(
                        &env.channel,
                        StatusUpdate::ToolStarted {
                            name: tc.name.clone(),
                        },
                        &env.metadata,
                    )
                    .await;

                let result = execute_chat_tool_standalone(
                    &tools,
                    &safety,
                    &ChatToolRequest {
                        tool_name: &tc.name,
                        params: &tc.arguments,
                    },
                    &job_ctx,
                )
                .await;

                let par_tool = tools.get(&tc.name).await;
                let _ = channels
                    .send_status(
                        &env.channel,
                        StatusUpdate::tool_completed(
                            tc.name.clone(),
                            &result,
                            &tc.arguments,
                            par_tool.as_deref(),
                        ),
                        &env.metadata,
                    )
                    .await;

                (idx, tc, result)
            });
        }

        self.collect_and_reorder_parallel_results(join_set, runnable)
            .await
    }

    /// Execute runnable deferred tools (inline for ≤1, JoinSet for >1).
    pub(super) async fn execute_runnable_deferred(
        &self,
        runnable: &[crate::llm::ToolCall],
        exec: &DeferredEnv,
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        if runnable.is_empty() {
            return Vec::new();
        }
        if runnable.len() == 1 {
            return self.run_deferred_inline(runnable, exec).await;
        }
        self.run_deferred_parallel(runnable, exec).await
    }
}
