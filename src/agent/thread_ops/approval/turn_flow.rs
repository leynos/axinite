//! Turn lifecycle handling for approval flows: finalization, rejection,
//! failure, and agentic-loop continuation after tool execution.

use crate::agent::Agent;
use crate::agent::dispatcher::AgenticLoopResult;
use crate::agent::session::PendingApproval;
use crate::agent::submission::SubmissionResult;
use crate::agent::thread_ops::TurnPersistContext;
use crate::channels::StatusUpdate;
use crate::error::Error;
use crate::llm::ChatMessage;

use super::context::TurnScope;

impl Agent {
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
        let persist_ctx = TurnPersistContext {
            thread_id: scope.thread_id,
            user_id: &scope.env.user_id,
            turn_number,
        };
        self.persist_tool_calls(&persist_ctx, &tool_calls).await;
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
    pub(super) async fn continue_loop_after_tool(
        &self,
        scope: TurnScope,
        context_messages: Vec<ChatMessage>,
    ) -> Result<SubmissionResult, Error> {
        let message = scope.to_message();
        let result = self
            .run_agentic_loop(
                &message,
                crate::agent::dispatcher::RunLoopCtx {
                    session: scope.session.clone(),
                    thread_id: scope.thread_id,
                    initial_messages: context_messages,
                },
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
    pub(super) async fn complete_rejection_and_persist(
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
}
