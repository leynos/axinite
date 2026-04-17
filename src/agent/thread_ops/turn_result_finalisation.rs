//! Result finalisation helpers for completed user turns.

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

impl Agent {
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
    pub(super) async fn handle_loop_result(
        &self,
        message: &IncomingMessage,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        result: Result<AgenticLoopResult, Error>,
    ) -> Result<SubmissionResult, Error> {
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
                        thread
                            .turns
                            .last()
                            .map(|t| (t.turn_number, t.tool_calls.clone()))
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
                let thread = sess.threads.get_mut(&thread_id).ok_or_else(|| {
                    Error::from(crate::error::JobError::NotFound { id: thread_id })
                })?;
                thread.fail_turn(error_text.clone());
                drop(sess);
                self.persist_assistant_response(thread_id, &message.user_id, &error_text)
                    .await;
                Ok(SubmissionResult::error(error_text))
            }
        }
    }
}

#[cfg(all(test, feature = "libsql"))]
mod tests {
    use std::sync::Arc;

    use anyhow::Result;
    use rstest::rstest;
    use tokio::sync::Mutex;
    use uuid::Uuid;

    use super::*;
    use crate::agent::thread_ops::test_support::{
        incoming_message, local_backend, make_agent, session_manager,
    };
    use crate::agent::{PendingApproval, SessionManager};
    use crate::db::Database;
    use crate::llm::{ChatMessage, LlmProvider};
    use crate::testing::StubLlm;

    async fn make_session_with_started_turn() -> (Arc<Mutex<Session>>, Uuid) {
        let mut session = Session::new("user-1");
        let thread = session.create_thread();
        let thread_id = thread.id;
        thread.start_turn("hello");
        (Arc::new(Mutex::new(session)), thread_id)
    }

    async fn make_persisting_agent(
        session_manager: Arc<SessionManager>,
    ) -> Result<(Agent, Arc<dyn Database>, tempfile::TempDir)> {
        let (backend, tempdir) = local_backend().await?;
        let store: Arc<dyn Database> = backend;
        let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm::new("ok"));
        let agent = make_agent(Some(Arc::clone(&store)), llm, session_manager);
        Ok((agent, store, tempdir))
    }

    fn pending_approval_fixture() -> PendingApproval {
        PendingApproval {
            request_id: Uuid::new_v4(),
            tool_name: "dangerous_tool".to_string(),
            parameters: serde_json::json!({ "path": "/tmp/file" }),
            display_parameters: serde_json::json!({ "path": "/tmp/file" }),
            description: "Modify a file".to_string(),
            tool_call_id: "call-1".to_string(),
            context_messages: vec![ChatMessage::user("hello")],
            deferred_tool_calls: Vec::new(),
            user_timezone: None,
        }
    }

    #[rstest]
    #[tokio::test]
    async fn handle_loop_result_response_persists_assistant_reply(
        incoming_message: IncomingMessage,
        session_manager: Arc<SessionManager>,
    ) -> Result<()> {
        let (agent, store, _tempdir) = make_persisting_agent(session_manager).await?;
        let (session, thread_id) = make_session_with_started_turn().await;

        let result = agent
            .handle_loop_result(
                &incoming_message,
                &session,
                thread_id,
                Ok(AgenticLoopResult::Response("done".to_string())),
            )
            .await
            .expect("response finalisation should succeed");

        assert!(
            matches!(result, SubmissionResult::Response { ref content } if content == "done"),
            "expected response submission result"
        );

        let messages = store
            .list_conversation_messages(thread_id)
            .await
            .expect("assistant response should be persisted");
        assert!(
            messages
                .iter()
                .any(|message| message.role == "assistant" && message.content == "done"),
            "expected persisted assistant response"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn handle_loop_result_need_approval_returns_submission_result(
        incoming_message: IncomingMessage,
        session_manager: Arc<SessionManager>,
    ) -> Result<()> {
        let (agent, _store, _tempdir) = make_persisting_agent(session_manager).await?;
        let (session, thread_id) = make_session_with_started_turn().await;
        let pending = pending_approval_fixture();
        let request_id = pending.request_id;
        let expected_description = pending.description.clone();
        let expected_tool_name = pending.tool_name.clone();
        let expected_parameters = pending.display_parameters.clone();

        let result = agent
            .handle_loop_result(
                &incoming_message,
                &session,
                thread_id,
                Ok(AgenticLoopResult::NeedApproval { pending }),
            )
            .await
            .expect("approval finalisation should succeed");

        assert!(
            matches!(
                result,
                SubmissionResult::NeedApproval {
                    request_id: actual_request_id,
                    tool_name: ref actual_tool_name,
                    description: ref actual_description,
                    parameters: ref actual_parameters
                } if actual_request_id == request_id
                    && actual_tool_name == &expected_tool_name
                    && actual_description == &expected_description
                    && actual_parameters == &expected_parameters
            ),
            "expected need-approval submission result"
        );

        let sess = session.lock().await;
        let thread = sess
            .threads
            .get(&thread_id)
            .expect("thread should still exist after approval finalisation");
        assert_eq!(thread.state, ThreadState::AwaitingApproval);
        assert!(
            thread.pending_approval.is_some(),
            "pending approval should be stored on the thread"
        );
        Ok(())
    }

    #[rstest]
    #[tokio::test]
    async fn handle_loop_result_error_persists_failure_and_marks_thread_failed(
        incoming_message: IncomingMessage,
        session_manager: Arc<SessionManager>,
    ) -> Result<()> {
        let (agent, store, _tempdir) = make_persisting_agent(session_manager).await?;
        let (session, thread_id) = make_session_with_started_turn().await;
        let inner_error = "boom".to_string();
        let expected_error_text = format!("Database error: Query failed: {inner_error}");

        let result = agent
            .handle_loop_result(
                &incoming_message,
                &session,
                thread_id,
                Err(Error::from(crate::error::DatabaseError::Query(inner_error))),
            )
            .await
            .expect("error finalisation should succeed");

        assert!(
            matches!(
                result,
                SubmissionResult::Error { ref message } if message == &expected_error_text
            ),
            "expected error submission result"
        );

        let sess = session.lock().await;
        let thread = sess
            .threads
            .get(&thread_id)
            .expect("thread should still exist after error finalisation");
        assert_eq!(thread.state, ThreadState::Idle);
        assert!(
            thread
                .last_turn()
                .and_then(|turn| turn.error.as_ref())
                .is_some_and(|error| error == &expected_error_text),
            "expected thread.fail_turn to record the error"
        );
        drop(sess);

        let messages = store
            .list_conversation_messages(thread_id)
            .await
            .expect("assistant error reply should be persisted");
        assert!(
            messages.iter().any(|message| {
                message.role == "assistant" && message.content == expected_error_text
            }),
            "expected persisted assistant error message"
        );
        Ok(())
    }
}
