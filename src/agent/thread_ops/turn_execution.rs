//! User turn execution and agentic loop orchestration.
//!
//! Keeps the top-level phase ordering in one place while sibling modules own
//! turn preparation, context compaction/checkpointing, and result
//! finalisation.

use crate::agent::Agent;
use crate::agent::submission::SubmissionResult;
use crate::agent::thread_ops::{PrepareTurnResult, UserTurnRequest};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

impl Agent {
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
        if let Some(result) = self
            .check_thread_state(message, &req.session, req.thread_id)
            .await?
        {
            return Ok(result);
        }

        // Phase 2: Safety validation
        if let Some(result) = self.validate_safety(message, &req.content) {
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
        self.maybe_compact_context(message, &req.session, req.thread_id)
            .await?;

        // Phase 5: Create checkpoint
        self.checkpoint_before_turn(&req.session, req.thread_id)
            .await?;

        // Phase 6: Prepare turn
        let turn_messages = match self.prepare_turn(message, &req).await? {
            PrepareTurnResult::Prepared { turn_messages } => turn_messages,
            PrepareTurnResult::Rejected(result) => return Ok(result),
        };

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
            .run_agentic_loop(
                message,
                crate::agent::dispatcher::RunLoopCtx {
                    session: req.session.clone(),
                    thread_id: req.thread_id,
                    initial_messages: turn_messages,
                },
            )
            .await;

        // Phase 8: Handle loop result
        self.handle_loop_result(message, &req.session, req.thread_id, result)
            .await
    }

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
                message_id = %message.id,
                "Inbound message blocked: contains leaked secret"
            );
            return Some(SubmissionResult::error(warning));
        }

        None
    }

    /// Auto-compact context if needed before adding new turn.
}

mod tests {
    #[test]
    fn module_compiles() {
        // TODO: Add integration-level coverage for turn orchestration using a
        // dependency-injected Agent fixture and higher-level message flow tests.
    }
}
