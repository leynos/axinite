//! Execution stage for chat tool execution.
//!
//! Runs the preflight-approved tool calls, batches them where safe, and
//! captures raw results for the later postflight phase to interpret.

use std::sync::Arc;

use tokio::task::JoinSet;

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::error::Error;
use crate::safety::SafetyLayer;
use crate::tools::ToolRegistry;
use crate::tools::monty::broker::{BrokerContext, BrokerServices, CallerPolicy};

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
        if runs_alone(&runnable[start].1.name) {
            let batch = &runnable[start..=start];
            run_tool_batch_inline(delegate, batch, &mut exec_results).await;
            if stops_before_later_calls(&runnable[start], &exec_results) {
                break;
            }
            start += 1;
            continue;
        }

        let mut end = start;
        while end < runnable.len() && !runs_alone(&runnable[end].1.name) {
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

/// Whether an authentication barrier call ended the turn.
///
/// A `tool_auth` or `tool_activate` call that now requires authorization
/// stops the batch: later calls were planned against credentials the user has
/// not supplied yet.
fn stops_before_later_calls(
    call: &(usize, crate::llm::ToolCall),
    exec_results: &[Option<Result<String, Error>>],
) -> bool {
    let Some(result) = &exec_results[call.0] else {
        return false;
    };
    is_auth_barrier_tool(&call.1.name) && check_auth_required(&call.1.name, result).is_some()
}

pub(super) fn is_auth_barrier_tool(tool_name: &str) -> bool {
    matches!(tool_name, "tool_auth" | "tool_activate")
}

/// Whether a call must run on its own rather than in a parallel batch.
///
/// `exec_code` builds a caller-issued broker for this specific invocation and
/// starts a child process; running it beside unrelated calls would give the
/// guest authority before the caller's decision is complete.
pub(super) fn runs_alone(tool_name: &str) -> bool {
    is_auth_barrier_tool(tool_name) || tool_name == "exec_code"
}

/// Builds the job context for one call, issuing `exec_code` authority when
/// this is the chat caller.
///
/// The broker is built from the call's own `allowed_tools` argument and the
/// session's current auto-approval snapshot. An unknown or unsupported name
/// fails here, before the helper starts.
pub(super) async fn prepare_job_context(
    delegate: &ChatDelegate<'_>,
    tc: &crate::llm::ToolCall,
) -> Result<JobContext, Error> {
    let mut job_ctx = delegate.job_ctx.clone();
    if tc.name != "exec_code" {
        return Ok(job_ctx);
    }
    let names: Vec<String> = tc
        .arguments
        .get("allowed_tools")
        .and_then(serde_json::Value::as_array)
        .map(|entries| {
            entries
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    let policy = {
        let session = delegate.session.lock().await;
        CallerPolicy {
            user_id: delegate.message.user_id.clone(),
            hook_context: "chat".to_string(),
            auto_approved: session.auto_approved_tools.clone(),
        }
    };
    let services = BrokerServices {
        tools: delegate.agent.tools().clone(),
        safety: delegate.agent.safety().clone(),
        hooks: Some(Arc::clone(delegate.agent.hooks())),
        policy,
    };
    let broker = BrokerContext::new(services, &names)
        .await
        .map_err(|error| crate::error::ToolError::InvalidParameters {
            name: "exec_code".to_string(),
            reason: error.to_string(),
        })?;
    job_ctx.broker = Some(Arc::new(broker));
    Ok(job_ctx)
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

/// Run a batch of tools in parallel (for large batches).
pub(super) async fn run_tool_batch_parallel(
    delegate: &ChatDelegate<'_>,
    runnable: &[(usize, crate::llm::ToolCall)],
    exec_results: &mut [Option<Result<String, Error>>],
) {
    let mut join_set = JoinSet::new();

    for (pf_idx, tc) in runnable {
        let pf_idx = *pf_idx;
        // Clone all data the spawned task needs — it cannot borrow from `delegate`.
        let tools = delegate.agent.tools().clone();
        let safety = delegate.agent.safety().clone();
        let channels = delegate.agent.channels.clone();
        let job_ctx = delegate.job_ctx.clone();
        let tc = tc.clone();
        let channel = delegate.message.channel.clone();
        let metadata = delegate.message.metadata.clone();

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
                &ToolCallSpec {
                    name: &tc.name,
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
    let job_ctx = prepare_job_context(delegate, tc).await?;
    let result = delegate
        .agent
        .execute_chat_tool(&tc.name, &tc.arguments, &job_ctx)
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
