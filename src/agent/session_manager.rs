//! Session manager for multi-user, multi-thread conversation handling.
//!
//! Maps external channel thread IDs to internal UUIDs and manages undo state
//! for each thread.

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{Mutex, RwLock};
use uuid::Uuid;

use crate::agent::session::Session;
use crate::agent::undo::UndoManager;
use crate::hooks::HookRegistry;

/// Warn when session count exceeds this threshold.
const SESSION_COUNT_WARNING_THRESHOLD: usize = 1000;

/// Key for mapping external thread IDs to internal ones.
#[derive(Clone, Hash, Eq, PartialEq)]
struct ThreadKey {
    user_id: String,
    channel: String,
    external_thread_id: Option<String>,
}

/// Manages sessions, threads, and undo state for all users.
pub struct SessionManager {
    sessions: RwLock<HashMap<String, Arc<Mutex<Session>>>>,
    thread_map: RwLock<HashMap<ThreadKey, Uuid>>,
    undo_managers: RwLock<HashMap<Uuid, Arc<Mutex<UndoManager>>>>,
    hooks: Option<Arc<HookRegistry>>,
}

impl SessionManager {
    /// Create a new session manager.
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            thread_map: RwLock::new(HashMap::new()),
            undo_managers: RwLock::new(HashMap::new()),
            hooks: None,
        }
    }

    /// Attach a hook registry for session lifecycle events.
    pub fn with_hooks(mut self, hooks: Arc<HookRegistry>) -> Self {
        self.hooks = Some(hooks);
        self
    }

    /// Get or create a session for a user.
    pub async fn get_or_create_session(&self, user_id: &str) -> Arc<Mutex<Session>> {
        // Fast path: check if session exists
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(user_id) {
                return Arc::clone(session);
            }
        }

        // Slow path: create new session
        let mut sessions = self.sessions.write().await;
        // Double-check after acquiring write lock
        if let Some(session) = sessions.get(user_id) {
            return Arc::clone(session);
        }

        let new_session = Session::new(user_id);
        let session_id = new_session.id.to_string();
        let session = Arc::new(Mutex::new(new_session));
        sessions.insert(user_id.to_string(), Arc::clone(&session));

        if sessions.len() >= SESSION_COUNT_WARNING_THRESHOLD && sessions.len() % 100 == 0 {
            tracing::warn!(
                "High session count: {} active sessions. \
                 Pruning runs every 10 minutes; consider reducing session_idle_timeout.",
                sessions.len()
            );
        }

        // Fire OnSessionStart hook (fire-and-forget)
        if let Some(ref hooks) = self.hooks {
            let hooks = hooks.clone();
            let uid = user_id.to_string();
            let sid = session_id;
            tokio::spawn(async move {
                use crate::hooks::HookEvent;
                let event = HookEvent::SessionStart {
                    user_id: uid,
                    session_id: sid,
                };
                if let Err(e) = hooks.run(&event).await {
                    tracing::warn!("OnSessionStart hook error: {}", e);
                }
            });
        }

        session
    }

    /// Resolve an external thread ID to an internal thread.
    ///
    /// Returns the session and thread ID. Creates both if they don't exist.
    pub async fn resolve_thread(
        &self,
        user_id: &str,
        channel: &str,
        external_thread_id: Option<&str>,
    ) -> (Arc<Mutex<Session>>, Uuid) {
        let session = self.get_or_create_session(user_id).await;

        let key = ThreadKey {
            user_id: user_id.to_string(),
            channel: channel.to_string(),
            external_thread_id: external_thread_id.map(String::from),
        };

        // Check if we have a mapping
        {
            let thread_map = self.thread_map.read().await;
            if let Some(&thread_id) = thread_map.get(&key) {
                // Verify thread still exists in session
                let sess = session.lock().await;
                if sess.threads.contains_key(&thread_id) {
                    return (Arc::clone(&session), thread_id);
                }
            }
        }

        // Check if external_thread_id is itself a known thread UUID that
        // exists in the session but was never registered in the thread_map
        // (e.g. created by chat_new_thread_handler or hydrated from DB).
        // We only adopt it if no thread_map entry maps to this UUID —
        // otherwise it belongs to a different channel scope.
        if let Some(ext_tid) = external_thread_id
            && let Ok(ext_uuid) = Uuid::parse_str(ext_tid)
        {
            let thread_map = self.thread_map.read().await;
            let mapped_elsewhere = thread_map.values().any(|&v| v == ext_uuid);
            drop(thread_map);

            if !mapped_elsewhere {
                let sess = session.lock().await;
                if sess.threads.contains_key(&ext_uuid) {
                    drop(sess);

                    let mut thread_map = self.thread_map.write().await;
                    // Re-check after acquiring write lock to prevent race condition
                    // where another task mapped this UUID between our read and write.
                    if !thread_map.values().any(|&v| v == ext_uuid) {
                        thread_map.insert(key, ext_uuid);
                        drop(thread_map);
                        // Ensure undo manager exists
                        let mut undo_managers = self.undo_managers.write().await;
                        undo_managers
                            .entry(ext_uuid)
                            .or_insert_with(|| Arc::new(Mutex::new(UndoManager::new())));
                        return (session, ext_uuid);
                    }
                    // If it was mapped elsewhere while we were unlocked, fall through
                    // to create a new thread, preserving channel isolation.
                }
            }
        }

        // Create new thread (always create a new one for a new key)
        let thread_id = {
            let mut sess = session.lock().await;
            let thread = sess.create_thread();
            thread.id
        };

        // Store mapping
        {
            let mut thread_map = self.thread_map.write().await;
            thread_map.insert(key, thread_id);
        }

        // Create undo manager for thread
        {
            let mut undo_managers = self.undo_managers.write().await;
            undo_managers.insert(thread_id, Arc::new(Mutex::new(UndoManager::new())));
        }

        (session, thread_id)
    }

    /// Register a hydrated thread so subsequent `resolve_thread` calls find it.
    ///
    /// Inserts into the thread_map and creates an undo manager for the thread.
    pub async fn register_thread(
        &self,
        user_id: &str,
        channel: &str,
        thread_id: Uuid,
        session: Arc<Mutex<Session>>,
    ) {
        let key = ThreadKey {
            user_id: user_id.to_string(),
            channel: channel.to_string(),
            external_thread_id: Some(thread_id.to_string()),
        };

        {
            let mut thread_map = self.thread_map.write().await;
            thread_map.insert(key, thread_id);
        }

        {
            let mut undo_managers = self.undo_managers.write().await;
            undo_managers
                .entry(thread_id)
                .or_insert_with(|| Arc::new(Mutex::new(UndoManager::new())));
        }

        // Ensure the session is tracked
        {
            let mut sessions = self.sessions.write().await;
            sessions.entry(user_id.to_string()).or_insert(session);
        }
    }

    /// Get undo manager for a thread.
    pub async fn get_undo_manager(&self, thread_id: Uuid) -> Arc<Mutex<UndoManager>> {
        // Fast path
        {
            let managers = self.undo_managers.read().await;
            if let Some(mgr) = managers.get(&thread_id) {
                return Arc::clone(mgr);
            }
        }

        // Create if missing
        let mut managers = self.undo_managers.write().await;
        // Double-check
        if let Some(mgr) = managers.get(&thread_id) {
            return Arc::clone(mgr);
        }

        let mgr = Arc::new(Mutex::new(UndoManager::new()));
        managers.insert(thread_id, Arc::clone(&mgr));
        mgr
    }

    /// Remove sessions that have been idle for longer than the given duration.
    ///
    /// Returns the number of sessions pruned.
    pub async fn prune_stale_sessions(&self, max_idle: std::time::Duration) -> usize {
        let cutoff = chrono::Utc::now() - chrono::TimeDelta::seconds(max_idle.as_secs() as i64);

        // Find stale sessions (user_id + session_id)
        let stale_sessions: Vec<(String, String)> = {
            let sessions = self.sessions.read().await;
            sessions
                .iter()
                .filter_map(|(user_id, session)| {
                    // Try to lock; skip if contended (someone is actively using it)
                    let sess = session.try_lock().ok()?;
                    if sess.last_active_at < cutoff {
                        Some((user_id.clone(), sess.id.to_string()))
                    } else {
                        None
                    }
                })
                .collect()
        };

        let stale_users: Vec<String> = stale_sessions
            .iter()
            .map(|(user_id, _)| user_id.clone())
            .collect();

        if stale_users.is_empty() {
            return 0;
        }

        // Collect thread IDs from stale sessions for cleanup
        let mut stale_thread_ids: Vec<Uuid> = Vec::new();
        {
            let sessions = self.sessions.read().await;
            for user_id in &stale_users {
                if let Some(session) = sessions.get(user_id)
                    && let Ok(sess) = session.try_lock()
                {
                    stale_thread_ids.extend(sess.threads.keys());
                }
            }
        }

        // Fire OnSessionEnd hooks for stale sessions (fire-and-forget)
        if let Some(ref hooks) = self.hooks {
            for (user_id, session_id) in &stale_sessions {
                let hooks = hooks.clone();
                let uid = user_id.clone();
                let sid = session_id.clone();
                tokio::spawn(async move {
                    use crate::hooks::HookEvent;
                    let event = HookEvent::SessionEnd {
                        user_id: uid,
                        session_id: sid,
                    };
                    if let Err(e) = hooks.run(&event).await {
                        tracing::warn!("OnSessionEnd hook error: {}", e);
                    }
                });
            }
        }

        // Remove sessions
        let count = {
            let mut sessions = self.sessions.write().await;
            let before = sessions.len();
            for user_id in &stale_users {
                sessions.remove(user_id);
            }
            before - sessions.len()
        };

        // Clean up thread mappings that point to stale sessions
        {
            let mut thread_map = self.thread_map.write().await;
            thread_map.retain(|key, _| !stale_users.contains(&key.user_id));
        }

        // Clean up undo managers for stale threads
        {
            let mut undo_managers = self.undo_managers.write().await;
            for thread_id in &stale_thread_ids {
                undo_managers.remove(thread_id);
            }
        }

        if count > 0 {
            tracing::info!(
                "Pruned {} stale session(s) (idle > {}s)",
                count,
                max_idle.as_secs()
            );
        }

        count
    }
}

impl Default for SessionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
