//! Concurrent session stress tests (QA Plan P3 - 4.2).

use super::super::*;

#[tokio::test]
async fn concurrent_get_or_create_same_user_returns_same_session() {
    let manager = Arc::new(SessionManager::new());

    let handles: Vec<_> = (0..30)
        .map(|_| {
            let mgr = Arc::clone(&manager);
            tokio::spawn(async move { mgr.get_or_create_session("shared-user").await })
        })
        .collect();

    let mut sessions = Vec::new();
    for handle in handles {
        sessions.push(handle.await.expect("task should not panic"));
    }

    // All 30 must return the *same* Arc (double-checked locking guarantee).
    for s in &sessions {
        assert!(Arc::ptr_eq(&sessions[0], s));
    }
}

#[tokio::test]
async fn concurrent_resolve_thread_distinct_users_no_cross_talk() {
    let manager = Arc::new(SessionManager::new());

    let handles: Vec<_> = (0..20)
        .map(|i| {
            let mgr = Arc::clone(&manager);
            tokio::spawn(async move {
                let user = format!("user-{i}");
                let (session, tid) = mgr.resolve_thread(&user, "gateway", None).await;
                (user, session, tid)
            })
        })
        .collect();

    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.expect("task should not panic"));
    }

    // All thread IDs must be unique.
    let tids: std::collections::HashSet<_> = results.iter().map(|(_, _, t)| *t).collect();
    assert_eq!(tids.len(), 20);

    // Each session should contain exactly 1 thread (its own).
    for (_, session, tid) in &results {
        let sess = session.lock().await;
        assert!(sess.threads.contains_key(tid));
        assert_eq!(sess.threads.len(), 1);
    }
}

#[tokio::test]
async fn concurrent_resolve_thread_same_user_different_channels() {
    let manager = Arc::new(SessionManager::new());
    let channels = ["gateway", "telegram", "slack", "cli", "repl"];

    let handles: Vec<_> = channels
        .iter()
        .map(|ch| {
            let mgr = Arc::clone(&manager);
            let channel = ch.to_string();
            tokio::spawn(async move {
                let (session, tid) = mgr.resolve_thread("multi-ch", &channel, None).await;
                (channel, session, tid)
            })
        })
        .collect();

    let mut results = Vec::new();
    for handle in handles {
        results.push(handle.await.expect("task should not panic"));
    }

    // All 5 threads must be unique (different channels = different keys).
    let tids: std::collections::HashSet<_> = results.iter().map(|(_, _, t)| *t).collect();
    assert_eq!(tids.len(), 5);

    // All threads should live in the same session.
    let sess = results[0].1.lock().await;
    assert_eq!(sess.threads.len(), 5);
}

#[tokio::test]
async fn concurrent_get_undo_manager_same_thread_returns_same_arc() {
    let manager = Arc::new(SessionManager::new());
    let (_, tid) = manager.resolve_thread("undo-user", "gateway", None).await;

    let handles: Vec<_> = (0..20)
        .map(|_| {
            let mgr = Arc::clone(&manager);
            tokio::spawn(async move { mgr.get_undo_manager(tid).await })
        })
        .collect();

    let mut managers = Vec::new();
    for handle in handles {
        managers.push(handle.await.expect("task should not panic"));
    }

    // All 20 must point to the same UndoManager.
    for m in &managers {
        assert!(Arc::ptr_eq(&managers[0], m));
    }
}
