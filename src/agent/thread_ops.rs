//! Thread and session operations for the agent.
//!
//! Extracted from `agent_loop.rs` to isolate thread management (user input
//! processing, undo/redo, approval, auth, persistence) from the core loop.
//!
//! This module is organized into submodules by responsibility:
//! - `approval`: Tool approval handling
//! - `control`: Thread control commands (undo, redo, interrupt, compact, clear, new, switch, resume)
//! - `dispatch`: Submission dispatch and hook adapters
//! - `document_store`: Document storage for extracted content
//! - `hydration`: Thread hydration from database
//! - `message_rebuild`: Message reconstruction from DB records
//! - `persistence`: Database persistence for messages and tool calls
//! - `turn_execution`: User turn execution and agentic loop orchestration

pub(crate) mod approval;
mod control;
mod dispatch;
mod document_store;
mod hydration;
mod message_rebuild;
mod persistence;
mod turn_execution;

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::submission::{Submission, SubmissionParser};
use crate::channels::IncomingMessage;
use crate::error::Error;

use dispatch::DispatchCtx;
use document_store::store_extracted_documents as store_extracted_documents_impl;

pub(super) async fn store_extracted_documents(
    workspace: &Arc<crate::workspace::Workspace>,
    message: &IncomingMessage,
) {
    store_extracted_documents_impl(workspace, message).await;
}

impl Agent {
    async fn check_auth_mode_intercept(
        &self,
        message: &IncomingMessage,
        submission: &Submission,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Option<Result<Option<String>, Error>> {
        // Check for pending auth and claim it atomically to prevent concurrent bypass
        let pending_auth = {
            let mut sess = session.lock().await;
            sess.threads.get_mut(&thread_id).and_then(|t| {
                if t.in_flight_auth || t.pending_auth.is_none() {
                    return None;
                }
                t.in_flight_auth = true;
                t.pending_auth.clone()
            })
        };

        if let Some(pending) = pending_auth {
            match submission {
                Submission::UserInput { content } => {
                    let scope = crate::agent::thread_ops::approval::TurnScope::new(
                        session.clone(),
                        thread_id,
                        message,
                    );
                    let result = match self.process_auth_token(scope, &pending, content).await {
                        Ok(None) => Ok(Some(String::new())),
                        Ok(Some(s)) => Ok(Some(s)),
                        Err(e) => Err(e),
                    };

                    // Clear in_flight_auth after processing; process_auth_token is
                    // authoritative for clearing or keeping pending_auth.
                    {
                        let mut sess = session.lock().await;
                        if let Some(thread) = sess.threads.get_mut(&thread_id) {
                            thread.in_flight_auth = false;
                        }
                    }

                    return Some(result);
                }
                _ => {
                    // Any control submission (interrupt, undo, etc.) cancels auth mode.
                    // Clear the in_flight_auth marker; pending_auth is cleared separately
                    // by the control handler path.
                    let mut sess = session.lock().await;
                    if let Some(thread) = sess.threads.get_mut(&thread_id) {
                        thread.in_flight_auth = false;
                    }
                    // Fall through to normal handling
                }
            }
        }

        None
    }

    async fn set_tool_context_for_message(&self, message: &IncomingMessage) {
        // Set message tool context for this turn (current channel and target)
        // For Signal, use signal_target from metadata (group:ID or phone number),
        // otherwise fall back to user_id
        let target_opt = message
            .metadata
            .get("signal_target")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .filter(|s| !s.is_empty())
            .or_else(|| {
                if !message.user_id.is_empty() {
                    Some(message.user_id.clone())
                } else {
                    None
                }
            });
        if let Some(target) = target_opt {
            self.tools()
                .set_message_tool_context(Some(message.channel.clone()), Some(target))
                .await;
        }
    }

    pub(super) async fn handle_message(
        &self,
        message: &IncomingMessage,
    ) -> Result<Option<String>, Error> {
        // Parse submission type first
        let submission = SubmissionParser::parse(&message.content);

        let (session, thread_id) = self.hydrate_and_resolve_session_thread(message).await;

        if let Some(result) = self
            .check_auth_mode_intercept(message, &submission, session.clone(), thread_id)
            .await
        {
            return result;
        }

        // Log at info level only for tracking without exposing PII (user_id can be a phone number)
        tracing::info!(message_id = %message.id, "Processing message");

        // Log sensitive details at debug level for troubleshooting
        tracing::debug!(
            message_id = %message.id,
            channel = %message.channel,
            thread_id = ?message.thread_id,
            "Message details"
        );

        self.set_tool_context_for_message(message).await;

        let submission = match self.apply_inbound_hook(message, submission).await {
            Ok(s) => s,
            Err(msg) => return Ok(Some(msg)),
        };

        tracing::trace!(
            "[agent_loop] Parsed submission: {:?}",
            std::any::type_name_of_val(&submission)
        );

        tracing::trace!(
            "Received message on {} ({} chars)",
            message.channel,
            message.content.len()
        );

        if matches!(submission, Submission::Quit) {
            return Ok(None);
        }

        let ctx = DispatchCtx {
            message: message.clone(),
            session: session.clone(),
            thread_id,
        };
        let result = self.dispatch_submission(ctx, submission).await?;
        self.map_submission_result(message, result).await
    }
}
