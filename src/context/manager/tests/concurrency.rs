//! Concurrent stress tests for the context manager (QA Plan P3 - 4.2).

use crate::context::manager::ContextManager;
use crate::error::JobError;

#[tokio::test]
async fn concurrent_creates_produce_unique_ids() {
    let manager = std::sync::Arc::new(ContextManager::new(100));

    let handles: Vec<_> = (0..50)
        .map(|i| {
            let mgr = std::sync::Arc::clone(&manager);
            tokio::spawn(async move {
                mgr.create_job(format!("Job {i}"), format!("Desc {i}"))
                    .await
            })
        })
        .collect();

    let mut ids = std::collections::HashSet::new();
    for handle in handles {
        let result = handle.await.expect("task should not panic");
        let job_id = result.expect("create_job should succeed");
        assert!(ids.insert(job_id), "Duplicate job ID: {job_id}");
    }

    assert_eq!(ids.len(), 50);
    assert_eq!(manager.all_jobs().await.len(), 50);
}

#[tokio::test]
async fn concurrent_creates_respect_max_jobs_limit() {
    // max_jobs = 5, but create_job only counts *active* jobs (InProgress).
    // Pending jobs don't count against the limit, so we need to transition them.
    let manager = std::sync::Arc::new(ContextManager::new(5));

    // First, create 5 jobs and make them active.
    for i in 0..5 {
        let id = manager
            .create_job(format!("Job {i}"), "desc")
            .await
            .unwrap();
        manager
            .update_context(id, |ctx| {
                ctx.transition_to(crate::context::JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();
    }

    // Now try to create 10 more concurrently -- all should fail.
    let handles: Vec<_> = (0..10)
        .map(|i| {
            let mgr = std::sync::Arc::clone(&manager);
            tokio::spawn(async move { mgr.create_job(format!("Overflow {i}"), "desc").await })
        })
        .collect();

    for handle in handles {
        let result = handle.await.expect("task should not panic");
        assert!(
            matches!(result, Err(JobError::MaxJobsExceeded { .. })),
            "Expected MaxJobsExceeded, got: {:?}",
            result
        );
    }

    // Still exactly 5 jobs.
    assert_eq!(manager.all_jobs().await.len(), 5);
}

#[tokio::test]
async fn concurrent_creates_and_reads_no_corruption() {
    let manager = std::sync::Arc::new(ContextManager::new(100));

    // Spawn writers that create jobs.
    let writer_handles: Vec<_> = (0..20)
        .map(|i| {
            let mgr = std::sync::Arc::clone(&manager);
            tokio::spawn(async move {
                mgr.create_job_for_user(
                    format!("user-{}", i % 5),
                    format!("Job {i}"),
                    format!("Description for job {i}"),
                )
                .await
            })
        })
        .collect();

    // Concurrently, spawn readers that list jobs.
    let reader_handles: Vec<_> = (0..20)
        .map(|_| {
            let mgr = std::sync::Arc::clone(&manager);
            tokio::spawn(async move {
                let _all = mgr.all_jobs().await;
                let _active = mgr.active_jobs().await;
                let _summary = mgr.summary().await;
            })
        })
        .collect();

    // Wait for all writers.
    let mut ids = Vec::new();
    for handle in writer_handles {
        let result = handle.await.expect("writer should not panic");
        ids.push(result.expect("create should succeed"));
    }

    // Wait for all readers.
    for handle in reader_handles {
        handle.await.expect("reader should not panic");
    }

    // All 20 jobs created with unique IDs.
    let unique: std::collections::HashSet<_> = ids.iter().collect();
    assert_eq!(unique.len(), 20);

    // Each user has 4 jobs (20 jobs / 5 users).
    for u in 0..5 {
        let user_jobs = manager.all_jobs_for(&format!("user-{u}")).await;
        assert_eq!(user_jobs.len(), 4, "user-{u} should have 4 jobs");
    }
}

#[tokio::test]
async fn concurrent_updates_do_not_lose_state() {
    let manager = std::sync::Arc::new(ContextManager::new(100));

    // Create 10 jobs.
    let mut job_ids = Vec::new();
    for i in 0..10 {
        let id = manager
            .create_job(format!("Job {i}"), "desc")
            .await
            .unwrap();
        job_ids.push(id);
    }

    // Concurrently transition all to InProgress.
    let handles: Vec<_> = job_ids
        .iter()
        .map(|&id| {
            let mgr = std::sync::Arc::clone(&manager);
            tokio::spawn(async move {
                mgr.update_context(id, |ctx| {
                    ctx.transition_to(crate::context::JobState::InProgress, None)
                })
                .await
            })
        })
        .collect();

    for handle in handles {
        let result = handle.await.expect("task should not panic");
        result
            .expect("update should succeed")
            .expect("transition should succeed");
    }

    // All 10 should now be InProgress.
    let active = manager.active_jobs().await;
    assert_eq!(active.len(), 10);
    for id in &job_ids {
        let ctx = manager.get_context(*id).await.unwrap();
        assert_eq!(ctx.state, crate::context::JobState::InProgress);
    }
}

#[tokio::test]
async fn concurrent_remove_and_read() {
    let manager = std::sync::Arc::new(ContextManager::new(100));

    // Create 20 jobs
    let mut job_ids = Vec::new();
    for i in 0..20 {
        let id = manager
            .create_job(format!("Job {i}"), "desc")
            .await
            .unwrap();
        job_ids.push(id);
    }

    // Concurrently remove the first 10 while reading the last 10
    let remove_handles: Vec<_> = job_ids[..10]
        .iter()
        .map(|&id| {
            let mgr = std::sync::Arc::clone(&manager);
            tokio::spawn(async move { mgr.remove_job(id).await })
        })
        .collect();

    let read_handles: Vec<_> = job_ids[10..]
        .iter()
        .map(|&id| {
            let mgr = std::sync::Arc::clone(&manager);
            tokio::spawn(async move { mgr.get_context(id).await })
        })
        .collect();

    for handle in remove_handles {
        handle
            .await
            .expect("remove task should not panic")
            .expect("remove should succeed");
    }

    for handle in read_handles {
        let ctx = handle
            .await
            .expect("read task should not panic")
            .expect("read should succeed");
        assert!(job_ids[10..].contains(&ctx.job_id));
    }

    assert_eq!(manager.all_jobs().await.len(), 10);
}
