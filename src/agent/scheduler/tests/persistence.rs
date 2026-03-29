use super::*;
use crate::db::libsql::LibSqlBackend;

async fn make_test_scheduler_with_store(
    max_tokens_per_job: u64,
) -> (Scheduler, Arc<dyn Database>, tempfile::TempDir) {
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
    let dir = tempdir().expect("failed to create tempdir");
    let path = dir.path().join("scheduler-test.db");
    let backend = LibSqlBackend::new_local(&path)
        .await
        .expect("failed to open libsql backend");
    backend
        .run_migrations()
        .await
        .expect("failed to run migrations");
    let store: Arc<dyn Database> = Arc::new(backend);

    (
        Scheduler::new(config, cm, llm, safety, tools, Some(store.clone()), hooks),
        store,
        dir,
    )
}

async fn register_job_in_scheduler(sched: &Scheduler, store: &Arc<dyn Database>, job_id: Uuid) {
    let ctx = sched
        .context_manager
        .get_context(job_id)
        .await
        .expect("failed to get context");
    store
        .save_job(&ctx)
        .await
        .expect("failed to save job to store");

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
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_stop_persists_cancellation_before_returning() {
    let (sched, store, _dir) = make_test_scheduler_with_store(1000).await;
    let job_id = sched
        .context_manager
        .create_job_for_user("user1", "test", "desc")
        .await
        .expect("failed to create job");
    sched
        .context_manager
        .update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .expect("failed to update context")
        .expect("failed to transition to in-progress");

    register_job_in_scheduler(&sched, &store, job_id).await;

    sched
        .stop(job_id, "Stopped by scheduler")
        .await
        .expect("failed to stop job");

    let job = store
        .get_job(job_id)
        .await
        .expect("failed to load job")
        .expect("job should exist");
    assert_eq!(job.state, JobState::Cancelled);
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn test_stop_does_not_overwrite_completed_jobs() {
    let (sched, store, _dir) = make_test_scheduler_with_store(1000).await;
    let job_id = sched
        .context_manager
        .create_job_for_user("user1", "test", "desc")
        .await
        .expect("failed to create job");
    sched
        .context_manager
        .update_context(job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)
                .expect("failed to transition to in-progress");
            ctx.transition_to(JobState::Completed, None)
        })
        .await
        .expect("failed to update context")
        .expect("failed to transition to completed");

    register_job_in_scheduler(&sched, &store, job_id).await;

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
        .await
        .expect("failed to load job")
        .expect("job should exist");
    assert_eq!(job.state, JobState::Completed);
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
