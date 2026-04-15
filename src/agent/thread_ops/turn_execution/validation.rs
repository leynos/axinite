//! Validation helpers for user turn execution.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::{Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::channels::IncomingMessage;
use crate::error::Error;

/// Check thread state and return error if not in a processable state.
pub(super) async fn check_thread_state(
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
    agent: &Agent,
    message: &IncomingMessage,
    content: &str,
) -> Option<SubmissionResult> {
    let validation = agent.safety().validate_input(content);
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

    let violations = agent.safety().check_policy(content);
    if violations
        .iter()
        .any(|rule| rule.action == crate::safety::PolicyAction::Block)
    {
        return Some(SubmissionResult::error("Input rejected by safety policy."));
    }

    if let Some(warning) = agent.safety().scan_inbound_for_secrets(content) {
        tracing::warn!(
            message_id = %message.id,
            "Inbound message blocked: contains leaked secret"
        );
        return Some(SubmissionResult::error(warning));
    }

    None
}
