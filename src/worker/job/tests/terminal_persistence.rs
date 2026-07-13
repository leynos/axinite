//! Tests for terminal-state persistence, rollback on failure, and budgets.

use std::sync::Arc;

use crate::context::{ContextManager, JobState};
use crate::db::Database;
use crate::testing::worker_harness::*;
use crate::tools::Tool;
use crate::worker::job::Worker;

#[tokio::test]
async fn test_mark_completed_twice_returns_error()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let worker = make_worker(vec![]).await?;

    worker
        .context_manager()
        .update_context(worker.job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)
        })
        .await
        .expect("failed to update context before completion test")
        .expect("failed to transition job to in-progress before completion test");

    worker
        .mark_completed()
        .await
        .expect("failed to mark job completed in duplicate-completion test");

    let ctx = worker
        .context_manager()
        .get_context(worker.job_id)
        .await
        .expect("failed to reload job context after first completion");
    assert_eq!(ctx.state, JobState::Completed);

    let result = worker.mark_completed().await;
    assert!(
        result.is_err(),
        "Completed → Completed transition should be rejected by state machine"
    );
    Ok(())
}

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
#[tokio::test]
async fn test_mark_completed_persists_result_before_returning()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (worker, store, _dir) = make_worker_with_store(vec![]).await?;

    worker
        .context_manager()
        .update_context(worker.job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)
        })
        .await
        .expect("failed to update context")
        .expect("failed to transition to in-progress");

    worker
        .mark_completed()
        .await
        .expect("failed to mark job completed");

    let job = store
        .get_job(worker.job_id)
        .await
        .expect("failed to load job")
        .expect("job should exist");
    assert_eq!(job.state, JobState::Completed);

    let events = store
        .list_job_events(worker.job_id, None, None)
        .await
        .expect("failed to list job events");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event_type, "result");
    assert_eq!(events[0].data["status"], "completed");
    Ok(())
}

#[cfg(feature = "libsql")]
async fn make_worker_with_unpersisted_store(
    tools: Vec<Arc<dyn Tool>>,
) -> anyhow::Result<(Worker, tempfile::TempDir)> {
    use crate::db::libsql::LibSqlBackend;
    use tempfile::tempdir;

    let registry = Arc::new(build_registry(tools).await);
    let cm = Arc::new(ContextManager::new(5));
    let job_id = cm.create_job("test", "test job").await?;
    let dir = tempdir()?;
    let path = dir.path().join("worker-test.db");
    let backend = LibSqlBackend::new_local(&path).await?;
    backend.run_migrations().await?;
    let store: Arc<dyn Database> = Arc::new(backend);
    let deps = base_deps(cm, registry, Some(store), None);

    Ok((Worker::new(job_id, deps), dir))
}

#[cfg(feature = "libsql")]
async fn assert_terminal_persistence_failure_rolls_back(
    transition: TerminalMethod,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (worker, _dir) = make_worker_with_unpersisted_store(vec![]).await?;
    transition_to_in_progress(&worker).await?;

    let result = transition.apply_transition(&worker).await;
    assert!(result.is_err(), "terminal persistence should fail");

    let ctx = worker.context_manager().get_context(worker.job_id).await?;
    assert_eq!(
        ctx.state,
        JobState::InProgress,
        "persistence failure should roll context back to InProgress"
    );
    Ok(())
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_mark_completed_rolls_back_context_when_persistence_fails()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    assert_terminal_persistence_failure_rolls_back(TerminalMethod::Completed).await
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_mark_failed_rolls_back_context_when_persistence_fails()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    assert_terminal_persistence_failure_rolls_back(TerminalMethod::Failed("test failure")).await
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_mark_stuck_rolls_back_context_when_persistence_fails()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    assert_terminal_persistence_failure_rolls_back(TerminalMethod::Stuck("test stuck")).await
}

#[tokio::test]
async fn test_token_budget_exceeded_fails_job()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let worker = make_worker(vec![]).await?;

    // Transition to InProgress (required for mark_failed)
    worker
        .context_manager()
        .update_context(worker.job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)
        })
        .await
        .expect("failed to update context before token-budget failure test")
        .expect("failed to transition job to in-progress before token-budget failure test");

    // Set a token budget
    worker
        .context_manager()
        .update_context(worker.job_id, |ctx| {
            ctx.max_tokens = 100;
        })
        .await
        .expect("failed to set max token budget for token-budget failure test");

    // Simulate adding tokens that exceed the budget
    let budget_result = worker
        .context_manager()
        .update_context(worker.job_id, |ctx| ctx.add_tokens(200))
        .await
        .expect("failed to apply token usage for token-budget failure test");

    assert!(
        budget_result.is_err(),
        "Should return error when token budget exceeded"
    );

    // Verify that mark_failed transitions job to Failed
    worker
        .mark_failed(&budget_result.unwrap_err())
        .await
        .expect("failed to mark job failed after token budget exceeded");
    let ctx = worker
        .context_manager()
        .get_context(worker.job_id)
        .await
        .expect("failed to reload job context after token-budget failure");
    assert_eq!(ctx.state, JobState::Failed);
    Ok(())
}

#[tokio::test]
async fn test_iteration_cap_marks_failed_not_stuck()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let worker = make_worker(vec![]).await?;

    // Transition to InProgress (required for mark_failed)
    worker
        .context_manager()
        .update_context(worker.job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)
        })
        .await
        .expect("failed to update context before iteration-cap failure test")
        .expect("failed to transition job to in-progress before iteration-cap failure test");

    // Simulate what the execution loop does when max_iterations is exceeded
    worker
        .mark_failed("Maximum iterations exceeded: job hit the iteration cap")
        .await
        .expect("failed to mark job failed after hitting the iteration cap");

    let ctx = worker
        .context_manager()
        .get_context(worker.job_id)
        .await
        .expect("failed to reload job context after iteration-cap failure");
    assert_eq!(
        ctx.state,
        JobState::Failed,
        "Iteration cap should transition to Failed, not Stuck"
    );
    Ok(())
}
