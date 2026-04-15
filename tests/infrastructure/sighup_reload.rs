//! Integration tests for SIGHUP hot-reload of HTTP webhook configuration.
//!
//! Exercises the reload path end-to-end by driving `WebhookServer` and
//! `HttpChannel` directly — no live binary spawning.

use std::net::{SocketAddr, TcpListener as StdTcpListener};
use std::time::Duration;

use axum::http::StatusCode;
use reqwest::Client;
use secrecy::SecretString;
use serde_json::json;

use ironclaw::channels::{HttpChannel, NativeChannel, WebhookServer, WebhookServerConfig};
use ironclaw::config::HttpConfig;
use rstest::{fixture, rstest};

use crate::support::webhook_helpers;

/// Build a minimal health-check server using the given already-bound listener.
/// Returns the started server and the bound address.
async fn health_server(
    listener: tokio::net::TcpListener,
) -> Result<(WebhookServer, SocketAddr), Box<dyn std::error::Error>> {
    let addr = listener.local_addr()?;
    let config = WebhookServerConfig { addr };
    let mut server = WebhookServer::new(config);
    server.add_routes(webhook_helpers::health_routes());
    server.start_with_listener(listener).await?;
    Ok((server, addr))
}

/// POST a webhook payload and return the HTTP status.
async fn post_webhook(
    client: &Client,
    addr: SocketAddr,
    secret: &str,
) -> Result<reqwest::StatusCode, reqwest::Error> {
    Ok(client
        .post(format!("http://{}/webhook", addr))
        .json(&json!({"content": "hello", "secret": secret}))
        .send()
        .await?
        .status())
}

#[fixture]
fn http_client() -> Result<Client, reqwest::Error> {
    webhook_helpers::test_http_client()
}

#[rstest]
#[tokio::test]
async fn test_sighup_config_reload_address_change(
    http_client: Result<Client, reqwest::Error>,
) -> Result<(), Box<dyn std::error::Error>> {
    let http_client = http_client?;
    let listener1 = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let (mut server, addr1) = health_server(listener1).await?;

    // Confirm first address responds.
    let resp = http_client
        .get(format!("http://{}/health", addr1))
        .send()
        .await
        .expect("health check");
    assert_eq!(resp.status(), StatusCode::OK);

    // Restart on a second ephemeral port.
    let listener2 = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr2 = listener2.local_addr()?;
    server
        .restart_with_listener(listener2)
        .await
        .expect("restart");

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
    Ok(())
}

#[rstest]
#[tokio::test]
async fn test_sighup_secret_update_zero_downtime(
    http_client: Result<Client, reqwest::Error>,
) -> Result<(), Box<dyn std::error::Error>> {
    let http_client = http_client?;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;

    let channel = HttpChannel::new(HttpConfig {
        host: "127.0.0.1".to_string(),
        port: addr.port(),
        webhook_secret: Some(SecretString::from("old-secret".to_string())),
        user_id: "test-user".to_string(),
    });

    // Start the channel so the internal sender is populated.
    // `_stream` is intentionally kept to hold the returned `MessageStream` alive,
    // ensuring the `HttpChannel`'s internal sender/registration is not dropped
    // and the channel lifecycle remains active for the duration of the test.
    let _stream = channel.start().await.expect("start channel");

    let mut server = WebhookServer::new(WebhookServerConfig { addr });
    server.add_routes(channel.routes());
    server.start_with_listener(listener).await?;

    // Old secret should be accepted.
    let status = post_webhook(&http_client, addr, "old-secret").await?;
    assert_eq!(status, StatusCode::OK, "old secret should work initially");

    // Hot-swap secret via the public API.
    channel
        .update_secret(Some(SecretString::from("new-secret".to_string())))
        .await;

    // Old secret should now be rejected.
    let status = post_webhook(&http_client, addr, "old-secret").await?;
    assert_eq!(
        status,
        StatusCode::UNAUTHORIZED,
        "old secret should fail after swap"
    );

    // New secret should be accepted.
    let status = post_webhook(&http_client, addr, "new-secret").await?;
    assert_eq!(status, StatusCode::OK, "new secret should work after swap");

    server.shutdown().await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn test_sighup_rollback_on_address_bind_failure(
    http_client: Result<Client, reqwest::Error>,
) -> Result<(), Box<dyn std::error::Error>> {
    let http_client = http_client?;
    let listener1 = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let (mut server, addr1) = health_server(listener1).await?;

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
    Ok(())
}
