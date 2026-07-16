//! Unit tests for webhook server startup and health routes.

use std::net::TcpListener as StdTcpListener;

use axum::Json;
use rstest::{fixture, rstest};
use serde_json::json;

use super::*;

/// A started webhook server with a `/health` route and a pre-built client.
struct StartedWebhookServer {
    server: WebhookServer,
    client: reqwest::Client,
}

/// Bind an ephemeral localhost listener and keep it reserved until the
/// server takes ownership, eliminating port-probe races in tests.
async fn bind_ephemeral_listener()
-> Result<(tokio::net::TcpListener, SocketAddr), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    Ok((listener, addr))
}

/// Create a [`WebhookServer`] with a `/health` route, start it on a
/// pre-bound ephemeral listener, and return the server and a client.
#[fixture]
async fn started_webhook_server()
-> Result<StartedWebhookServer, Box<dyn std::error::Error + Send + Sync>> {
    let (listener, _) = bind_ephemeral_listener().await?;
    let mut server = WebhookServer::new(WebhookServerConfig {
        addr: "127.0.0.1:0".parse()?,
    });
    server.add_routes(Router::new().route(
        "/health",
        axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
    ));
    server.start_with_listener(listener).await?;
    Ok(StartedWebhookServer {
        server,
        client: reqwest::Client::new(),
    })
}

#[rstest]
#[tokio::test]
async fn test_start_binds_ephemeral_addr_and_serves_health_check()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut server = WebhookServer::new(WebhookServerConfig {
        addr: "127.0.0.1:0".parse()?,
    });
    server.add_routes(Router::new().route(
        "/health",
        axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
    ));

    server.start().await?;
    let addr = server.current_addr();

    assert_ne!(
        addr.port(),
        0,
        "Server should resolve an ephemeral bind to a concrete port"
    );

    let response = reqwest::Client::new()
        .get(format!("http://{}/health", addr))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        200,
        "Server should respond to health check after start()"
    );

    server.shutdown().await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn test_start_and_restart_with_addr_use_production_bind_path()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut server = WebhookServer::new(WebhookServerConfig {
        addr: "127.0.0.1:0".parse()?,
    });
    server.add_routes(Router::new().route(
        "/health",
        axum::routing::get(|| async { Json(json!({"status": "ok"})) }),
    ));

    server.start().await?;
    let addr1 = server.current_addr();
    assert_ne!(
        addr1.port(),
        0,
        "Server should resolve the initial ephemeral bind to a concrete port"
    );

    let client = reqwest::Client::new();
    let response = client
        .get(format!("http://{}/health", addr1))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        200,
        "Server should respond to health check after start()"
    );

    let restart_addr: SocketAddr = "127.0.0.1:0".parse()?;
    server.restart_with_addr(restart_addr).await?;
    let addr2 = server.current_addr();

    assert_ne!(
        addr2.port(),
        0,
        "Server should resolve the restarted ephemeral bind to a concrete port"
    );
    assert_ne!(
        addr1, addr2,
        "Address should change after restart_with_addr"
    );

    let old_result = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        client.get(format!("http://{}/health", addr1)).send(),
    )
    .await;
    assert!(
        matches!(old_result, Err(_) | Ok(Err(_))),
        "Old address should not respond after restart_with_addr"
    );

    let response = client
        .get(format!("http://{}/health", addr2))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        200,
        "Server should respond to health check after restart_with_addr"
    );

    server.shutdown().await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn test_restart_with_listener(
    #[future] started_webhook_server: Result<
        StartedWebhookServer,
        Box<dyn std::error::Error + Send + Sync>,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let StartedWebhookServer { mut server, client } = started_webhook_server.await?;
    let addr1 = server.current_addr();

    assert_eq!(
        server.current_addr(),
        addr1,
        "Server should be bound to the initial address before restart_with_listener"
    );

    let response = client
        .get(format!("http://{}/health", addr1))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        200,
        "Server should respond to health check before restart_with_listener"
    );

    let (listener, addr2) = bind_ephemeral_listener().await?;
    server.restart_with_listener(listener).await?;

    assert_eq!(
        server.current_addr(),
        addr2,
        "Server address should be updated after restart_with_listener"
    );
    assert_ne!(
        addr1, addr2,
        "Address should change after restart_with_listener"
    );

    let response = client
        .get(format!("http://{}/health", addr2))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        200,
        "Restarted server should respond to health check on new address"
    );

    let old_result = tokio::time::timeout(
        std::time::Duration::from_millis(200),
        client.get(format!("http://{}/health", addr1)).send(),
    )
    .await;
    assert!(
        matches!(old_result, Err(_) | Ok(Err(_))),
        "Old address should not respond after server restarts"
    );

    server.shutdown().await;
    Ok(())
}

#[rstest]
#[tokio::test]
async fn test_restart_with_addr_rollback_on_bind_failure(
    #[future] started_webhook_server: Result<
        StartedWebhookServer,
        Box<dyn std::error::Error + Send + Sync>,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let StartedWebhookServer { mut server, client } = started_webhook_server.await?;
    let addr1 = server.current_addr();

    let response = client
        .get(format!("http://{}/health", addr1))
        .send()
        .await?;
    assert_eq!(response.status(), 200, "Server should be listening");

    // Occupy a second port so the restart bind fails deterministically.
    let occupied_listener = StdTcpListener::bind("127.0.0.1:0")?;
    let conflict_addr = occupied_listener.local_addr()?;

    let result = server.restart_with_addr(conflict_addr).await;
    assert!(
        result.is_err(),
        "Restart with already-bound address should fail"
    );

    drop(occupied_listener);

    let response = client
        .get(format!("http://{}/health", addr1))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        200,
        "Old listener should still be running after failed restart"
    );

    assert_eq!(
        server.current_addr(),
        addr1,
        "Server address should be restored after failed restart"
    );

    server.shutdown().await;
    Ok(())
}
