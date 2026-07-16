//! Integration tests for Slack-style HMAC-SHA256 webhook signature
//! verification in the webhook handler.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::ServiceExt;

use super::helpers::{setup_slack_router, slack_signature};

#[tokio::test]
async fn test_webhook_hmac_rejects_missing_sig_headers() {
    let (wasm_router, app) = setup_slack_router().await;

    wasm_router
        .register_hmac_secret("slack", "my-signing-secret")
        .await;

    // Send request without HMAC signature headers
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/slack")
        .header("content-type", "application/json")
        .body(Body::from("token=xyzz0WbapA4vBCDEFasx0q6G"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Missing HMAC signature headers should return 401"
    );
}

#[tokio::test]
async fn test_webhook_hmac_rejects_invalid_signature() {
    let (wasm_router, app) = setup_slack_router().await;

    wasm_router
        .register_hmac_secret("slack", "my-signing-secret")
        .await;

    let req = Request::builder()
        .method("POST")
        .uri("/webhook/slack")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", "1234567890")
        .header("x-slack-signature", "v0=deadbeefdeadbeef")
        .body(Body::from("token=xyzz0WbapA4vBCDEFasx0q6G"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Invalid HMAC signature should return 401"
    );
}

#[tokio::test]
async fn test_webhook_hmac_accepts_valid_signature() {
    let (wasm_router, app) = setup_slack_router().await;

    let signing_secret = "my-signing-secret";
    wasm_router
        .register_hmac_secret("slack", signing_secret)
        .await;

    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let timestamp = now_secs.to_string();
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    let signature = slack_signature(signing_secret, &timestamp, body);

    let req = Request::builder()
        .method("POST")
        .uri("/webhook/slack")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", &timestamp)
        .header("x-slack-signature", &signature)
        .body(Body::from(&body[..]))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Should NOT be 401 — signature is valid (may be 500 since no WASM module)
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Valid HMAC signature should not return 401"
    );
}

#[tokio::test]
async fn test_webhook_hmac_skips_check_for_no_secret() {
    let (_wasm_router, app) = setup_slack_router().await;

    // No HMAC secret registered — should not require signature
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/slack")
        .header("content-type", "application/json")
        .body(Body::from("token=xyzz0WbapA4vBCDEFasx0q6G"))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Should NOT be 401 (may be 500 since no WASM module, but not auth failure)
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "No HMAC secret registered — should skip check"
    );
}

#[tokio::test]
async fn test_webhook_hmac_uses_correct_body() {
    let (wasm_router, app) = setup_slack_router().await;

    let signing_secret = "my-signing-secret";
    wasm_router
        .register_hmac_secret("slack", signing_secret)
        .await;

    let timestamp = "1234567890";
    let body_a = b"token=xyzz0WbapA4vBCDEFasx0q6G";
    let body_b = b"token=MODIFIED";

    // Sign body A
    let signature = slack_signature(signing_secret, timestamp, body_a);

    // But send body B
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/slack")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", timestamp)
        .header("x-slack-signature", &signature)
        .body(Body::from(&body_b[..]))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Signature for different body should return 401"
    );
}

#[tokio::test]
async fn test_webhook_hmac_uses_correct_timestamp() {
    let (wasm_router, app) = setup_slack_router().await;

    let signing_secret = "my-signing-secret";
    wasm_router
        .register_hmac_secret("slack", signing_secret)
        .await;

    let timestamp_a = "1234567890";
    let timestamp_b = "9999999999";
    let body = b"token=xyzz0WbapA4vBCDEFasx0q6G";

    // Sign with timestamp A
    let signature = slack_signature(signing_secret, timestamp_a, body);

    // But send timestamp B in the header
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/slack")
        .header("content-type", "application/json")
        .header("x-slack-request-timestamp", timestamp_b)
        .header("x-slack-signature", &signature)
        .body(Body::from(&body[..]))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Signature with mismatched timestamp should return 401"
    );
}
