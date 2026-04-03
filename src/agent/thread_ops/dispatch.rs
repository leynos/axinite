//! Submission dispatch and hook adapters for thread operations.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::submission::{Submission, SubmissionResult};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

impl Agent {
    /// Apply the BeforeInbound hook to the parsed submission.
    ///
    /// Returns `Ok(submission)` (possibly with modified content) to proceed,
    /// or `Err(message)` when the hook rejects the input.
    pub(super) async fn apply_inbound_hook(
        &self,
        message: &IncomingMessage,
        submission: Submission,
    ) -> Result<Submission, String> {
        let content = match &submission {
            Submission::UserInput { content } => content.clone(),
            _ => return Ok(submission),
        };
        let event = crate::hooks::HookEvent::Inbound {
            user_id: message.user_id.clone(),
            channel: message.channel.clone(),
            content,
            thread_id: message.thread_id.clone(),
        };
        match self.hooks().run(&event).await {
            Err(crate::hooks::HookError::Rejected { reason })
            | Ok(crate::hooks::HookOutcome::Reject { reason }) => {
                Err(format!("[Message rejected: {}]", reason))
            }
            Err(err) => {
                tracing::warn!("BeforeInbound hook failed open: {}", err);
                Ok(submission)
            }
            Ok(crate::hooks::HookOutcome::Continue {
                modified: Some(new_content),
            }) => Ok(Submission::UserInput {
                content: new_content,
            }),
            _ => Ok(submission), // Continue, fail-open errors already logged in registry
        }
    }

    /// Convert a `SubmissionResult` into the `Option<String>` reply format.
    ///
    /// For `NeedApproval`, sends the approval status to the channel and returns
    /// an empty string to signal the caller to skip an additional `respond()` call.
    pub(super) async fn map_submission_result(
        &self,
        incoming_msg: &IncomingMessage,
        result: SubmissionResult,
    ) -> Result<Option<String>, Error> {
        match result {
            SubmissionResult::Response { content } => {
                if crate::llm::is_silent_reply(&content) {
                    tracing::debug!("Suppressing silent reply token");
                    Ok(None)
                } else {
                    Ok(Some(content))
                }
            }
            SubmissionResult::Ok { message } => Ok(message),
            SubmissionResult::Error { message } => Ok(Some(format!("Error: {}", message))),
            SubmissionResult::Interrupted => Ok(Some("Interrupted.".into())),
            SubmissionResult::NeedApproval {
                request_id,
                tool_name,
                description,
                parameters,
            } => {
                // Each channel renders the approval prompt via send_status.
                // Web gateway shows an inline card, REPL prints a formatted prompt, etc.
                let _ = self
                    .channels
                    .send_status(
                        &incoming_msg.channel,
                        StatusUpdate::ApprovalNeeded {
                            request_id: request_id.to_string(),
                            tool_name,
                            description,
                            parameters,
                        },
                        &incoming_msg.metadata,
                    )
                    .await;

                // Empty string signals the caller to skip respond() (no duplicate text)
                Ok(Some(String::new()))
            }
        }
    }

    pub(super) async fn dispatch_submission(
        &self,
        message: &IncomingMessage,
        submission: Submission,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        match submission {
            Submission::UserInput { content } => {
                self.process_user_input(message, session, thread_id, &content)
                    .await
            }
            Submission::SystemCommand { command, args } => {
                tracing::debug!(
                    "[agent_loop] SystemCommand: command={}, channel={}",
                    command,
                    message.channel
                );
                self.handle_system_command(&command, &args, &message.channel)
                    .await
            }
            Submission::Undo => self.process_undo(session, thread_id).await,
            Submission::Redo => self.process_redo(session, thread_id).await,
            Submission::Interrupt => self.process_interrupt(session, thread_id).await,
            Submission::Compact => self.process_compact(session, thread_id).await,
            Submission::Clear => self.process_clear(session, thread_id).await,
            Submission::NewThread => self.process_new_thread(message).await,
            Submission::Heartbeat => self.process_heartbeat().await,
            Submission::Summarize => self.process_summarize(session, thread_id).await,
            Submission::Suggest => self.process_suggest(session, thread_id).await,
            Submission::JobStatus { job_id } => {
                self.process_job_status(&message.user_id, job_id.as_deref())
                    .await
            }
            Submission::JobCancel { job_id } => {
                self.process_job_cancel(&message.user_id, &job_id).await
            }
            Submission::Quit => Ok(SubmissionResult::Ok { message: None }),
            Submission::SwitchThread { thread_id: target } => {
                self.process_switch_thread(message, target).await
            }
            Submission::Resume { checkpoint_id } => {
                self.process_resume(session, thread_id, checkpoint_id).await
            }
            Submission::ExecApproval {
                request_id,
                approved,
                always,
            } => {
                self.process_approval(
                    message,
                    session,
                    thread_id,
                    Some(request_id),
                    approved,
                    always,
                )
                .await
            }
            Submission::ApprovalResponse { approved, always } => {
                self.process_approval(message, session, thread_id, None, approved, always)
                    .await
            }
        }
    }
}
