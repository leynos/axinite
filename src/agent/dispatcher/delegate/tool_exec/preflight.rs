//! Preflight stage for chat tool execution.
//!
//! Applies hooks, validates tool calls, and classifies each call as runnable,
//! rejected, or requiring explicit user approval before execution.

use std::sync::Arc;

use crate::agent::Agent;
use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::error::Error;
use crate::tools::redact_params;

/// Outcome of preflight check for a single tool call.
pub(crate) enum PreflightOutcome {
    /// Tool call was rejected by a hook.
    Rejected(String),
    /// Tool call is runnable.
    Runnable,
}

/// Result of grouping tool calls into batches.
pub(crate) struct ToolBatch {
    /// Preflight outcomes for each tool call.
    pub(super) preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)>,
    /// Indices of runnable tools (pointing into preflight).
    pub(super) runnable: Vec<(usize, crate::llm::ToolCall)>,
}

/// A tool call that requires user approval, together with its index in the
/// original call sequence (used to build the deferred-call slice).
pub(super) struct ApprovalCandidate {
    pub idx: usize,
    pub tool_call: crate::llm::ToolCall,
    pub tool: Arc<dyn crate::tools::Tool>,
}

struct BeforeToolCallCtx<'a> {
    delegate: &'a ChatDelegate<'a>,
    original_tc: &'a crate::llm::ToolCall,
    sensitive: &'a [&'a str],
}

struct BeforeToolCallAgentCtx<'a> {
    agent: &'a Agent,
    user_id: &'a str,
    original_tc: &'a crate::llm::ToolCall,
    sensitive: &'a [&'a str],
}

/// Restore original values for sensitive fields into a mutable JSON object.
///
/// After a hook modifies tool parameters, any sensitive key that was
/// redacted before the hook must be put back from the original call to
/// prevent secret loss.
fn restore_sensitive_fields(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    original_args: &serde_json::Value,
    sensitive: &[&str],
) {
    for key in sensitive {
        if let Some(orig_val) = original_args.get(*key) {
            obj.insert((*key).to_string(), orig_val.clone());
        }
    }
}

/// Apply hook parameter modification to a tool call.
fn apply_hook_param_modification(
    tc: &mut crate::llm::ToolCall,
    original_tc: &crate::llm::ToolCall,
    sensitive: &[&str],
    new_params: &str,
) {
    match serde_json::from_str::<serde_json::Value>(new_params) {
        Ok(mut parsed) => {
            if let Some(obj) = parsed.as_object_mut() {
                restore_sensitive_fields(obj, &original_tc.arguments, sensitive);
            }
            tc.arguments = parsed;
        }
        Err(e) => {
            tracing::warn!(
                tool = %tc.name,
                "Hook returned non-JSON modification for ToolCall, ignoring: {}",
                e
            );
        }
    }
}

/// Apply the BeforeToolCall hook and return rejection message if any.
async fn run_before_tool_call_hook(
    ctx: &BeforeToolCallCtx<'_>,
    tc: &mut crate::llm::ToolCall,
) -> Option<String> {
    let hook_params = redact_params(&tc.arguments, ctx.sensitive);
    let event = crate::hooks::HookEvent::ToolCall {
        tool_name: tc.name.clone(),
        parameters: hook_params,
        user_id: ctx.delegate.message.user_id.clone(),
        context: "chat".to_string(),
    };
    match ctx.delegate.agent.hooks().run(&event).await {
        Err(crate::hooks::HookError::Rejected { reason }) => {
            Some(format!("Tool call rejected by hook: {}", reason))
        }
        Err(err) => Some(format!("Tool call blocked by hook policy: {}", err)),
        Ok(crate::hooks::HookOutcome::Continue {
            modified: Some(new_params),
        }) => {
            apply_hook_param_modification(tc, ctx.original_tc, ctx.sensitive, &new_params);
            None
        }
        _ => None,
    }
}

async fn run_before_tool_call_hook_for_agent(
    ctx: &BeforeToolCallAgentCtx<'_>,
    tc: &mut crate::llm::ToolCall,
) -> Option<String> {
    let hook_params = redact_params(&tc.arguments, ctx.sensitive);
    let event = crate::hooks::HookEvent::ToolCall {
        tool_name: tc.name.clone(),
        parameters: hook_params,
        user_id: ctx.user_id.to_string(),
        context: "chat".to_string(),
    };
    match ctx.agent.hooks().run(&event).await {
        Err(crate::hooks::HookError::Rejected { reason }) => {
            Some(format!("Tool call rejected by hook: {}", reason))
        }
        Err(err) => Some(format!("Tool call blocked by hook policy: {}", err)),
        Ok(crate::hooks::HookOutcome::Continue {
            modified: Some(new_params),
        }) => {
            apply_hook_param_modification(tc, ctx.original_tc, ctx.sensitive, &new_params);
            None
        }
        _ => None,
    }
}

/// Apply the BeforeToolCall hook and return rejection message if any.
pub(crate) async fn apply_before_tool_call_hook_for_agent(
    agent: &Agent,
    user_id: &str,
    original_tc: &crate::llm::ToolCall,
    tc: &mut crate::llm::ToolCall,
    sensitive: &[&str],
) -> Option<String> {
    let ctx = BeforeToolCallAgentCtx {
        agent,
        user_id,
        original_tc,
        sensitive,
    };
    run_before_tool_call_hook_for_agent(&ctx, tc).await
}

/// Check if a tool requires approval based on its configuration and auto-approve settings.
async fn tool_requires_approval(
    delegate: &ChatDelegate<'_>,
    tool: &Arc<dyn crate::tools::Tool>,
    tc: &crate::llm::ToolCall,
) -> bool {
    use crate::tools::ApprovalRequirement;
    match tool.requires_approval(&tc.arguments) {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::Always => true,
        ApprovalRequirement::UnlessAutoApproved => {
            let sess = delegate.session.lock().await;
            !sess.is_tool_auto_approved(&tc.name)
        }
    }
}

async fn approval_required_tool(
    delegate: &ChatDelegate<'_>,
    tool_opt: Option<Arc<dyn crate::tools::Tool>>,
    tc: &crate::llm::ToolCall,
) -> Option<Arc<dyn crate::tools::Tool>> {
    if delegate.agent.config.auto_approve_tools {
        return None;
    }
    let tool = tool_opt?;
    if tool_requires_approval(delegate, &tool, tc).await {
        Some(tool)
    } else {
        None
    }
}

/// The outcome of pre-flight classification for a single tool call.
enum ToolCallOutcome {
    /// The before-hook rejected this call with a message.
    Rejected(String),
    /// The call requires user approval before it may run.
    NeedsApproval(ApprovalCandidate),
    /// The call is cleared to run immediately.
    Runnable,
}

async fn classify_tool_call(
    delegate: &ChatDelegate<'_>,
    idx: usize,
    original_tc: &crate::llm::ToolCall,
    tc: &mut crate::llm::ToolCall,
) -> ToolCallOutcome {
    let tool_opt = delegate.agent.tools().get(&tc.name).await;
    let sensitive = tool_opt
        .as_ref()
        .map(|t| t.sensitive_params())
        .unwrap_or(&[]);
    let hook_ctx = BeforeToolCallCtx {
        delegate,
        original_tc,
        sensitive,
    };

    if let Some(rejection_msg) = run_before_tool_call_hook(&hook_ctx, tc).await {
        return ToolCallOutcome::Rejected(rejection_msg);
    }

    if let Some(tool) = approval_required_tool(delegate, tool_opt, tc).await {
        return ToolCallOutcome::NeedsApproval(ApprovalCandidate {
            idx,
            tool_call: tc.clone(),
            tool,
        });
    }

    ToolCallOutcome::Runnable
}

/// Group tool calls into preflight outcomes and runnable batch.
pub(super) async fn group_tool_calls(
    delegate: &ChatDelegate<'_>,
    tool_calls: &[crate::llm::ToolCall],
) -> Result<(ToolBatch, Option<ApprovalCandidate>), Error> {
    let mut preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)> = Vec::new();
    let mut runnable: Vec<(usize, crate::llm::ToolCall)> = Vec::new();
    let mut approval_needed = None;

    for (idx, original_tc) in tool_calls.iter().enumerate() {
        let mut tc = original_tc.clone();

        match classify_tool_call(delegate, idx, original_tc, &mut tc).await {
            ToolCallOutcome::Rejected(msg) => {
                preflight.push((tc, PreflightOutcome::Rejected(msg)));
            }
            ToolCallOutcome::NeedsApproval(candidate) => {
                approval_needed = Some(candidate);
                break;
            }
            ToolCallOutcome::Runnable => {
                let pf_idx = preflight.len();
                preflight.push((tc.clone(), PreflightOutcome::Runnable));
                runnable.push((pf_idx, tc));
            }
        }
    }

    Ok((
        ToolBatch {
            preflight,
            runnable,
        },
        approval_needed,
    ))
}
