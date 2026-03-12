use std::collections::HashMap;
use std::sync::Arc;

use rstest::rstest;
use secrecy::SecretString;
use tokio::sync::Mutex;

use super::fixtures::test_state;
use super::*;
use crate::testing::credentials::test_secrets_store;

#[rstest]
#[tokio::test]
async fn credentials_returns_204_when_no_grants(test_state: OrchestratorState) {
    let job_id = Uuid::new_v4();
    let token = test_state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(test_state);

    let req = Request::builder()
        .uri(format!("/worker/{}/credentials", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
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
        .uri(format!("/worker/{}/credentials", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::SERVICE_UNAVAILABLE);
}

#[tokio::test]
async fn credentials_returns_secrets_when_store_configured() {
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
        .unwrap();

    let token_store = TokenStore::new();
    let jm = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    let job_id = Uuid::new_v4();
    let token = token_store.create_token(job_id).await;
    token_store
        .store_grants(
            job_id,
            vec![crate::orchestrator::auth::CredentialGrant {
                secret_name: "test_secret".to_string(),
                env_var: "MY_SECRET".to_string(),
            }],
        )
        .await;

    let state = OrchestratorState {
        llm: Arc::new(StubLlm::default()),
        tools: Arc::new(ToolRegistry::new()),
        job_manager: Arc::new(jm),
        token_store,
        job_event_tx: None,
        prompt_queue: Arc::new(Mutex::new(HashMap::new())),
        store: None,
        secrets_store: Some(secrets_store),
        user_id: "default".to_string(),
    };

    let router = OrchestratorApi::router(state);
    let req = Request::builder()
        .uri(format!("/worker/{}/credentials", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let json: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(json.len(), 1);
    assert_eq!(json[0]["env_var"], "MY_SECRET");
    assert_eq!(json[0]["value"], "supersecretvalue");
}
