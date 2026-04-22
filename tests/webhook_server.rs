//! Integration tests for WebhookServer.

use std::net::SocketAddr;
use std::net::TcpListener as StdTcpListener;

use rstest::{fixture, rstest};

#[path = "support/webhook.rs"]
mod support;

use support::webhook_server_helpers::{StartedWebhookServer, start_health_server};

/// Binds an ephemeral port, creates a [`WebhookServer`] with a `/health`
/// route, starts the server on the already-bound listener, and returns the
/// address and a client.
#[fixture]
async fn started_webhook_server()
-> Result<StartedWebhookServer, Box<dyn std::error::Error + Send + Sync>> {
    start_health_server().await
}

#[rstest]
#[tokio::test]
async fn test_restart_with_addr_rebinds_listener(
    #[future] started_webhook_server: Result<
        StartedWebhookServer,
        Box<dyn std::error::Error + Send + Sync>,
    >,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let StartedWebhookServer {
        mut server,
        addr: addr1,
        client,
    } = started_webhook_server.await?;

    assert_eq!(
        server.current_addr(),
        addr1,
        "Server should be bound to initial address"
    );

    let response = client
        .get(format!("http://{}/health", addr1))
        .send()
        .await?;
    assert_eq!(
        response.status(),
        200,
        "First server should respond to health check"
    );

    // Find a second available port and restart.
    // NOTE: This allocates an ephemeral port via StdTcpListener and then drops
    // the listener, which creates a TOCTOU race: another process could claim the
    // port before restart_with_addr binds to it. This is unavoidable for testing
    // restart_with_addr (which accepts an address, not a bound listener). The test
    // accepts this risk because the probability of collision on an ephemeral port
    // in a controlled test environment is acceptably low.
    let port2 = {
        let listener = StdTcpListener::bind("127.0.0.1:0")?;
        listener.local_addr()?.port()
    };
    let addr2: SocketAddr = format!("127.0.0.1:{}", port2).parse()?;

    server.restart_with_addr(addr2).await?;

    assert_eq!(
        server.current_addr(),
        addr2,
        "Server address should be updated after restart"
    );
    assert_ne!(
        addr1, addr2,
        "Address should change after restart_with_addr"
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
    match old_result {
        // Timeout expired — the old address no longer accepts connections.
        Err(_) => {}
        // Request reached the client stack but the old listener was gone.
        Ok(Err(_)) => {}
        Ok(Ok(resp)) => {
            panic!(
                "Old address should not respond after server restarts, got status {}",
                resp.status()
            );
        }
    }

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
    let StartedWebhookServer {
        mut server,
        addr: addr1,
        client,
    } = started_webhook_server.await?;

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
