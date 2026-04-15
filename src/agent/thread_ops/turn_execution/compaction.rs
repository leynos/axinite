//! Compaction and undo-checkpoint helpers for user turn execution.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::compaction::ContextCompactor;
use crate::agent::session::Session;
use crate::channels::{IncomingMessage, StatusUpdate};
use crate::error::Error;

/// Auto-compact context if needed before adding new turn.
pub(super) async fn maybe_compact_context(
    agent: &Agent,
    message: &IncomingMessage,
    session: &Arc<Mutex<Session>>,
    thread_id: Uuid,
) -> Result<(), Error> {
    let (messages, strategy) = {
        let sess = session.lock().await;
        let thread = sess
            .threads
            .get(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?;
        let messages = thread.messages();
        let strategy = agent.context_monitor.suggest_compaction(&messages);
        (messages, strategy)
    };

    let Some(strategy) = strategy else {
        return Ok(());
    };

    let pct = agent.context_monitor.usage_percent(&messages);
    tracing::info!("Context at {:.1}% capacity, auto-compacting", pct);

    let _ = agent
        .channels
        .send_status(
            &message.channel,
            StatusUpdate::Status(format!("Context at {:.0}% capacity, compacting...", pct)),
            &message.metadata,
        )
        .await;

    let workspace = agent.workspace().map(Arc::clone);
    let mut thread = {
        let mut sess = session.lock().await;
        sess.threads
            .remove(&thread_id)
            .ok_or_else(|| Error::from(crate::error::JobError::NotFound { id: thread_id }))?
    };

    let compactor = ContextCompactor::new(agent.llm().clone());
    let compaction_result = compactor
        .compact(&mut thread, strategy, workspace.as_deref())
        .await;

    {
        let mut sess = session.lock().await;
        sess.threads.insert(thread_id, thread);
    }

    if let Err(e) = compaction_result {
        tracing::warn!("Auto-compaction failed: {}", e);
    }
    Ok(())
}

/// Create checkpoint before turn.
pub(super) async fn checkpoint_before_turn(
    agent: &Agent,
    session: &Arc<Mutex<Session>>,
    thread_id: Uuid,
) -> Result<(), Error> {
    let undo_mgr = agent.session_manager.get_undo_manager(thread_id).await;
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
