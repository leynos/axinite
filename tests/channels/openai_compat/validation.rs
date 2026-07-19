//! Request validation, authentication, and limit tests for the
//! OpenAI-compatible API endpoints.

use std::net::SocketAddr;
use std::sync::Arc;

use axinite::channels::web::server::{GatewayState, start_server};
use axinite::channels::web::sse::SseManager;
use axinite::channels::web::ws::WsConnectionTracker;

use super::helpers::{AUTH_TOKEN, client, start_test_server};

#[tokio::test]
async fn test_chat_completions_model_too_long() {
    let (addr, _state, mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "m".repeat(300),
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("model"),
        "Expected model validation error, got: {}",
        body
    );

    // Validation should fail before provider invocation.
    let models = mock_state.completion_models.lock().await;
    assert!(
        models.is_empty(),
        "provider should not be called: {:?}",
        *models
    );
}

#[tokio::test]
async fn test_chat_completions_model_with_control_chars() {
    let (addr, _state, mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "gpt-4\noops",
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("control"),
        "Expected model validation error, got: {}",
        body
    );

    // Validation should fail before provider invocation.
    let models = mock_state.completion_models.lock().await;
    assert!(
        models.is_empty(),
        "provider should not be called: {:?}",
        *models
    );
}

#[tokio::test]
async fn test_chat_completions_model_with_surrounding_whitespace() {
    let (addr, _state, mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": " gpt-4 ",
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 400);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert!(
        body["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("leading or trailing whitespace"),
        "Expected model validation error, got: {}",
        body
    );

    let models = mock_state.completion_models.lock().await;
    assert!(
        models.is_empty(),
        "provider should not be called: {:?}",
        *models
    );
}

#[tokio::test]
async fn test_chat_completions_no_auth() {
    let (addr, _state, _mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    let resp = client()
        .post(&url)
        // No auth header
        .json(&serde_json::json!({
            "model": "mock-model-v1",
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_models_endpoint() {
    let (addr, _state, _mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/models", addr);

    let resp = client()
        .get(&url)
        .bearer_auth(AUTH_TOKEN)
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    let body: serde_json::Value = resp.json().await.unwrap();

    assert_eq!(body["object"], "list");
    let data = body["data"].as_array().unwrap();
    assert_eq!(data.len(), 2);
    assert_eq!(data[0]["id"], "mock-model-v1");
    assert_eq!(data[1]["id"], "mock-model-v2");
    assert_eq!(data[0]["object"], "model");
}

#[tokio::test]
async fn test_models_no_auth() {
    let (addr, _state, _mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/models", addr);

    let resp = client().get(&url).send().await.unwrap();
    assert_eq!(resp.status(), 401);
}

#[tokio::test]
async fn test_no_llm_provider_returns_503() {
    // Create state WITHOUT llm_provider
    let state = Arc::new(GatewayState {
        feature_flags: std::sync::Arc::new(tokio::sync::RwLock::new(Default::default())),
        msg_tx: tokio::sync::RwLock::new(None),
        sse: SseManager::new(),
        workspace: None,
        session_manager: None,
        log_broadcaster: None,
        log_level_handle: None,
        extension_manager: None,
        tool_registry: None,
        store: None,
        job_manager: None,
        prompt_queue: None,
        scheduler: None,
        user_id: "test-user".to_string(),
        shutdown_tx: tokio::sync::RwLock::new(None),
        ws_tracker: Some(Arc::new(WsConnectionTracker::new())),
        llm_provider: None, // No LLM!
        skill_registry: None,
        skill_catalog: None,
        chat_rate_limiter: axinite::channels::web::server::RateLimiter::new(30, 60),
        oauth_rate_limiter: axinite::channels::web::server::RateLimiter::new(10, 60),
        registry_entries: Vec::new(),
        cost_guard: None,
        routine_engine: Arc::new(tokio::sync::RwLock::new(None)),
        startup_time: std::time::Instant::now(),
    });

    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let bound_addr = start_server(addr, state, AUTH_TOKEN.to_string())
        .await
        .unwrap();

    let url = format!("http://{}/v1/chat/completions", bound_addr);
    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "mock-model-v1",
            "messages": [{"role": "user", "content": "Hi"}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 503);
}

#[tokio::test]
async fn test_chat_completions_body_too_large() {
    let (addr, _state, _mock_state) = start_test_server().await;
    let url = format!("http://{}/v1/chat/completions", addr);

    // Build a payload over 10 MB (the gateway's DefaultBodyLimit)
    let big_content = "x".repeat(11 * 1024 * 1024);
    let resp = client()
        .post(&url)
        .bearer_auth(AUTH_TOKEN)
        .json(&serde_json::json!({
            "model": "mock-model-v1",
            "messages": [{"role": "user", "content": big_content}]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 413);
}
