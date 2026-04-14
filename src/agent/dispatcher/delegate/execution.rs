//! Tool execution helpers: inline and parallel batch runners for
//! `ChatDelegate`.

use crate::channels::StatusUpdate;
use crate::error::Error;

use super::ChatDelegate;
use crate::agent::dispatcher::types::*;

impl<'a> ChatDelegate<'a> {
    /// Execute a single tool inline (for small batches).
    pub(super) async fn execute_one_tool(
        &self,
        tc: &crate::llm::ToolCall,
    ) -> Result<String, Error> {
        self.send_tool_started(&tc.name).await;
        let result = self
            .agent
            .execute_chat_tool(&tc.name, &tc.arguments, &self.job_ctx)
            .await;
        self.send_tool_completed(&tc.name, &result, &tc.arguments)
            .await;
        result
    }

    /// Run a batch of tools inline (sequential execution for small batches).
    pub(super) async fn run_tool_batch_inline(
        &self,
        preflight: &[(crate::llm::ToolCall, PreflightOutcome)],
        runnable: &[usize],
        exec_results: &mut [Option<Result<String, Error>>],
    ) {
        for pf_idx in runnable {
            let tc = &preflight[*pf_idx].0;
            let result = self.execute_one_tool(tc).await;
            exec_results[*pf_idx] = Some(result);
        }
    }

    /// Run a batch of tools in parallel (for large batches).
    pub(super) async fn run_tool_batch_parallel(
        &self,
        preflight: &[(crate::llm::ToolCall, PreflightOutcome)],
        runnable: &[usize],
        exec_results: &mut [Option<Result<String, Error>>],
    ) {
        use tokio::task::JoinSet;

        let mut join_set = JoinSet::new();

        for pf_idx in runnable {
            let pf_idx = *pf_idx;
            let tools = self.agent.tools().clone();
            let safety = self.agent.safety().clone();
            let channels = self.agent.channels.clone();
            let job_ctx = self.job_ctx.clone();
            let tc = preflight[pf_idx].0.clone();
            let channel = self.message.channel.clone();
            let metadata = self.message.metadata.clone();

            join_set.spawn(async move {
                let _ = channels
                    .send_status(
                        &channel,
                        StatusUpdate::ToolStarted {
                            name: tc.name.clone(),
                        },
                        &metadata,
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
                        &channel,
                        StatusUpdate::tool_completed(
                            tc.name.clone(),
                            &result,
                            &tc.arguments,
                            par_tool.as_deref(),
                        ),
                        &metadata,
                    )
                    .await;

                (pf_idx, result)
            });
        }

        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((pf_idx, result)) => {
                    exec_results[pf_idx] = Some(result);
                }
                Err(e) => {
                    if e.is_panic() {
                        tracing::error!("Chat tool execution task panicked: {}", e);
                    } else {
                        tracing::error!("Chat tool execution task cancelled: {}", e);
                    }
                }
            }
        }

        // Fill panicked slots with error results
        for pf_idx in runnable.iter().copied() {
            let tc = &preflight[pf_idx].0;
            if exec_results[pf_idx].is_none() {
                tracing::error!(
                    tool = %tc.name,
                    "Filling failed task slot with error"
                );
                exec_results[pf_idx] = Some(Err(crate::error::ToolError::ExecutionFailed {
                    name: tc.name.clone(),
                    reason: "Task failed during execution".to_string(),
                }
                .into()));
            }
        }
    }
}
