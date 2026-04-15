//! Tool-call preflight for dispatcher execution.
//! Evaluates hooks, restores redacted parameters when hooks rewrite arguments,
//! resolves approval gates, and groups runnable calls without disturbing the
//! original tool-call order.

use std::sync::Arc;

use crate::error::Error;
use crate::tools::redact_params;

use super::ChatDelegate;
use crate::agent::dispatcher::types::*;

/// Return `true` if a tool invocation requires interactive approval.
pub(in crate::agent::dispatcher) fn approval_requirement_needs_approval(
    requirement: crate::tools::ApprovalRequirement,
    session: &crate::agent::session::Session,
    tool_name: &str,
) -> bool {
    use crate::tools::ApprovalRequirement;

    match requirement {
        ApprovalRequirement::Never => false,
        ApprovalRequirement::UnlessAutoApproved => !session.is_tool_auto_approved(tool_name),
        ApprovalRequirement::Always => true,
    }
}

/// Restore original values of sensitive parameters into a hook-modified JSON
/// object, ensuring that fields the hook was not permitted to see are not
/// inadvertently erased.
fn restore_sensitive_params(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    original_tc: &crate::llm::ToolCall,
    sensitive: &[&str],
) {
    for key in sensitive {
        if let Some(orig_val) = original_tc.arguments.get(*key) {
            obj.insert((*key).to_string(), orig_val.clone());
        }
    }
}

/// Apply hook-modified parameters back onto `tc`, restoring any sensitive
/// fields from the original arguments to prevent them being erased.
fn apply_hook_params(
    tc: &mut crate::llm::ToolCall,
    original_tc: &crate::llm::ToolCall,
    sensitive: &[&str],
    new_params: &str,
) {
    match serde_json::from_str::<serde_json::Value>(new_params) {
        Ok(mut parsed) => {
            if let Some(obj) = parsed.as_object_mut() {
                restore_sensitive_params(obj, original_tc, sensitive);
                tc.arguments = parsed;
            } else {
                tracing::warn!(
                    tool = %tc.name,
                    "Hook returned non-object ToolCall arguments, ignoring"
                );
            }
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

/// The outcome of pre-flighting a single tool call in `group_tool_calls`.
enum ToolPreflightResult {
    /// The hook or policy rejected this call before execution.
    Rejected(crate::llm::ToolCall, PreflightOutcome),
    /// The tool requires human approval; the loop must stop here.
    NeedsApproval(usize, crate::llm::ToolCall, Arc<dyn crate::tools::Tool>),
    /// The tool may proceed; append to the runnable batch.
    Runnable(crate::llm::ToolCall),
}

impl<'a> ChatDelegate<'a> {
    /// Return `true` if tool approval is enforced (auto-approve is disabled).
    fn tool_approval_enforced(&self) -> bool {
        !self.agent.config.auto_approve_tools
    }

    /// Return `true` if `tool` requires human approval for this invocation.
    /// Consults the session's auto-approve list when the requirement is
    /// `UnlessAutoApproved`.
    async fn resolve_needs_approval(
        &self,
        tool: &Arc<dyn crate::tools::Tool>,
        tc_name: &str,
        arguments: &serde_json::Value,
    ) -> bool {
        let requirement = tool.requires_approval(arguments);
        let sess = self.session.lock().await;
        approval_requirement_needs_approval(requirement, &sess, tc_name)
    }

    /// Run the `BeforeToolCall` hook for one tool invocation.
    ///
    /// Returns `Some(PreflightOutcome::Rejected(…))` when the hook blocks the
    /// call (the caller should push that outcome and `continue` to the next
    /// tool). Returns `None` when the call should proceed; `tc.arguments` may
    /// have been mutated to incorporate hook-supplied parameter overrides.
    async fn run_tool_hook_preflight(
        &self,
        tc: &mut crate::llm::ToolCall,
        original_tc: &crate::llm::ToolCall,
        sensitive: &[&str],
    ) -> Option<PreflightOutcome> {
        let hook_params = redact_params(&tc.arguments, sensitive);
        let event = crate::hooks::HookEvent::ToolCall {
            tool_name: tc.name.clone(),
            parameters: hook_params,
            user_id: self.message.user_id.clone(),
            context: "chat".to_string(),
        };

        match self.agent.hooks().run(&event).await {
            Err(crate::hooks::HookError::Rejected { reason }) => Some(PreflightOutcome::Rejected(
                format!("Tool call rejected by hook: {}", reason),
            )),
            Err(err) => Some(PreflightOutcome::Rejected(format!(
                "Tool call blocked by hook policy: {}",
                err
            ))),
            Ok(crate::hooks::HookOutcome::Continue {
                modified: Some(new_params),
            }) => {
                apply_hook_params(tc, original_tc, sensitive, &new_params);
                None
            }
            _ => None,
        }
    }

    /// Evaluate the hook and approval pre-flight for a single tool call.
    ///
    /// Returns the appropriate [`ToolPreflightResult`] variant so that
    /// `group_tool_calls` can remain free of nested conditional logic.
    async fn preflight_one_tool_call(
        &self,
        idx: usize,
        original_tc: &crate::llm::ToolCall,
    ) -> ToolPreflightResult {
        let mut tc = original_tc.clone();
        let tool_opt = self.agent.tools().get(&tc.name).await;
        let sensitive = tool_opt
            .as_ref()
            .map(|t| t.sensitive_params())
            .unwrap_or(&[]);

        if let Some(rejected) = self
            .run_tool_hook_preflight(&mut tc, original_tc, sensitive)
            .await
        {
            return ToolPreflightResult::Rejected(tc, rejected);
        }

        // Approval gate: only reached when enforcement is on and a matching
        // tool is found.  The inner check is intentionally kept as a separate
        // `if` so each condition is independently visible (CodeScene: Complex
        // Conditional).
        #[expect(
            clippy::collapsible_if,
            reason = "Approval-enforced + tool-found + needs-approval are intentionally \
                      decomposed for readability per CodeScene Complex Conditional pattern"
        )]
        if self.tool_approval_enforced() {
            if let Some(tool) = tool_opt {
                if self
                    .resolve_needs_approval(&tool, &tc.name, &tc.arguments)
                    .await
                {
                    return ToolPreflightResult::NeedsApproval(idx, tc, tool);
                }
            }
        }

        ToolPreflightResult::Runnable(tc)
    }

    /// Group tool calls into preflight outcomes and runnable batch.
    pub(super) async fn group_tool_calls(
        &self,
        tool_calls: &[crate::llm::ToolCall],
    ) -> Result<
        (
            ToolBatch,
            Option<(usize, crate::llm::ToolCall, Arc<dyn crate::tools::Tool>)>,
        ),
        Error,
    > {
        let mut preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)> = Vec::new();
        let mut runnable: Vec<usize> = Vec::new();
        let mut approval_needed: Option<(
            usize,
            crate::llm::ToolCall,
            Arc<dyn crate::tools::Tool>,
        )> = None;

        for (idx, original_tc) in tool_calls.iter().enumerate() {
            match self.preflight_one_tool_call(idx, original_tc).await {
                ToolPreflightResult::Rejected(tc, outcome) => {
                    preflight.push((tc, outcome));
                }
                ToolPreflightResult::NeedsApproval(idx, tc, tool) => {
                    approval_needed = Some((idx, tc, tool));
                    break;
                }
                ToolPreflightResult::Runnable(tc) => {
                    let preflight_idx = preflight.len();
                    preflight.push((tc, PreflightOutcome::Runnable));
                    runnable.push(preflight_idx);
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
}
