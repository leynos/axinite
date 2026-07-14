//! Orchestration of the deferred-tools continuation after a primary tool
//! approval: preflight, execution, postflight recording, and deferred
//! approval hand-off.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::dispatcher::check_auth_required;
use crate::agent::session::{PendingApproval, Session};
use crate::agent::submission::SubmissionResult;
use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::error::Error;
use crate::llm::ChatMessage;
use crate::tools::redact_params;

use super::auth::AuthInterceptParams;
use super::context::TurnScope;
use super::deferred_exec::DeferredEnv;
use super::deferred_preflight::DeferredPreflightOutcome;

/// Context for entering deferred approval.
struct DeferredApprovalContext<'a> {
    scope: &'a TurnScope,
    approval_idx: usize,
    tc: crate::llm::ToolCall,
    tool: Arc<dyn crate::tools::Tool>,
    deferred_tool_calls: &'a [crate::llm::ToolCall],
    context_messages: &'a [ChatMessage],
    pending: &'a PendingApproval,
}

/// Deferred flow parameter object for bundling co-travelling arguments.
#[derive(Clone)]
pub(super) struct DeferredFlow<'a> {
    pub(super) scope: &'a TurnScope,
    pub(super) job_ctx: &'a JobContext,
    pub(super) pending: &'a PendingApproval,
    pub(super) context_messages: Vec<ChatMessage>,
    pub(super) deferred_tool_calls: Vec<crate::llm::ToolCall>,
}

struct TurnWriteCtx<'a> {
    session: &'a Arc<Mutex<Session>>,
    thread_id: Uuid,
}

async fn record_tool_error_and_push(
    ctx: &TurnWriteCtx<'_>,
    reason_ctx: &mut Vec<ChatMessage>,
    tc: &crate::llm::ToolCall,
    error_msg: String,
) {
    {
        let mut sess = ctx.session.lock().await;
        if let Some(thread) = sess.threads.get_mut(&ctx.thread_id)
            && let Some(turn) = thread.last_turn_mut()
        {
            turn.record_tool_error(error_msg.clone());
        }
    }

    reason_ctx.push(ChatMessage::tool_result(&tc.id, &tc.name, error_msg));
}

impl Agent {
    /// Postflight: record results, emit ToolResult previews, check for deferred auth.
    async fn postflight_record_and_maybe_deferred_auth(
        &self,
        scope: &TurnScope,
        preflight: Vec<(crate::llm::ToolCall, DeferredPreflightOutcome)>,
        exec_results: Vec<(crate::llm::ToolCall, Result<String, Error>)>,
        context_messages: &mut Vec<ChatMessage>,
        pending: &PendingApproval,
    ) -> Option<String> {
        let mut exec_results = std::collections::VecDeque::from(exec_results);
        let mut deferred_auth: Option<String> = None;
        let turn_write_ctx = TurnWriteCtx {
            session: &scope.session,
            thread_id: scope.thread_id,
        };

        for (tc, outcome) in preflight {
            let Some(deferred_result) = (match outcome {
                DeferredPreflightOutcome::Rejected(error_msg) => {
                    record_tool_error_and_push(&turn_write_ctx, context_messages, &tc, error_msg)
                        .await;
                    None
                }
                DeferredPreflightOutcome::Runnable => Some(
                    exec_results
                        .pop_front()
                        .map(|(_executed_tc, result)| result)
                        .unwrap_or_else(|| {
                            Err(crate::error::ToolError::ExecutionFailed {
                                name: tc.name.clone(),
                                reason: "No result available".to_string(),
                            }
                            .into())
                        }),
                ),
            }) else {
                continue;
            };

            // Sanitize first before any use of the output
            let is_deferred_error = deferred_result.is_err();
            let (deferred_content, _) = crate::tools::execute::process_tool_result(
                self.safety(),
                &tc.name,
                &tc.id,
                &deferred_result,
            );

            // Send ToolResult preview using sanitized content (only on success and non-empty)
            if !is_deferred_error && !deferred_content.is_empty() {
                let preview = crate::agent::dispatcher::truncate_for_preview(
                    &deferred_content,
                    crate::agent::dispatcher::PREVIEW_MAX_CHARS,
                );
                let _ = self
                    .channels
                    .send_status(
                        &scope.env.channel,
                        StatusUpdate::ToolResult {
                            name: tc.name.clone(),
                            preview,
                        },
                        &scope.env.metadata,
                    )
                    .await;
            }

            // Record sanitized result in thread
            {
                let mut sess = scope.session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&scope.thread_id)
                    && let Some(turn) = thread.last_turn_mut()
                {
                    if is_deferred_error {
                        turn.record_tool_error(deferred_content.clone());
                    } else {
                        turn.record_tool_result_content(&deferred_content);
                    }
                }
            }

            // Auth detection — defer return until all results are recorded
            if deferred_auth.is_none()
                && let Some((ext_name, instructions)) =
                    check_auth_required(&tc.name, &deferred_result)
            {
                // Build fresh PendingApproval representing the live deferred continuation.
                // Take the original pending and update it with the current context_messages
                // (which includes results from deferred calls that have already executed)
                // and clear deferred_tool_calls since we can't resume partial deferred batches.
                let fresh_pending = PendingApproval {
                    request_id: pending.request_id,
                    tool_name: tc.name.clone(),
                    parameters: tc.arguments.clone(),
                    display_parameters: redact_params(&tc.arguments, &[]),
                    description: format!("Authenticate to continue with {}", tc.name),
                    tool_call_id: tc.id.clone(),
                    context_messages: context_messages.clone(),
                    deferred_tool_calls: Vec::new(),
                    user_timezone: pending.user_timezone.clone(),
                };
                self.handle_auth_intercept(AuthInterceptParams {
                    session: &scope.session,
                    thread_id: scope.thread_id,
                    env: &scope.env,
                    tool_result: &deferred_result,
                    ext_name,
                    instructions: instructions.clone(),
                    pending: Some(fresh_pending),
                })
                .await;
                deferred_auth = Some(instructions);
            }

            context_messages.push(ChatMessage::tool_result(&tc.id, &tc.name, deferred_content));
        }

        deferred_auth
    }

    /// Enter deferred approval mode and notify.
    async fn enter_deferred_approval_and_notify(
        &self,
        ctx: DeferredApprovalContext<'_>,
    ) -> SubmissionResult {
        let DeferredApprovalContext {
            scope,
            approval_idx,
            tc,
            tool,
            deferred_tool_calls,
            context_messages,
            pending,
        } = ctx;
        let new_pending = PendingApproval {
            request_id: Uuid::new_v4(),
            tool_name: tc.name.clone(),
            parameters: tc.arguments.clone(),
            display_parameters: redact_params(&tc.arguments, tool.sensitive_params()),
            description: tool.description().to_string(),
            tool_call_id: tc.id.clone(),
            context_messages: context_messages.to_vec(),
            deferred_tool_calls: deferred_tool_calls[approval_idx + 1..].to_vec(),
            // Carry forward the resolved timezone from the original pending approval
            user_timezone: pending.user_timezone.clone(),
        };

        let request_id = new_pending.request_id;
        let tool_name = new_pending.tool_name.clone();
        let description = new_pending.description.clone();
        let parameters = new_pending.display_parameters.clone();

        {
            let mut sess = scope.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                thread.await_approval(new_pending);
            }
        }

        let _ = self
            .channels
            .send_status(
                &scope.env.channel,
                StatusUpdate::Status("Awaiting approval".into()),
                &scope.env.metadata,
            )
            .await;

        SubmissionResult::NeedApproval {
            request_id,
            tool_name,
            description,
            parameters,
        }
    }

    /// Handle deferred tools flow: preflight, execute, postflight.
    /// Returns the (possibly mutated) context_messages and an optional SubmissionResult.
    pub(super) async fn handle_deferred_tools_flow<'a>(
        &self,
        mut flow: DeferredFlow<'a>,
    ) -> Result<(Vec<ChatMessage>, Option<SubmissionResult>), Error> {
        // Preflight deferred tools
        let (preflight, runnable, approval_needed) = self
            .preflight_deferred_tools(flow.scope, &flow.deferred_tool_calls)
            .await;

        // Execute runnable deferred tools
        let exec = DeferredEnv {
            job_ctx: flow.job_ctx.clone(),
            env: flow.scope.env.clone(),
        };
        let exec_results = self.execute_runnable_deferred(&runnable, &exec).await;

        // Postflight: record results and check for auth
        if let Some(instructions) = self
            .postflight_record_and_maybe_deferred_auth(
                flow.scope,
                preflight,
                exec_results,
                &mut flow.context_messages,
                flow.pending,
            )
            .await
        {
            return Ok((
                flow.context_messages,
                Some(SubmissionResult::response(instructions)),
            ));
        }

        // Handle deferred approval needed
        if let Some((idx, tc, tool)) = approval_needed {
            let result = self
                .enter_deferred_approval_and_notify(DeferredApprovalContext {
                    scope: flow.scope,
                    approval_idx: idx,
                    tc,
                    tool,
                    deferred_tool_calls: &flow.deferred_tool_calls,
                    context_messages: &flow.context_messages,
                    pending: flow.pending,
                })
                .await;
            return Ok((flow.context_messages, Some(result)));
        }

        // Continue agentic loop - not handled here, return None
        Ok((flow.context_messages, None))
    }
}
