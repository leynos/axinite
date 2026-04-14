//! Context compaction and checkpoint helpers for user turns.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::compaction::ContextCompactor;
use crate::agent::session::{Session, Thread};
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

impl Agent {
    async fn notify_compaction_status(&self, message: &IncomingMessage, pct: f32) {
        let _ = self
            .channels
            .send_status(
                &message.channel,
                StatusUpdate::Status(format!("Context at {:.0}% capacity, compacting...", pct)),
                &message.metadata,
            )
            .await;
    }

    async fn try_compact_snapshot(
        &self,
        snapshot: &mut Thread,
        strategy: crate::agent::context_monitor::CompactionStrategy,
    ) -> bool {
        let compactor = ContextCompactor::new(self.llm().clone());
        match compactor
            .compact(snapshot, strategy, self.workspace().map(|w| w.as_ref()))
            .await
        {
            Ok(_) => true,
            Err(e) => {
                tracing::warn!("Auto-compaction failed: {}", e);
                false
            }
        }
    }

    async fn apply_compaction_if_fresh(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
        snapshot: Thread,
    ) {
        let mut sess = session.lock().await;
        if let Some(thread) = sess.threads.get_mut(&thread_id) {
            if thread.updated_at == snapshot.updated_at
                && thread.turns.len() == snapshot.turns.len()
            {
                *thread = snapshot;
            } else {
                tracing::warn!(
                    thread_id = %thread_id,
                    "Skipped applying stale auto-compaction result"
                );
            }
        }
    }

    /// Auto-compact context if needed before adding new turn.
    pub(super) async fn maybe_compact_context(
        &self,
        message: &IncomingMessage,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<(), Error> {
        let mut thread_snapshot = {
            let sess = session.lock().await;
            sess.threads
                .get(&thread_id)
                .cloned()
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?
        };

        let messages = thread_snapshot.messages();
        let Some(strategy) = self.context_monitor.suggest_compaction(&messages) else {
            return Ok(());
        };

        let pct = self.context_monitor.usage_percent(&messages);
        tracing::info!("Context at {:.1}% capacity, auto-compacting", pct);
        self.notify_compaction_status(message, pct as f32).await;

        if !self
            .try_compact_snapshot(&mut thread_snapshot, strategy)
            .await
        {
            return Ok(());
        }

        self.apply_compaction_if_fresh(session, thread_id, thread_snapshot)
            .await;
        Ok(())
    }

    /// Create checkpoint before turn.
    pub(super) async fn checkpoint_before_turn(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<(), Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
        let (turn_number, messages) = {
            let sess = session.lock().await;
            let thread = sess
                .threads
                .get(&thread_id)
                .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
            (thread.turn_number(), thread.messages())
        };

        let mut mgr = undo_mgr.lock().await;
        mgr.checkpoint(
            turn_number,
            messages,
            format!("Before turn {}", turn_number),
        );
        Ok(())
    }
}
