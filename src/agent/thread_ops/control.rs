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
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let mut mgr = undo_mgr.lock().await;

        if let Some(msg) = Self::availability_message(&mgr, op) {
            return Ok(SubmissionResult::ok_with_message(msg.to_string()));
        }

        let (turn, messages) = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            (thread.turn_number(), thread.messages())
        };

        let Some(cp) = Self::perform_rewind(&mut mgr, op, turn, messages) else {
            return Ok(SubmissionResult::error(Self::failure_msg(op)));
        };

        let msg = Self::success_msg(op, cp.turn_number, mgr.undo_count());
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
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        undo_mgr.lock().await.clear();

        let mut sess = session.lock().await;
        let thread = sess
            .threads
            .get_mut(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
        thread.turns.clear();
        thread.state = ThreadState::Idle;
        thread.updated_at = Utc::now();

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

        if let Some(checkpoint) = mgr.restore(checkpoint_id) {
            let mut sess = session.lock().await;
            let thread = sess
                .threads
                .get_mut(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            thread.restore_from_messages(checkpoint.messages);
            thread.updated_at = Utc::now();
            Ok(SubmissionResult::ok_with_message(format!(
                "Resumed from checkpoint: {}",
                checkpoint.description
            )))
        } else {
            Ok(SubmissionResult::error("Checkpoint not found."))
        }
    }
}

mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::agent::agent_loop::{Agent, AgentDeps};
    use crate::agent::cost_guard::{CostGuard, CostGuardConfig};
    use crate::channels::{ChannelManager, IncomingMessage};
    use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
    use crate::context::ContextManager;
    use crate::hooks::HookRegistry;
    use crate::safety::SafetyLayer;
    use crate::testing::StubLlm;
    use crate::tools::ToolRegistry;

    fn make_test_agent() -> Agent {
        let deps = AgentDeps {
            store: None,
            llm: Arc::new(StubLlm::new("ok")),
            cheap_llm: None,
            safety: Arc::new(SafetyLayer::new(&SafetyConfig {
                max_output_length: 100_000,
                injection_check_enabled: true,
            })),
            tools: Arc::new(ToolRegistry::new()),
            workspace: None,
            extension_manager: None,
            skill_registry: None,
            skill_catalog: None,
            skills_config: SkillsConfig::default(),
            hooks: Arc::new(HookRegistry::new()),
            cost_guard: Arc::new(CostGuard::new(CostGuardConfig::default())),
            sse_tx: None,
            http_interceptor: None,
            transcription: None,
            document_extraction: None,
        };

        Agent::new(
            AgentConfig {
                name: "test-agent".to_string(),
                max_parallel_jobs: 1,
                job_timeout: Duration::from_secs(60),
                stuck_threshold: Duration::from_secs(60),
                repair_check_interval: Duration::from_secs(30),
                max_repair_attempts: 1,
                use_planning: false,
                session_idle_timeout: Duration::from_secs(300),
                allow_local_tools: false,
                max_cost_per_day_cents: None,
                max_actions_per_hour: None,
                max_tool_iterations: 4,
                auto_approve_tools: false,
                default_timezone: "UTC".to_string(),
                max_tokens_per_job: 0,
            },
            deps,
            Arc::new(ChannelManager::new()),
            None,
            None,
            None,
            Some(Arc::new(ContextManager::new(1))),
            None,
        )
    }

    fn test_message(user_id: &str) -> IncomingMessage {
        IncomingMessage {
            id: Uuid::new_v4(),
            channel: "test".to_string(),
            user_id: user_id.to_string(),
            user_name: None,
            content: "hello".to_string(),
            thread_id: None,
            received_at: chrono::Utc::now(),
            metadata: serde_json::Value::Null,
            attachments: vec![],
            timezone: Some("UTC".to_string()),
        }
    }

    #[tokio::test]
    async fn process_interrupt_rejects_idle_thread() {
        let agent = make_test_agent();
        let mut session = Session::new("user-1");
        let thread_id = session.create_thread().id;
        let session = Arc::new(Mutex::new(session));

        let result = agent
            .process_interrupt(Arc::clone(&session), thread_id)
            .await
            .expect("interrupt should succeed");

        assert!(matches!(
            result,
            SubmissionResult::Ok {
                message: Some(ref message)
            } if message == "Nothing to interrupt."
        ));
        let guard = session.lock().await;
        assert_eq!(guard.threads[&thread_id].state, ThreadState::Idle);
    }

    #[tokio::test]
    async fn process_clear_resets_thread() {
        let agent = make_test_agent();
        let mut session = Session::new("user-1");
        let thread = session.create_thread();
        let thread_id = thread.id;
        thread.start_turn("first turn");
        let session = Arc::new(Mutex::new(session));

        let result = agent
            .process_clear(Arc::clone(&session), thread_id)
            .await
            .expect("clear should succeed");

        assert!(matches!(
            result,
            SubmissionResult::Ok {
                message: Some(ref message)
            } if message == "Thread cleared."
        ));
        let guard = session.lock().await;
        assert!(guard.threads[&thread_id].turns.is_empty());
        assert_eq!(guard.threads[&thread_id].state, ThreadState::Idle);
    }

    #[tokio::test]
    async fn process_switch_thread_returns_error_for_unknown() {
        let agent = make_test_agent();
        let result = agent
            .process_switch_thread(&test_message("user-1"), Uuid::new_v4())
            .await
            .expect("switch should return a submission result");

        assert!(matches!(
            result,
            SubmissionResult::Error { ref message } if message == "Thread not found."
        ));
    }
}
