//! Approval and auth-intercept flows for thread operations.

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
        }
        Ok(pending)
    }

    /// Restage pending approval if request ID doesn't match.
    async fn restage_on_request_id_mismatch(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        provided: Option<Uuid>,
        pending: &PendingApproval,
    ) -> Result<Option<SubmissionResult>, Error> {
        if let Some(req_id) = provided
            && req_id != pending.request_id
        {
            // Put it back and return error
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
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

    /// Set thread state to Processing.
    async fn set_thread_processing(&self, session: &Arc<Mutex<Session>>, thread_id: Uuid) {
        let mut sess = session.lock().await;
        if let Some(thread) = sess.threads.get_mut(&thread_id) {
            thread.state = ThreadState::Processing;
        }
    }

    /// Build JobContext for approval execution.
    fn build_job_context_for_approval(
        &self,
        message: &IncomingMessage,
        pending: &PendingApproval,
    ) -> JobContext {
        let mut job_ctx =
            JobContext::with_user(&message.user_id, "chat", "Interactive chat session");
        job_ctx.http_interceptor = self.deps.http_interceptor.clone();
        // Prefer a valid timezone from the approval message, fall back to the
        // resolved timezone stored when the approval was originally requested.
        let tz_candidate = message
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
        message: &IncomingMessage,
        pending: &PendingApproval,
        job_ctx: &JobContext,
    ) -> (Result<String, Error>, Option<Arc<dyn crate::tools::Tool>>) {
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::ToolStarted {
                    name: pending.tool_name.clone(),
                },
                &message.metadata,
            )
            .await;

        let tool_result = self
            .execute_chat_tool(&pending.tool_name, &pending.parameters, job_ctx)
            .await;

        let tool_ref = self.tools().get(&pending.tool_name).await;
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::tool_completed(
                    pending.tool_name.clone(),
                    &tool_result,
                    &pending.display_parameters,
                    tool_ref.as_deref(),
                ),
                &message.metadata,
            )
            .await;

        if let Ok(ref output) = tool_result
            && !output.is_empty()
        {
            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::ToolResult {
                        name: pending.tool_name.clone(),
                        preview: output.clone(),
                    },
                    &message.metadata,
                )
                .await;
        }

        (tool_result, tool_ref)
    }

    /// Record sanitized primary tool result and return content with error flag.
    async fn record_sanitised_primary_result(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
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
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id)
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
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &IncomingMessage,
        pending: &PendingApproval,
        tool_result: &Result<String, Error>,
    ) -> Option<SubmissionResult> {
        if let Some((ext_name, instructions)) = check_auth_required(&pending.tool_name, tool_result)
        {
            self.handle_auth_intercept(
                session,
                thread_id,
                message,
                tool_result,
                ext_name,
                instructions.clone(),
            )
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
                    ApprovalRequirement::UnlessAutoApproved => {
                        let sess = session.lock().await;
                        !sess.is_tool_auto_approved(&tc.name)
                    }
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

    /// Execute runnable deferred tools (inline for ≤1, JoinSet for >1).
    async fn execute_runnable_deferred(
        &self,
        runnable: &[crate::llm::ToolCall],
        job_ctx: &JobContext,
        channel: &str,
        metadata: &serde_json::Value,
    ) -> Vec<(crate::llm::ToolCall, Result<String, Error>)> {
        if runnable.len() <= 1 {
            // Single tool (or none): execute inline
            let mut results = Vec::new();
            for tc in runnable {
                let _ = self
                    .channels
                    .send_status(
                        channel,
                        StatusUpdate::ToolStarted {
                            name: tc.name.clone(),
                        },
                        metadata,
                    )
                    .await;

                let result = self
                    .execute_chat_tool(&tc.name, &tc.arguments, job_ctx)
                    .await;

                let deferred_tool = self.tools().get(&tc.name).await;
                let _ = self
                    .channels
                    .send_status(
                        channel,
                        StatusUpdate::tool_completed(
                            tc.name.clone(),
                            &result,
                            &tc.arguments,
                            deferred_tool.as_deref(),
                        ),
                        metadata,
                    )
                    .await;

                results.push((tc.clone(), result));
            }
            results
        } else {
            // Multiple tools: execute in parallel via JoinSet
            let mut join_set = JoinSet::new();
            let runnable_count = runnable.len();

            for (spawn_idx, tc) in runnable.iter().enumerate() {
                let tools = self.tools().clone();
                let safety = self.safety().clone();
                let channels = self.channels.clone();
                let job_ctx = job_ctx.clone();
                let tc = tc.clone();
                let channel = channel.to_string();
                let metadata = metadata.clone();

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
                        &tc.name,
                        &tc.arguments,
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

                    (spawn_idx, tc, result)
                });
            }

            // Collect and reorder by original index
            let mut ordered: Vec<Option<(crate::llm::ToolCall, Result<String, Error>)>> =
                (0..runnable_count).map(|_| None).collect();
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
    }

    /// Postflight: record results, emit ToolResult previews, check for deferred auth.
    async fn postflight_record_and_maybe_deferred_auth(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &IncomingMessage,
        exec_results: Vec<(crate::llm::ToolCall, Result<String, Error>)>,
        context_messages: &mut Vec<ChatMessage>,
    ) -> Option<String> {
        let mut deferred_auth: Option<String> = None;

        for (tc, deferred_result) in exec_results {
            if let Ok(ref output) = deferred_result
                && !output.is_empty()
            {
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::ToolResult {
                            name: tc.name.clone(),
                            preview: output.clone(),
                        },
                        &message.metadata,
                    )
                    .await;
            }

            // Sanitize first, then record the cleaned version in thread.
            // Must happen before auth detection which may set deferred_auth.
            let is_deferred_error = deferred_result.is_err();
            let (deferred_content, _) = crate::tools::execute::process_tool_result(
                self.safety(),
                &tc.name,
                &tc.id,
                &deferred_result,
            );

            // Record sanitized result in thread
            {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id)
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
                self.handle_auth_intercept(
                    session,
                    thread_id,
                    message,
                    &deferred_result,
                    ext_name,
                    instructions.clone(),
                )
                .await;
                deferred_auth = Some(instructions);
            }

            context_messages.push(ChatMessage::tool_result(&tc.id, &tc.name, deferred_content));
        }

        deferred_auth
    }

    /// Enter deferred approval mode and notify.
    #[allow(clippy::too_many_arguments)]
    async fn enter_deferred_approval_and_notify(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &IncomingMessage,
        approval_idx: usize,
        tc: crate::llm::ToolCall,
        tool: Arc<dyn crate::tools::Tool>,
        deferred_tool_calls: &[crate::llm::ToolCall],
        context_messages: &[ChatMessage],
        pending: &PendingApproval,
    ) -> SubmissionResult {
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
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
                thread.await_approval(new_pending);
            }
        }

        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::Status("Awaiting approval".into()),
                &message.metadata,
            )
            .await;

        SubmissionResult::NeedApproval {
            request_id,
            tool_name,
            description,
            parameters,
        }
    }

    /// Continue loop after tool execution.
    async fn continue_loop_after_tool(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        context_messages: Vec<ChatMessage>,
    ) -> Result<SubmissionResult, Error> {
        let result = self
            .run_agentic_loop(message, session.clone(), thread_id, context_messages)
            .await;

        match result {
            Ok(AgenticLoopResult::Response(response)) => {
                let (turn_number, tool_calls) = {
                    let mut sess = session.lock().await;
                    let thread = sess.threads.get_mut(&thread_id).ok_or_else(|| {
                        Error::from(crate::error::JobError::NotFound { id: thread_id })
                    })?;
                    thread.complete_turn(&response);
                    thread
                        .turns
                        .last()
                        .map(|t| (t.turn_number, t.tool_calls.clone()))
                        .unwrap_or_default()
                };
                // User message already persisted at turn start; save tool calls then assistant response
                self.persist_tool_calls(thread_id, &message.user_id, turn_number, &tool_calls)
                    .await;
                self.persist_assistant_response(thread_id, &message.user_id, &response)
                    .await;
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status("Done".into()),
                        &message.metadata,
                    )
                    .await;
                Ok(SubmissionResult::response(response))
            }
            Ok(AgenticLoopResult::NeedApproval {
                pending: new_pending,
            }) => {
                let request_id = new_pending.request_id;
                let tool_name = new_pending.tool_name.clone();
                let description = new_pending.description.clone();
                let parameters = new_pending.display_parameters.clone();
                {
                    let mut sess = session.lock().await;
                    let thread = sess.threads.get_mut(&thread_id).ok_or_else(|| {
                        Error::from(crate::error::JobError::NotFound { id: thread_id })
                    })?;
                    thread.await_approval(new_pending);
                }
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status("Awaiting approval".into()),
                        &message.metadata,
                    )
                    .await;
                Ok(SubmissionResult::NeedApproval {
                    request_id,
                    tool_name,
                    description,
                    parameters,
                })
            }
            Err(e) => {
                let mut sess = session.lock().await;
                let thread = sess.threads.get_mut(&thread_id).ok_or_else(|| {
                    Error::from(crate::error::JobError::NotFound { id: thread_id })
                })?;
                thread.fail_turn(e.to_string());
                // User message already persisted at turn start
                Ok(SubmissionResult::error(e.to_string()))
            }
        }
    }

    /// Complete rejection and persist.
    async fn complete_rejection_and_persist(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &IncomingMessage,
        pending: &PendingApproval,
    ) -> Result<SubmissionResult, Error> {
        // Rejected - complete the turn with a rejection message and persist
        let rejection = format!(
            "Tool '{}' was rejected. The agent will not execute this tool.\n\n\
             You can continue the conversation or try a different approach.",
            pending.tool_name
        );
        {
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
                thread.clear_pending_approval();
                thread.complete_turn(&rejection);
            }
        }
        // User message already persisted at turn start; save rejection response
        self.persist_assistant_response(thread_id, &message.user_id, &rejection)
            .await;

        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::Status("Rejected".into()),
                &message.metadata,
            )
            .await;

        Ok(SubmissionResult::response(rejection))
    }

    /// Process an approval or rejection of a pending tool execution.
    pub(super) async fn process_approval(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        request_id: Option<Uuid>,
        approved: bool,
        always: bool,
    ) -> Result<SubmissionResult, Error> {
        // a) Get pending approval
        let pending = match self
            .take_pending_approval_if_awaiting(&session, thread_id)
            .await?
        {
            Some(p) => p,
            None => return Ok(SubmissionResult::ok_with_message("")),
        };

        // b) Check request ID mismatch
        if let Some(res) = self
            .restage_on_request_id_mismatch(&session, thread_id, request_id, &pending)
            .await?
        {
            return Ok(res);
        }

        // c) Handle rejection
        if !approved {
            return self
                .complete_rejection_and_persist(&session, thread_id, message, &pending)
                .await;
        }

        // d) Auto-approve and set processing state
        self.auto_approve_if_always(&session, always, &pending.tool_name)
            .await;
        self.set_thread_processing(&session, thread_id).await;

        // e) Build context and execute primary tool
        let job_ctx = self.build_job_context_for_approval(message, &pending);
        let (tool_result, _) = self
            .execute_primary_tool_and_notify(message, &pending, &job_ctx)
            .await;

        // f) Record result and check for auth intercept
        let (result_content, _) = self
            .record_sanitised_primary_result(&session, thread_id, &pending, &tool_result)
            .await;
        if let Some(res) = self
            .maybe_auth_intercept_after_primary(
                &session,
                thread_id,
                message,
                &pending,
                &tool_result,
            )
            .await
        {
            return Ok(res);
        }

        // g) Build context messages and process deferred tools
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
                    &message.channel,
                    StatusUpdate::Thinking(format!(
                        "Executing {} deferred tool(s)...",
                        deferred_tool_calls.len()
                    )),
                    &message.metadata,
                )
                .await;
        }

        // Preflight deferred tools
        let (runnable, approval_needed) = self
            .preflight_deferred_tools(&session, &deferred_tool_calls)
            .await;

        // Execute runnable deferred tools
        let exec_results = self
            .execute_runnable_deferred(&runnable, &job_ctx, &message.channel, &message.metadata)
            .await;

        // Postflight: record results and check for auth
        if let Some(instructions) = self
            .postflight_record_and_maybe_deferred_auth(
                &session,
                thread_id,
                message,
                exec_results,
                &mut context_messages,
            )
            .await
        {
            return Ok(SubmissionResult::response(instructions));
        }

        // Handle deferred approval needed
        if let Some((idx, tc, tool)) = approval_needed {
            return Ok(self
                .enter_deferred_approval_and_notify(
                    &session,
                    thread_id,
                    message,
                    idx,
                    tc,
                    tool,
                    &deferred_tool_calls,
                    &context_messages,
                    &pending,
                )
                .await);
        }

        // h) Continue agentic loop
        self.continue_loop_after_tool(message, session, thread_id, context_messages)
            .await
    }

    /// Handle an auth-required result from a tool execution.
    ///
    /// Enters auth mode on the thread, completes + persists the turn,
    /// and sends the AuthRequired status to the channel.
    /// Returns the instructions string for the caller to wrap in a response.
    async fn handle_auth_intercept(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        message: &IncomingMessage,
        tool_result: &Result<String, Error>,
        ext_name: String,
        instructions: String,
    ) {
        let auth_data = parse_auth_result(tool_result);
        {
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
                thread.enter_auth_mode(ext_name.clone());
                thread.complete_turn(&instructions);
            }
        }
        // User message already persisted at turn start; save auth instructions
        self.persist_assistant_response(thread_id, &message.user_id, &instructions)
            .await;
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::AuthRequired {
                    extension_name: ext_name,
                    instructions: Some(instructions.clone()),
                    auth_url: auth_data.auth_url,
                    setup_url: auth_data.setup_url,
                },
                &message.metadata,
            )
            .await;
    }

    /// Activate extension after successful auth and notify.
    async fn activate_extension_and_notify(
        &self,
        message: &IncomingMessage,
        ext_name: &str,
    ) -> Option<String> {
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
                        &message.channel,
                        StatusUpdate::AuthCompleted {
                            extension_name: ext_name.to_string(),
                            success: true,
                            message: msg.clone(),
                        },
                        &message.metadata,
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
                        &message.channel,
                        StatusUpdate::AuthCompleted {
                            extension_name: ext_name.to_string(),
                            success: true,
                            message: msg.clone(),
                        },
                        &message.metadata,
                    )
                    .await;
                Some(msg)
            }
        }
    }

    /// Re-enter auth mode and notify.
    #[allow(clippy::too_many_arguments)]
    async fn reenter_auth_mode_and_notify(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        ext_name: &str,
        instructions: String,
        auth_url: Option<String>,
        setup_url: Option<String>,
    ) -> Option<String> {
        {
            let mut sess = session.lock().await;
            if let Some(thread) = sess.threads.get_mut(&thread_id) {
                thread.enter_auth_mode(ext_name.to_string());
            }
        }
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::AuthRequired {
                    extension_name: ext_name.to_string(),
                    instructions: Some(instructions.clone()),
                    auth_url,
                    setup_url,
                },
                &message.metadata,
            )
            .await;
        Some(instructions)
    }

    /// Handle an auth token submitted while the thread is in auth mode.
    ///
    /// The token goes directly to the extension manager's credential store,
    /// completely bypassing logging, turn creation, history, and compaction.
    pub(super) async fn process_auth_token(
        &self,
        message: &IncomingMessage,
        pending: &crate::agent::session::PendingAuth,
        token: &str,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<Option<String>, Error> {
        let token = token.trim();

        let ext_mgr = match self.deps.extension_manager.as_ref() {
            Some(mgr) => mgr,
            None => return Ok(Some("Extension manager not available.".to_string())),
        };

        match ext_mgr.auth(&pending.extension_name, Some(token)).await {
            Ok(result) if result.is_authenticated() => {
                {
                    let mut sess = session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&thread_id) {
                        thread.pending_auth = None;
                    }
                }
                tracing::info!(
                    "Extension '{}' authenticated via auth mode",
                    pending.extension_name
                );

                // Auto-activate so tools are available immediately after auth
                Ok(self
                    .activate_extension_and_notify(message, &pending.extension_name)
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
                Ok(self
                    .reenter_auth_mode_and_notify(
                        message,
                        session,
                        thread_id,
                        &pending.extension_name,
                        instructions,
                        auth_url,
                        setup_url,
                    )
                    .await)
            }
            Err(e) => {
                let msg = format!(
                    "Authentication failed for {}: {}",
                    pending.extension_name, e
                );
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::AuthCompleted {
                            extension_name: pending.extension_name.clone(),
                            success: false,
                            message: msg.clone(),
                        },
                        &message.metadata,
                    )
                    .await;
                Ok(Some(msg))
            }
        }
    }
}
