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
                thread.fail_turn(e.to_string());
                Ok(SubmissionResult::error(e.to_string()))
            }
        }
    }
}
