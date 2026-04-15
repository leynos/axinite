//! Thread control command handlers.
//!
//! Contains handlers for thread lifecycle state transitions:
//! - Undo/redo operations
//! - Interrupt processing
//! - Context compaction
//! - Thread clearing
//! - New thread creation
//! - Thread switching
//! - Resume from checkpoint

use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::compaction::ContextCompactor;
use crate::agent::session::{Session, ThreadState};
use crate::agent::submission::SubmissionResult;
use crate::agent::undo::{Checkpoint, UndoManager};
use crate::error::Error;
use crate::llm::ChatMessage;

#[derive(Clone, Copy)]
enum RewindOp {
    Undo,
    Redo,
}

impl Agent {
    fn availability_message(mgr: &UndoManager, op: RewindOp) -> Option<&'static str> {
        match op {
            RewindOp::Undo if !mgr.can_undo() => Some("Nothing to undo."),
            RewindOp::Redo if !mgr.can_redo() => Some("Nothing to redo."),
            _ => None,
        }
    }

    fn failure_msg(op: RewindOp) -> &'static str {
        match op {
            RewindOp::Undo => "Undo failed.",
            RewindOp::Redo => "Redo failed.",
        }
    }

    fn success_msg(op: RewindOp, turn: usize, undo_count: usize) -> String {
        match op {
            RewindOp::Undo => format!("Undone to turn {turn}.\n{undo_count} undo(s) remaining."),
            RewindOp::Redo => format!("Redone to turn {turn}."),
        }
    }

    fn perform_rewind(
        mgr: &mut UndoManager,
        op: RewindOp,
        current_turn: usize,
        current_messages: Vec<ChatMessage>,
    ) -> Option<Checkpoint> {
        match op {
            RewindOp::Undo => mgr.undo(current_turn, current_messages),
            RewindOp::Redo => mgr.redo(current_turn, current_messages),
        }
    }

    async fn restore_thread_from_checkpoint(
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        messages: Vec<ChatMessage>,
    ) -> Result<(), Error> {
        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
        thread.restore_from_messages(messages);
        thread.updated_at = Utc::now();
        Ok(())
    }

    async fn process_rewind(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
        op: RewindOp,
    ) -> Result<SubmissionResult, Error> {
        let (turn, messages) = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            (thread.turn_number(), thread.messages())
        };

        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if let Some(msg) = Self::availability_message(&mgr, op) {
            return Ok(SubmissionResult::ok_with_message(msg.to_string()));
        }

        let Some(cp) = Self::perform_rewind(&mut mgr, op, turn, messages) else {
            return Ok(SubmissionResult::error(Self::failure_msg(op)));
        };

        let msg = Self::success_msg(op, cp.turn_number, mgr.undo_count());
        drop(mgr);
        Self::restore_thread_from_checkpoint(&session, thread_id, cp.messages).await?;
        Ok(SubmissionResult::ok_with_message(msg))
    }

    pub(super) async fn process_undo(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        self.process_rewind(session, thread_id, RewindOp::Undo)
            .await
    }

    pub(super) async fn process_redo(
        &self,
        session: Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<SubmissionResult, Error> {
        self.process_rewind(session, thread_id, RewindOp::Redo)
            .await
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
        let (mut thread_snapshot, usage, strategy) = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;

            let messages = thread.messages();
            let usage = self.context_monitor.usage_percent(&messages);
            let strategy = self
                .context_monitor
                .suggest_compaction(&messages)
                .unwrap_or(
                    crate::agent::context_monitor::CompactionStrategy::Summarize { keep_recent: 5 },
                );

            (thread.clone(), usage, strategy)
        };

        let original_updated_at = thread_snapshot.updated_at;
        let original_turns_len = thread_snapshot.turns.len();
        let compactor = ContextCompactor::new(self.llm().clone());
        match compactor
            .compact(
                &mut thread_snapshot,
                strategy,
                self.workspace().map(|w| w.as_ref()),
            )
            .await
        {
            Ok(result) => {
                let mut sess = session.lock().await;
                let thread = sess.threads.get_mut(&thread_id).ok_or_else(|| {
                    Error::from(crate::error::JobError::NotFound { id: thread_id })
                })?;
                if thread.updated_at != original_updated_at
                    || thread.turns.len() != original_turns_len
                {
                    return Ok(SubmissionResult::error(
                        "Thread changed while compaction was running. Please retry.",
                    ));
                }
                thread.turns = thread_snapshot.turns;
                thread.updated_at = Utc::now();

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
        thread.updated_at = Utc::now();
        drop(sess);

        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        undo_mgr.lock().await.clear();

        Ok(SubmissionResult::ok_with_message("Thread cleared."))
    }

    pub(super) async fn process_new_thread(
        &self,
        message: &crate::channels::IncomingMessage,
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
        message: &crate::channels::IncomingMessage,
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
        {
            let sess = session.lock().await;
            let _thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
        }

        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        let Some(checkpoint) = mgr.restore(checkpoint_id) else {
            return Ok(SubmissionResult::error("Checkpoint not found."));
        };
        let description = checkpoint.description.clone();
        let messages = checkpoint.messages;
        drop(mgr);

        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
        thread.restore_from_messages(messages);
        thread.updated_at = Utc::now();

        Ok(SubmissionResult::ok_with_message(format!(
            "Resumed from checkpoint: {}",
            description
        )))
    }
}
