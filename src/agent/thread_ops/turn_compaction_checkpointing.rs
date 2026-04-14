//! Context compaction and checkpoint helpers for user turns.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::compaction::ContextCompactor;
use crate::agent::session::Session;
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

impl Agent {
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
        if let Some(strategy) = self.context_monitor.suggest_compaction(&messages) {
            let pct = self.context_monitor.usage_percent(&messages);
            tracing::info!("Context at {:.1}% capacity, auto-compacting", pct);

            let _ = self
                .channels
                .send_status(
                    &message.channel,
                    StatusUpdate::Status(format!("Context at {:.0}% capacity, compacting...", pct)),
                    &message.metadata,
                )
                .await;

            let compactor = ContextCompactor::new(self.llm().clone());
            if let Err(e) = compactor
                .compact(
                    &mut thread_snapshot,
                    strategy,
                    self.workspace().map(|w| w.as_ref()),
                )
                .await
            {
                tracing::warn!("Auto-compaction failed: {}", e);
            } else {
                let mut sess = session.lock().await;
                if let Some(thread) = sess.threads.get_mut(&thread_id) {
                    if thread.updated_at == thread_snapshot.updated_at
                        && thread.turns.len() == thread_snapshot.turns.len()
                    {
                        *thread = thread_snapshot;
                    } else {
                        tracing::warn!(
                            thread_id = %thread_id,
                            "Skipped applying stale auto-compaction result"
                        );
                    }
                }
            }
        }
        Ok(())
    }

    /// Create checkpoint before turn.
    pub(super) async fn checkpoint_before_turn(
        &self,
        session: &Arc<Mutex<Session>>,
        thread_id: Uuid,
    ) -> Result<(), Error> {
        let undo_mgr = self.session_manager.get_undo_manager(thread_id).await;
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
        Ok(())
    }
}
