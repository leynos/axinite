//! Turn preparation helpers for interactive user input.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::{Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::channels::IncomingMessage;
use crate::error::Error;

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
    /// Check thread state and return error if not in a processable state.
    pub(super) async fn check_thread_state(
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
    pub(super) fn validate_safety(
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

    /// Prepare turn by augmenting content and starting the turn.
    pub(super) async fn prepare_turn(
        &self,
        message: &IncomingMessage,
        req: &UserTurnRequest,
    ) -> Result<(Vec<crate::llm::ChatMessage>, String), Error> {
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

        Ok((turn_messages, effective_content.to_string()))
    }
}
