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

mod compaction;
mod validation;

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::dispatcher::AgenticLoopResult;
use crate::agent::session::{Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::agent::thread_ops::TurnPersistContext;
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

use compaction::{checkpoint_before_turn, maybe_compact_context};
use validation::{check_thread_state, validate_safety};

/// Request parameters for processing a user turn.
///
/// Groups the session, thread ID, and content to reduce the argument count
/// of `process_user_input` (addresses CodeScene "Excess Number of Function Arguments").
#[derive(Clone)]
pub(crate) struct UserTurnRequest {
    pub session: Arc<Mutex<Session>>,
    pub thread_id: Uuid,
    pub content: String,
}

impl Agent {
    /// Prepare turn by augmenting content and starting the turn.
    async fn prepare_turn(
        &self,
        message: &IncomingMessage,
        req: &UserTurnRequest,
    ) -> Result<Vec<crate::llm::ChatMessage>, Error> {
        let content = req.content.as_str();
        let augmented =
            crate::agent::attachments::augment_with_attachments(content, &message.attachments);
        let (effective_content, image_parts) = match &augmented {
            Some(result) => (result.text.as_str(), result.image_parts.clone()),
            None => (content, Vec::new()),
        };

        let turn_messages = {
            let mut sess = req.session.lock().await;
            let thread = sess.threads.get_mut(&req.thread_id).ok_or_else(|| {
                Error::from(crate::error::JobError::NotFound { id: req.thread_id })
            })?;
            let turn = thread.start_turn(effective_content);
            turn.image_content_parts = image_parts;
            thread.messages()
        };

        tracing::debug!(
            message_id = %message.id,
            thread_id = %req.thread_id,
            "Persisting user message to DB"
        );
        self.persist_user_message(req.thread_id, &message.user_id, effective_content)
            .await;

        tracing::debug!(
            message_id = %message.id,
            thread_id = %req.thread_id,
            "User message persisted, starting agentic loop"
        );

        Ok(turn_messages)
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

                let persist_ctx = TurnPersistContext {
                    thread_id,
                    user_id: &message.user_id,
                    turn_number,
                };
                self.persist_tool_calls(&persist_ctx, &tool_calls).await;
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
                let error_text = e.to_string();
                drop(sess);
                self.persist_assistant_response(thread_id, &message.user_id, &error_text)
                    .await;

                let mut sess = session.lock().await;
                let thread = sess.threads.get_mut(&thread_id).ok_or_else(|| {
                    Error::from(crate::error::JobError::NotFound { id: thread_id })
                })?;
                thread.fail_turn(error_text.clone());
                Ok(SubmissionResult::error(error_text))
            }
        }
    }

    pub(super) async fn process_user_input(
        &self,
        message: &IncomingMessage,
        req: UserTurnRequest,
    ) -> Result<SubmissionResult, Error> {
        tracing::debug!(
            message_id = %message.id,
            thread_id = %req.thread_id,
            content_len = req.content.len(),
            "Processing user input"
        );

        // Phase 1: Check thread state
        if let Some(result) = check_thread_state(message, &req.session, req.thread_id).await? {
            return Ok(result);
        }

        // Phase 2: Safety validation
        if let Some(result) = validate_safety(self, message, &req.content) {
            return Ok(result);
        }

        // Phase 3: Route explicit commands
        let temp_message = IncomingMessage {
            content: req.content.to_string(),
            ..message.clone()
        };
        if let Some(intent) = self.router.route_command(&temp_message) {
            return self.handle_job_or_command(intent, message).await;
        }

        // Phase 4: Auto-compact context if needed
        maybe_compact_context(self, message, &req.session, req.thread_id).await?;

        // Phase 5: Create checkpoint
        checkpoint_before_turn(self, &req.session, req.thread_id).await?;

        // Phase 6: Prepare turn
        let turn_messages = self.prepare_turn(message, &req).await?;

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
            .run_agentic_loop(message, req.session.clone(), req.thread_id, turn_messages)
            .await;

        // Phase 8: Handle loop result
        self.handle_loop_result(message, &req.session, req.thread_id, result)
            .await
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_compiles() {
        // TODO: Add integration-level coverage for turn orchestration using a
        // dependency-injected Agent fixture and higher-level message flow tests.
    }
}
