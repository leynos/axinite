//! Tests for job lifecycle tools: listing, status inspection, and cancellation.

use std::sync::Arc;

use crate::context::{ContextManager, JobContext, JobState};
use crate::tools::builtin::job::{CancelJobTool, JobStatusTool, ListJobsTool};
use crate::tools::tool::NativeTool;

#[tokio::test]
async fn test_list_jobs_tool() {
    let manager = Arc::new(ContextManager::new(5));

    // Create some jobs
    manager.create_job("Job 1", "Desc 1").await.unwrap();
    manager.create_job("Job 2", "Desc 2").await.unwrap();

    let tool = ListJobsTool::new(manager);

    let params = serde_json::json!({});
    let ctx = JobContext::default();
    let result = tool.execute(params, &ctx).await.unwrap();

    let jobs = result.result.get("jobs").unwrap().as_array().unwrap();
    assert_eq!(jobs.len(), 2);
}

#[tokio::test]
async fn test_job_status_tool() {
    let manager = Arc::new(ContextManager::new(5));
    let job_id = manager.create_job("Test Job", "Description").await.unwrap();

    let tool = JobStatusTool::new(manager);

    let params = serde_json::json!({
        "job_id": job_id.to_string()
    });
    let ctx = JobContext::default();
    let result = tool.execute(params, &ctx).await.unwrap();

    assert_eq!(
        result.result.get("title").unwrap().as_str().unwrap(),
        "Test Job"
    );
}

#[tokio::test]
async fn test_list_jobs_formatting() {
    let manager = Arc::new(ContextManager::new(10));
    let pending_id = manager
        .create_job_for_user("default", "Pending Job", "Todo")
        .await
        .unwrap();
    let completed_id = manager
        .create_job_for_user("default", "Completed Job", "Done")
        .await
        .unwrap();
    let failed_id = manager
        .create_job_for_user("default", "Failed Job", "Oops")
        .await
        .unwrap();
    manager
        .create_job_for_user("other-user", "Other User Job", "Ignore")
        .await
        .unwrap();

    manager
        .update_context(completed_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)?;
            ctx.transition_to(JobState::Completed, Some("done".to_string()))
        })
        .await
        .unwrap()
        .unwrap();
    manager
        .update_context(failed_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)?;
            ctx.transition_to(JobState::Failed, Some("boom".to_string()))
        })
        .await
        .unwrap()
        .unwrap();

    let tool = ListJobsTool::new(Arc::clone(&manager));
    let ctx = JobContext::default();
    let result = tool.execute(serde_json::json!({}), &ctx).await.unwrap();

    let jobs = result.result.get("jobs").unwrap().as_array().unwrap();
    assert_eq!(jobs.len(), 3);
    assert!(jobs.iter().any(|job| {
        job.get("job_id").and_then(|v| v.as_str()) == Some(&pending_id.to_string())
            && job.get("status").and_then(|v| v.as_str()) == Some("Pending")
    }));
    assert!(jobs.iter().any(|job| {
        job.get("job_id").and_then(|v| v.as_str()) == Some(&completed_id.to_string())
            && job.get("status").and_then(|v| v.as_str()) == Some("Completed")
    }));
    assert!(jobs.iter().any(|job| {
        job.get("job_id").and_then(|v| v.as_str()) == Some(&failed_id.to_string())
            && job.get("status").and_then(|v| v.as_str()) == Some("Failed")
    }));

    let summary = result.result.get("summary").unwrap();
    assert_eq!(summary.get("total").and_then(|v| v.as_u64()), Some(3));
    assert_eq!(summary.get("pending").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(summary.get("completed").and_then(|v| v.as_u64()), Some(1));
    assert_eq!(summary.get("failed").and_then(|v| v.as_u64()), Some(1));
}

#[tokio::test]
async fn test_job_status_transitions() {
    let manager = Arc::new(ContextManager::new(5));
    let job_id = manager
        .create_job_for_user("default", "Transition Job", "Track me")
        .await
        .unwrap();
    manager
        .update_context(job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, Some("started".to_string()))?;
            ctx.transition_to(JobState::Completed, Some("finished".to_string()))
        })
        .await
        .unwrap()
        .unwrap();

    let tool = JobStatusTool::new(Arc::clone(&manager));
    let ctx = JobContext::default();
    let result = tool
        .execute(serde_json::json!({ "job_id": job_id.to_string() }), &ctx)
        .await
        .unwrap();

    assert_eq!(
        result.result.get("status").and_then(|v| v.as_str()),
        Some("Completed")
    );
    assert!(result.result.get("started_at").unwrap().is_string());
    assert!(result.result.get("completed_at").unwrap().is_string());
}

#[tokio::test]
async fn test_cancel_job_running() {
    let manager = Arc::new(ContextManager::new(5));
    let job_id = manager
        .create_job_for_user("default", "Running Job", "In progress")
        .await
        .unwrap();
    manager
        .update_context(job_id, |ctx| ctx.transition_to(JobState::InProgress, None))
        .await
        .unwrap()
        .unwrap();

    let tool = CancelJobTool::new(Arc::clone(&manager));
    let ctx = JobContext::default();
    let result = tool
        .execute(serde_json::json!({ "job_id": job_id.to_string() }), &ctx)
        .await
        .unwrap();

    assert_eq!(
        result.result.get("status").and_then(|v| v.as_str()),
        Some("cancelled")
    );
    let updated = manager.get_context(job_id).await.unwrap();
    assert_eq!(updated.state, JobState::Cancelled);
}

#[tokio::test]
async fn test_cancel_job_completed() {
    let manager = Arc::new(ContextManager::new(5));
    let job_id = manager
        .create_job_for_user("default", "Completed Job", "Already done")
        .await
        .unwrap();
    manager
        .update_context(job_id, |ctx| {
            ctx.transition_to(JobState::InProgress, None)?;
            ctx.transition_to(JobState::Completed, Some("done".to_string()))
        })
        .await
        .unwrap()
        .unwrap();

    let tool = CancelJobTool::new(Arc::clone(&manager));
    let ctx = JobContext::default();
    let result = tool
        .execute(serde_json::json!({ "job_id": job_id.to_string() }), &ctx)
        .await
        .unwrap();

    let error = result.result.get("error").and_then(|v| v.as_str()).unwrap();
    assert!(error.contains("Cannot cancel job"));
    assert!(error.contains("completed"));
}
