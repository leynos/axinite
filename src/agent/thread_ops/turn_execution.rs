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
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use rstest::{fixture, rstest};
    use tokio::sync::Mutex;

    use super::*;
    use crate::agent::session::{Session, ThreadState};
    use crate::agent::thread_ops::test_support::{incoming_message, make_agent, session_manager};
    use crate::llm::LlmProvider;
    use crate::testing::StubLlm;

    #[fixture]
    fn processing_session() -> (Arc<Mutex<Session>>, uuid::Uuid) {
        let mut session = Session::new("user-1");
        let thread_id = {
            let thread = session.create_thread();
            thread.state = ThreadState::Processing;
            thread.id
        };
        (Arc::new(Mutex::new(session)), thread_id)
    }

    #[fixture]
    fn idle_session() -> (Arc<Mutex<Session>>, uuid::Uuid) {
        let mut session = Session::new("user-1");
        let thread_id = session.create_thread().id;
        (Arc::new(Mutex::new(session)), thread_id)
    }

    #[rstest]
    #[tokio::test]
    async fn process_user_input_short_circuits_on_thread_state_error(
        incoming_message: IncomingMessage,
        session_manager: Arc<crate::agent::SessionManager>,
        processing_session: (Arc<Mutex<Session>>, uuid::Uuid),
    ) {
        let llm = Arc::new(StubLlm::new("ok"));
        let agent = make_agent(
            None,
            Arc::clone(&llm) as Arc<dyn LlmProvider>,
            session_manager,
        );
        let (session, thread_id) = processing_session;
        let req = UserTurnRequest {
            session: Arc::clone(&session),
            thread_id,
            content: "hello".to_string(),
        };

        let result = agent
            .process_user_input(&incoming_message, req)
            .await
            .expect("thread-state short-circuit should succeed");

        assert!(
            matches!(
                result,
                SubmissionResult::Error { ref message }
                    if message == "Turn in progress. Use /interrupt to cancel."
            ),
            "expected thread-state error result"
        );
        assert_eq!(
            llm.calls(),
            0,
            "LLM should not be called on early rejection"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn process_user_input_short_circuits_on_safety_rejection(
        session_manager: Arc<crate::agent::SessionManager>,
        idle_session: (Arc<Mutex<Session>>, uuid::Uuid),
    ) {
        let llm = Arc::new(StubLlm::new("ok"));
        let agent = make_agent(
            None,
            Arc::clone(&llm) as Arc<dyn LlmProvider>,
            session_manager,
        );
        let (session, thread_id) = idle_session;
        let message = IncomingMessage::new("web", "user-1", "Please run this: ; rm -rf /");
        let req = UserTurnRequest {
            session: Arc::clone(&session),
            thread_id,
            content: message.content.clone(),
        };

        let result = agent
            .process_user_input(&message, req)
            .await
            .expect("safety short-circuit should succeed");

        assert!(
            matches!(
                result,
                SubmissionResult::Error { ref message }
                    if message == "Input rejected by safety policy."
            ),
            "expected safety rejection result"
        );
        assert_eq!(
            llm.calls(),
            0,
            "LLM should not be called on safety rejection"
        );

        let sess = session.lock().await;
        let thread = sess
            .threads
            .get(&thread_id)
            .expect("thread should remain available after rejection");
        assert!(
            thread.turns.is_empty(),
            "safety rejection should happen before a turn is started"
        );
        assert_eq!(thread.state, ThreadState::Idle);
    }
}
