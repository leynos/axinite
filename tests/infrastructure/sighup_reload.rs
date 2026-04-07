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
use secrecy::SecretString;
use serde_json::json;

use ironclaw::channels::{HttpChannel, NativeChannel, WebhookServer, WebhookServerConfig};
use ironclaw::config::HttpConfig;

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
async fn post_webhook(
    client: &reqwest::Client,
    addr: SocketAddr,
    secret: &str,
) -> reqwest::StatusCode {
    client
        .post(format!("http://{}/webhook", addr))
        .json(&json!({"content": "hello", "secret": secret}))
        .send()
        .await
        .expect("webhook request")
        .status()
}

#[tokio::test]
async fn test_sighup_config_reload_address_change() {
    let addr1 = ephemeral_addr();
    let mut server = health_server(addr1);
    server.start().await.expect("start on first address");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("build client");

    // Confirm first address responds.
    let resp = client
        .get(format!("http://{}/health", addr1))
        .send()
        .await
        .expect("health check");
    assert_eq!(resp.status(), StatusCode::OK);

    // Restart on a second ephemeral port.
    let addr2 = ephemeral_addr();
    server.restart_with_addr(addr2).await.expect("restart");

    // New address should respond.
    let resp = client
        .get(format!("http://{}/health", addr2))
        .send()
        .await
        .expect("health check on new address");
    assert_eq!(resp.status(), StatusCode::OK, "new address should respond");

    // Old address should refuse connections.
    let old_result = tokio::time::timeout(
        Duration::from_millis(200),
        client.get(format!("http://{}/health", addr1)).send(),
    )
    .await;
    assert!(
        old_result.is_err() || old_result.ok().and_then(|r| r.ok()).is_none(),
        "old address should not respond after restart"
    );

    server.shutdown().await;
}

#[tokio::test]
async fn test_sighup_secret_update_zero_downtime() {
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

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("build client");

    // Old secret should be accepted.
    let status = post_webhook(&client, addr, "old-secret").await;
    assert_eq!(status, StatusCode::OK, "old secret should work initially");

    // Hot-swap secret.
    state
        .update_secret(Some(SecretString::from("new-secret".to_string())))
        .await;

    // Old secret should now be rejected.
    let status = post_webhook(&client, addr, "old-secret").await;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "old secret should fail after swap"
    );

    // New secret should be accepted.
    let status = post_webhook(&client, addr, "new-secret").await;
    assert_eq!(status, StatusCode::OK, "new secret should work after swap");

    server.shutdown().await;
}

#[tokio::test]
async fn test_sighup_rollback_on_address_bind_failure() {
    let addr1 = ephemeral_addr();
    let mut server = health_server(addr1);
    server.start().await.expect("start on first address");

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .expect("build client");

    // Confirm initial address works.
    let resp = client
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
    let resp = client
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
