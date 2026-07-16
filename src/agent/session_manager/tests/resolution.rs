//! Tests for session creation and external-to-internal thread resolution.

use super::super::*;

#[tokio::test]
async fn test_get_or_create_session() {
    let manager = SessionManager::new();

    let session1 = manager.get_or_create_session("user-1").await;
    let session2 = manager.get_or_create_session("user-1").await;

    // Same user should get same session
    assert!(Arc::ptr_eq(&session1, &session2));

    let session3 = manager.get_or_create_session("user-2").await;
    assert!(!Arc::ptr_eq(&session1, &session3));
}

#[tokio::test]
async fn test_resolve_thread() {
    let manager = SessionManager::new();

    let (session1, thread1) = manager.resolve_thread("user-1", "cli", None).await;
    let (session2, thread2) = manager.resolve_thread("user-1", "cli", None).await;

    // Same channel+user should get same thread
    assert!(Arc::ptr_eq(&session1, &session2));
    assert_eq!(thread1, thread2);

    // Different channel should get different thread
    let (_, thread3) = manager.resolve_thread("user-1", "http", None).await;
    assert_ne!(thread1, thread3);
}

#[tokio::test]
async fn test_undo_manager() {
    let manager = SessionManager::new();
    let (_, thread_id) = manager.resolve_thread("user-1", "cli", None).await;

    let undo1 = manager.get_undo_manager(thread_id).await;
    let undo2 = manager.get_undo_manager(thread_id).await;

    assert!(Arc::ptr_eq(&undo1, &undo2));
}

#[tokio::test]
async fn test_resolve_thread_with_explicit_external_id() {
    let manager = SessionManager::new();

    // Two calls with the same explicit external thread ID should resolve
    // to the same internal thread.
    let (_, t1) = manager
        .resolve_thread("user-1", "gateway", Some("ext-abc"))
        .await;
    let (_, t2) = manager
        .resolve_thread("user-1", "gateway", Some("ext-abc"))
        .await;
    assert_eq!(t1, t2);

    // A different external ID on the same channel/user gets a new thread.
    let (_, t3) = manager
        .resolve_thread("user-1", "gateway", Some("ext-xyz"))
        .await;
    assert_ne!(t1, t3);
}

#[tokio::test]
async fn test_resolve_thread_none_vs_some_external_id() {
    let manager = SessionManager::new();

    // None external_thread_id is a distinct key from Some("ext-1").
    let (_, t_none) = manager.resolve_thread("user-1", "cli", None).await;
    let (_, t_some) = manager.resolve_thread("user-1", "cli", Some("ext-1")).await;
    assert_ne!(t_none, t_some);
}

#[tokio::test]
async fn test_resolve_thread_different_users_isolated() {
    let manager = SessionManager::new();

    let (_, t1) = manager
        .resolve_thread("user-a", "gateway", Some("same-ext"))
        .await;
    let (_, t2) = manager
        .resolve_thread("user-b", "gateway", Some("same-ext"))
        .await;

    // Same channel + same external ID but different users = different threads
    assert_ne!(t1, t2);
}

#[tokio::test]
async fn test_resolve_thread_different_channels_isolated() {
    let manager = SessionManager::new();

    let (_, t1) = manager
        .resolve_thread("user-1", "gateway", Some("thread-x"))
        .await;
    let (_, t2) = manager
        .resolve_thread("user-1", "telegram", Some("thread-x"))
        .await;

    // Same user + same external ID but different channels = different threads
    assert_ne!(t1, t2);
}

#[tokio::test]
async fn test_resolve_thread_stale_mapping_creates_new_thread() {
    let manager = SessionManager::new();

    // Create a thread normally
    let (session, original_tid) = manager
        .resolve_thread("user-1", "gateway", Some("ext-1"))
        .await;

    // Simulate the thread being removed from the session (e.g. pruned)
    {
        let mut sess = session.lock().await;
        sess.threads.remove(&original_tid);
    }

    // Next resolve should detect the stale mapping and create a fresh thread
    let (_, new_tid) = manager
        .resolve_thread("user-1", "gateway", Some("ext-1"))
        .await;
    assert_ne!(original_tid, new_tid);

    // The new thread should actually exist in the session
    let sess = session.lock().await;
    assert!(sess.threads.contains_key(&new_tid));
}

#[tokio::test]
async fn test_multiple_threads_per_user() {
    let manager = SessionManager::new();

    let (_, t1) = manager
        .resolve_thread("user-1", "gateway", Some("thread-a"))
        .await;
    let (_, t2) = manager
        .resolve_thread("user-1", "gateway", Some("thread-b"))
        .await;
    let (session, t3) = manager
        .resolve_thread("user-1", "gateway", Some("thread-c"))
        .await;

    // All three should be distinct
    assert_ne!(t1, t2);
    assert_ne!(t2, t3);
    assert_ne!(t1, t3);

    // All three should exist in the same session
    let sess = session.lock().await;
    assert!(sess.threads.contains_key(&t1));
    assert!(sess.threads.contains_key(&t2));
    assert!(sess.threads.contains_key(&t3));
}

#[tokio::test]
async fn test_resolve_thread_active_thread_set() {
    let manager = SessionManager::new();

    let (session, thread_id) = manager
        .resolve_thread("user-1", "gateway", Some("ext-1"))
        .await;

    // The resolved thread should be set as the active thread
    let sess = session.lock().await;
    assert_eq!(sess.active_thread, Some(thread_id));
}

#[tokio::test]
async fn test_resolve_thread_finds_existing_session_thread_by_uuid() {
    use crate::agent::session::{Session, Thread};

    let manager = SessionManager::new();
    let tid = Uuid::new_v4();

    // Simulate chat_new_thread_handler: create thread directly in session
    // without registering it in thread_map
    let session = Arc::new(Mutex::new(Session::new("user-direct")));
    {
        let mut sess = session.lock().await;
        let thread = Thread::with_id(tid, sess.id);
        sess.threads.insert(tid, thread);
    }
    {
        let mut sessions = manager.sessions.write().await;
        sessions.insert("user-direct".to_string(), Arc::clone(&session));
    }

    // resolve_thread should find the existing thread by UUID
    // instead of creating a duplicate
    let (_, resolved) = manager
        .resolve_thread("user-direct", "gateway", Some(&tid.to_string()))
        .await;
    assert_eq!(
        resolved, tid,
        "should reuse existing thread, not create a new one"
    );

    // Verify no duplicate threads were created
    let sess = session.lock().await;
    assert_eq!(
        sess.threads.len(),
        1,
        "should have exactly 1 thread, not a duplicate"
    );
}
