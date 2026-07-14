//! The `process_approval` entry point: pending-approval bookkeeping,
//! primary tool execution, and hand-off to the deferred-tools flow.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::dispatcher::check_auth_required;
use crate::agent::session::{PendingApproval, Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::channels::StatusUpdate;
use crate::context::JobContext;
use crate::error::Error;
use crate::llm::ChatMessage;

use super::auth::AuthInterceptParams;
use super::context::{ApprovalParams, MsgEnv, TurnScope};
use super::deferred_flow::DeferredFlow;

impl Agent {
    /// Take pending approval if thread is in AwaitingApproval state.
    async fn take_pending_approval_if_awaiting(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<Option<PendingApproval>, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        if thread.state != ThreadState::AwaitingApproval {
            // Stale or duplicate approval (tool already executed) — silently ignore.
            tracing::debug!(
                %thread_id,
                state = ?thread.state,
                "Ignoring stale approval: thread not in AwaitingApproval state"
            );
            return Ok(None);
        }

        let pending = thread.take_pending_approval();
        if pending.is_none() {
            tracing::debug!(
                %thread_id,
                "Ignoring stale approval: no pending approval found"
            );
        } else {
            // Atomically transition to Processing under the same lock to prevent race with interrupt
            thread.state = ThreadState::Processing;
            thread.updated_at = chrono::Utc::now();
        }
        Ok(pending)
    }

    /// Restage pending approval if request ID doesn't match.
    async fn restage_on_request_id_mismatch(
        &self,
        scope: &TurnScope,
        provided: Option<Uuid>,
        pending: &PendingApproval,
    ) -> Result<Option<SubmissionResult>, Error> {
        if let Some(req_id) = provided
            && req_id != pending.request_id
        {
            // Put it back and return error
            let mut sess = scope.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                thread.await_approval(pending.clone());
            }
            return Ok(Some(SubmissionResult::error(
                "Request ID mismatch. Use the correct request ID.",
            )));
        }
        Ok(None)
    }

    /// Auto-approve tool if always flag is set.
    async fn auto_approve_if_always(
        &self,
        session: &Arc<Mutex<Session>>,
        always: bool,
        tool_name: &str,
    ) {
        if always {
            let mut sess = session.lock().await;
            sess.auto_approve_tool(tool_name);
            tracing::info!("Auto-approved tool '{}' for session {}", tool_name, sess.id);
        }
    }

    /// Build JobContext for approval execution.
    fn build_job_context_for_approval(
        &self,
        env: &MsgEnv,
        pending: &PendingApproval,
    ) -> JobContext {
        let mut job_ctx = JobContext::with_user(&env.user_id, "chat", "Interactive chat session");
        job_ctx.http_interceptor = self.deps.http_interceptor.clone();
        // Prefer a valid timezone from the approval message, fall back to the
        // resolved timezone stored when the approval was originally requested.
        let tz_candidate = env
            .timezone
            .as_deref()
            .filter(|tz| crate::timezone::parse_timezone(tz).is_some())
            .or(pending.user_timezone.as_deref());
        if let Some(tz) = tz_candidate {
            job_ctx.user_timezone = tz.to_string();
        }
        job_ctx
    }

    /// Execute primary tool and send notifications.
    async fn execute_primary_tool_and_notify(
        &self,
        env: &MsgEnv,
        pending: &PendingApproval,
        job_ctx: &JobContext,
    ) -> (Result<String, Error>, Option<Arc<dyn crate::tools::Tool>>) {
        let _ = self
            .channels
            .send_status(
                &env.channel,
                StatusUpdate::ToolStarted {
                    name: pending.tool_name.clone(),
                },
                &env.metadata,
            )
            .await;

        let tool_result = self
            .execute_chat_tool(&pending.tool_name, &pending.parameters, job_ctx)
            .await;

        let tool_ref = self.tools().get(&pending.tool_name).await;
        let _ = self
            .channels
            .send_status(
                &env.channel,
                StatusUpdate::tool_completed(
                    pending.tool_name.clone(),
                    &tool_result,
                    &pending.display_parameters,
                    tool_ref.as_deref(),
                ),
                &env.metadata,
            )
            .await;

        // Process tool result through safety pipeline for preview
        let processed_preview = if let Ok(ref _output) = tool_result {
            let (processed, _) = crate::tools::execute::process_tool_result(
                self.safety(),
                &pending.tool_name,
                &pending.tool_call_id,
                &tool_result,
            );
            processed
        } else {
            String::new()
        };

        if !processed_preview.is_empty() {
            let preview = crate::agent::dispatcher::truncate_for_preview(
                &processed_preview,
                crate::agent::dispatcher::PREVIEW_MAX_CHARS,
            );
            let _ = self
                .channels
                .send_status(
                    &env.channel,
                    StatusUpdate::ToolResult {
                        name: pending.tool_name.clone(),
                        preview,
                    },
                    &env.metadata,
                )
                .await;
        }

        (tool_result, tool_ref)
    }

    /// Record sanitized primary tool result and return content with error flag.
    async fn record_sanitised_primary_result(
        &self,
        scope: &TurnScope,
        pending: &PendingApproval,
        tool_result: &Result<String, Error>,
    ) -> (String, bool) {
        let is_tool_error = tool_result.is_err();
        let (result_content, _) = crate::tools::execute::process_tool_result(
            self.safety(),
            &pending.tool_name,
            &pending.tool_call_id,
            tool_result,
        );

        // Record sanitized result in thread
        {
            let mut sess = scope.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&scope.thread_id)
                && let Some(turn) = thread.last_turn_mut()
            {
                if is_tool_error {
                    turn.record_tool_error(result_content.clone());
                } else {
                    turn.record_tool_result_content(&result_content);
                }
            }
        }

        (result_content, is_tool_error)
    }

    /// Check for auth intercept after primary tool execution.
    async fn maybe_auth_intercept_after_primary(
        &self,
        scope: &TurnScope,
        pending: &PendingApproval,
        tool_result: &Result<String, Error>,
    ) -> Option<SubmissionResult> {
        if let Some((ext_name, instructions)) = check_auth_required(&pending.tool_name, tool_result)
        {
            self.handle_auth_intercept(AuthInterceptParams {
                session: &scope.session,
                thread_id: scope.thread_id,
                env: &scope.env,
                tool_result,
                ext_name,
                instructions: instructions.clone(),
                pending: Some(pending.clone()),
            })
            .await;
            return Some(SubmissionResult::response(instructions));
        }
        None
    }

    /// Build context messages and notify for deferred execution.
    async fn build_context_and_notify_for_deferred(
        &self,
        env: &MsgEnv,
        pending: &PendingApproval,
        result_content: String,
    ) -> (Vec<ChatMessage>, Vec<crate::llm::ToolCall>) {
        let mut context_messages = pending.context_messages.clone();
        context_messages.push(ChatMessage::tool_result(
            &pending.tool_call_id,
            &pending.tool_name,
            result_content,
        ));

        let deferred_tool_calls = pending.deferred_tool_calls.clone();

        // Notify about deferred execution
        if !deferred_tool_calls.is_empty() {
            let _ = self
                .channels
                .send_status(
                    &env.channel,
                    StatusUpdate::Thinking(format!(
                        "Executing {} deferred tool(s)...",
                        deferred_tool_calls.len()
                    )),
                    &env.metadata,
                )
                .await;
        }

        (context_messages, deferred_tool_calls)
    }

    /// Process an approval or rejection of a pending tool execution.
    pub(in crate::agent::thread_ops) async fn process_approval(
        &self,
        scope: TurnScope,
        params: ApprovalParams,
    ) -> Result<SubmissionResult, Error> {
        // a) Get pending approval
        let pending = match self
            .take_pending_approval_if_awaiting(&scope.session, scope.thread_id)
            .await?
        {
            Some(p) => p,
            None => return Ok(SubmissionResult::ok_with_message("")),
        };

        // b) Check request ID mismatch
        if let Some(res) = self
            .restage_on_request_id_mismatch(&scope, params.request_id, &pending)
            .await?
        {
            return Ok(res);
        }

        // c) Handle rejection
        if !params.approved {
            return self.complete_rejection_and_persist(&scope, &pending).await;
        }

        // d) Auto-approve (thread already transitioned to Processing in take_pending_approval_if_awaiting)
        self.auto_approve_if_always(&scope.session, params.always, &pending.tool_name)
            .await;

        // e) Build context and execute primary tool
        let job_ctx = self.build_job_context_for_approval(&scope.env, &pending);
        let (tool_result, _) = self
            .execute_primary_tool_and_notify(&scope.env, &pending, &job_ctx)
            .await;

        // f) Record result and check for auth intercept
        let (result_content, _) = self
            .record_sanitised_primary_result(&scope, &pending, &tool_result)
            .await;
        if let Some(res) = self
            .maybe_auth_intercept_after_primary(&scope, &pending, &tool_result)
            .await
        {
            return Ok(res);
        }

        // g) Build context messages and process deferred tools
        let (context_messages, deferred_tool_calls) = self
            .build_context_and_notify_for_deferred(&scope.env, &pending, result_content)
            .await;

        // Handle deferred tools flow
        let (context_messages, maybe_outcome) = self
            .handle_deferred_tools_flow(DeferredFlow {
                scope: &scope,
                job_ctx: &job_ctx,
                pending: &pending,
                context_messages,
                deferred_tool_calls,
            })
            .await?;
        if let Some(result) = maybe_outcome {
            return Ok(result);
        }

        // h) Continue agentic loop
        self.continue_loop_after_tool(scope, context_messages).await
    }
}
