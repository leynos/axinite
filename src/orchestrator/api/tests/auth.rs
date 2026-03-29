//! Tests for worker route authentication middleware.

use rstest::rstest;

use super::fixtures::{test_state, worker_uri};
use super::*;
use crate::worker::api::{WORKER_HEALTH_ROUTE, WORKER_JOB_ROUTE};

#[rstest]
#[tokio::test]
async fn health_requires_no_auth(test_state: OrchestratorState) {
    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .uri(WORKER_HEALTH_ROUTE)
        .body(Body::empty())
        .expect("build health check request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send health check request");
    assert_eq!(resp.status(), StatusCode::OK);
}

#[rstest]
#[tokio::test]
async fn worker_route_rejects_missing_token(test_state: OrchestratorState) {
    let router = OrchestratorApi::router(test_state);

    let job_id = Uuid::new_v4();
    let req = Request::builder()
        .uri(worker_uri(WORKER_JOB_ROUTE, job_id))
        .body(Body::empty())
        .expect("build worker job request without auth");

    let resp = router
        .oneshot(req)
        .await
        .expect("send worker job request without auth");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[rstest]
#[tokio::test]
async fn worker_route_rejects_wrong_token(test_state: OrchestratorState) {
    let router = OrchestratorApi::router(test_state);

    let job_id = Uuid::new_v4();
    let req = Request::builder()
        .uri(worker_uri(WORKER_JOB_ROUTE, job_id))
        .header("Authorization", "Bearer totally-bogus")
        .body(Body::empty())
        .expect("build worker job request with invalid token");

    let resp = router
        .oneshot(req)
        .await
        .expect("send worker job request with invalid token");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[rstest]
#[tokio::test]
async fn worker_route_accepts_valid_token(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;

    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .uri(worker_uri(WORKER_JOB_ROUTE, job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .expect("build worker job request with valid token");

    let resp = router
        .oneshot(req)
        .await
        .expect("send worker job request with valid token");
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
        .uri(worker_uri(WORKER_JOB_ROUTE, job_b))
        .header("Authorization", format!("Bearer {}", token_a))
        .body(Body::empty())
        .expect("build worker job request with mismatched token");

    let resp = router
        .oneshot(req)
        .await
        .expect("send worker job request with mismatched token");
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
