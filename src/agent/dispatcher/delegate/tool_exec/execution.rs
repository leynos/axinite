//! Execution stage for chat tool execution.
//!
//! Runs the preflight-approved tool calls, batches them where safe, and
//! captures raw results for the later postflight phase to interpret.

use std::sync::Arc;

use tokio::task::JoinSet;

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::channels::{ChannelManager, StatusUpdate};
use crate::context::JobContext;
use crate::error::Error;
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;

use super::postflight::check_auth_required;

/// Allocate the exec-results buffer and dispatch Phase 2 tool execution.
pub(super) async fn run_phase2(
    delegate: &ChatDelegate<'_>,
    preflight_len: usize,
    runnable: &[(usize, crate::llm::ToolCall)],
) -> Vec<Option<Result<String, Error>>> {
    let mut exec_results: Vec<Option<Result<String, Error>>> =
        (0..preflight_len).map(|_| None).collect();
    let mut start = 0;
    while start < runnable.len() {
        if is_auth_barrier_tool(&runnable[start].1.name) {
            let batch = &runnable[start..=start];
            run_tool_batch_inline(delegate, batch, &mut exec_results).await;
            if let Some(result) = &exec_results[runnable[start].0]
                && check_auth_required(&runnable[start].1.name, result).is_some()
            {
                break;
            }
            start += 1;
            continue;
        }

        let mut end = start;
        while end < runnable.len() && !is_auth_barrier_tool(&runnable[end].1.name) {
            end += 1;
        }

        let batch = &runnable[start..end];
        if batch.len() <= 1 {
            run_tool_batch_inline(delegate, batch, &mut exec_results).await;
        } else {
            run_tool_batch_parallel(delegate, batch, &mut exec_results).await;
        }
        start = end;
    }
    exec_results
}

pub(super) fn is_auth_barrier_tool(tool_name: &str) -> bool {
    matches!(tool_name, "tool_auth" | "tool_activate")
}

/// Run a batch of tools inline (sequential execution for small batches).
pub(super) async fn run_tool_batch_inline(
    delegate: &ChatDelegate<'_>,
    runnable: &[(usize, crate::llm::ToolCall)],
    exec_results: &mut [Option<Result<String, Error>>],
) {
    for (pf_idx, tc) in runnable {
        let result = execute_one_tool(delegate, tc).await;
        exec_results[*pf_idx] = Some(result);
    }
}

/// Owned snapshot of everything one parallel tool task needs.
///
/// A spawned task cannot borrow from the delegate, so each field is cloned.
struct ParallelToolTask {
    pf_idx: usize,
    tc: crate::llm::ToolCall,
    tools: Arc<ToolRegistry>,
    safety: Arc<SafetyLayer>,
    channels: Arc<ChannelManager>,
    job_ctx: JobContext,
    channel: String,
    metadata: serde_json::Value,
}

impl ParallelToolTask {
    /// Clone the delegate state needed to run `tc` on its own task.
    fn new(delegate: &ChatDelegate<'_>, pf_idx: usize, tc: &crate::llm::ToolCall) -> Self {
        Self {
            pf_idx,
            tc: tc.clone(),
            tools: delegate.agent.tools().clone(),
            safety: delegate.agent.safety().clone(),
            channels: delegate.agent.channels.clone(),
            job_ctx: delegate.job_ctx.clone(),
            channel: delegate.message.channel.clone(),
            metadata: delegate.message.metadata.clone(),
        }
    }

    /// Run the tool, reporting start and completion, and return its slot and result.
    async fn run(self) -> (usize, Result<String, Error>) {
        let _ = self
            .channels
            .send_status(
                &self.channel,
                StatusUpdate::ToolStarted {
                    name: self.tc.name.clone(),
                },
                &self.metadata,
            )
            .await;

        let result = execute_chat_tool_standalone(
            &self.tools,
            &self.safety,
            &ToolCallSpec {
                name: &self.tc.name,
                params: &self.tc.arguments,
            },
            &self.job_ctx,
        )
        .await;

        let par_tool = self.tools.get(&self.tc.name).await;
        let _ = self
            .channels
            .send_status(
                &self.channel,
                StatusUpdate::tool_completed(
                    self.tc.name.clone(),
                    &result,
                    &self.tc.arguments,
                    par_tool.as_deref(),
                ),
                &self.metadata,
            )
            .await;

        (self.pf_idx, result)
    }
}

/// Run a batch of tools in parallel (for large batches).
pub(super) async fn run_tool_batch_parallel(
    delegate: &ChatDelegate<'_>,
    runnable: &[(usize, crate::llm::ToolCall)],
    exec_results: &mut [Option<Result<String, Error>>],
) {
    let mut join_set = JoinSet::new();
    for (pf_idx, tc) in runnable {
        join_set.spawn(ParallelToolTask::new(delegate, *pf_idx, tc).run());
    }
    collect_parallel_results(&mut join_set, exec_results).await;
    fill_failed_slots(runnable, exec_results);
}

/// Drain finished tasks into their result slots, logging panics and cancellations.
async fn collect_parallel_results(
    join_set: &mut JoinSet<(usize, Result<String, Error>)>,
    exec_results: &mut [Option<Result<String, Error>>],
) {
    while let Some(join_result) = join_set.join_next().await {
        match join_result {
            Ok((pf_idx, result)) => exec_results[pf_idx] = Some(result),
            Err(e) if e.is_panic() => {
                tracing::error!("Chat tool execution task panicked: {}", e);
            }
            Err(e) => tracing::error!("Chat tool execution task cancelled: {}", e),
        }
    }
}

/// Give every slot whose task failed an execution error.
fn fill_failed_slots(
    runnable: &[(usize, crate::llm::ToolCall)],
    exec_results: &mut [Option<Result<String, Error>>],
) {
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

/// Execute a single tool inline (for small batches).
pub(super) async fn execute_one_tool(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
) -> Result<String, Error> {
    send_tool_started(delegate, &tc.name).await;
    let result = delegate
        .agent
        .execute_chat_tool(&tc.name, &tc.arguments, &delegate.job_ctx)
        .await;
    send_tool_completed(delegate, &tc.name, &result, &tc.arguments).await;
    result
}

/// Send ToolStarted status update.
async fn send_tool_started(delegate: &ChatDelegate<'_>, tool_name: &str) {
    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::ToolStarted {
                name: tool_name.to_string(),
            },
            &delegate.message.metadata,
        )
        .await;
}

/// Send tool_completed status update.
async fn send_tool_completed(
    delegate: &ChatDelegate<'_>,
    tool_name: &str,
    result: &Result<String, Error>,
    arguments: &serde_json::Value,
) {
    let disp_tool = delegate.agent.tools().get(tool_name).await;
    let _ = delegate
        .agent
        .channels
        .send_status(
            &delegate.message.channel,
            StatusUpdate::tool_completed(
                tool_name.to_string(),
                result,
                arguments,
                disp_tool.as_deref(),
            ),
            &delegate.message.metadata,
        )
        .await;
}

/// Specification for a tool call to be executed.
pub(crate) struct ToolCallSpec<'a> {
    pub(crate) name: &'a str,
    pub(crate) params: &'a serde_json::Value,
}

/// Execute a chat tool without requiring `&Agent`.
///
/// This standalone function enables parallel invocation from spawned JoinSet
/// tasks, which cannot borrow `&self`. Delegates to the shared
/// `execute_tool_with_safety` pipeline.
pub(crate) async fn execute_chat_tool_standalone(
    tools: &ToolRegistry,
    safety: &SafetyLayer,
    spec: &ToolCallSpec<'_>,
    job_ctx: &JobContext,
) -> Result<String, Error> {
    crate::tools::execute::execute_tool_with_safety(tools, safety, spec.name, spec.params, job_ctx)
        .await
}
