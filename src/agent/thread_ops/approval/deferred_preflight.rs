//! Preflight for deferred tool calls: `BeforeToolCall` hook checks and
//! approval gating before execution.

use std::sync::Arc;

use crate::agent::Agent;
use crate::channels::IncomingMessage;
use crate::tools::redact_params;

use super::context::TurnScope;

/// Preflight outcome for a deferred tool call.
pub(super) enum DeferredPreflightOutcome {
    /// Hook rejected the call before execution.
    Rejected(String),
    /// The call is ready to execute.
    Runnable,
}

enum DeferredPreflight {
    Rejected {
        tc: crate::llm::ToolCall,
        msg: String,
    },
    NeedsApproval {
        idx: usize,
        tc: crate::llm::ToolCall,
        tool: Arc<dyn crate::tools::Tool>,
    },
    Runnable {
        tc: crate::llm::ToolCall,
    },
}

struct DeferredToolCallCtx<'a> {
    agent: &'a Agent,
    auto_approved: &'a std::collections::HashSet<String>,
    message: &'a IncomingMessage,
    idx: usize,
}

struct DeferredHookCtx<'a> {
    agent: &'a Agent,
    message: &'a IncomingMessage,
    original_tc: &'a crate::llm::ToolCall,
    sensitive: &'a [&'a str],
}

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

async fn run_before_tool_call_hook_for_deferred(
    ctx: &DeferredHookCtx<'_>,
    tc: &mut crate::llm::ToolCall,
) -> Option<String> {
    let hook_params = redact_params(&tc.arguments, ctx.sensitive);
    let event = crate::hooks::HookEvent::ToolCall {
        tool_name: tc.name.clone(),
        parameters: hook_params,
        user_id: ctx.message.user_id.clone(),
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
            match serde_json::from_str::<serde_json::Value>(&new_params) {
                Ok(mut parsed) => {
                    if let Some(obj) = parsed.as_object_mut() {
                        restore_sensitive_fields(obj, &ctx.original_tc.arguments, ctx.sensitive);
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
            None
        }
        _ => None,
    }
}

async fn approval_required_deferred_tool(
    agent: &Agent,
    auto_approved: &std::collections::HashSet<String>,
    tc: &crate::llm::ToolCall,
) -> Option<Arc<dyn crate::tools::Tool>> {
    let tool = agent.tools().get(&tc.name).await?;
    use crate::tools::ApprovalRequirement;

    let needs_approval = match tool.requires_approval(&tc.arguments) {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !auto_approved.contains(&tc.name),
        ApprovalRequirement::Always => true,
    };

    if needs_approval { Some(tool) } else { None }
}

async fn classify_deferred_tool_call(
    ctx: &DeferredToolCallCtx<'_>,
    original_tc: &crate::llm::ToolCall,
) -> DeferredPreflight {
    let mut tc = original_tc.clone();
    let tool_opt = ctx.agent.tools().get(&tc.name).await;
    let sensitive = tool_opt
        .as_ref()
        .map(|tool| tool.sensitive_params())
        .unwrap_or(&[]);

    let hook_ctx = DeferredHookCtx {
        agent: ctx.agent,
        message: ctx.message,
        original_tc,
        sensitive,
    };

    if let Some(msg) = run_before_tool_call_hook_for_deferred(&hook_ctx, &mut tc).await {
        return DeferredPreflight::Rejected { tc, msg };
    }

    if let Some(tool) = approval_required_deferred_tool(ctx.agent, ctx.auto_approved, &tc).await {
        return DeferredPreflight::NeedsApproval {
            idx: ctx.idx,
            tc,
            tool,
        };
    }

    DeferredPreflight::Runnable { tc }
}

impl Agent {
    /// Preflight deferred tools: collect runnable and find first needing approval.
    pub(super) async fn preflight_deferred_tools(
        &self,
        scope: &TurnScope,
        deferred: &[crate::llm::ToolCall],
    ) -> (
        Vec<(crate::llm::ToolCall, DeferredPreflightOutcome)>,
        Vec<crate::llm::ToolCall>,
        Option<(usize, crate::llm::ToolCall, Arc<dyn crate::tools::Tool>)>,
    ) {
        // Precompute auto-approved tools to avoid repeated locking
        let auto_approved: std::collections::HashSet<String> = {
            let sess = scope.session.lock().await;
            sess.auto_approved_tools.iter().cloned().collect()
        };

        let mut preflight: Vec<(crate::llm::ToolCall, DeferredPreflightOutcome)> = Vec::new();
        let mut runnable: Vec<crate::llm::ToolCall> = Vec::new();
        let mut approval_needed: Option<(
            usize,
            crate::llm::ToolCall,
            Arc<dyn crate::tools::Tool>,
        )> = None;
        let message = scope.to_message();

        for (idx, original_tc) in deferred.iter().enumerate() {
            let classify_ctx = DeferredToolCallCtx {
                agent: self,
                auto_approved: &auto_approved,
                message: &message,
                idx,
            };

            match classify_deferred_tool_call(&classify_ctx, original_tc).await {
                DeferredPreflight::Rejected { tc, msg } => {
                    preflight.push((tc, DeferredPreflightOutcome::Rejected(msg)));
                }
                DeferredPreflight::NeedsApproval { idx, tc, tool } => {
                    approval_needed = Some((idx, tc, tool));
                    break;
                }
                DeferredPreflight::Runnable { tc } => {
                    preflight.push((tc.clone(), DeferredPreflightOutcome::Runnable));
                    runnable.push(tc);
                }
            }
        }

        (preflight, runnable, approval_needed)
    }
}
