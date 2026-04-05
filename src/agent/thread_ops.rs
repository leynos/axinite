//! Thread and session operations for the agent.
//!
//! Extracted from `agent_loop.rs` to isolate thread management (user input
//! processing, undo/redo, approval, auth, persistence) from the core loop.

pub(crate) mod approval;
mod dispatch;
mod document_store;
mod message_rebuild;
mod persistence;

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::compaction::ContextCompactor;
use crate::agent::dispatcher::AgenticLoopResult;
use crate::agent::session::{Session, ThreadState};
use crate::agent::submission::{Submission, SubmissionParser, SubmissionResult};
use crate::channels::web::util::truncate_preview;
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;
use crate::llm::ChatMessage;

use document_store::store_extracted_documents as store_extracted_documents_impl;
use message_rebuild::rebuild_chat_messages_from_db;
use persistence::gateway_conversation_params;

pub(super) async fn store_extracted_documents(
    workspace: &Arc<crate::workspace::Workspace>,
    message: &IncomingMessage,
) {
    store_extracted_documents_impl(workspace, message).await;
}

impl Agent {
    async fn hydrate_and_resolve_session_thread(
        &self,
        message: &IncomingMessage,
    ) -> (Arc<Mutex<Session>>, Uuid) {
        // Hydrate thread from DB if it's a historical thread not in memory
        if let Some(ref external_thread_id) = message.thread_id {
            tracing::trace!(
                message_id = %message.id,
                thread_id = %external_thread_id,
                "Hydrating thread from DB"
            );
            self.maybe_hydrate_thread(message, external_thread_id).await;
        }

        tracing::debug!(
            message_id = %message.id,
            "Resolving session and thread"
        );
        let (session, thread_id) = self
            .session_manager
            .resolve_thread(
                &message.user_id,
                &message.channel,
                message.thread_id.as_deref(),
            )
            .await;
        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            "Resolved session and thread"
        );

        (session, thread_id)
    }

    async fn check_auth_mode_intercept(
        &self,
        message: &IncomingMessage,
        submission: &Submission,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Option<Result<Option<String>, Error>> {
        // Atomically take pending_auth to avoid TOCTOU race
        let pending_auth = {
            let mut sess = session.lock().await;
            sess.threads
                .get_mut(&thread_id)
                .and_then(|t| t.take_pending_auth())
        };

        if let Some(pending) = pending_auth {
            match submission {
                Submission::UserInput { content } => {
                    let scope = crate::agent::thread_ops::approval::TurnScope::new(
                        session, thread_id, message,
                    );
                    return Some(self.process_auth_token(scope, &pending, content).await);
                }
                _ => {
                    // Any control submission (interrupt, undo, etc.) cancels auth mode
                    // pending_auth was already cleared by take_pending_auth() above
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
        let target = message
            .metadata
            .get("signal_target")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| message.user_id.clone());
        self.tools()
            .set_message_tool_context(Some(message.channel.clone()), Some(target))
            .await;
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

        let result = self
            .dispatch_submission(message, submission, session, thread_id)
            .await?;
        self.map_submission_result(message, result).await
    }

    /// Hydrate a historical thread from DB into memory if not already present.
    ///
    /// Called before `resolve_thread` so that the session manager finds the
    /// thread on lookup instead of creating a new one.
    ///
    /// Creates an in-memory thread with the exact UUID the frontend sent,
    /// even when the conversation has zero messages (e.g. a brand-new
    /// assistant thread). Without this, `resolve_thread` would mint a
    /// fresh UUID and all messages would land in the wrong conversation.
    pub(super) async fn maybe_hydrate_thread(
        &self,
        message: &IncomingMessage,
        external_thread_id: &str,
    ) {
        // Only hydrate UUID-shaped thread IDs (web gateway uses UUIDs)
        let thread_uuid = match Uuid::parse_str(external_thread_id) {
            Ok(id) => id,
            Err(_) => return,
        };

        // Check if already in memory
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        {
            let sess = session.lock().await;
            if sess.threads.contains_key(&thread_uuid) {
                return;
            }
        }

        // Load history from DB (may be empty for a newly created thread).
        let mut chat_messages: Vec<ChatMessage> = Vec::new();
        let msg_count;

        if let Some(store) = self.store() {
            let db_messages = store
                .list_conversation_messages(thread_uuid)
                .await
                .unwrap_or_default();
            msg_count = db_messages.len();
            chat_messages = rebuild_chat_messages_from_db(&db_messages, self.safety());
        } else {
            msg_count = 0;
        }

        // Create thread with the historical ID and restore messages
        let session_id = {
            let sess = session.lock().await;
            sess.id
        };

        let mut thread = crate::agent::session::Thread::with_id(thread_uuid, session_id);
        if !chat_messages.is_empty() {
            thread.restore_from_messages(chat_messages);
        }

        // Insert into session and register with session manager
        {
            let mut sess = session.lock().await;
            sess.threads.insert(thread_uuid, thread);
            sess.active_thread = Some(thread_uuid);
            sess.last_active_at = chrono::Utc::now();
        }

        self.session_manager
            .register_thread(
                &message.user_id,
                &message.channel,
                thread_uuid,
                Arc::clone(&session),
            )
            .await;

        tracing::debug!(
            "Hydrated thread {} from DB ({} messages)",
            thread_uuid,
            msg_count
        );
    }

    pub(super) async fn process_user_input(
        &self,
        message: &IncomingMessage,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        content: &str,
    ) -> Result<SubmissionResult, Error> {
        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            content_len = content.len(),
            "Processing user input"
        );

        // First check thread state without holding lock during I/O
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

        // Check thread state
        match thread_state {
            ThreadState::Processing => {
                tracing::warn!(
                    message_id = %message.id,
                    thread_id = %thread_id,
                    "Thread is processing, rejecting new input"
                );
                return Ok(SubmissionResult::error(
                    "Turn in progress. Use /interrupt to cancel.",
                ));
            }
            ThreadState::AwaitingApproval => {
                tracing::warn!(
                    message_id = %message.id,
                    thread_id = %thread_id,
                    "Thread awaiting approval, rejecting new input"
                );
                return Ok(SubmissionResult::error(
                    "Waiting for approval. Use /interrupt to cancel.",
                ));
            }
            ThreadState::Completed => {
                tracing::warn!(
                    message_id = %message.id,
                    thread_id = %thread_id,
                    "Thread completed, rejecting new input"
                );
                return Ok(SubmissionResult::error(
                    "Thread completed. Use /thread new.",
                ));
            }
            ThreadState::Idle | ThreadState::Interrupted => {
                // Can proceed
            }
        }

        // Safety validation for user input
        let validation = self.safety().validate_input(content);
        if !validation.is_valid {
            let details = validation
                .errors
                .iter()
                .map(|e| format!("{}: {}", e.field, e.message))
                .collect::<Vec<_>>()
                .join("; ");
            return Ok(SubmissionResult::error(format!(
                "Input rejected by safety validation: {}",
                details
            )));
        }

        let violations = self.safety().check_policy(content);
        if violations
            .iter()
            .any(|rule| rule.action == crate::safety::PolicyAction::Block)
        {
            return Ok(SubmissionResult::error("Input rejected by safety policy."));
        }

        // Scan inbound messages for secrets (API keys, tokens).
        // Catching them here prevents the LLM from echoing them back, which
        // would trigger the outbound leak detector and create error loops.
        if let Some(warning) = self.safety().scan_inbound_for_secrets(content) {
            tracing::warn!(
                user = %message.user_id,
                channel = %message.channel,
                "Inbound message blocked: contains leaked secret"
            );
            return Ok(SubmissionResult::error(warning));
        }

        // Handle explicit commands (starting with /) directly
        // Everything else goes through the normal agentic loop with tools
        let temp_message = IncomingMessage {
            content: content.to_string(),
            ..message.clone()
        };

        if let Some(intent) = self.router.route_command(&temp_message) {
            // Explicit command like /status, /job, /list - handle directly
            return self.handle_job_or_command(intent, message).await;
        }

        // Natural language goes through the agentic loop
        // Job tools (create_job, list_jobs, etc.) are in the tool registry

        // Auto-compact if needed BEFORE adding new turn
        {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            let messages = thread.messages();
            if let Some(strategy) = self.context_monitor.suggest_compaction(&messages) {
                let pct = self.context_monitor.usage_percent(&messages);
                tracing::info!("Context at {:.1}% capacity, auto-compacting", pct);

                // Notify the user that compaction is happening
                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status(format!(
                            "Context at {:.0}% capacity, compacting...",
                            pct
                        )),
                        &message.metadata,
                    )
                    .await;

                let compactor = ContextCompactor::new(self.llm().clone());
                if let Err(e) = compactor
                    .compact(thread, strategy, self.workspace().map(|w| w.as_ref()))
                    .await
                {
                    tracing::warn!("Auto-compaction failed: {}", e);
                }
            }
        }

        // Create checkpoint before turn
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            let mut mgr = undo_mgr.lock().await;
            mgr.checkpoint(
                thread.turn_number(),
                thread.messages(),
                format!("Before turn {}", thread.turn_number()),
            );
        }

        // Augment content with attachment context (transcripts, metadata, images)
        let augmented =
            crate::agent::attachments::augment_with_attachments(content, &message.attachments);
        let (effective_content, image_parts) = match &augmented {
            Some(result) => (result.text.as_str(), result.image_parts.clone()),
            None => (content, Vec::new()),
        };

        // Start the turn and get messages
        let turn_messages = {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            let turn = thread.start_turn(effective_content);
            turn.image_content_parts = image_parts;
            thread.messages()
        };

        // Persist user message to DB immediately so it survives crashes
        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            "Persisting user message to DB"
        );
        self.persist_user_message(thread_id, &message.user_id, effective_content)
            .await;

        tracing::debug!(
            message_id = %message.id,
            thread_id = %thread_id,
            "User message persisted, starting agentic loop"
        );

        // Send thinking status
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::Thinking("Processing...".into()),
                &message.metadata,
            )
            .await;

        // Run the agentic tool execution loop
        let result = self
            .run_agentic_loop(message, session.clone(), thread_id, turn_messages)
            .await;

        // Re-acquire lock and check if interrupted
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

        // Re-acquire lock for processing result
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        // Complete, fail, or request approval
        match result {
            Ok(AgenticLoopResult::Response(response)) => {
                // Drop the session lock before running the response transform hook
                drop(sess);

                // Hook: TransformResponse — allow hooks to modify or reject the final response
                let response = {
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
                        _ => response, // fail-open: use original
                    }
                };

                // Re-acquire lock to complete turn and snapshot data
                let (turn_number, tool_calls) = {
                    let mut sess = session.lock().await;
                    let thread = sess.threads.get_mut(&thread_id).ok_or_else(|| {
                        Error::from(crate::error::JobError::NotFound { id: thread_id })
                    })?;
                    thread.complete_turn(&response);
                    thread
                        .turns
                        .last()
                        .map(|t| (t.turn_number, t.tool_calls.clone()))
                        .unwrap_or_default()
                };
                // Lock is dropped here at end of block

                let _ = self
                    .channels
                    .send_status(
                        &message.channel,
                        StatusUpdate::Status("Done".into()),
                        &message.metadata,
                    )
                    .await;

                // Persist tool calls then assistant response (user message already persisted at turn start)
                self.persist_tool_calls(thread_id, &message.user_id, turn_number, &tool_calls)
                    .await;
                self.persist_assistant_response(thread_id, &message.user_id, &response)
                    .await;

                Ok(SubmissionResult::response(response))
            }
            Ok(AgenticLoopResult::NeedApproval { pending }) => {
                // Store pending approval in thread and update state
                let request_id = pending.request_id;
                let tool_name = pending.tool_name.clone();
                let description = pending.description.clone();
                let parameters = pending.display_parameters.clone();
                thread.await_approval(pending);
                // Drop the session lock before async operations
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
                // User message already persisted at turn start; nothing else to save
                Ok(SubmissionResult::error(e.to_string()))
            }
        }
    }

    /// Persist the user message to the DB at turn start (before the agentic loop).
    ///
    /// This ensures the user message is durable even if the process crashes
    /// mid-response. Call this right after `thread.start_turn()`.
    pub(super) async fn persist_user_message(
        &self,
        thread_id: Uuid,
        user_id: &str,
        user_input: &str,
    ) {
        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };

        if let Err(e) = store
            .ensure_conversation(gateway_conversation_params(thread_id, user_id))
            .await
        {
            tracing::warn!("Failed to ensure conversation {}: {}", thread_id, e);
            return;
        }

        if let Err(e) = store
            .add_conversation_message(thread_id, "user", user_input)
            .await
        {
            tracing::warn!("Failed to persist user message: {}", e);
        }
    }

    /// Persist the assistant response to the DB after the agentic loop completes.
    ///
    /// Re-ensures the conversation row exists so that assistant responses are
    /// still persisted even if `persist_user_message` failed transiently at
    /// turn start (e.g. a brief DB blip that resolved before response time).
    pub(super) async fn persist_assistant_response(
        &self,
        thread_id: Uuid,
        user_id: &str,
        response: &str,
    ) {
        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };

        if let Err(e) = store
            .ensure_conversation(gateway_conversation_params(thread_id, user_id))
            .await
        {
            tracing::warn!("Failed to ensure conversation {}: {}", thread_id, e);
            return;
        }

        if let Err(e) = store
            .add_conversation_message(thread_id, "assistant", response)
            .await
        {
            tracing::warn!("Failed to persist assistant message: {}", e);
        }
    }

    /// Persist tool call summaries to the DB as a `role="tool_calls"` message.
    ///
    /// Stored between the user and assistant messages so that
    /// `build_turns_from_db_messages` can reconstruct the tool call history.
    /// Content is a JSON array of tool call summaries.
    pub(super) async fn persist_tool_calls(
        &self,
        thread_id: Uuid,
        user_id: &str,
        turn_number: usize,
        tool_calls: &[crate::agent::session::TurnToolCall],
    ) {
        if tool_calls.is_empty() {
            return;
        }

        let store = match self.store() {
            Some(s) => Arc::clone(s),
            None => return,
        };

        let summaries: Vec<serde_json::Value> = tool_calls
            .iter()
            .enumerate()
            .map(|(i, tc)| {
                let mut obj = serde_json::json!({
                    "name": tc.name,
                    "call_id": format!("turn{}_{}", turn_number, i),
                });
                if let Some(ref result) = tc.result {
                    let preview = match result {
                        serde_json::Value::String(s) => truncate_preview(s, 500),
                        other => truncate_preview(&other.to_string(), 500),
                    };
                    obj["result_preview"] = serde_json::Value::String(preview);
                    // Store full result (truncated to ~1000 chars) for LLM context rebuild
                    let full_result = match result {
                        serde_json::Value::String(s) => truncate_preview(s, 1000),
                        other => truncate_preview(&other.to_string(), 1000),
                    };
                    obj["result"] = serde_json::Value::String(full_result);
                }
                if let Some(ref error) = tc.error {
                    obj["error"] = serde_json::Value::String(truncate_preview(error, 200));
                }
                obj
            })
            .collect();

        let content = match serde_json::to_string(&summaries) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to serialize tool calls: {}", e);
                return;
            }
        };

        if let Err(e) = store
            .ensure_conversation(gateway_conversation_params(thread_id, user_id))
            .await
        {
            tracing::warn!("Failed to ensure conversation {}: {}", thread_id, e);
            return;
        }

        if let Err(e) = store
            .add_conversation_message(thread_id, "tool_calls", &content)
            .await
        {
            tracing::warn!("Failed to persist tool calls: {}", e);
        }
    }

    pub(super) async fn process_undo(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if !mgr.can_undo() {
            return Ok(SubmissionResult::ok_with_message("Nothing to undo."));
        }

        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        // Save current state to redo, get previous checkpoint
        let current_messages = thread.messages();
        let current_turn = thread.turn_number();

        if let Some(checkpoint) = mgr.undo(current_turn, current_messages) {
            // Extract values before consuming the reference
            let turn_number = checkpoint.turn_number;
            let messages = checkpoint.messages.clone();
            let undo_count = mgr.undo_count();
            // Restore thread from checkpoint
            thread.restore_from_messages(messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Undone to turn {}. {} undo(s) remaining.",
                turn_number, undo_count
            )))
        } else {
            Ok(SubmissionResult::error("Undo failed."))
        }
    }

    pub(super) async fn process_redo(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if !mgr.can_redo() {
            return Ok(SubmissionResult::ok_with_message("Nothing to redo."));
        }

        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        let current_messages = thread.messages();
        let current_turn = thread.turn_number();

        if let Some(checkpoint) = mgr.redo(current_turn, current_messages) {
            thread.restore_from_messages(checkpoint.messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Redone to turn {}.",
                checkpoint.turn_number
            )))
        } else {
            Ok(SubmissionResult::error("Redo failed."))
        }
    }

    pub(super) async fn process_interrupt(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        match thread.state {
            ThreadState::Processing | ThreadState::AwaitingApproval => {
                thread.interrupt();
                Ok(SubmissionResult::ok_with_message("Interrupted."))
            }
            _ => Ok(SubmissionResult::ok_with_message("Nothing to interrupt.")),
        }
    }

    pub(super) async fn process_compact(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

        let messages = thread.messages();
        let usage = self.context_monitor.usage_percent(&messages);
        let strategy = self
            .context_monitor
            .suggest_compaction(&messages)
            .unwrap_or(
                crate::agent::context_monitor::CompactionStrategy::Summarize { keep_recent: 5 },
            );

        let compactor = ContextCompactor::new(self.llm().clone());
        match compactor
            .compact(thread, strategy, self.workspace().map(|w| w.as_ref()))
            .await
        {
            Ok(result) => {
                let mut msg = format!(
                    "Compacted: {} turns removed, {} → {} tokens (was {:.1}% full)",
                    result.turns_removed, result.tokens_before, result.tokens_after, usage
                );
                if result.summary_written {
                    msg.push_str(", summary saved to workspace");
                }
                Ok(SubmissionResult::ok_with_message(msg))
            }
            Err(e) => Ok(SubmissionResult::error(format!("Compaction failed: {}", e))),
        }
    }

    pub(super) async fn process_clear(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
        thread.turns.clear();
        thread.state = ThreadState::Idle;

        // Clear undo history too
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        undo_mgr.lock().await.clear();

        Ok(SubmissionResult::ok_with_message("Thread cleared."))
    }

    pub(super) async fn process_new_thread(
        &self,
        message: &IncomingMessage,
    ) -> Result<SubmissionResult, Error> {
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        let mut sess = session.lock().await;
        let thread = sess.create_thread();
        let thread_id = thread.id;
        Ok(SubmissionResult::ok_with_message(format!(
            "New thread: {}",
            thread_id
        )))
    }

    pub(super) async fn process_switch_thread(
        &self,
        message: &IncomingMessage,
        target_thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        let mut sess = session.lock().await;

        if sess.switch_thread(target_thread_id) {
            Ok(SubmissionResult::ok_with_message(format!(
                "Switched to thread {}",
                target_thread_id
            )))
        } else {
            Ok(SubmissionResult::error("Thread not found."))
        }
    }

    pub(super) async fn process_resume(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        checkpoint_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if let Some(checkpoint) = mgr.restore(checkpoint_id) {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.restore_from_messages(checkpoint.messages);
            Ok(SubmissionResult::ok_with_message(format!(
                "Resumed from checkpoint: {}",
                checkpoint.description
            )))
        } else {
            Ok(SubmissionResult::error("Checkpoint not found."))
        }
    }
}
