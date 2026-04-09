//! User turn execution and agentic loop orchestration.
//!
//! Handles the full lifecycle of a user input turn:
//! - Thread state validation
//! - Safety checks (input validation, policy, secrets)
//! - Command routing
//! - Auto-compaction
//! - Undo checkpointing
//! - Attachment augmentation
//! - Agentic loop execution
//! - Response persistence

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::compaction::ContextCompactor;
use crate::agent::dispatcher::AgenticLoopResult;
use crate::agent::session::{Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

impl Agent {
    /// Check thread state and return error if not in a processable state.
    async fn check_thread_state(
        &self,
        message: &IncomingMessage,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<Option<SubmissionResult>, Error> {
        let thread_state = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.state
        };

        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            thread_state = ?thread_state,
            "Checked thread state"
        );

        match thread_state {
            ThreadState::Processing => {
                tracing::warn!(
                    message_id = %message.id,
                    thread_id = %thread_id,
                    "Thread is processing, rejecting new input"
                );
                Ok(Some(SubmissionResult::error(
                    "Turn in progress. Use /interrupt to cancel.",
                )))
            }
            ThreadState::AwaitingApproval => {
                tracing::warn!(
                    message_id = %message.id,
                    thread_id = %thread_id,
                    "Thread awaiting approval, rejecting new input"
                );
                Ok(Some(SubmissionResult::error(
                    "Waiting for approval. Use /interrupt to cancel.",
                )))
            }
            ThreadState::Completed => {
                tracing::warn!(
                    message_id = %message.id,
                    thread_id = %thread_id,
                    "Thread completed, rejecting new input"
                );
                Ok(Some(SubmissionResult::error(
                    "Thread completed. Use /thread new.",
                )))
            }
            ThreadState::Idle | ThreadState::Interrupted => Ok(None),
        }
    }

    /// Validate safety for user input.
    fn validate_safety(
        &self,
        message: &IncomingMessage,
        content: &str,
    ) -> Option<SubmissionResult> {
        let validation = self.safety().validate_input(content);
        if !validation.is_valid {
            let details = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Some(SubmissionResult::error(format!(
                "Input rejected by safety validation: {}",
                details
            )));
        }

        let violations = self.safety().check_policy(content);
        if violations
            .iter()
            .any(|rule| rule.action == crate::safety::PolicyAction::Block)
        {
            return Some(SubmissionResult::error("Input rejected by safety policy."));
        }

        // Scan inbound messages for secrets (API keys, tokens).
        if let Some(warning) = self.safety().scan_inbound_for_secrets(content) {
            tracing::warn!(
                user = %message.user_id,
                channel = %message.channel,
                "Inbound message blocked: contains leaked secret"
            );
            return Some(SubmissionResult::error(warning));
        }

        None
    }

    /// Auto-compact context if needed before adding new turn.
    async fn maybe_compact_context(
        &self,
        message: &IncomingMessage,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<(), Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        let messages = thread.messages();
        if let Some(strategy) = self.context_monitor.suggest_compaction(&messages) {
            let pct = self.context_monitor.usage_percent(&messages);
            tracing::info!("Context at {:.1}% capacity, auto-compacting", pct);

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Status(format!("Context at {:.0}% capacity, compacting...", pct)),
                    &message.metadata,
                )
                .await;

            let compactor = ContextCompactor::new(self.llm().clone());
            if let Err(e) = compactor
                .compact(thread, strategy, self.workspace().map(|w| w.as_ref()))
                .await
            {
                tracing::warn!("Auto-compaction failed: {}", e);
            }
        }
        Ok(())
    }

    /// Create checkpoint before turn.
    async fn checkpoint_before_turn(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<(), Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let sess = session.lock().await;
        let thread = sess
            .threads
            .get(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        let mut mgr = undo_mgr.lock().await;
        mgr.checkpoint(
            thread.turn_number(),
            thread.messages(),
            format!("Before turn {}", thread.turn_number()),
        );
        Ok(())
    }

    /// Prepare turn by augmenting content and starting the turn.
    async fn prepare_turn(
        &self,
        message: &IncomingMessage,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        content: &str,
    ) -> Result<(Vec<crate::llm::ChatMessage>, String), Error> {
        let augmented =
            crate::agent::attachments::augment_with_attachments(content, &message.attachments);
        let (effective_content, image_parts) = match &augmented {
            Some(result) => (result.text.as_str(), result.image_parts.clone()),
            None => (content, Vec::new()),
        };

        let turn_messages = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            let turn = thread.start_turn(effective_content);
            turn.image_content_parts = image_parts;
            thread.messages()
        };

        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            "Persisting user message to DB"
        );
        self.persist_user_message(thread_id, &message.user_id, effective_content)
            .await;

        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            "User message persisted, starting agentic loop"
        );

        Ok((turn_messages, effective_content.to_string()))
    }

    /// Apply response transform hook.
    async fn apply_response_transform_hook(
        &self,
        message: &IncomingMessage,
        thread_id: Uuid,
        response: String,
    ) -> String {
        let event = crate::hooks::HookEvent::ResponseTransform {
            user_id: message.user_id.clone(),
            thread_id: thread_id.to_string(),
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
            _ => response,
        }
    }

    /// Handle the result from the agentic loop.
    async fn handle_loop_result(
        &self,
        message: &IncomingMessage,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        result: Result<AgenticLoopResult, Error>,
    ) -> Result<SubmissionResult, Error> {
        // Check for interruption first
        let interrupted = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.state == ThreadState::Interrupted
        };

        if interrupted {
            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Status("Interrupted".into()),
                    &message.metadata,
                )
                .await;
            return Ok(SubmissionResult::Interrupted);
        }

        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        match result {
            Ok(AgenticLoopResult::Response(response)) => {
                drop(sess);
                let response = self
                    .apply_response_transform_hook(message, thread_id, response)
                    .await;

                let completion = {
                    let mut sess = session.lock().await;
                    let thread = sess.threads.get_mut(&thread_id).ok_or_else(|| {
                        Error::from(crate::error::JobError::NotFound { id: thread_id })
                    })?;
                    if thread.state == ThreadState::Interrupted {
                        None
                    } else {
                        thread.complete_turn(&response);
                        Some(
                            thread
                                .turns
                                .last()
                                .map(|t| (t.turn_number, t.tool_calls.clone()))
                                .unwrap_or_default(),
                        )
                    }
                };

                let Some((turn_number, tool_calls)) = completion else {
                    let _ = self
                        .channels
                        .send_status(
                            &message.channel,
                            StatusUpdate::Status("Interrupted".into()),
                            &message.metadata,
                        )
                        .await;
                    return Ok(SubmissionResult::Interrupted);
                };

                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status("Done".into()),
                        &message.metadata,
                    )
                    .await;

                self.persist_tool_calls(thread_id, &message.user_id, turn_number, &tool_calls)
                    .await;
                self.persist_assistant_response(thread_id, &message.user_id, &response)
                    .await;

                Ok(SubmissionResult::response(response))
            }
            Ok(AgenticLoopResult::NeedApproval { pending }) => {
                let request_id = pending.request_id;
                let tool_name = pending.tool_name.clone();
                let description = pending.description.clone();
                let parameters = pending.display_parameters.clone();
                thread.await_approval(pending);
                drop(sess);

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
                thread.fail_turn(e.to_string());
                Ok(SubmissionResult::error(e.to_string()))
            }
        }
    }

    pub(super) async fn process_user_input(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        content: &str,
    ) -> Result<SubmissionResult, Error> {
        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            content_len = content.len(),
            "Processing user input"
        );

        // Phase 1: Check thread state
        if let Some(result) = self
            .check_thread_state(message, &session, thread_id)
            .await?
        {
            return Ok(result);
        }

        // Phase 2: Safety validation
        if let Some(result) = self.validate_safety(message, content) {
            return Ok(result);
        }

        // Phase 3: Route explicit commands
        let temp_message = IncomingMessage {
            content: content.to_string(),
            ..message.clone()
        };
        if let Some(intent) = self.router.route_command(&temp_message) {
            return self.handle_job_or_command(intent, message).await;
        }

        // Phase 4: Auto-compact context if needed
        self.maybe_compact_context(message, &session, thread_id)
            .await?;

        // Phase 5: Create checkpoint
        self.checkpoint_before_turn(&session, thread_id).await?;

        // Phase 6: Prepare turn
        let (turn_messages, _effective_content) = self
            .prepare_turn(message, &session, thread_id, content)
            .await?;

        // Phase 7: Send thinking status and run agentic loop
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::Thinking("Processing...".into()),
                &message.metadata,
            )
            .await;

        let result = self
            .run_agentic_loop(message, session.clone(), thread_id, turn_messages)
            .await;

        // Phase 8: Handle loop result
        self.handle_loop_result(message, &session, thread_id, result)
            .await
    }
}
