//! Approval and auth-intercept flows for thread operations.
//!
//! This module manages the approval and authentication state machines for tool execution.
//!
//! ## State Machine
//!
//! The approval flow follows this state progression:
//! - **Initial/Unapproved**: Tool execution requires user approval
//! - **Pending Approval**: Thread enters `AwaitingApproval` state with `PendingApproval` stored
//! - **Approved/Authorised**: User approves; tool executes and thread returns to `Idle`
//! - **Rejected/Terminated**: User rejects; thread returns to `Idle` with rejection recorded
//!
//! The auth flow follows this progression:
//! - **Auth Required**: Extension requires authentication token
//! - **Pending Auth**: Thread has `pending_auth` set; next user message is intercepted
//! - **Authenticated**: Token provided and validated; extension activated
//! - **Auth Failed**: Token invalid; re-enters auth mode for retry
//!
//! ## Entry Points
//!
//! - `process_approval`: Called by the dispatch layer when user approves/rejects a pending tool.
//!   Caller must ensure thread is in `AwaitingApproval` state with valid `PendingApproval`.
//!
//! - `process_auth_token`: Called when user provides auth token while thread has `pending_auth`.
//!   Caller must ensure thread has valid `PendingAuth` and handle retry on failure.
//!
//! ## Invariants
//!
//! - Callers must hold valid thread metadata (thread_id, session) before invoking.
//! - Idempotent retries are supported; duplicate approvals with same request_id are ignored.
//! - State transitions are atomic under the session lock.
//! - Side effects (DB persistence, status updates) occur after state transitions complete.
//! - Concurrency: Single-writer assumption per thread; session lock must be held for state changes.

use std::sync::Arc;

use tokio::sync::Mutex;
use tokio::task::JoinSet;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::dispatcher::{
    AgenticLoopResult, check_auth_required, execute_chat_tool_standalone, parse_auth_result,
};
use crate::agent::session::{PendingApproval, Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::context::JobContext;
use crate::error::Error;
use crate::llm::ChatMessage;
use crate::tools::redact_params;

/// Message environment context.
#[derive(Clone)]
pub(crate) struct MsgEnv {
    channel: String,
    user_id: String,
    metadata: serde_json::Value,
    timezone: Option<String>,
    content: String,
}

impl From<&IncomingMessage> for MsgEnv {
    fn from(m: &IncomingMessage) -> Self {
        Self {
            channel: m.channel.clone(),
            user_id: m.user_id.clone(),
            metadata: m.metadata.clone(),
            timezone: m.timezone.clone(),
            content: m.content.clone(),
        }
    }
}

/// Turn scope context bundling session, thread, and message environment.
#[derive(Clone)]
pub(crate) struct TurnScope {
    session: Arc<Mutex<Session>>,
    thread_id: Uuid,
    env: MsgEnv,
}

impl TurnScope {
    pub(crate) fn new(
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &IncomingMessage,
    ) -> Self {
        Self {
            session,
            thread_id,
            env: MsgEnv::from(message),
        }
    }

    /// Create a mock IncomingMessage from the environment for use with
    /// functions that require the full message type.
    fn to_message(&self) -> IncomingMessage {
        IncomingMessage {
            id: uuid::Uuid::new_v4(),
            channel: self.env.channel.clone(),
            user_id: self.env.user_id.clone(),
            user_name: None,
            content: self.env.content.clone(),
            thread_id: None,
            received_at: chrono::Utc::now(),
            metadata: self.env.metadata.clone(),
            attachments: vec![],
            timezone: self.env.timezone.clone(),
        }
    }
}

/// Approval parameters.
#[derive(Clone, Copy)]
pub(crate) struct ApprovalParams {
    pub(crate) request_id: Option<Uuid>,
    pub(crate) approved: bool,
    pub(crate) always: bool,
}

/// Deferred execution environment.
#[derive(Clone)]
pub(crate) struct DeferredEnv {
    job_ctx: JobContext,
    env: MsgEnv,
}

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

/// Parameters for auth re-entry.
struct AuthReentry {
    ext_name: String,
    instructions: String,
    auth_url: Option<String>,
    setup_url: Option<String>,
}

/// Deferred flow parameter object for bundling co-travelling arguments.
#[derive(Clone)]
struct DeferredFlow<'a> {
    scope: &'a TurnScope,
    job_ctx: &'a JobContext,
    pending: &'a PendingApproval,
    context_messages: Vec<ChatMessage>,
    deferred_tool_calls: Vec<crate::llm::ToolCall>,
}

/// Parameters for auth intercept handling.
struct AuthInterceptParams<'a> {
    /// Session containing the thread.
    session: &'a Arc<Mutex<Session>>,
    /// Thread ID for the conversation.
    thread_id: Uuid,
    /// Message environment context.
    env: &'a MsgEnv,
    /// Tool execution result (used to extract auth URLs).
    tool_result: &'a Result<String, Error>,
    /// Extension name requiring authentication.
    ext_name: String,
    /// Instructions to display to the user.
    instructions: String,
    /// Pending approval to preserve for continuation after auth.
    pending: Option<PendingApproval>,
}

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
                    turn.record_tool_result(serde_json::json!(result_content));
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

    /// Preflight deferred tools: collect runnable and find first needing approval.
    async fn preflight_deferred_tools(
        &self,
        session: &Arc<Mutex<Session>>,
        deferred: &[crate::llm::ToolCall],
    ) -> (
        Vec<crate::llm::ToolCall>,
        Option<(usize, crate::llm::ToolCall, Arc<dyn crate::tools::Tool>)>,
    ) {
        // Precompute auto-approved tools to avoid repeated locking
        let auto_approved: std::collections::HashSet<String> = {
            let sess = session.lock().await;
            sess.auto_approved_tools.iter().cloned().collect()
        };

        let mut runnable: Vec<crate::llm::ToolCall> = Vec::new();
        let mut approval_needed: Option<(
            usize,
            crate::llm::ToolCall,
            Arc<dyn crate::tools::Tool>,
        )> = None;

        for (idx, tc) in deferred.iter().enumerate() {
            if let Some(tool) = self.tools().get(&tc.name).await {
                use crate::tools::ApprovalRequirement;
                let needs_approval = match tool.requires_approval(&tc.arguments) {
                    ApprovalRequirement::Never => false,
                    ApprovalRequirement::UnlessAutoApproved => !auto_approved.contains(&tc.name),
                    ApprovalRequirement::Always => true,
                };

                if needs_approval {
                    approval_needed = Some((idx, tc.clone(), tool));
                    break; // remaining tools stay deferred
                }
            }

            runnable.push(tc.clone());
        }

        (runnable, approval_needed)
    }

    /// Run deferred tools inline (single or empty).
    async fn run_deferred_inline(
        &self,
        runnable: &[crate::llm::ToolCall],
        exec: &DeferredEnv,
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        let mut results = Vec::new();
        for tc in runnable {
            let _ = self
                .channels
                .send_status(
                    &exec.env.channel,
                    StatusUpdate::ToolStarted {
                        name: tc.name.clone(),
                    },
                    &exec.env.metadata,
                )
                .await;

            let result = self
                .execute_chat_tool(&tc.name, &tc.arguments, &exec.job_ctx)
                .await;

            let deferred_tool = self.tools().get(&tc.name).await;
            let _ = self
                .channels
                .send_status(
                    &exec.env.channel,
                    StatusUpdate::tool_completed(
                        tc.name.clone(),
                        &result,
                        &tc.arguments,
                        deferred_tool.as_deref(),
                    ),
                    &exec.env.metadata,
                )
                .await;

            results.push((tc.clone(), result));
        }
        results
    }

    /// Collect and reorder parallel results.
    async fn collect_and_reorder_parallel_results(
        &self,
        mut join_set: JoinSet<(usize, crate::llm::ToolCall, Result<String, Error>)>,
        runnable: &[crate::llm::ToolCall],
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        let mut ordered: Vec<Option<(crate::llm::ToolCall, Result<String, Error>)>> =
            (0..runnable.len()).map(|_| None).collect();
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok((idx, tc, result)) => {
                    ordered[idx] = Some((tc, result));
                }
                Err(e) => {
                    if e.is_panic() {
                        tracing::error!("Deferred tool execution task panicked: {}", e);
                    } else {
                        tracing::error!("Deferred tool execution task cancelled: {}", e);
                    }
                }
            }
        }

        // Fill panicked slots with error results
        ordered
            .into_iter()
            .enumerate()
            .map(|(i, opt)| {
                opt.unwrap_or_else(|| {
                    let tc = runnable[i].clone();
                    let err: Error = crate::error::ToolError::ExecutionFailed {
                        name: tc.name.clone(),
                        reason: "Task failed during execution".to_string(),
                    }
                    .into();
                    (tc, Err(err))
                })
            })
            .collect()
    }

    /// Run deferred tools in parallel via JoinSet.
    async fn run_deferred_parallel(
        &self,
        runnable: &[crate::llm::ToolCall],
        exec: &DeferredEnv,
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        let mut join_set = JoinSet::new();

        for (idx, tc) in runnable.iter().cloned().enumerate() {
            let tools = self.tools().clone();
            let safety = self.safety().clone();
            let channels = self.channels.clone();
            let job_ctx = exec.job_ctx.clone();
            let env = exec.env.clone();
            join_set.spawn(async move {
                let _ = channels
                    .send_status(
                        &env.channel,
                        StatusUpdate::ToolStarted {
                            name: tc.name.clone(),
                        },
                        &env.metadata,
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
                        &env.channel,
                        StatusUpdate::tool_completed(
                            tc.name.clone(),
                            &result,
                            &tc.arguments,
                            par_tool.as_deref(),
                        ),
                        &env.metadata,
                    )
                    .await;

                (idx, tc, result)
            });
        }

        self.collect_and_reorder_parallel_results(join_set, runnable)
            .await
    }

    /// Execute runnable deferred tools (inline for ≤1, JoinSet for >1).
    async fn execute_runnable_deferred(
        &self,
        runnable: &[crate::llm::ToolCall],
        exec: &DeferredEnv,
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        if runnable.is_empty() {
            return Vec::new();
        }
        if runnable.len() == 1 {
            return self.run_deferred_inline(runnable, exec).await;
        }
        self.run_deferred_parallel(runnable, exec).await
    }

    /// Postflight: record results, emit ToolResult previews, check for deferred auth.
    async fn postflight_record_and_maybe_deferred_auth(
        &self,
        scope: &TurnScope,
        exec_results: Vec<(crate::llm::ToolCall, Result<String, Error>)>,
        context_messages: &mut Vec<ChatMessage>,
        pending: &PendingApproval,
    ) -> Option<String> {
        let mut deferred_auth: Option<String> = None;

        for (tc, deferred_result) in exec_results {
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
                        turn.record_tool_result(serde_json::json!(deferred_content));
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

    /// Finalize turn and persist response.
    async fn finalize_turn_and_persist_response(
        &self,
        scope: &TurnScope,
        response: &str,
    ) -> Result<(), Error> {
        // Acquire session lock and check for interruption before finalizing turn.
        // This mirrors the pattern in process_user_input to prevent races.
        let (turn_number, tool_calls) = {
            let mut sess = scope.session.lock().await;
            let thread = sess.threads.get_mut(&scope.thread_id).ok_or_else(|| {
                Error::from(crate::error::JobError::NotFound {
                    id: scope.thread_id,
                })
            })?;

            // Check for interrupt before completing turn
            if thread.state == crate::agent::session::ThreadState::Interrupted {
                return Ok(());
            }

            thread.complete_turn(response);
            thread
                .turns
                .last()
                .map(|t| (t.turn_number, t.tool_calls.clone()))
                .unwrap_or_default()
        };

        // User message already persisted at turn start; save tool calls then assistant response
        self.persist_tool_calls(
            scope.thread_id,
            &scope.env.user_id,
            turn_number,
            &tool_calls,
        )
        .await;
        self.persist_assistant_response(scope.thread_id, &scope.env.user_id, response)
            .await;
        let _ = self
            .channels
            .send_status(
                &scope.env.channel,
                StatusUpdate::Status("Done".into()),
                &scope.env.metadata,
            )
            .await;
        Ok(())
    }

    /// Enter awaiting approval state and notify.
    async fn enter_awaiting_approval_and_notify(
        &self,
        scope: &TurnScope,
        new_pending: PendingApproval,
    ) -> Result<SubmissionResult, Error> {
        let request_id = new_pending.request_id;
        let tool_name = new_pending.tool_name.clone();
        let description = new_pending.description.clone();
        let parameters = new_pending.display_parameters.clone();
        {
            let mut sess = scope.session.lock().await;
            let thread = sess.threads.get_mut(&scope.thread_id).ok_or_else(|| {
                Error::from(crate::error::JobError::NotFound {
                    id: scope.thread_id,
                })
            })?;
            thread.await_approval(new_pending);
        }
        let _ = self
            .channels
            .send_status(
                &scope.env.channel,
                StatusUpdate::Status("Awaiting approval".into()),
                &scope.env.metadata,
            )
            .await;
        Ok(SubmissionResult::NeedApproval {
            request_id,
            tool_name,
            description,
            parameters,
        })
    }

    /// Fail turn and return error submission result.
    async fn fail_turn_and_error(
        &self,
        scope: &TurnScope,
        error: String,
    ) -> Result<SubmissionResult, Error> {
        {
            let mut sess = scope.session.lock().await;
            let thread = sess.threads.get_mut(&scope.thread_id).ok_or_else(|| {
                Error::from(crate::error::JobError::NotFound {
                    id: scope.thread_id,
                })
            })?;
            thread.fail_turn(error.clone());
        }
        // User message already persisted at turn start; save the failure response
        self.persist_assistant_response(scope.thread_id, &scope.env.user_id, &error)
            .await;
        Ok(SubmissionResult::error(error))
    }

    /// Continue loop after tool execution.
    async fn continue_loop_after_tool(
        &self,
        scope: TurnScope,
        context_messages: Vec<ChatMessage>,
    ) -> Result<SubmissionResult, Error> {
        let message = scope.to_message();
        let result = self
            .run_agentic_loop(
                &message,
                scope.session.clone(),
                scope.thread_id,
                context_messages,
            )
            .await;

        match result {
            Ok(AgenticLoopResult::Response(response)) => {
                // Hook: TransformResponse — allow hooks to modify or reject the final response
                let response = {
                    let event = crate::hooks::HookEvent::ResponseTransform {
                        user_id: scope.env.user_id.clone(),
                        thread_id: scope.thread_id.to_string(),
                        response: response.clone(),
                    };
                    match self.hooks().run(&event).await {
                        Err(crate::hooks::HookError::Rejected { reason }) => {
                            format!("[Response filtered: {}]", reason)
                        }
                        Ok(crate::hooks::HookOutcome::Reject { reason }) => {
                            format!("[Response filtered: {}]", reason)
                        }
                        Err(err) => {
                            tracing::warn!("TransformResponse hook failed open: {}", err);
                            response
                        }
                        Ok(crate::hooks::HookOutcome::Continue {
                            modified: Some(new_response),
                        }) => new_response,
                        _ => response, // fail-open: use original
                    }
                };

                self.finalize_turn_and_persist_response(&scope, &response)
                    .await?;
                Ok(SubmissionResult::response(response))
            }
            Ok(AgenticLoopResult::NeedApproval { pending }) => {
                self.enter_awaiting_approval_and_notify(&scope, pending)
                    .await
            }
            Err(e) => self.fail_turn_and_error(&scope, e.to_string()).await,
        }
    }

    /// Complete rejection and persist.
    async fn complete_rejection_and_persist(
        &self,
        scope: &TurnScope,
        pending: &PendingApproval,
    ) -> Result<SubmissionResult, Error> {
        // Rejected - complete the turn with a rejection message and persist
        let rejection = format!(
            "Tool '{}' was rejected. The agent will not execute this tool.\n\n\
             You can continue the conversation or try a different approach.",
            pending.tool_name
        );
        {
            let mut sess = scope.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                thread.clear_pending_approval();
                thread.complete_turn(&rejection);
            }
        }
        // User message already persisted at turn start; save rejection response
        self.persist_assistant_response(scope.thread_id, &scope.env.user_id, &rejection)
            .await;

        let _ = self
            .channels
            .send_status(
                &scope.env.channel,
                StatusUpdate::Status("Rejected".into()),
                &scope.env.metadata,
            )
            .await;

        Ok(SubmissionResult::response(rejection))
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

    /// Handle deferred tools flow: preflight, execute, postflight.
    /// Returns the (possibly mutated) context_messages and an optional SubmissionResult.
    async fn handle_deferred_tools_flow<'a>(
        &self,
        mut flow: DeferredFlow<'a>,
    ) -> Result<(Vec<ChatMessage>, Option<SubmissionResult>), Error> {
        // Preflight deferred tools
        let (runnable, approval_needed) = self
            .preflight_deferred_tools(&flow.scope.session, &flow.deferred_tool_calls)
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

    /// Process an approval or rejection of a pending tool execution.
    pub(super) async fn process_approval(
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

    /// Handle an auth-required result from a tool execution.
    ///
    /// Enters auth mode on the thread, stores the pending approval (if provided)
    /// to preserve deferred tool calls and context messages, completes + persists
    /// the turn, and sends the AuthRequired status to the channel.
    async fn handle_auth_intercept(&self, params: AuthInterceptParams<'_>) {
        let auth_data = parse_auth_result(params.tool_result);
        {
            let mut sess = params.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&params.thread_id) {
                // Complete turn first (resets state to Idle)
                thread.complete_turn(&params.instructions);
                // Store pending approval to preserve deferred tool calls and context
                // messages so the tool chain can resume after auth completion.
                if let Some(pending) = params.pending {
                    thread.await_approval(pending);
                }
                // Set pending auth (state unchanged)
                thread.enter_auth_mode(params.ext_name.clone());
            }
        }
        // User message already persisted at turn start; save auth instructions
        self.persist_assistant_response(
            params.thread_id,
            &params.env.user_id,
            &params.instructions,
        )
        .await;
        let _ = self
            .channels
            .send_status(
                &params.env.channel,
                StatusUpdate::AuthRequired {
                    extension_name: params.ext_name,
                    instructions: Some(params.instructions.clone()),
                    auth_url: auth_data.auth_url,
                    setup_url: auth_data.setup_url,
                },
                &params.env.metadata,
            )
            .await;
    }

    /// Activate extension after successful auth and notify.
    async fn activate_extension_and_notify(&self, env: &MsgEnv, ext_name: &str) -> Option<String> {
        let ext_mgr = match self.deps.extension_manager.as_ref() {
            Some(mgr) => mgr,
            None => {
                return Some(format!(
                    "{} authenticated, but extension manager is unavailable.",
                    ext_name
                ));
            }
        };

        match ext_mgr.activate(ext_name).await {
            Ok(activate_result) => {
                let tool_count = activate_result.tools_loaded.len();
                let tool_list = if activate_result.tools_loaded.is_empty() {
                    String::new()
                } else {
                    format!("\n\nTools: {}", activate_result.tools_loaded.join(", "))
                };
                let msg = format!(
                    "{} authenticated and activated ({} tools loaded).{}",
                    ext_name, tool_count, tool_list
                );
                let _ = self
                    .channels
                    .send_status(
                        &env.channel,
                        StatusUpdate::AuthCompleted {
                            extension_name: ext_name.to_string(),
                            success: true,
                            message: msg.clone(),
                        },
                        &env.metadata,
                    )
                    .await;
                Some(msg)
            }
            Err(e) => {
                tracing::warn!(
                    "Extension '{}' authenticated but activation failed: {}",
                    ext_name,
                    e
                );
                let msg = format!(
                    "{} authenticated successfully, but activation failed: {}. \
                     Try activating manually.",
                    ext_name, e
                );
                let _ = self
                    .channels
                    .send_status(
                        &env.channel,
                        StatusUpdate::AuthCompleted {
                            extension_name: ext_name.to_string(),
                            success: false,
                            message: msg.clone(),
                        },
                        &env.metadata,
                    )
                    .await;
                Some(msg)
            }
        }
    }

    /// Re-enter auth mode and notify.
    async fn reenter_auth_mode_and_notify(
        &self,
        scope: &TurnScope,
        reentry: AuthReentry,
    ) -> Option<String> {
        {
            let mut sess = scope.session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                thread.enter_auth_mode(reentry.ext_name.clone());
            }
        }
        let _ = self
            .channels
            .send_status(
                &scope.env.channel,
                StatusUpdate::AuthRequired {
                    extension_name: reentry.ext_name.clone(),
                    instructions: Some(reentry.instructions.clone()),
                    auth_url: reentry.auth_url,
                    setup_url: reentry.setup_url,
                },
                &scope.env.metadata,
            )
            .await;
        Some(reentry.instructions)
    }

    /// Handle an auth token submitted while the thread is in auth mode.
    ///
    /// The token goes directly to the extension manager's credential store,
    /// completely bypassing logging, turn creation, history, and compaction.
    pub(super) async fn process_auth_token(
        &self,
        scope: TurnScope,
        pending: &crate::agent::session::PendingAuth,
        token: &str,
    ) -> Result<Option<String>, Error> {
        let token = token.trim();

        let ext_mgr = match self.deps.extension_manager.as_ref() {
            Some(mgr) => mgr,
            None => return Ok(Some("Extension manager not available.".to_string())),
        };

        match ext_mgr.auth(&pending.extension_name, Some(token)).await {
            Ok(result) if result.is_authenticated() => {
                {
                    let mut sess = scope.session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                        thread.pending_auth = None;
                    }
                }
                tracing::info!(
                    "Extension '{}' authenticated via auth mode",
                    pending.extension_name
                );

                // Auto-activate so tools are available immediately after auth
                Ok(self
                    .activate_extension_and_notify(&scope.env, &pending.extension_name)
                    .await)
            }
            Ok(result) => {
                // Invalid token, re-enter auth mode
                let instructions = result
                    .instructions()
                    .map(String::from)
                    .unwrap_or_else(|| "Invalid token. Please try again.".to_string());
                let auth_url = result.auth_url().map(String::from);
                let setup_url = result.setup_url().map(String::from);
                let reentry = AuthReentry {
                    ext_name: pending.extension_name.clone(),
                    instructions,
                    auth_url,
                    setup_url,
                };
                let _ = self.reenter_auth_mode_and_notify(&scope, reentry).await;
                Ok(None)
            }
            Err(e) => {
                let msg = format!(
                    "Authentication failed for {}: {}",
                    pending.extension_name, e
                );
                // Restore pending_auth so the next user message is still intercepted
                {
                    let mut sess = scope.session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&scope.thread_id) {
                        thread.pending_auth = Some(pending.clone());
                    }
                }
                // Re-enter auth mode to allow retry
                let reentry = AuthReentry {
                    ext_name: pending.extension_name.clone(),
                    instructions: format!("{} Please try again.", msg),
                    auth_url: None,
                    setup_url: None,
                };
                let _ = self.reenter_auth_mode_and_notify(&scope, reentry).await;
                Ok(None)
            }
        }
    }
}
