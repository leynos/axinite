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

#[cfg(all(test, feature = "libsql", feature = "test-helpers"))]
mod tests {
    use std::sync::Arc;

    use uuid::Uuid;

    use super::*;
    use crate::agent::{AgentDeps, SessionManager};
    use crate::channels::ChannelManager;
    use crate::config::{AgentConfig, SafetyConfig, SkillsConfig};
    use crate::db::libsql::LibSqlBackend;
    use crate::db::{Database, EnsureConversationParams, NativeConversationStore, NativeDatabase};
    use crate::error::DatabaseError;
    use crate::hooks::HookRegistry;
    use crate::safety::SafetyLayer;
    use crate::testing::StubLlm;
    use crate::tools::ToolRegistry;

    async fn local_backend() -> (Arc<LibSqlBackend>, tempfile::TempDir) {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let db_path = tempdir.path().join("hydration-test.db");
        let backend = LibSqlBackend::new_local(&db_path)
            .await
            .expect("local backend creation should succeed");
        NativeDatabase::run_migrations(&backend)
            .await
            .expect("migrations should succeed");
        (Arc::new(backend), tempdir)
    }

    fn make_agent(store: Option<Arc<dyn Database>>, session_manager: Arc<SessionManager>) -> Agent {
        let deps = AgentDeps {
            store,
            llm: Arc::new(StubLlm::new("ok")),
            cheap_llm: None,
            safety: Arc::new(SafetyLayer::new(&SafetyConfig {
                max_output_length: 100_000,
                injection_check_enabled: false,
            })),
            tools: Arc::new(ToolRegistry::new()),
            workspace: None,
            extension_manager: None,
            skill_registry: None,
            skill_catalog: None,
            skills_config: SkillsConfig::default(),
            hooks: Arc::new(HookRegistry::new()),
            cost_guard: Arc::new(crate::agent::cost_guard::CostGuard::new(
                crate::agent::cost_guard::CostGuardConfig::default(),
            )),
            sse_tx: None,
            http_interceptor: None,
            transcription: None,
            document_extraction: None,
        };

        Agent::new(
            AgentConfig::for_testing(),
            deps,
            Arc::new(ChannelManager::new()),
            None,
            None,
            None,
            None,
            Some(session_manager),
        )
    }

    fn test_message(thread_id: impl Into<String>) -> IncomingMessage {
        IncomingMessage::new("web", "user-1", "hello").with_thread(thread_id)
    }

    #[tokio::test]
    async fn maybe_hydrate_thread_skips_non_uuid_thread_ids() {
        let (backend, _tempdir) = local_backend().await;
        let session_manager = Arc::new(SessionManager::new());
        let agent = make_agent(
            Some(Arc::clone(&backend) as Arc<dyn Database>),
            Arc::clone(&session_manager),
        );

        let result = agent
            .maybe_hydrate_thread(&test_message("not-a-uuid"), "not-a-uuid")
            .await;

        assert!(result.is_ok(), "non-UUID thread IDs should be ignored");
    }

    #[tokio::test]
    async fn maybe_hydrate_thread_skips_existing_in_memory_threads() {
        let (backend, _tempdir) = local_backend().await;
        let session_manager = Arc::new(SessionManager::new());
        let agent = make_agent(
            Some(Arc::clone(&backend) as Arc<dyn Database>),
            Arc::clone(&session_manager),
        );
        let thread_id = Uuid::new_v4();
        let session = session_manager.get_or_create_session("user-1").await;

        {
            let mut sess = session.lock().await;
            let thread = crate::agent::session::Thread::with_id(thread_id, sess.id);
            sess.threads.insert(thread_id, thread);
        }

        let result = agent
            .maybe_hydrate_thread(&test_message(thread_id.to_string()), &thread_id.to_string())
            .await;

        assert!(
            result.is_ok(),
            "existing in-memory threads should not hit scoped DB hydration"
        );
    }

    #[tokio::test]
    async fn maybe_hydrate_thread_loads_messages_and_registers_thread() {
        let (backend, _tempdir) = local_backend().await;
        let session_manager = Arc::new(SessionManager::new());
        let agent = make_agent(
            Some(Arc::clone(&backend) as Arc<dyn Database>),
            Arc::clone(&session_manager),
        );
        let thread_id = Uuid::new_v4();

        backend
            .ensure_conversation(EnsureConversationParams {
                id: thread_id,
                channel: "web",
                user_id: "user-1",
                thread_id: Some(&thread_id.to_string()),
            })
            .await
            .expect("conversation should be ensured");
        backend
            .add_conversation_message(thread_id, "user", "hello")
            .await
            .expect("user message should be added");
        backend
            .add_conversation_message(thread_id, "assistant", "world")
            .await
            .expect("assistant message should be added");

        agent
            .maybe_hydrate_thread(&test_message(thread_id.to_string()), &thread_id.to_string())
            .await
            .expect("hydration should succeed");

        let session = session_manager.get_or_create_session("user-1").await;
        let sess = session.lock().await;
        let thread = sess
            .threads
            .get(&thread_id)
            .expect("hydrated thread should be present");
        let messages = thread.messages();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, "hello");
        assert_eq!(messages[1].content, "world");
        drop(sess);

        let (_resolved_session, resolved_thread_id) = session_manager
            .resolve_thread("user-1", "web", Some(&thread_id.to_string()))
            .await;
        assert_eq!(resolved_thread_id, thread_id);
    }

    #[tokio::test]
    async fn maybe_hydrate_thread_propagates_scoped_not_found() {
        let (backend, _tempdir) = local_backend().await;
        let session_manager = Arc::new(SessionManager::new());
        let agent = make_agent(
            Some(Arc::clone(&backend) as Arc<dyn Database>),
            Arc::clone(&session_manager),
        );
        let thread_id = Uuid::new_v4();

        let err = agent
            .maybe_hydrate_thread(&test_message(thread_id.to_string()), &thread_id.to_string())
            .await
            .expect_err("missing scoped conversation should propagate");

        assert!(
            matches!(
                err,
                Error::Database(DatabaseError::NotFound { ref entity, ref id })
                    if entity == "conversation" && id == &thread_id.to_string()
            ),
            "expected scoped NotFound error, got: {err:?}"
        );
    }
}
