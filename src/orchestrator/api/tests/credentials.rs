//! Tests for the worker credentials endpoint.

use std::sync::Arc;

use rstest::rstest;
use secrecy::SecretString;

use super::fixtures::{test_state, worker_uri};
use super::*;
use crate::testing::credentials::test_secrets_store;
use crate::worker::api::WORKER_CREDENTIALS_ROUTE;

fn with_secrets_store(
    mut state: OrchestratorState,
    secrets_store: Arc<dyn crate::secrets::SecretsStore + Send + Sync>,
) -> OrchestratorState {
    state.secrets_store = Some(secrets_store);
    state
}

#[rstest]
#[tokio::test]
async fn credentials_returns_204_when_no_grants(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .uri(worker_uri(WORKER_CREDENTIALS_ROUTE, job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .expect("build credentials request without grants");

    let resp = router
        .oneshot(req)
        .await
        .expect("send credentials request without grants");
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[rstest]
#[tokio::test]
async fn credentials_returns_503_when_no_secrets_store(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;

    test_state
        .token_store
        .store_grants(
            job_id,
            vec![crate::orchestrator::auth::CredentialGrant {
                secret_name: "test_secret".to_string(),
                env_var: "TEST_SECRET".to_string(),
            }],
        )
        .await;

    let router = OrchestratorApi::router(test_state);
    let req = Request::builder()
        .uri(worker_uri(WORKER_CREDENTIALS_ROUTE, job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .expect("build credentials request without secrets store");

    let resp = router
        .oneshot(req)
        .await
        .expect("send credentials request without secrets store");
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[rstest]
#[tokio::test]
async fn credentials_returns_secrets_when_store_configured(test_state: OrchestratorState) {
    let secrets_store = Arc::new(test_secrets_store());
    secrets_store
        .create(
            "default",
            crate::secrets::CreateSecretParams {
                name: "test_secret".to_string(),
                value: SecretString::from("supersecretvalue".to_string()),
                provider: None,
                expires_at: None,
            },
        )
        .await
        .expect("store test secret for credentials response");

    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    test_state
        .token_store
        .store_grants(
            job_id,
            vec![crate::orchestrator::auth::CredentialGrant {
                secret_name: "test_secret".to_string(),
                env_var: "MY_SECRET".to_string(),
            }],
        )
        .await;

    let state = with_secrets_store(test_state, secrets_store);

    let router = OrchestratorApi::router(state);
    let req = Request::builder()
        .uri(worker_uri(WORKER_CREDENTIALS_ROUTE, job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .expect("build credentials request with configured store");

    let resp = router
        .oneshot(req)
        .await
        .expect("send credentials request with configured store");
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096)
        .await
        .expect("read credentials response body");
    let json: Vec<serde_json::Value> =
        serde_json::from_slice(&body).expect("parse credentials response JSON");
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["env_var"], "MY_SECRET");
    assert_eq!(json[0]["value"], "supersecretvalue");
}
