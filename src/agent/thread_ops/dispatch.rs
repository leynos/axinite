//! Submission dispatch and hook adapters for thread operations.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::submission::{Submission, SubmissionParser, SubmissionResult};
use crate::agent::thread_ops::approval::{ApprovalParams, TurnScope};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

/// Dispatch context for bundling co-travelling arguments.
#[derive(Clone)]
pub(super) struct DispatchCtx {
    pub message: IncomingMessage,
    pub session: Arc<Mutex<Session>>,
    pub thread_id: Uuid,
}

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
        let event = crate::hooks::HookEvent::Inbound {
            user_id: message.user_id.clone(),
            channel: message.channel.clone(),
            content: message.content.clone(),
            thread_id: message.thread_id.clone(),
        };
        match self.hooks().run(&event).await {
            Err(crate::hooks::HookError::Rejected { reason }) => {
                Err(format!("[Message rejected: {}]", reason))
            }
            Err(err) => {
                tracing::warn!("BeforeInbound hook failed open: {}", err);
                Ok(submission)
            }
            Ok(crate::hooks::HookOutcome::Continue {
                modified: Some(new_content),
            }) => Ok(SubmissionParser::parse(&new_content)),
            Ok(crate::hooks::HookOutcome::Continue { modified: None }) => Ok(submission),
            Ok(crate::hooks::HookOutcome::Reject { reason }) => {
                Err(format!("[Message rejected: {}]", reason))
            }
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
                    // Return empty string sentinel instead of None to avoid shutdown signal
                    Ok(Some(String::new()))
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
                let status_result = self
                    .channels
                    .send_status(
                        &incoming_msg.channel,
                        StatusUpdate::ApprovalNeeded {
                            request_id: request_id.to_string(),
                            tool_name: tool_name.clone(),
                            description: description.clone(),
                            parameters,
                        },
                        &incoming_msg.metadata,
                    )
                    .await;

                if let Err(err) = status_result {
                    tracing::warn!("Failed to send approval status update: {err}");
                    Ok(Some(format!(
                        "Approval required for `{tool_name}`: {description}"
                    )))
                } else {
                    // Empty string signals the caller to skip respond() (no duplicate text)
                    Ok(Some(String::new()))
                }
            }
        }
    }

    pub(super) async fn dispatch_submission(
        &self,
        ctx: DispatchCtx,
        submission: Submission,
    ) -> Result<SubmissionResult, Error> {
        match submission {
            Submission::UserInput { content } => {
                self.process_user_input(&ctx.message, ctx.session, ctx.thread_id, &content)
                    .await
            }
            Submission::SystemCommand { command, args } => {
                tracing::debug!(
                    "[agent_loop] SystemCommand: command={}, channel={}",
                    command,
                    ctx.message.channel
                );
                self.handle_system_command(&command, &args, &ctx.message.channel)
                    .await
            }
            Submission::Undo => self.process_undo(ctx.session, ctx.thread_id).await,
            Submission::Redo => self.process_redo(ctx.session, ctx.thread_id).await,
            Submission::Interrupt => self.process_interrupt(ctx.session, ctx.thread_id).await,
            Submission::Compact => self.process_compact(ctx.session, ctx.thread_id).await,
            Submission::Clear => self.process_clear(ctx.session, ctx.thread_id).await,
            Submission::NewThread => self.process_new_thread(&ctx.message).await,
            Submission::Heartbeat => self.process_heartbeat().await,
            Submission::Summarize => self.process_summarize(ctx.session, ctx.thread_id).await,
            Submission::Suggest => self.process_suggest(ctx.session, ctx.thread_id).await,
            Submission::JobStatus { job_id } => {
                self.process_job_status(&ctx.message.user_id, job_id.as_deref())
                    .await
            }
            Submission::JobCancel { job_id } => {
                self.process_job_cancel(&ctx.message.user_id, &job_id).await
            }
            Submission::Quit => Ok(SubmissionResult::Ok { message: None }),
            Submission::SwitchThread { thread_id: target } => {
                self.process_switch_thread(&ctx.message, target).await
            }
            Submission::Resume { checkpoint_id } => {
                self.process_resume(ctx.session, ctx.thread_id, checkpoint_id)
                    .await
            }
            Submission::ExecApproval {
                request_id,
                approved,
                always,
            } => {
                let scope = TurnScope::new(ctx.session.clone(), ctx.thread_id, &ctx.message);
                let params = ApprovalParams {
                    request_id: Some(request_id),
                    approved,
                    always,
                };
                self.process_approval(scope, params).await
            }
            Submission::ApprovalResponse { approved, always } => {
                let scope = TurnScope::new(ctx.session.clone(), ctx.thread_id, &ctx.message);
                let params = ApprovalParams {
                    request_id: None,
                    approved,
                    always,
                };
                self.process_approval(scope, params).await
            }
        }
    }
}
