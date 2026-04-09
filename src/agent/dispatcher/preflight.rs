//! Preflight checks and batching for tool calls.
//!
//! Contains the preflight phase logic that groups tool calls into batches
//! and determines which tools can run vs which need approval.

use std::sync::Arc;

use crate::agent::dispatcher::delegate::ChatDelegate;
use crate::error::Error;
use crate::llm::{ChatMessage, ReasoningContext};
use crate::tools::redact_params;

/// Outcome of preflight check for a single tool call.
pub(super) enum PreflightOutcome {
    /// Tool call was rejected by a hook.
    Rejected(String),
    /// Tool call is runnable.
    Runnable,
}

/// Result of grouping tool calls into batches.
pub(super) struct ToolBatch {
    /// Preflight outcomes for each tool call.
    pub(super) preflight: Vec<(crate::llm::ToolCall, PreflightOutcome)>,
    /// Indices of runnable tools (pointing into preflight).
    pub(super) runnable: Vec<(usize, crate::llm::ToolCall)>,
}

impl<'a> ChatDelegate<'a> {
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
        let mut runnable: Vec<(usize, crate::llm::ToolCall)> = Vec::new();
        let mut approval_needed: Option<(
            usize,
            crate::llm::ToolCall,
            Arc<dyn crate::tools::Tool>,
        )> = None;

        for (idx, original_tc) in tool_calls.iter().enumerate() {
            let mut tc = original_tc.clone();

            let tool_opt = self.agent.tools().get(&tc.name).await;
            let sensitive = tool_opt
                .as_ref()
                .map(|t| t.sensitive_params())
                .unwrap_or(&[]);

            // Hook: BeforeToolCall
            let hook_params = redact_params(&tc.arguments, sensitive);
            let event = crate::hooks::HookEvent::ToolCall {
                tool_name: tc.name.clone(),
                parameters: hook_params,
                user_id: self.message.user_id.clone(),
                context: "chat".to_string(),
            };
            match self.agent.hooks().run(&event).await {
                Err(crate::hooks::HookError::Rejected { reason }) => {
                    preflight.push((
                        tc,
                        PreflightOutcome::Rejected(format!(
                            "Tool call rejected by hook: {}",
                            reason
                        )),
                    ));
                    continue;
                }
                Err(err) => {
                    preflight.push((
                        tc,
                        PreflightOutcome::Rejected(format!(
                            "Tool call blocked by hook policy: {}",
                            err
                        )),
                    ));
                    continue;
                }
                Ok(crate::hooks::HookOutcome::Continue {
                    modified: Some(new_params),
                }) => match serde_json::from_str::<serde_json::Value>(&new_params) {
                    Ok(mut parsed) => {
                        if let Some(obj) = parsed.as_object_mut() {
                            for key in sensitive {
                                if let Some(orig_val) = original_tc.arguments.get(*key) {
                                    obj.insert((*key).to_string(), orig_val.clone());
                                }
                            }
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
                },
                _ => {}
            }

            // Check if tool requires approval
            if !self.agent.config.auto_approve_tools
                && let Some(tool) = tool_opt
            {
                use crate::tools::ApprovalRequirement;
                let needs_approval = match tool.requires_approval(&tc.arguments) {
                    ApprovalRequirement::Never => false,
                    ApprovalRequirement::UnlessAutoApproved => {
                        let sess = self.session.lock().await;
                        !sess.is_tool_auto_approved(&tc.name)
                    }
                    ApprovalRequirement::Always => true,
                };

                if needs_approval {
                    approval_needed = Some((idx, tc, tool));
                    break;
                }
            }

            let preflight_idx = preflight.len();
            preflight.push((tc.clone(), PreflightOutcome::Runnable));
            runnable.push((preflight_idx, tc));
        }

        Ok((
            ToolBatch {
                preflight,
                runnable,
            },
            approval_needed,
        ))
    }

    /// Handle rejected tool call outcome.
    pub(super) async fn handle_rejected_tool(
        &self,
        tc: &crate::llm::ToolCall,
        error_msg: &str,
        reason_ctx: &mut ReasoningContext,
    ) {
        {
            let mut sess = self.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&self.thread_id)
                && let Some(turn) = thread.last_turn_mut()
            {
                turn.record_tool_error(error_msg.to_string());
            }
        }
        reason_ctx.messages.push(ChatMessage::tool_result(
            &tc.id,
            &tc.name,
            error_msg.to_string(),
        ));
    }
}
