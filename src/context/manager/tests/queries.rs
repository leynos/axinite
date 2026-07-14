//! Unit tests for context manager queries: stuck jobs, active counts,
//! per-user filtering, and summaries.

use crate::context::manager::ContextManager;

#[tokio::test]
async fn find_stuck_jobs_returns_only_stuck() {
    let manager = ContextManager::new(10);

    let id1 = manager.create_job("Job 1", "desc").await.unwrap();
    let id2 = manager.create_job("Job 2", "desc").await.unwrap();
    let id3 = manager.create_job("Job 3", "desc").await.unwrap();

    // Transition id1 and id2 to InProgress, then mark id2 as stuck
    for id in [id1, id2, id3] {
        manager
            .update_context(id, |ctx| {
                ctx.transition_to(crate::context::JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();
    }
    manager
        .update_context(id2, |ctx| ctx.mark_stuck("timed out"))
        .await
        .unwrap()
        .unwrap();

    let stuck = manager.find_stuck_jobs().await;
    assert_eq!(stuck.len(), 1);
    assert_eq!(stuck[0], id2);
}

#[tokio::test]
async fn find_stuck_contexts_returns_only_stuck_contexts() {
    let manager = ContextManager::new(10);
    let stuck_id = manager.create_job("stuck", "desc").await.unwrap();
    let active_id = manager.create_job("active", "desc").await.unwrap();

    manager
        .update_context(stuck_id, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();
    manager
        .update_context(stuck_id, |ctx| ctx.mark_stuck("timeout"))
        .await
        .unwrap()
        .unwrap();

    manager
        .update_context(active_id, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();

    let stuck_contexts = manager.find_stuck_contexts().await;

    assert_eq!(stuck_contexts.len(), 1);
    assert_eq!(stuck_contexts[0].job_id, stuck_id);
}

#[tokio::test]
async fn active_count_tracks_non_terminal_jobs() {
    let manager = ContextManager::new(10);

    let id1 = manager.create_job("J1", "d").await.unwrap();
    let id2 = manager.create_job("J2", "d").await.unwrap();

    // Both pending (active)
    assert_eq!(manager.active_count().await, 2);

    // Transition id1 through to Failed (terminal)
    manager
        .update_context(id1, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();
    manager
        .update_context(id1, |ctx| {
            ctx.transition_to(crate::context::JobState::Failed, None)
        })
        .await
        .unwrap()
        .unwrap();

    // id1 is terminal, id2 still pending
    assert_eq!(manager.active_count().await, 1);

    // Transition id2 to cancelled
    manager
        .update_context(id2, |ctx| {
            ctx.transition_to(crate::context::JobState::Cancelled, None)
        })
        .await
        .unwrap()
        .unwrap();

    assert_eq!(manager.active_count().await, 0);
}

#[tokio::test]
async fn active_jobs_for_filters_by_user() {
    let manager = ContextManager::new(10);

    manager
        .create_job_for_user("alice", "A1", "d")
        .await
        .unwrap();
    manager
        .create_job_for_user("alice", "A2", "d")
        .await
        .unwrap();
    let bob_id = manager.create_job_for_user("bob", "B1", "d").await.unwrap();

    assert_eq!(manager.active_jobs_for("alice").await.len(), 2);
    assert_eq!(manager.active_jobs_for("bob").await.len(), 1);
    assert_eq!(manager.active_jobs_for("nobody").await.len(), 0);

    // Make bob's job terminal
    manager
        .update_context(bob_id, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();
    manager
        .update_context(bob_id, |ctx| {
            ctx.transition_to(crate::context::JobState::Failed, None)
        })
        .await
        .unwrap()
        .unwrap();

    assert_eq!(manager.active_jobs_for("bob").await.len(), 0);
    // But all_jobs_for still shows it
    assert_eq!(manager.all_jobs_for("bob").await.len(), 1);
}

#[tokio::test]
async fn summary_counts_states_correctly() {
    let manager = ContextManager::new(10);

    let id1 = manager.create_job("J1", "d").await.unwrap();
    let id2 = manager.create_job("J2", "d").await.unwrap();
    let id3 = manager.create_job("J3", "d").await.unwrap();

    // id1: Pending -> InProgress -> Completed
    manager
        .update_context(id1, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();
    manager
        .update_context(id1, |ctx| {
            ctx.transition_to(crate::context::JobState::Completed, None)
        })
        .await
        .unwrap()
        .unwrap();

    // id2: Pending -> InProgress -> Failed
    manager
        .update_context(id2, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();
    manager
        .update_context(id2, |ctx| {
            ctx.transition_to(crate::context::JobState::Failed, None)
        })
        .await
        .unwrap()
        .unwrap();

    // id3: stays Pending

    let s = manager.summary().await;
    assert_eq!(s.total, 3);
    assert_eq!(s.pending, 1);
    assert_eq!(s.completed, 1);
    assert_eq!(s.failed, 1);
    assert_eq!(s.in_progress, 0);
    assert_eq!(s.stuck, 0);
    assert_eq!(s.cancelled, 0);
    assert_eq!(s.submitted, 0);
    assert_eq!(s.accepted, 0);

    // Suppress unused field warning
    let _ = id3;
}

#[tokio::test]
async fn summary_for_scopes_to_user() {
    let manager = ContextManager::new(10);

    manager
        .create_job_for_user("alice", "A1", "d")
        .await
        .unwrap();
    let bob_id = manager.create_job_for_user("bob", "B1", "d").await.unwrap();

    // Transition bob's job to InProgress
    manager
        .update_context(bob_id, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();

    let alice_summary = manager.summary_for("alice").await;
    assert_eq!(alice_summary.total, 1);
    assert_eq!(alice_summary.pending, 1);
    assert_eq!(alice_summary.in_progress, 0);

    let bob_summary = manager.summary_for("bob").await;
    assert_eq!(bob_summary.total, 1);
    assert_eq!(bob_summary.pending, 0);
    assert_eq!(bob_summary.in_progress, 1);

    let nobody_summary = manager.summary_for("nobody").await;
    assert_eq!(nobody_summary.total, 0);
}

#[tokio::test]
async fn all_jobs_returns_all_regardless_of_state() {
    let manager = ContextManager::new(10);

    let id1 = manager.create_job("J1", "d").await.unwrap();
    manager.create_job("J2", "d").await.unwrap();

    // Make id1 terminal
    manager
        .update_context(id1, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();
    manager
        .update_context(id1, |ctx| {
            ctx.transition_to(crate::context::JobState::Failed, None)
        })
        .await
        .unwrap()
        .unwrap();

    // all_jobs includes terminal, active_jobs does not
    assert_eq!(manager.all_jobs().await.len(), 2);
    assert_eq!(manager.active_jobs().await.len(), 1);
}
