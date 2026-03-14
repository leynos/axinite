//! Tests for the worker status reporting endpoint.

use std::sync::Arc;

use rstest::rstest;

use super::fixtures::test_state;
use super::*;

#[rstest]
#[tokio::test]
async fn report_status_updates_handle(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;

    {
        let mut containers = test_state.job_manager.containers.write().await;
        containers.insert(
            job_id,
            crate::orchestrator::job_manager::ContainerHandle {
                job_id,
                container_id: "test-container".to_string(),
                state: crate::orchestrator::job_manager::ContainerState::Running,
                mode: crate::orchestrator::job_manager::JobMode::Worker,
                created_at: chrono::Utc::now(),
                project_dir: None,
                task_description: "test".to_string(),
                last_worker_status: None,
                worker_iteration: 0,
                completion_result: None,
            },
        );
    }

    let jm = Arc::clone(&test_state.job_manager);
    let router = OrchestratorApi::router(test_state);

    let update = serde_json::json!({
        "state": "in_progress",
        "message": "Iteration 5",
        "iteration": 5
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/status", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&update).expect("serialize worker status update"),
        ))
        .expect("build worker status update request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send worker status update request");
    assert_eq!(resp.status(), StatusCode::OK);

    let handle = jm
        .get_handle(job_id)
        .await
        .expect("retrieve updated container handle");
    assert_eq!(handle.worker_iteration, 5);
    assert_eq!(handle.last_worker_status.as_deref(), Some("Iteration 5"));
}
