//! Unit tests for HTTP channel request handling and authentication.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use secrecy::SecretString;
use tower::ServiceExt;

use super::*;

fn test_channel(secret: Option<&str>) -> HttpChannel {
    HttpChannel::new(HttpConfig {
        host: "127.0.0.1".to_string(),
        port: 0,
        webhook_secret: secret.map(|s| SecretString::from(s.to_string())),
        user_id: "http".to_string(),
    })
}

#[tokio::test]
async fn test_http_channel_requires_secret() {
    let channel = test_channel(None);
    let result = channel.start().await;
    assert!(result.is_err());
}

#[tokio::test]
async fn webhook_correct_secret_returns_ok() {
    let channel = test_channel(Some("test-secret-123"));
    // Start the channel so the tx sender is populated (otherwise 503).
    let _stream = channel.start().await.unwrap();
    let app = channel.routes();

    let body = serde_json::json!({
        "content": "hello",
        "secret": "test-secret-123"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn webhook_wrong_secret_returns_unauthorized() {
    let channel = test_channel(Some("correct-secret"));
    let _stream = channel.start().await.unwrap();
    let app = channel.routes();

    let body = serde_json::json!({
        "content": "hello",
        "secret": "wrong-secret"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn webhook_missing_secret_returns_unauthorized() {
    let channel = test_channel(Some("correct-secret"));
    let _stream = channel.start().await.unwrap();
    let app = channel.routes();

    let body = serde_json::json!({
        "content": "hello"
    });
    let req = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body).unwrap()))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn test_update_secret_hot_swap() {
    let channel = test_channel(Some("old-secret"));
    let _stream = channel.start().await.unwrap();
    let app1 = channel.routes();

    // Request with old-secret should succeed
    let body_old = serde_json::json!({
        "content": "hello",
        "secret": "old-secret"
    });
    let req1 = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body_old).unwrap()))
        .unwrap();
    let resp1 = app1.oneshot(req1).await.unwrap();
    assert_eq!(
        resp1.status(),
        StatusCode::OK,
        "old secret should work initially"
    );

    // Update secret to new-secret
    channel
        .update_secret(Some(SecretString::from("new-secret".to_string())))
        .await;

    let app2 = channel.routes();

    // Request with old-secret should fail
    let req2 = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body_old).unwrap()))
        .unwrap();
    let resp2 = app2.oneshot(req2).await.unwrap();
    assert_eq!(
        resp2.status(),
        StatusCode::UNAUTHORIZED,
        "old secret should fail after update"
    );

    let app3 = channel.routes();

    // Request with new-secret should succeed
    let body_new = serde_json::json!({
        "content": "hello",
        "secret": "new-secret"
    });
    let req3 = Request::builder()
        .method("POST")
        .uri("/webhook")
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_vec(&body_new).unwrap()))
        .unwrap();
    let resp3 = app3.oneshot(req3).await.unwrap();
    assert_eq!(
        resp3.status(),
        StatusCode::OK,
        "new secret should work after update"
    );
}

#[tokio::test]
async fn test_concurrent_requests_during_secret_update() {
    use std::sync::Arc as StdArc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    let channel = test_channel(Some("initial-secret"));
    let _stream = channel.start().await.unwrap();
    let app = channel.routes();

    // Counters for request outcomes
    let success_count = StdArc::new(AtomicUsize::new(0));

    let mut handles = vec![];

    // Spawn 5 concurrent tasks that keep making requests with the initial secret
    for i in 0..5 {
        let app = app.clone();
        let success = StdArc::clone(&success_count);

        let handle = tokio::spawn(async move {
            let body = serde_json::json!({
                "content": format!("test-{}", i),
                "secret": "initial-secret"
            });

            let req = Request::builder()
                .method("POST")
                .uri("/webhook")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            if resp.status() == StatusCode::OK {
                success.fetch_add(1, Ordering::SeqCst);
            }
        });
        handles.push(handle);
    }

    // Update secret mid-flight (tests that RwLock allows readers while writer holds lock)
    tokio::time::sleep(Duration::from_millis(5)).await;
    channel
        .update_secret(Some(SecretString::from("updated-secret".to_string())))
        .await;

    // Spawn 5 more tasks that use the new secret
    for i in 5..10 {
        let app = app.clone();
        let success = StdArc::clone(&success_count);

        let handle = tokio::spawn(async move {
            let body = serde_json::json!({
                "content": format!("test-{}", i),
                "secret": "updated-secret"
            });

            let req = Request::builder()
                .method("POST")
                .uri("/webhook")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&body).unwrap()))
                .unwrap();

            let resp = app.oneshot(req).await.unwrap();
            if resp.status() == StatusCode::OK {
                success.fetch_add(1, Ordering::SeqCst);
            }
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        let _ = handle.await;
    }

    // Verify all requests succeeded with their respective secrets
    assert_eq!(
        success_count.load(Ordering::SeqCst),
        10,
        "All concurrent requests should succeed with correct secrets after update"
    );
}
