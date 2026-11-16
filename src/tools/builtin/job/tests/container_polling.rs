//! Tests for container-state polling outcome dispatch.
//!
//! The sandbox execution path must not report success for a container that
//! was stopped without a worker-reported completion result.

use std::sync::Arc;
use std::time::Instant;

use uuid::Uuid;

use crate::context::ContextManager;
use crate::orchestrator::auth::TokenStore;
use crate::orchestrator::job_manager::{
    CompletionResult, ContainerHandle, ContainerId, ContainerJobConfig, ContainerJobManager,
    ContainerState, JobMode,
};
use crate::tools::builtin::job::CreateJobTool;

use super::super::sandbox::ContainerJobView;

/// Build a job manager holding one stopped container for `job_id`.
///
/// `completion_result` models what the worker reported, if anything, before
/// the container stopped.
async fn manager_with_stopped_container(
    job_id: Uuid,
    completion_result: Option<CompletionResult>,
) -> ContainerJobManager {
    let manager = ContainerJobManager::new(ContainerJobConfig::default(), TokenStore::new());
    manager.containers().write().await.insert(
        job_id,
        ContainerHandle {
            job_id,
            container_id: Some(ContainerId::new("container-1")),
            state: ContainerState::Stopped,
            mode: JobMode::Worker,
            created_at: chrono::Utc::now(),
            project_dir: None,
            task_description: "test job".to_string(),
            last_worker_status: None,
            worker_iteration: 0,
            completion_result,
        },
    );
    manager
}

#[tokio::test]
async fn test_stopped_container_without_result_reports_failure() {
    let job_id = Uuid::new_v4();
    let manager = manager_with_stopped_container(job_id, None).await;
    let tool = CreateJobTool::new(Arc::new(ContextManager::new(5)));

    let project_dir = String::from("/workspace");
    let browse_id = String::from("browse");
    let view = ContainerJobView {
        jm: &manager,
        job_id,
        project_dir_str: &project_dir,
        browse_id: &browse_id,
        start: Instant::now(),
    };

    let error = tool
        .await_container_completion(&view)
        .await
        .expect_err("a stopped container without a worker result must not succeed");
    let crate::tools::tool::ToolError::ExecutionFailed(message) = error else {
        panic!("expected an execution failure, got {error:?}");
    };
    assert!(
        message.contains("Container stopped without a completion result"),
        "unexpected failure message: {message}"
    );
    assert!(
        manager.get_handle(job_id).await.is_none(),
        "the stopped container handle should be cleaned up"
    );
}

#[tokio::test]
async fn test_stopped_container_with_successful_result_reports_success() {
    let job_id = Uuid::new_v4();
    let manager = manager_with_stopped_container(
        job_id,
        Some(CompletionResult {
            success: true,
            message: Some("work finished".to_string()),
        }),
    )
    .await;
    let tool = CreateJobTool::new(Arc::new(ContextManager::new(5)));

    let project_dir = String::from("/workspace");
    let browse_id = String::from("browse");
    let view = ContainerJobView {
        jm: &manager,
        job_id,
        project_dir_str: &project_dir,
        browse_id: &browse_id,
        start: Instant::now(),
    };

    let output = tool
        .await_container_completion(&view)
        .await
        .expect("a successful worker result should complete the job");
    assert_eq!(
        output.result.get("status").and_then(|s| s.as_str()),
        Some("completed")
    );
    assert_eq!(
        output.result.get("output").and_then(|s| s.as_str()),
        Some("work finished")
    );
}

#[tokio::test]
async fn test_stopped_container_with_failed_result_reports_failure() {
    let job_id = Uuid::new_v4();
    let manager = manager_with_stopped_container(
        job_id,
        Some(CompletionResult {
            success: false,
            message: Some("worker gave up".to_string()),
        }),
    )
    .await;
    let tool = CreateJobTool::new(Arc::new(ContextManager::new(5)));

    let project_dir = String::from("/workspace");
    let browse_id = String::from("browse");
    let view = ContainerJobView {
        jm: &manager,
        job_id,
        project_dir_str: &project_dir,
        browse_id: &browse_id,
        start: Instant::now(),
    };

    let error = tool
        .await_container_completion(&view)
        .await
        .expect_err("an unsuccessful worker result must fail the job");
    let crate::tools::tool::ToolError::ExecutionFailed(message) = error else {
        panic!("expected an execution failure, got {error:?}");
    };
    assert!(
        message.contains("worker gave up"),
        "unexpected failure message: {message}"
    );
}
