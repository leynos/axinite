//! Tests for the worker event reporting endpoint and SSE fan-out.

use rstest::{fixture, rstest};

use super::*;

#[fixture]
fn event_test_state() -> (
    OrchestratorState,
    broadcast::Receiver<(Uuid, crate::channels::web::types::SseEvent)>,
) {
    let (tx, rx) = broadcast::channel(16);
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
    (state, rx)
}

#[rstest]
#[tokio::test]
async fn job_event_broadcasts_message(
    event_test_state: (
        OrchestratorState,
        broadcast::Receiver<(Uuid, crate::channels::web::types::SseEvent)>,
    ),
) {
    let (state, mut rx) = event_test_state;
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
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
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize job message event payload"),
        ))
        .expect("build job message event request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send job message event request");
    assert_eq!(resp.status(), StatusCode::OK);

    let (recv_id, event) = rx.recv().await.expect("receive job message SSE event");
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

#[rstest]
#[tokio::test]
async fn job_event_handles_tool_use(
    event_test_state: (
        OrchestratorState,
        broadcast::Receiver<(Uuid, crate::channels::web::types::SseEvent)>,
    ),
) {
    let (state, mut rx) = event_test_state;
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
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
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize tool-use event payload"),
        ))
        .expect("build tool-use event request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send tool-use event request");
    assert_eq!(resp.status(), StatusCode::OK);

    let (_recv_id, event) = rx.recv().await.expect("receive tool-use SSE event");
    match event {
        crate::channels::web::types::SseEvent::JobToolUse { tool_name, .. } => {
            assert_eq!(tool_name, "shell");
        }
        other => panic!("Expected JobToolUse, got {:?}", other),
    }
}

#[rstest]
#[tokio::test]
async fn job_event_handles_unknown_type(
    event_test_state: (
        OrchestratorState,
        broadcast::Receiver<(Uuid, crate::channels::web::types::SseEvent)>,
    ),
) {
    let (state, mut rx) = event_test_state;
    let job_id = Uuid::new_v4();
    let token = state.token_store.create_token(job_id).await;
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
        .body(Body::from(
            serde_json::to_vec(&payload).expect("serialize unknown-type event payload"),
        ))
        .expect("build unknown-type event request");

    let resp = router
        .oneshot(req)
        .await
        .expect("send unknown-type event request");
    assert_eq!(resp.status(), StatusCode::OK);

    let (_recv_id, event) = rx.recv().await.expect("receive unknown-type SSE event");
    assert!(matches!(
        event,
        crate::channels::web::types::SseEvent::JobStatus { .. }
    ));
}
