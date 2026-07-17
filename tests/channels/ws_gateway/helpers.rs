//! Shared helpers for the WebSocket gateway integration tests: test-server
//! startup, authenticated client connection, and frame reception.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

use axinite::channels::IncomingMessage;
use axinite::channels::web::server::{GatewayState, start_server};
use axinite::channels::web::sse::SseManager;
use axinite::channels::web::ws::WsConnectionTracker;

pub(super) const AUTH_TOKEN: &str = "test-token-12345";
pub(super) const TIMEOUT: Duration = Duration::from_secs(5);

/// Start a gateway server on a random port and return the bound address + agent
/// message receiver.
pub(super) async fn start_test_server() -> (
    SocketAddr,
    Arc<GatewayState>,
    mpsc::Receiver<IncomingMessage>,
) {
    let (agent_tx, agent_rx) = mpsc::channel(64);

    let state = Arc::new(GatewayState {
        msg_tx: tokio::sync::RwLock::new(Some(agent_tx)),
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
        llm_provider: None,
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
    let bound_addr = start_server(addr, state.clone(), AUTH_TOKEN.to_string())
        .await
        .expect("Failed to start test server");

    (bound_addr, state, agent_rx)
}

/// Connect a WebSocket client with auth token in query parameter.
pub(super) async fn connect_ws(
    addr: SocketAddr,
) -> tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>> {
    let url = format!("ws://{}/api/chat/ws?token={}", addr, AUTH_TOKEN);
    let mut request = url.into_client_request().unwrap();
    // Server requires an Origin header from localhost to prevent cross-site WS hijacking.
    request.headers_mut().insert(
        "Origin",
        format!("http://127.0.0.1:{}", addr.port()).parse().unwrap(),
    );
    let (stream, _response) = tokio_tungstenite::connect_async(request)
        .await
        .expect("Failed to connect WebSocket");
    stream
}

/// Read the next text frame from the WebSocket, with a timeout.
pub(super) async fn recv_text(
    stream: &mut (impl StreamExt<Item = Result<Message, tokio_tungstenite::tungstenite::Error>> + Unpin),
) -> String {
    let msg = timeout(TIMEOUT, stream.next())
        .await
        .expect("Timed out waiting for WS message")
        .expect("Stream ended")
        .expect("WS error");
    match msg {
        Message::Text(text) => text.to_string(),
        other => panic!("Expected Text frame, got {:?}", other),
    }
}
