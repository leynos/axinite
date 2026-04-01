//! Persistence-focused scheduler tests covering libSQL-backed cancellation
//! durability, shutdown recovery, and stop-path state handling.

use super::*;
use crate::db::libsql::LibSqlBackend;
use anyhow::{Result, anyhow};

async fn make_test_scheduler_with_store(
    max_tokens_per_job: u64,
) -> Result<(Scheduler, Arc<dyn Database>, tempfile::TempDir)> {
    use tempfile::tempdir;

    let config = make_test_config(max_tokens_per_job);
    let cm = Arc::new(ContextManager::new(5));
    let llm: Arc<dyn LlmProvider> = Arc::new(StubLlm);
    let safety = Arc::new(SafetyLayer::new(&SafetyConfig {
        max_output_length: 100_000,
        injection_check_enabled: false,
    }));
    let tools = Arc::new(ToolRegistry::new());
    let hooks = Arc::new(HookRegistry::default());
    let dir = tempdir()?;
    let path = dir.path().join("scheduler-test.db");
    let backend = LibSqlBackend::new_local(&path).await?;
    backend.run_migrations().await?;
    let store: Arc<dyn Database> = Arc::new(backend);

    Ok((
        Scheduler::new(config, cm, llm, safety, tools, Some(store.clone()), hooks),
        store,
        dir,
    ))
}

async fn register_job_in_scheduler(
    sched: &Scheduler,
    store: &Arc<dyn Database>,
    job_id: Uuid,
) -> Result<()> {
    let ctx = sched.context_manager.get_context(job_id).await?;
    store.save_job(&ctx).await?;

    let (tx, mut rx) = mpsc::channel(1);
    let handle = tokio::spawn(async move {
        let _ = rx.recv().await;
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    sched
        .jobs
        .write()
        .await
        .insert(job_id, ScheduledJob { handle, tx });
    Ok(())
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_stop_persists_cancellation_before_returning() -> Result<()> {
    let (sched, store, _dir) = make_test_scheduler_with_store(1000).await?;
    let job_id = sched
        .context_manager
        .create_job_for_user("user1", "test", "desc")
        .await?;
    sched
        .context_manager
        .update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .map_err(|error| anyhow!(error))?
        .map_err(|error| anyhow!(error))?;

    register_job_in_scheduler(&sched, &store, job_id).await?;

    sched.stop(job_id, "Stopped by scheduler").await?;

    let job = store
        .get_job(job_id)
        .await?
        .ok_or_else(|| anyhow!("job should exist"))?;
    assert_eq!(job.state, JobState::Cancelled);
    Ok(())
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_stop_does_not_overwrite_completed_jobs() -> Result<()> {
    let (sched, store, _dir) = make_test_scheduler_with_store(1000).await?;
    let job_id = sched
        .context_manager
        .create_job_for_user("user1", "test", "desc")
        .await?;
    sched
        .context_manager
        .update_context(job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)
                .expect("failed to transition to in-progress");
            ctx.transition_to(JobState::Completed, None)
        })
        .await
        .map_err(|error| anyhow!(error))?
        .map_err(|error| anyhow!(error))?;

    register_job_in_scheduler(&sched, &store, job_id).await?;

    let error = sched
        .stop(job_id, "Cancelled by user")
        .await
        .expect_err("completed job should reject cancellation");
    assert!(matches!(
        error,
        JobError::InvalidTransition {
            target,
            ..
        } if target == JobState::Cancelled.to_string()
    ));

    let job = store
        .get_job(job_id)
        .await?
        .ok_or_else(|| anyhow!("job should exist"))?;
    assert_eq!(job.state, JobState::Completed);
    Ok(())
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_stop_all_timeout_transitions_context_and_db_to_cancelled() -> Result<()> {
    let (sched, store, _dir) = make_test_scheduler_with_store(1000).await?;
    let job_id = sched
        .context_manager
        .create_job_for_user("user1", "test", "desc")
        .await?;
    sched
        .context_manager
        .update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .map_err(|error| anyhow!(error))?
        .map_err(|error| anyhow!(error))?;

    let ctx = sched.context_manager.get_context(job_id).await?;
    store.save_job(&ctx).await?;

    let (tx, rx) = mpsc::channel(1);
    tx.send(WorkerMessage::Stop)
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    let handle = tokio::spawn(async move {
        let _held_receiver = rx;
        tokio::time::sleep(Duration::from_secs(60)).await;
    });
    sched
        .jobs
        .write()
        .await
        .insert(job_id, ScheduledJob { handle, tx });

    sched.stop_all().await;

    let ctx = sched.context_manager.get_context(job_id).await?;
    assert_eq!(ctx.state, JobState::Cancelled);

    let job = store
        .get_job(job_id)
        .await?
        .ok_or_else(|| anyhow!("job should exist"))?;
    assert_eq!(job.state, JobState::Cancelled);
    assert!(!sched.is_running(job_id).await);
    Ok(())
}

#[tokio::test]
async fn test_stop_returns_not_found_for_unknown_job() {
    let sched = make_test_scheduler(1000);
    let job_id = Uuid::new_v4();

    let error = sched
        .stop(job_id, "Stopped by scheduler")
        .await
        .expect_err("unknown job should not stop successfully");
    assert!(matches!(error, JobError::NotFound { id } if id == job_id));
}
