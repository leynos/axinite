//! Persistence-focused scheduler tests covering libSQL-backed cancellation
//! durability, shutdown recovery, and stop-path state handling.

use super::*;
use anyhow::{Result, anyhow};

#[cfg(feature = "libsql")]
use crate::db::libsql::LibSqlBackend;

#[cfg(feature = "libsql")]
fn test_db_path(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("scheduler-test.db")
}

#[cfg(feature = "libsql")]
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
    let path = test_db_path(&dir);
    let backend = LibSqlBackend::new_local(&path).await?;
    backend.run_migrations().await?;
    let store: Arc<dyn Database> = Arc::new(backend);

    Ok((
        Scheduler::new(config, cm, llm, safety, tools, Some(store.clone()), hooks),
        store,
        dir,
    ))
}

#[cfg(feature = "libsql")]
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
    sched.jobs.write().await.insert(
        job_id,
        ScheduledJob {
            handle,
            tx,
            pending_cancel_persist: false,
        },
    );
    Ok(())
}

#[cfg(feature = "libsql")]
async fn install_cancel_failure_trigger(dir: &tempfile::TempDir) -> Result<()> {
    let db = libsql::Builder::new_local(test_db_path(dir))
        .build()
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    let conn = db.connect().map_err(|error| anyhow!(error.to_string()))?;
    conn.execute(
        r#"
        CREATE TRIGGER fail_cancel_status_update
        BEFORE UPDATE OF status ON agent_jobs
        WHEN NEW.status = 'cancelled' AND OLD.status != 'cancelled'
        BEGIN
            SELECT RAISE(FAIL, 'forced cancel persistence failure');
        END
        "#,
        (),
    )
    .await
    .map_err(|error| anyhow!(error.to_string()))?;
    Ok(())
}

#[cfg(feature = "libsql")]
async fn drop_cancel_failure_trigger(dir: &tempfile::TempDir) -> Result<()> {
    let db = libsql::Builder::new_local(test_db_path(dir))
        .build()
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
    let conn = db.connect().map_err(|error| anyhow!(error.to_string()))?;
    conn.execute("DROP TRIGGER fail_cancel_status_update", ())
        .await
        .map_err(|error| anyhow!(error.to_string()))?;
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
    assert_eq!(
        store.get_agent_job_failure_reason(job_id).await?,
        Some("Stopped by scheduler".to_string())
    );
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
            ctx.transition_to(JobState::InProgress, None)?;
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
        } if target == JobState::Cancelled
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
    sched.jobs.write().await.insert(
        job_id,
        ScheduledJob {
            handle,
            tx,
            pending_cancel_persist: false,
        },
    );

    sched.stop_all().await;

    let ctx = sched.context_manager.get_context(job_id).await?;
    assert_eq!(ctx.state, JobState::Cancelled);

    let job = store
        .get_job(job_id)
        .await?
        .ok_or_else(|| anyhow!("job should exist"))?;
    assert_eq!(job.state, JobState::Cancelled);
    assert_eq!(
        store.get_agent_job_failure_reason(job_id).await?,
        Some("Stopped by scheduler".to_string())
    );
    assert!(!sched.is_running(job_id).await);
    Ok(())
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_stop_retry_persists_cancelled_after_initial_store_failure() -> Result<()> {
    let (sched, store, dir) = make_test_scheduler_with_store(1000).await?;
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

    let (tx, mut rx) = mpsc::channel(1);
    let handle = tokio::spawn(async move {
        let _ = rx.recv().await;
    });
    sched.jobs.write().await.insert(
        job_id,
        ScheduledJob {
            handle,
            tx,
            pending_cancel_persist: false,
        },
    );

    install_cancel_failure_trigger(&dir).await?;

    let error = sched
        .stop(job_id, "Stopped by scheduler")
        .await
        .expect_err("first cancellation persistence should fail");
    assert!(matches!(error, JobError::PersistenceError { id, .. } if id == job_id));

    sched.cleanup_finished().await;
    assert!(sched.is_running(job_id).await);

    drop_cancel_failure_trigger(&dir).await?;

    sched.stop(job_id, "Stopped by scheduler").await?;

    let job = store
        .get_job(job_id)
        .await?
        .ok_or_else(|| anyhow!("job should exist"))?;
    assert_eq!(job.state, JobState::Cancelled);
    assert_eq!(
        store.get_agent_job_failure_reason(job_id).await?,
        Some("Stopped by scheduler".to_string())
    );
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
