use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tokio::sync::{Mutex, broadcast};
use tower::ServiceExt;
use uuid::Uuid;

use crate::orchestrator::auth::TokenStore;
use crate::orchestrator::job_manager::{ContainerJobConfig, ContainerJobManager};
use crate::testing::StubLlm;
use crate::tools::{Tool, ToolOutput, ToolRegistry};

use super::*;

fn test_state() -> OrchestratorState {
    let token_store = TokenStore::new();
    let jm = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    OrchestratorState {
        llm: Arc::new(StubLlm::default()),
        tools: Arc::new(ToolRegistry::new()),
        job_manager: Arc::new(jm),
        token_store,
        job_event_tx: None,
        prompt_queue: Arc::new(Mutex::new(HashMap::new())),
        store: None,
        secrets_store: None,
        user_id: "default".to_string(),
    }
}

#[tokio::test]
async fn health_requires_no_auth() {
    let state = test_state();
    let router = OrchestratorApi::router(state);

    let req = Request::builder()
        .uri("/health")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn worker_route_rejects_missing_token() {
    let state = test_state();
    let router = OrchestratorApi::router(state);

    let job_id = Uuid::new_v4();
    let req = Request::builder()
        .uri(format!("/worker/{}/job", job_id))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn worker_route_rejects_wrong_token() {
    let state = test_state();
    let router = OrchestratorApi::router(state);

    let job_id = Uuid::new_v4();
    let req = Request::builder()
        .uri(format!("/worker/{}/job", job_id))
        .header("Authorization", "Bearer totally-bogus")
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn worker_route_accepts_valid_token() {
    let state = test_state();
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;

    let router = OrchestratorApi::router(state);

    let req = Request::builder()
        .uri(format!("/worker/{}/job", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn token_for_job_a_rejected_on_job_b() {
    let state = test_state();
    let job_a = Uuid::new_v4();
    let job_b = Uuid::new_v4();
    let token_a = state.token_store.create_token(job_a).await;

    let router = OrchestratorApi::router(state);

    let req = Request::builder()
        .uri(format!("/worker/{}/job", job_b))
        .header("Authorization", format!("Bearer {}", token_a))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn prompt_returns_204_when_queue_empty() {
    let state = test_state();
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    let req = Request::builder()
        .uri(format!("/worker/{}/prompt", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn prompt_returns_queued_prompt() {
    let state = test_state();
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;

    {
        let mut q = state.prompt_queue.lock().await;
        q.entry(job_id).or_default().push_back(PendingPrompt {
            content: "What is the status?".to_string(),
            done: false,
        });
    }

    let router = OrchestratorApi::router(state);
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

#[tokio::test]
async fn credentials_returns_204_when_no_grants() {
    let state = test_state();
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    let req = Request::builder()
        .uri(format!("/worker/{}/credentials", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .body(Body::empty())
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn credentials_returns_503_when_no_secrets_store() {
    let state = test_state();
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;

    state
        .token_store
        .store_grants(
            job_id,
            vec![crate::orchestrator::auth::CredentialGrant {
                secret_name: "test_secret".to_string(),
                env_var: "TEST_SECRET".to_string(),
            }],
        )
        .await;

    let router = OrchestratorApi::router(state);
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
    use crate::testing::credentials::test_secrets_store;
    use secrecy::SecretString;

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

#[tokio::test]
async fn extension_tool_proxy_rejects_non_extension_tool_names() {
    let state = test_state();
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    let payload = serde_json::json!({
        "tool_name": "shell",
        "params": {"command": "ls"}
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/extension_tool", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize proxy extension tool payload"),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn extension_tool_proxy_rejects_extension_tools_that_require_approval_for_params() {
    struct ApprovalAwareToolList;

    #[async_trait::async_trait]
    impl Tool for ApprovalAwareToolList {
        fn name(&self) -> &str {
            "tool_list"
        }

        fn description(&self) -> &str {
            "approval-aware tool_list"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {
                    "require_approval": { "type": "boolean" }
                }
            })
        }

        async fn execute(
            &self,
            _params: serde_json::Value,
            _ctx: &crate::context::JobContext,
        ) -> Result<ToolOutput, crate::tools::ToolError> {
            panic!("approval-gated proxy requests must not execute")
        }

        fn requires_approval(
            &self,
            params: &serde_json::Value,
        ) -> crate::tools::ApprovalRequirement {
            if params["require_approval"].as_bool() == Some(true) {
                crate::tools::ApprovalRequirement::Always
            } else {
                crate::tools::ApprovalRequirement::Never
            }
        }
    }

    let state = test_state();
    state.tools.register(Arc::new(ApprovalAwareToolList)).await;
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    let payload = serde_json::json!({
        "tool_name": "tool_list",
        "params": {"require_approval": true}
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/extension_tool", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize approval-gated proxy payload"),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn extension_tool_proxy_executes_registered_extension_tool_with_request_job_id() {
    struct FakeToolList {
        seen_job_id: Arc<tokio::sync::Mutex<Option<Uuid>>>,
    }

    #[async_trait::async_trait]
    impl Tool for FakeToolList {
        fn name(&self) -> &str {
            "tool_list"
        }

        fn description(&self) -> &str {
            "fake tool_list"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({
                "type": "object",
                "properties": {}
            })
        }

        async fn execute(
            &self,
            _params: serde_json::Value,
            ctx: &crate::context::JobContext,
        ) -> Result<ToolOutput, crate::tools::ToolError> {
            *self.seen_job_id.lock().await = Some(ctx.job_id);
            Ok(ToolOutput::success(
                serde_json::json!({"extensions": ["telegram"]}),
                Duration::from_millis(5),
            ))
        }
    }

    let state = test_state();
    let seen_job_id = Arc::new(tokio::sync::Mutex::new(None));
    state.tools.register_sync(Arc::new(FakeToolList {
        seen_job_id: Arc::clone(&seen_job_id),
    }));
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    let payload = serde_json::json!({
        "tool_name": "tool_list",
        "params": {"include_available": true}
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/extension_tool", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize registered proxy payload"),
        ))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 4096).await.unwrap();
    let proxy_resp: crate::worker::api::ProxyExtensionToolResponse =
        serde_json::from_slice(&body).unwrap();
    assert_eq!(proxy_resp.output.result["extensions"][0], "telegram");
    assert_eq!(proxy_resp.output.duration, Duration::from_millis(5));
    assert_eq!(proxy_resp.output.cost, None);
    assert_eq!(proxy_resp.output.raw, None);
    assert_eq!(*seen_job_id.lock().await, Some(job_id));
}

#[tokio::test]
async fn job_event_broadcasts_message() {
    let (tx, mut rx) = broadcast::channel(16);
    let token_store = TokenStore::new();
    let jm = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    let state = OrchestratorState {
        llm: Arc::new(StubLlm::default()),
        tools: Arc::new(ToolRegistry::new()),
        job_manager: Arc::new(jm),
        token_store: token_store.clone(),
        job_event_tx: Some(tx),
        prompt_queue: Arc::new(Mutex::new(HashMap::new())),
        store: None,
        secrets_store: None,
        user_id: "default".to_string(),
    };

    let job_id = Uuid::new_v4();
    let token = token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    let payload = serde_json::json!({
        "event_type": "message",
        "data": {
            "role": "assistant",
            "content": "Hello from worker"
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/event", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (recv_id, event) = rx.recv().await.unwrap();
    assert_eq!(recv_id, job_id);
    match event {
        crate::channels::web::types::SseEvent::JobMessage {
            job_id: jid,
            role,
            content,
        } => {
            assert_eq!(jid, job_id.to_string());
            assert_eq!(role, "assistant");
            assert_eq!(content, "Hello from worker");
        }
        other => panic!("Expected JobMessage, got {:?}", other),
    }
}

#[tokio::test]
async fn job_event_handles_tool_use() {
    let (tx, mut rx) = broadcast::channel(16);
    let token_store = TokenStore::new();
    let jm = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    let state = OrchestratorState {
        llm: Arc::new(StubLlm::default()),
        tools: Arc::new(ToolRegistry::new()),
        job_manager: Arc::new(jm),
        token_store: token_store.clone(),
        job_event_tx: Some(tx),
        prompt_queue: Arc::new(Mutex::new(HashMap::new())),
        store: None,
        secrets_store: None,
        user_id: "default".to_string(),
    };

    let job_id = Uuid::new_v4();
    let token = token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    let payload = serde_json::json!({
        "event_type": "tool_use",
        "data": {
            "tool_name": "shell",
            "input": {"command": "ls"}
        }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/event", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (_recv_id, event) = rx.recv().await.unwrap();
    match event {
        crate::channels::web::types::SseEvent::JobToolUse { tool_name, .. } => {
            assert_eq!(tool_name, "shell");
        }
        other => panic!("Expected JobToolUse, got {:?}", other),
    }
}

#[tokio::test]
async fn job_event_handles_unknown_type() {
    let (tx, mut rx) = broadcast::channel(16);
    let token_store = TokenStore::new();
    let jm = ContainerJobManager::new(ContainerJobConfig::default(), token_store.clone());
    let state = OrchestratorState {
        llm: Arc::new(StubLlm::default()),
        tools: Arc::new(ToolRegistry::new()),
        job_manager: Arc::new(jm),
        token_store: token_store.clone(),
        job_event_tx: Some(tx),
        prompt_queue: Arc::new(Mutex::new(HashMap::new())),
        store: None,
        secrets_store: None,
        user_id: "default".to_string(),
    };

    let job_id = Uuid::new_v4();
    let token = token_store.create_token(job_id).await;
    let router = OrchestratorApi::router(state);

    let payload = serde_json::json!({
        "event_type": "custom_thing",
        "data": { "message": "something custom" }
    });

    let req = Request::builder()
        .method("POST")
        .uri(format!("/worker/{}/event", job_id))
        .header("Authorization", format!("Bearer {}", token))
        .header("Content-Type", "application/json")
        .body(Body::from(serde_json::to_vec(&payload).unwrap()))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let (_recv_id, event) = rx.recv().await.unwrap();
    assert!(matches!(
        event,
        crate::channels::web::types::SseEvent::JobStatus { .. }
    ));
}

#[tokio::test]
async fn report_status_updates_handle() {
    let state = test_state();
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;

    {
        let mut containers = state.job_manager.containers.write().await;
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

    let jm = Arc::clone(&state.job_manager);
    let router = OrchestratorApi::router(state);

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
        .body(Body::from(serde_json::to_vec(&update).unwrap()))
        .unwrap();

    let resp = router.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let handle = jm.get_handle(job_id).await.unwrap();
    assert_eq!(handle.worker_iteration, 5);
    assert_eq!(handle.last_worker_status.as_deref(), Some("Iteration 5"));
}
