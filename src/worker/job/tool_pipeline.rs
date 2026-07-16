//! Per-call helpers for the job tool-execution pipeline.
//!
//! These free functions implement the individual stages used by
//! `Worker::execute_tool_inner` in `tool_exec`: tool resolution and approval,
//! rate limiting, the `BeforeToolCall` hook, parameter validation, outcome
//! logging, memory recording, and result serialization.

use std::sync::Arc;
use std::time::Duration;

use uuid::Uuid;

use crate::context::JobContext;
use crate::error::Error;
use crate::tools::rate_limiter::RateLimitResult;
use crate::tools::{ApprovalContext, Tool, ToolOutput, redact_params};

use super::WorkerDeps;

/// A timed tool execution result: the outer `Err` is a timeout, the inner
/// `Err` is a tool failure.
pub(super) type TimedToolResult =
    Result<Result<ToolOutput, crate::tools::ToolError>, tokio::time::error::Elapsed>;

/// Outcome summary used when recording a tool action in job memory.
pub(super) enum ActionOutcome {
    /// Successful execution: sanitized output string plus the raw value.
    Success {
        /// Sanitized, pretty-printed output for human-readable records.
        output_str: Option<String>,
        /// Raw JSON result returned by the tool.
        result: serde_json::Value,
    },
    /// Failed or timed-out execution with an error message.
    Failure(String),
}

/// Look up the tool and confirm it is not blocked by the approval policy.
pub(super) async fn resolve_approved_tool(
    deps: &WorkerDeps,
    tool_name: &str,
    params: &serde_json::Value,
) -> Result<Arc<dyn Tool>, Error> {
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

    Ok(tool)
}

/// Fetch the job context, propagating the HTTP interceptor for trace
/// recording and replay when the context has none of its own.
pub(super) async fn job_context_with_interceptor(
    deps: &WorkerDeps,
    job_id: Uuid,
) -> Result<JobContext, Error> {
    let mut job_ctx = deps.context_manager.get_context(job_id).await?;
    if job_ctx.http_interceptor.is_none() {
        job_ctx.http_interceptor = deps.http_interceptor.clone();
    }
    Ok(job_ctx)
}

/// Enforce the tool's per-user rate limit before hooks or execution
/// (the cheaper check runs first).
pub(super) async fn check_tool_rate_limit(
    deps: &WorkerDeps,
    tool: &dyn Tool,
    job_ctx: &JobContext,
    tool_name: &str,
) -> Result<(), Error> {
    let Some(config) = tool.rate_limit_config() else {
        return Ok(());
    };
    if let RateLimitResult::Limited { retry_after, .. } = deps
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
    Ok(())
}

/// Inputs for the `BeforeToolCall` hook stage of the tool pipeline.
pub(super) struct ToolCallHookArgs<'a> {
    /// Shared worker dependencies (hook registry, tools, safety, ...).
    pub(super) deps: &'a WorkerDeps,
    /// The resolved tool, consulted for its sensitive parameter names.
    pub(super) tool: &'a dyn Tool,
    /// Name of the tool being invoked.
    pub(super) tool_name: &'a str,
    /// Raw parameters supplied for the tool call.
    pub(super) params: &'a serde_json::Value,
    /// Job context providing the acting user's identity.
    pub(super) job_ctx: &'a JobContext,
    /// Identifier of the job the tool call belongs to.
    pub(super) job_id: Uuid,
}

/// Run the `BeforeToolCall` hook, returning the (possibly modified)
/// parameters or an error when a hook rejects the call.
pub(super) async fn apply_tool_call_hook(
    args: ToolCallHookArgs<'_>,
) -> Result<serde_json::Value, Error> {
    use crate::hooks::{HookError, HookEvent, HookOutcome};

    let ToolCallHookArgs {
        deps,
        tool,
        tool_name,
        params,
        job_ctx,
        job_id,
    } = args;
    let hook_params = redact_params(params, tool.sensitive_params());
    let event = HookEvent::ToolCall {
        tool_name: tool_name.to_string(),
        parameters: hook_params,
        user_id: job_ctx.user_id.clone(),
        context: format!("job:{}", job_id),
    };
    match deps.hooks.run(&event).await {
        Err(HookError::Rejected { reason }) => Err(crate::error::ToolError::ExecutionFailed {
            name: tool_name.to_string(),
            reason: format!("Blocked by hook: {}", reason),
        }
        .into()),
        Err(err) => Err(crate::error::ToolError::ExecutionFailed {
            name: tool_name.to_string(),
            reason: format!("Blocked by hook failure mode: {}", err),
        }
        .into()),
        Ok(HookOutcome::Continue {
            modified: Some(new_params),
        }) => Ok(serde_json::from_str(&new_params).unwrap_or_else(|e| {
            tracing::warn!(
                tool = %tool_name,
                "Hook returned non-JSON modification for ToolCall, ignoring: {}",
                e
            );
            params.clone()
        })),
        _ => Ok(params.clone()),
    }
}

/// Validate the tool parameters against the safety validator.
pub(super) fn validate_tool_params(
    deps: &WorkerDeps,
    tool_name: &str,
    params: &serde_json::Value,
) -> Result<(), Error> {
    let validation = deps.safety.validator().validate_tool_params(params);
    if validation.is_valid {
        return Ok(());
    }
    let details = validation
        .errors
        .iter()
        .map(|e| format!("{}: {}", e.field, e.message))
        .collect::<Vec<_>>()
        .join("; ");
    Err(crate::error::ToolError::InvalidParameters {
        name: tool_name.to_string(),
        reason: format!("Invalid tool parameters: {}", details),
    }
    .into())
}

/// Emit a debug log line describing how the tool call finished.
pub(super) fn log_tool_outcome(
    tool_name: &str,
    result: &TimedToolResult,
    elapsed: Duration,
    tool_timeout: Duration,
) {
    match result {
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
}

/// Distil the timed execution result into the outcome recorded in memory.
pub(super) fn summarize_outcome(
    deps: &WorkerDeps,
    tool_name: &str,
    result: &TimedToolResult,
) -> ActionOutcome {
    match result {
        Ok(Ok(output)) => {
            let output_str = serde_json::to_string_pretty(&output.result)
                .ok()
                .map(|s| deps.safety.sanitize_tool_output(tool_name, &s).content);
            ActionOutcome::Success {
                output_str,
                result: output.result.clone(),
            }
        }
        Ok(Err(e)) => ActionOutcome::Failure(e.to_string()),
        Err(_) => ActionOutcome::Failure("Execution timeout".to_string()),
    }
}

/// Inputs for recording a completed tool action in job memory.
pub(super) struct RecordActionArgs<'a> {
    /// Shared worker dependencies (context manager, store, ...).
    pub(super) deps: &'a WorkerDeps,
    /// Identifier of the job the action belongs to.
    pub(super) job_id: Uuid,
    /// Name of the tool that was invoked.
    pub(super) tool_name: &'a str,
    /// Redacted parameters recorded alongside the action.
    pub(super) safe_params: &'a serde_json::Value,
    /// Distilled execution outcome (success payload or failure message).
    pub(super) outcome: ActionOutcome,
    /// Wall-clock duration of the tool execution.
    pub(super) elapsed: Duration,
}

/// Record the action in job memory, returning the record for persistence.
pub(super) async fn record_action_in_memory(
    args: RecordActionArgs<'_>,
) -> Option<crate::context::ActionRecord> {
    let RecordActionArgs {
        deps,
        job_id,
        tool_name,
        safe_params,
        outcome,
        elapsed,
    } = args;
    let memory_update = deps
        .context_manager
        .update_memory(job_id, |mem| {
            let base = mem.create_action(tool_name, safe_params.clone());
            let rec = match &outcome {
                ActionOutcome::Success { output_str, result } => {
                    base.succeed(output_str.clone(), result.clone(), elapsed)
                }
                ActionOutcome::Failure(msg) => base.fail(msg.clone(), elapsed),
            };
            mem.record_action(rec.clone());
            rec
        })
        .await;
    match memory_update {
        Ok(rec) => Some(rec),
        Err(e) => {
            tracing::warn!(job_id = %job_id, tool = tool_name, "Failed to record action in memory: {e}");
            None
        }
    }
}

/// Map the timed result to the final serialized output string.
pub(super) fn finalize_output(
    tool_name: &str,
    result: TimedToolResult,
    tool_timeout: Duration,
) -> Result<String, Error> {
    let output = result
        .map_err(|_| crate::error::ToolError::Timeout {
            name: tool_name.to_string(),
            timeout: tool_timeout,
        })?
        .map_err(|e| crate::error::ToolError::ExecutionFailed {
            name: tool_name.to_string(),
            reason: e.to_string(),
        })?;

    serde_json::to_string_pretty(&output.result).map_err(|e| {
        crate::error::ToolError::ExecutionFailed {
            name: tool_name.to_string(),
            reason: format!("Failed to serialize result: {}", e),
        }
        .into()
    })
}
