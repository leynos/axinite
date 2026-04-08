//! Integration tests for SIGHUP hot-reload of HTTP webhook configuration.
//!
//! Exercises the reload path end-to-end by driving `WebhookServer` and
//! `HttpChannelState` directly — no live binary spawning.

#![cfg(unix)]

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::time::Duration;

use axum::Json;
use axum::http::StatusCode;
use axum::routing::get;
use reqwest::Client;
use secrecy::SecretString;
use serde_json::json;

use ironclaw::channels::{HttpChannel, NativeChannel, WebhookServer, WebhookServerConfig};
use ironclaw::config::HttpConfig;
use rstest::{fixture, rstest};

/// Obtain an ephemeral local address by binding a `StdTcpListener` on port 0,
/// reading the assigned `SocketAddr`, and immediately dropping the listener.
///
/// **TOCTOU race:** because the listener is dropped before the caller binds the
/// real server, another process on the same host may claim the same port in the
/// gap. This is a common test pattern for obtaining free ports, but it can
/// produce flaky failures under concurrent load. Use with that caveat in mind.
fn ephemeral_addr() -> SocketAddr {
    let listener = StdTcpListener::bind("127.0.0.1:0").expect("bind ephemeral port");
    listener.local_addr().expect("local_addr")
}

/// Build a minimal health-check server on the given address.
fn health_server(addr: SocketAddr) -> WebhookServer {
    let mut server = WebhookServer::new(WebhookServerConfig { addr });
    server.add_routes(
        axum::Router::new().route("/health", get(|| async { Json(json!({"status": "ok"})) })),
    );
    server
}

/// POST a webhook payload and return the HTTP status.
async fn post_webhook(client: &Client, addr: SocketAddr, secret: &str) -> reqwest::StatusCode {
    client
        .post(format!("http://{}/webhook", addr))
        .json(&json!({"content": "hello", "secret": secret}))
        .send()
        .await
        .expect("webhook request")
        .status()
}

#[fixture]
fn http_client() -> Client {
    Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("build client")
}

#[rstest]
#[tokio::test]
async fn test_sighup_config_reload_address_change(http_client: Client) {
    let addr1 = ephemeral_addr();
    let mut server = health_server(addr1);
    server.start().await.expect("start on first address");

    // Confirm first address responds.
    let resp = http_client
        .get(format!("http://{}/health", addr1))
        .send()
        .await
        .expect("health check");
    assert_eq!(resp.status(), StatusCode::OK);

    // Restart on a second ephemeral port.
    let addr2 = ephemeral_addr();
    server.restart_with_addr(addr2).await.expect("restart");

    // New address should respond.
    let resp = http_client
        .get(format!("http://{}/health", addr2))
        .send()
        .await
        .expect("health check on new address");
    assert_eq!(resp.status(), StatusCode::OK, "new address should respond");

    // Old address should refuse connections.
    let old_result = tokio::time::timeout(
        Duration::from_millis(200),
        http_client.get(format!("http://{}/health", addr1)).send(),
    )
    .await;

    match old_result {
        // Timeout expired — the old address no longer accepts connections.
        Err(_) => {}
        // Request reached the client stack but the old listener was gone.
        Ok(Err(_)) => {}
        Ok(Ok(resp)) => {
            panic!(
                "old address should not respond after restart, got status {}",
                resp.status()
            );
        }
    }

    server.shutdown().await;
}

#[rstest]
#[tokio::test]
async fn test_sighup_secret_update_zero_downtime(http_client: Client) {
    let addr = ephemeral_addr();

    let channel = HttpChannel::new(HttpConfig {
        host: "127.0.0.1".to_string(),
        port: addr.port(),
        webhook_secret: Some(SecretString::from("old-secret".to_string())),
        user_id: "test-user".to_string(),
    });

    // Start the channel so the internal sender is populated.
    let _stream = channel.start().await.expect("start channel");
    let state = channel.shared_state();

    let mut server = WebhookServer::new(WebhookServerConfig { addr });
    server.add_routes(channel.routes());
    server.start().await.expect("start webhook server");

    // Old secret should be accepted.
    let status = post_webhook(&http_client, addr, "old-secret").await;
    assert_eq!(status, StatusCode::OK, "old secret should work initially");

    // Hot-swap secret.
    state
        .update_secret(Some(SecretString::from("new-secret".to_string())))
        .await;

    // Old secret should now be rejected.
    let status = post_webhook(&http_client, addr, "old-secret").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "old secret should fail after swap"
    );

    // New secret should be accepted.
    let status = post_webhook(&http_client, addr, "new-secret").await;
    assert_eq!(status, StatusCode::OK, "new secret should work after swap");

    server.shutdown().await;
}

#[rstest]
#[tokio::test]
async fn test_sighup_rollback_on_address_bind_failure(http_client: Client) {
    let addr1 = ephemeral_addr();
    let mut server = health_server(addr1);
    server.start().await.expect("start on first address");

    // Confirm initial address works.
    let resp = http_client
        .get(format!("http://{}/health", addr1))
        .send()
        .await
        .expect("health check");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "initial address should respond"
    );

    // Occupy a second ephemeral port so bind deterministically fails.
    let occupied = StdTcpListener::bind("127.0.0.1:0").expect("bind conflict port");
    let conflict_addr = occupied.local_addr().expect("conflict local_addr");

    let result = server.restart_with_addr(conflict_addr).await;
    assert!(result.is_err(), "restart to occupied port should fail");

    drop(occupied);

    // Original listener must still respond.
    let resp = http_client
        .get(format!("http://{}/health", addr1))
        .send()
        .await
        .expect("health check after failed restart");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "original address should still respond after failed restart"
    );

    assert_eq!(
        server.current_addr(),
        addr1,
        "server address should be restored after failed restart"
    );

    server.shutdown().await;
}
