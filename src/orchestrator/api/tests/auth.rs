use rstest::rstest;

use super::fixtures::test_state;
use super::*;

#[rstest]
#[tokio::test]
async fn health_requires_no_auth(test_state: OrchestratorState) {
    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[rstest]
#[tokio::test]
async fn worker_route_rejects_missing_token(test_state: OrchestratorState) {
    let router = OrchestratorApi::router(test_state);

    let job_id = Uuid::new_v4();
    let req = Request::builder()
        .uri(format!("/worker/{}/job", job_id))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[rstest]
#[tokio::test]
async fn worker_route_rejects_wrong_token(test_state: OrchestratorState) {
    let router = OrchestratorApi::router(test_state);

    let job_id = Uuid::new_v4();
    let req = Request::builder()
        .uri(format!("/worker/{}/job", job_id))
        .header("Authorization", "Bearer totally-bogus")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[rstest]
#[tokio::test]
async fn worker_route_accepts_valid_token(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;

    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .uri(format!("/worker/{}/job", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[rstest]
#[tokio::test]
async fn token_for_job_a_rejected_on_job_b(test_state: OrchestratorState) {
    let job_a = Uuid::new_v4();
    let job_b = Uuid::new_v4();
    let token_a = test_state.token_store.create_token(job_a).await;

    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .uri(format!("/worker/{}/job", job_b))
        .header("Authorization", format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
