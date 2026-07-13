//! Tests for registering hydrated threads with the session manager.

use super::super::*;

#[tokio::test]
async fn test_register_thread() {
    use crate::agent::session::{Session, Thread};

    let manager = SessionManager::new();
    let thread_id = Uuid::new_v4();

    // Create a session with a hydrated thread
    let session = Arc::new(Mutex::new(Session::new("user-hydrate")));
    {
        let mut sess = session.lock().await;
        let thread = Thread::with_id(thread_id, sess.id);
        sess.threads.insert(thread_id, thread);
        sess.active_thread = Some(thread_id);
    }

    // Register the thread
    manager
        .register_thread("user-hydrate", "gateway", thread_id, Arc::clone(&session))
        .await;

    // resolve_thread should find it (using the UUID as external_thread_id)
    let (resolved_session, resolved_tid) = manager
        .resolve_thread("user-hydrate", "gateway", Some(&thread_id.to_string()))
        .await;
    assert_eq!(resolved_tid, thread_id);

    // Should be the same session object
    let sess = resolved_session.lock().await;
    assert!(sess.threads.contains_key(&thread_id));
}

#[tokio::test]
async fn test_register_thread_preserves_uuid_on_resolve() {
    use crate::agent::session::{Session, Thread};

    let manager = SessionManager::new();
    let known_uuid = Uuid::new_v4();

    let session = Arc::new(Mutex::new(Session::new("user-web")));
    let session_id = {
        let sess = session.lock().await;
        sess.id
    };

    // Simulate hydration: create thread with a known UUID
    {
        let mut sess = session.lock().await;
        let thread = Thread::with_id(known_uuid, session_id);
        sess.threads.insert(known_uuid, thread);
    }

    // Register it
    manager
        .register_thread("user-web", "gateway", known_uuid, Arc::clone(&session))
        .await;

    // resolve_thread with UUID as external_thread_id MUST return the same UUID,
    // not mint a new one (this was the root cause of the "wrong conversation" bug)
    let (_, resolved) = manager
        .resolve_thread("user-web", "gateway", Some(&known_uuid.to_string()))
        .await;
    assert_eq!(resolved, known_uuid);
}

#[tokio::test]
async fn test_register_thread_idempotent() {
    use crate::agent::session::{Session, Thread};

    let manager = SessionManager::new();
    let tid = Uuid::new_v4();

    let session = Arc::new(Mutex::new(Session::new("user-idem")));
    {
        let mut sess = session.lock().await;
        let thread = Thread::with_id(tid, sess.id);
        sess.threads.insert(tid, thread);
    }

    // Register twice
    manager
        .register_thread("user-idem", "gateway", tid, Arc::clone(&session))
        .await;
    manager
        .register_thread("user-idem", "gateway", tid, Arc::clone(&session))
        .await;

    // Should still resolve to the same thread
    let (_, resolved) = manager
        .resolve_thread("user-idem", "gateway", Some(&tid.to_string()))
        .await;
    assert_eq!(resolved, tid);
}

#[tokio::test]
async fn test_register_thread_creates_undo_manager() {
    use crate::agent::session::{Session, Thread};

    let manager = SessionManager::new();
    let tid = Uuid::new_v4();

    let session = Arc::new(Mutex::new(Session::new("user-undo")));
    {
        let mut sess = session.lock().await;
        let thread = Thread::with_id(tid, sess.id);
        sess.threads.insert(tid, thread);
    }

    manager
        .register_thread("user-undo", "gateway", tid, Arc::clone(&session))
        .await;

    // Undo manager should exist for the registered thread
    let undo = manager.get_undo_manager(tid).await;
    let undo2 = manager.get_undo_manager(tid).await;
    assert!(Arc::ptr_eq(&undo, &undo2));
}

#[tokio::test]
async fn test_register_thread_stores_session() {
    use crate::agent::session::{Session, Thread};

    let manager = SessionManager::new();
    let tid = Uuid::new_v4();

    let session = Arc::new(Mutex::new(Session::new("user-new")));
    {
        let mut sess = session.lock().await;
        let thread = Thread::with_id(tid, sess.id);
        sess.threads.insert(tid, thread);
    }

    // The user has no session yet in the manager
    {
        let sessions = manager.sessions.read().await;
        assert!(!sessions.contains_key("user-new"));
    }

    manager
        .register_thread("user-new", "gateway", tid, Arc::clone(&session))
        .await;

    // Now the session should be tracked
    {
        let sessions = manager.sessions.read().await;
        assert!(sessions.contains_key("user-new"));
    }
}

#[tokio::test]
async fn test_register_then_resolve_different_channel_creates_new() {
    use crate::agent::session::{Session, Thread};

    let manager = SessionManager::new();
    let tid = Uuid::new_v4();

    let session = Arc::new(Mutex::new(Session::new("user-cross")));
    {
        let mut sess = session.lock().await;
        let thread = Thread::with_id(tid, sess.id);
        sess.threads.insert(tid, thread);
    }

    // Register on "gateway" channel
    manager
        .register_thread("user-cross", "gateway", tid, Arc::clone(&session))
        .await;

    // Resolve on a different channel with the same UUID string should NOT
    // find the registered thread (channel is part of the key)
    let (_, resolved) = manager
        .resolve_thread("user-cross", "telegram", Some(&tid.to_string()))
        .await;
    assert_ne!(resolved, tid);
}
