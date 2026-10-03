//! Unit tests for WebSocket connection tracking and client messages.

use super::*;

#[test]
fn test_ws_connection_tracker() {
    let tracker = WsConnectionTracker::new();
    assert_eq!(tracker.connection_count(), 0);

    tracker.increment();
    assert_eq!(tracker.connection_count(), 1);

    tracker.increment();
    assert_eq!(tracker.connection_count(), 2);

    tracker.decrement();
    assert_eq!(tracker.connection_count(), 1);

    tracker.decrement();
    assert_eq!(tracker.connection_count(), 0);
}

#[test]
fn test_ws_connection_tracker_default() {
    let tracker = WsConnectionTracker::default();
    assert_eq!(tracker.connection_count(), 0);
}

#[tokio::test]
async fn test_handle_client_message_ping() {
    // Ping should produce a Pong on the direct channel
    let (direct_tx, mut direct_rx) = mpsc::channel(16);
    let state = make_test_state(None).await;

    handle_client_message(WsClientMessage::Ping, &state, "user1", &direct_tx).await;

    let response = direct_rx.recv().await.unwrap();
    assert!(matches!(response, WsServerMessage::Pong));
}

#[tokio::test]
async fn test_handle_client_message_sends_to_agent() {
    // A Message should be forwarded to the agent's msg_tx
    let (agent_tx, mut agent_rx) = mpsc::channel(16);
    let state = make_test_state(Some(agent_tx)).await;
    let (direct_tx, _direct_rx) = mpsc::channel(16);

    handle_client_message(
        WsClientMessage::Message {
            content: "hello agent".to_string(),
            thread_id: Some("t1".to_string()),
            timezone: None,
            images: Vec::new(),
        },
        &state,
        "user1",
        &direct_tx,
    )
    .await;

    let incoming = agent_rx.recv().await.unwrap();
    assert_eq!(incoming.content, "hello agent");
    assert_eq!(incoming.thread_id.as_deref(), Some("t1"));
    assert_eq!(incoming.channel, "gateway");
    assert_eq!(incoming.user_id, "user1");
}

#[tokio::test]
async fn test_handle_client_message_no_channel() {
    // When msg_tx is None, should send an error back
    let state = make_test_state(None).await;
    let (direct_tx, mut direct_rx) = mpsc::channel(16);

    handle_client_message(
        WsClientMessage::Message {
            content: "hello".to_string(),
            thread_id: None,
            timezone: None,
            images: Vec::new(),
        },
        &state,
        "user1",
        &direct_tx,
    )
    .await;

    let response = direct_rx.recv().await.unwrap();
    match response {
        WsServerMessage::Error { message } => {
            assert!(message.contains("not started"));
        }
        _ => panic!("Expected Error variant"),
    }
}

#[tokio::test]
async fn test_handle_client_approval_approve() {
    let (agent_tx, mut agent_rx) = mpsc::channel(16);
    let state = make_test_state(Some(agent_tx)).await;
    let (direct_tx, _direct_rx) = mpsc::channel(16);

    let request_id = Uuid::new_v4();
    handle_client_message(
        WsClientMessage::Approval {
            request_id: request_id.to_string(),
            action: "approve".to_string(),
            thread_id: Some("thread-42".to_string()),
        },
        &state,
        "user1",
        &direct_tx,
    )
    .await;

    let incoming = agent_rx.recv().await.unwrap();
    // The content should be a serialized ExecApproval
    assert!(incoming.content.contains("ExecApproval"));
    // Thread should be forwarded onto the IncomingMessage.
    assert_eq!(incoming.thread_id.as_deref(), Some("thread-42"));
}

#[tokio::test]
async fn test_handle_client_approval_invalid_action() {
    let state = make_test_state(None).await;
    let (direct_tx, mut direct_rx) = mpsc::channel(16);

    handle_client_message(
        WsClientMessage::Approval {
            request_id: Uuid::new_v4().to_string(),
            action: "maybe".to_string(),
            thread_id: None,
        },
        &state,
        "user1",
        &direct_tx,
    )
    .await;

    let response = direct_rx.recv().await.unwrap();
    match response {
        WsServerMessage::Error { message } => {
            assert!(message.contains("Unknown approval action"));
        }
        _ => panic!("Expected Error variant"),
    }
}

#[tokio::test]
async fn test_handle_client_approval_invalid_uuid() {
    let state = make_test_state(None).await;
    let (direct_tx, mut direct_rx) = mpsc::channel(16);

    handle_client_message(
        WsClientMessage::Approval {
            request_id: "not-a-uuid".to_string(),
            action: "approve".to_string(),
            thread_id: None,
        },
        &state,
        "user1",
        &direct_tx,
    )
    .await;

    let response = direct_rx.recv().await.unwrap();
    match response {
        WsServerMessage::Error { message } => {
            assert!(message.contains("Invalid request_id"));
        }
        _ => panic!("Expected Error variant"),
    }
}

/// Helper to create a GatewayState for testing.
async fn make_test_state(msg_tx: Option<mpsc::Sender<IncomingMessage>>) -> GatewayState {
    use crate::channels::web::sse::SseManager;

    GatewayState {
        msg_tx: tokio::sync::RwLock::new(msg_tx),
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
        user_id: "test".to_string(),
        shutdown_tx: tokio::sync::RwLock::new(None),
        ws_tracker: Some(Arc::new(WsConnectionTracker::new())),
        llm_provider: None,
        skill_registry: None,
        skill_catalog: None,
        chat_rate_limiter: crate::channels::web::server::RateLimiter::new(30, 60),
        oauth_rate_limiter: crate::channels::web::server::RateLimiter::new(10, 60),
        registry_entries: Vec::new(),
        cost_guard: None,
        routine_engine: Arc::new(tokio::sync::RwLock::new(None)),
        startup_time: std::time::Instant::now(),
        feature_flags: Arc::new(tokio::sync::RwLock::new(
            crate::channels::web::handlers::feature_registry::FeatureFlagRegistry::new(),
        )),
    }
}
