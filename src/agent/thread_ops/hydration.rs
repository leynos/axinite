//! Thread hydration from database.
//!
//! Handles loading historical threads from the database into memory,
//! including message reconstruction and session registration.

use std::sync::Arc;

use tokio::sync::Mutex;
use uuid::Uuid;

use crate::agent::Agent;
use crate::agent::session::Session;
use crate::agent::thread_ops::message_rebuild::rebuild_chat_messages_from_db;
use crate::channels::IncomingMessage;
use crate::error::Error;
use crate::llm::ChatMessage;

impl Agent {
    /// Hydrate and resolve session/thread for an incoming message.
    ///
    /// This is the main entry point for message handling. It hydrates the thread
    /// from the database if needed, then resolves the session and thread IDs.
    pub(super) async fn hydrate_and_resolve_session_thread(
        &self,
        message: &IncomingMessage,
    ) -> Result<(Arc<Mutex<Session>>, Uuid), Error> {
        // Hydrate thread from DB if it's a historical thread not in memory
        if let Some(ref external_thread_id) = message.thread_id {
            tracing::trace!(
                message_id = %message.id,
                thread_id = %external_thread_id,
                "Hydrating thread from DB"
            );
            self.maybe_hydrate_thread(message, external_thread_id)
                .await?;
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

        Ok((session, thread_id))
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
    ) -> Result<(), Error> {
        // Only hydrate UUID-shaped thread IDs (web gateway uses UUIDs)
        let thread_uuid = match Uuid::parse_str(external_thread_id) {
            Ok(id) => id,
            Err(_) => return Ok(()),
        };

        // Check if already in memory
        let session = self
            .session_manager
            .get_or_create_session(&message.user_id)
            .await;
        {
            let sess = session.lock().await;
            if sess.threads.contains_key(&thread_uuid) {
                return Ok(());
            }
        }

        // Load history from DB (may be empty for a newly created thread).
        let mut chat_messages: Vec<ChatMessage> = Vec::new();
        let msg_count;

        if let Some(store) = self.store() {
            let db_messages = store
                .list_conversation_messages_scoped(thread_uuid, &message.user_id, &message.channel)
                .await?;
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
            if sess.threads.contains_key(&thread_uuid) {
                return Ok(());
            }
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

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn module_compiles() {
        // TODO: Add higher-level hydration coverage with a stubbed backing
        // store and session-manager integration fixture.
    }
}
