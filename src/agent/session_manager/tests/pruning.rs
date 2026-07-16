//! Tests for stale-session pruning and associated state cleanup.

use super::super::*;

#[tokio::test]
async fn test_prune_stale_sessions() {
    let manager = SessionManager::new();

    // Create two sessions and resolve threads (which updates last_active_at)
    let (_, _thread_id) = manager.resolve_thread("user-active", "cli", None).await;
    let (s2, _thread_id) = manager.resolve_thread("user-stale", "cli", None).await;

    // Backdate the stale session's last_active_at AFTER thread creation
    {
        let mut sess = s2.lock().await;
        sess.last_active_at = chrono::Utc::now() - chrono::TimeDelta::seconds(86400 * 10); // 10 days ago
    }

    // Prune with 7-day timeout
    let pruned = manager
        .prune_stale_sessions(std::time::Duration::from_secs(86400 * 7))
        .await;
    assert_eq!(pruned, 1);

    // Active session should still exist
    let sessions = manager.sessions.read().await;
    assert!(sessions.contains_key("user-active"));
    assert!(!sessions.contains_key("user-stale"));
}

#[tokio::test]
async fn test_prune_no_stale_sessions() {
    let manager = SessionManager::new();
    let _s1 = manager.get_or_create_session("user-1").await;

    // Nothing should be pruned when timeout is long
    let pruned = manager
        .prune_stale_sessions(std::time::Duration::from_secs(86400 * 365))
        .await;
    assert_eq!(pruned, 0);
}

#[tokio::test]
async fn test_prune_cleans_thread_map_and_undo_managers() {
    let manager = SessionManager::new();

    let (stale_session, stale_tid) = manager.resolve_thread("user-stale", "cli", None).await;

    // Backdate the session
    {
        let mut sess = stale_session.lock().await;
        sess.last_active_at = chrono::Utc::now() - chrono::TimeDelta::seconds(86400 * 30);
    }

    // Verify thread_map and undo_managers have entries
    {
        let tm = manager.thread_map.read().await;
        assert!(!tm.is_empty());
    }
    {
        let um = manager.undo_managers.read().await;
        assert!(um.contains_key(&stale_tid));
    }

    let pruned = manager
        .prune_stale_sessions(std::time::Duration::from_secs(86400 * 7))
        .await;
    assert_eq!(pruned, 1);

    // Thread map and undo managers should be cleaned up
    {
        let tm = manager.thread_map.read().await;
        assert!(tm.is_empty());
    }
    {
        let um = manager.undo_managers.read().await;
        assert!(!um.contains_key(&stale_tid));
    }
}
