use rstest::rstest;

use super::fixtures::test_state;
use super::*;

#[rstest]
#[tokio::test]
async fn prompt_returns_204_when_queue_empty(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .uri(format!("/worker/{}/prompt", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[rstest]
#[tokio::test]
async fn prompt_returns_queued_prompt(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;

    {
        let mut q = test_state.prompt_queue.lock().await;
        q.entry(job_id).or_default().push_back(PendingPrompt {
            content: "What is the status?".to_string(),
            done: false,
        });
    }

    let router = OrchestratorApi::router(test_state);
    let req = Request::builder()
        .uri(format!("/worker/{}/prompt", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["content"], "What is the status?");
    assert_eq!(json["done"], false);
}
