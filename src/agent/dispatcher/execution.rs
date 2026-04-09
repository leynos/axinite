//! Tool execution logic.
//!
//! Contains the execution phase logic for running tools inline or in parallel.

use tokio::task::JoinSet;

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::error::Error;
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;

impl<'a> ChatDelegate<'a> {
    /// Send ToolStarted status update.
    pub(super) async fn send_tool_started(&self, tool_name: &str) {
        let _ = self
            .agent
            .channels
            .send_status(
                &self.message.channel,
                StatusUpdate::ToolStarted {
                    name: tool_name.to_string(),
                },
                &self.message.metadata,
            )
            .await;
    }

    /// Send tool_completed status update.
    pub(super) async fn send_tool_completed(
        &self,
        tool_name: &str,
        result: &Result<String, Error>,
        arguments: &serde_json::Value,
    ) {
        let disp_tool = self.agent.tools().get(tool_name).await;
        let _ = self
            .agent
            .channels
            .send_status(
                &self.message.channel,
                StatusUpdate::tool_completed(
                    tool_name.to_string(),
                    result,
                    arguments,
                    disp_tool.as_deref(),
                ),
                &self.message.metadata,
            )
            .await;
    }

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
        runnable: &[(usize, crate::llm::ToolCall)],
        exec_results: &mut [Option<Result<String, Error>>],
    ) {
        for (pf_idx, tc) in runnable {
            let result = self.execute_one_tool(tc).await;
            exec_results[*pf_idx] = Some(result);
        }
    }

    /// Run a batch of tools in parallel (for large batches).
    pub(super) async fn run_tool_batch_parallel(
        &self,
        runnable: &[(usize, crate::llm::ToolCall)],
        exec_results: &mut [Option<Result<String, Error>>],
    ) {
        let mut join_set = JoinSet::new();

        for (pf_idx, tc) in runnable {
            let pf_idx = *pf_idx;
            let tools = self.agent.tools().clone();
            let safety = self.agent.safety().clone();
            let channels = self.agent.channels.clone();
            let job_ctx = self.job_ctx.clone();
            let tc = tc.clone();
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
                    &tc.name,
                    &tc.arguments,
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
        for (pf_idx, tc) in runnable.iter() {
            if exec_results[*pf_idx].is_none() {
                tracing::error!(
                    tool = %tc.name,
                    "Filling failed task slot with error"
                );
                exec_results[*pf_idx] = Some(Err(crate::error::ToolError::ExecutionFailed {
                    name: tc.name.clone(),
                    reason: "Task failed during execution".to_string(),
                }
                .into()));
            }
        }
    }
}

/// Execute a chat tool without requiring `&Agent`.
///
/// This standalone function enables parallel invocation from spawned JoinSet
/// tasks, which cannot borrow `&self`. Delegates to the shared
/// `execute_tool_with_safety` pipeline.
pub(crate) async fn execute_chat_tool_standalone(
    tools: &ToolRegistry,
    safety: &SafetyLayer,
    tool_name: &str,
    params: &serde_json::Value,
    job_ctx: &JobContext,
) -> Result<String, Error> {
    crate::tools::execute::execute_tool_with_safety(tools, safety, tool_name, params, job_ctx).await
}
