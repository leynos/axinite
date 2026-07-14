//! Unit tests for job creation, lookup, update, removal, and memory
//! handling in the context manager.

use uuid::Uuid;

use crate::context::manager::ContextManager;
use crate::error::JobError;

#[tokio::test]
async fn test_create_job() {
    let manager = ContextManager::new(5);
    let job_id = manager.create_job("Test", "Description").await.unwrap();

    let context = manager.get_context(job_id).await.unwrap();
    assert_eq!(context.title, "Test");
}

#[tokio::test]
async fn test_create_job_for_user_sets_user_id() {
    let manager = ContextManager::new(5);
    let job_id = manager
        .create_job_for_user("user-123", "Test", "Description")
        .await
        .unwrap();

    let context = manager.get_context(job_id).await.unwrap();
    assert_eq!(context.user_id, "user-123");
}

#[tokio::test]
async fn test_max_jobs_limit() {
    let manager = ContextManager::new(2);

    manager.create_job("Job 1", "Desc").await.unwrap();
    manager.create_job("Job 2", "Desc").await.unwrap();

    // Start the jobs to make them active
    for job_id in manager.all_jobs().await {
        manager
            .update_context(job_id, |ctx| {
                ctx.transition_to(crate::context::JobState::InProgress, None)
            })
            .await
            .unwrap()
            .unwrap();
    }

    // Third job should fail
    let result = manager.create_job("Job 3", "Desc").await;
    assert!(matches!(result, Err(JobError::MaxJobsExceeded { max: 2 })));
}

#[tokio::test]
async fn test_update_context() {
    let manager = ContextManager::new(5);
    let job_id = manager.create_job("Test", "Desc").await.unwrap();

    manager
        .update_context(job_id, |ctx| {
            ctx.transition_to(crate::context::JobState::InProgress, None)
        })
        .await
        .unwrap()
        .unwrap();

    let context = manager.get_context(job_id).await.unwrap();
    assert_eq!(context.state, crate::context::JobState::InProgress);
}

#[tokio::test]
async fn get_context_not_found() {
    let manager = ContextManager::new(5);
    let bogus_id = Uuid::new_v4();
    let result = manager.get_context(bogus_id).await;
    assert!(matches!(result, Err(JobError::NotFound { id }) if id == bogus_id));
}

#[tokio::test]
async fn update_context_not_found() {
    let manager = ContextManager::new(5);
    let bogus_id = Uuid::new_v4();
    let result = manager.update_context(bogus_id, |_ctx| {}).await;
    assert!(matches!(result, Err(JobError::NotFound { id }) if id == bogus_id));
}

#[tokio::test]
async fn remove_job_returns_context_and_memory() {
    let manager = ContextManager::new(5);
    let job_id = manager.create_job("Removable", "bye bye").await.unwrap();

    let (ctx, mem) = manager.remove_job(job_id).await.unwrap();
    assert_eq!(ctx.title, "Removable");
    assert_eq!(mem.job_id, job_id);

    // After removal, get should fail
    assert!(matches!(
        manager.get_context(job_id).await,
        Err(JobError::NotFound { .. })
    ));
    assert!(matches!(
        manager.get_memory(job_id).await,
        Err(JobError::NotFound { .. })
    ));
}

#[tokio::test]
async fn remove_job_not_found() {
    let manager = ContextManager::new(5);
    let result = manager.remove_job(Uuid::new_v4()).await;
    assert!(matches!(result, Err(JobError::NotFound { .. })));
}

#[tokio::test]
async fn get_memory_and_update_memory() {
    let manager = ContextManager::new(5);
    let job_id = manager.create_job("Mem test", "desc").await.unwrap();

    // Fresh memory should be empty
    let mem = manager.get_memory(job_id).await.unwrap();
    assert_eq!(mem.job_id, job_id);
    assert!(mem.actions.is_empty());
    assert!(mem.conversation.is_empty());

    // Update memory by adding a message
    manager
        .update_memory(job_id, |m| {
            m.add_message(crate::llm::ChatMessage::user("hello from test"));
        })
        .await
        .unwrap();

    let mem = manager.get_memory(job_id).await.unwrap();
    assert_eq!(mem.conversation.len(), 1);
    assert_eq!(mem.conversation.messages()[0].content, "hello from test");
}

#[tokio::test]
async fn update_memory_not_found() {
    let manager = ContextManager::new(5);
    let result = manager.update_memory(Uuid::new_v4(), |_| {}).await;
    assert!(matches!(result, Err(JobError::NotFound { .. })));
}

#[tokio::test]
async fn get_memory_not_found() {
    let manager = ContextManager::new(5);
    let result = manager.get_memory(Uuid::new_v4()).await;
    assert!(matches!(result, Err(JobError::NotFound { .. })));
}

#[tokio::test]
async fn default_context_manager_has_max_10() {
    let manager = ContextManager::default();
    // Create 10 jobs and make them active
    for i in 0..10 {
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
    // 11th should fail
    let result = manager.create_job("overflow", "d").await;
    assert!(matches!(result, Err(JobError::MaxJobsExceeded { max: 10 })));
}

#[tokio::test]
async fn create_job_uses_default_user() {
    let manager = ContextManager::new(5);
    let job_id = manager.create_job("Test", "desc").await.unwrap();
    let ctx = manager.get_context(job_id).await.unwrap();
    assert_eq!(ctx.user_id, "default");
}
