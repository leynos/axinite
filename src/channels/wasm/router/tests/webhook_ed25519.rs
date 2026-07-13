//! Integration tests for Discord-style Ed25519 webhook signature
//! verification in the webhook handler.

use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ed25519_dalek::Signer;
use tower::ServiceExt;

use crate::channels::wasm::router::{
    RegisteredEndpoint, WasmChannelRouter, create_wasm_channel_router,
};

use super::helpers::{create_test_channel, setup_discord_router, test_signing_key};

#[tokio::test]
async fn test_webhook_rejects_missing_sig_headers() {
    let (wasm_router, app) = setup_discord_router().await;

    // Register a signature key
    let signing_key = test_signing_key();
    let pub_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
    wasm_router
        .register_signature_key("discord", &pub_key_hex)
        .await
        .unwrap();

    // Send request without signature headers
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/discord")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"type":1}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Missing signature headers should return 401"
    );
}

#[tokio::test]
async fn test_webhook_rejects_invalid_signature() {
    let (wasm_router, app) = setup_discord_router().await;

    let signing_key = test_signing_key();
    let pub_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
    wasm_router
        .register_signature_key("discord", &pub_key_hex)
        .await
        .unwrap();

    let req = Request::builder()
        .method("POST")
        .uri("/webhook/discord")
        .header("content-type", "application/json")
        .header("x-signature-ed25519", "deadbeefdeadbeef")
        .header("x-signature-timestamp", "1234567890")
        .body(Body::from(r#"{"type":1}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Invalid signature should return 401"
    );
}

#[tokio::test]
async fn test_webhook_accepts_valid_signature() {
    let (wasm_router, app) = setup_discord_router().await;

    let signing_key = test_signing_key();
    let pub_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
    wasm_router
        .register_signature_key("discord", &pub_key_hex)
        .await
        .unwrap();

    // Use current timestamp so staleness check passes
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let timestamp = now_secs.to_string();
    let body_bytes = br#"{"type":1}"#;

    let mut message = Vec::new();
    message.extend_from_slice(timestamp.as_bytes());
    message.extend_from_slice(body_bytes);
    let signature = signing_key.sign(&message);
    let sig_hex = hex::encode(signature.to_bytes());

    let req = Request::builder()
        .method("POST")
        .uri("/webhook/discord")
        .header("content-type", "application/json")
        .header("x-signature-ed25519", &sig_hex)
        .header("x-signature-timestamp", &timestamp)
        .body(Body::from(&body_bytes[..]))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Should NOT be 401 — signature is valid (may be 500 since no WASM module)
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Valid signature should not return 401"
    );
}

#[tokio::test]
async fn test_webhook_skips_sig_for_no_key() {
    let (_wasm_router, app) = setup_discord_router().await;

    // No signature key registered — should not require signature
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/discord")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"type":1}"#))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Should NOT be 401 (may be 500 since no WASM module, but not auth failure)
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "No signature key registered — should skip sig check"
    );
}

#[tokio::test]
async fn test_webhook_sig_check_uses_body() {
    let (wasm_router, app) = setup_discord_router().await;

    let signing_key = test_signing_key();
    let pub_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
    wasm_router
        .register_signature_key("discord", &pub_key_hex)
        .await
        .unwrap();

    let timestamp = "1234567890";
    // Sign body A
    let body_a = br#"{"type":1}"#;
    let mut message = Vec::new();
    message.extend_from_slice(timestamp.as_bytes());
    message.extend_from_slice(body_a);
    let signature = signing_key.sign(&message);
    let sig_hex = hex::encode(signature.to_bytes());

    // But send body B
    let body_b = br#"{"type":2}"#;
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/discord")
        .header("content-type", "application/json")
        .header("x-signature-ed25519", &sig_hex)
        .header("x-signature-timestamp", timestamp)
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
async fn test_webhook_sig_check_uses_timestamp() {
    let (wasm_router, app) = setup_discord_router().await;

    let signing_key = test_signing_key();
    let pub_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
    wasm_router
        .register_signature_key("discord", &pub_key_hex)
        .await
        .unwrap();

    // Sign with timestamp A
    let timestamp_a = "1234567890";
    let body = br#"{"type":1}"#;
    let mut message = Vec::new();
    message.extend_from_slice(timestamp_a.as_bytes());
    message.extend_from_slice(body);
    let signature = signing_key.sign(&message);
    let sig_hex = hex::encode(signature.to_bytes());

    // But send timestamp B in the header
    let timestamp_b = "9999999999";
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/discord")
        .header("content-type", "application/json")
        .header("x-signature-ed25519", &sig_hex)
        .header("x-signature-timestamp", timestamp_b)
        .body(Body::from(&body[..]))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Signature with mismatched timestamp should return 401"
    );
}

#[tokio::test]
async fn test_webhook_sig_plus_secret() {
    let wasm_router = Arc::new(WasmChannelRouter::new());
    let channel = create_test_channel("discord");

    let endpoints = vec![RegisteredEndpoint {
        channel_name: "discord".to_string(),
        path: "/webhook/discord".to_string(),
        methods: vec!["POST".to_string()],
        require_secret: true,
    }];

    // Register with BOTH secret and signature key
    wasm_router
        .register(channel, endpoints, Some("my-secret".to_string()), None)
        .await;

    let signing_key = test_signing_key();
    let pub_key_hex = hex::encode(signing_key.verifying_key().to_bytes());
    wasm_router
        .register_signature_key("discord", &pub_key_hex)
        .await
        .unwrap();

    let app = create_wasm_channel_router(wasm_router.clone(), None);

    // Use current timestamp so staleness check passes
    let now_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let timestamp = now_secs.to_string();
    let body = br#"{"type":1}"#;
    let mut message = Vec::new();
    message.extend_from_slice(timestamp.as_bytes());
    message.extend_from_slice(body);
    let signature = signing_key.sign(&message);
    let sig_hex = hex::encode(signature.to_bytes());

    // Provide valid signature AND valid secret
    let req = Request::builder()
        .method("POST")
        .uri("/webhook/discord?secret=my-secret")
        .header("content-type", "application/json")
        .header("x-signature-ed25519", &sig_hex)
        .header("x-signature-timestamp", &timestamp)
        .body(Body::from(&body[..]))
        .unwrap();

    let resp = app.oneshot(req).await.unwrap();
    // Should pass both checks (may be 500 due to no WASM module, but not 401)
    assert_ne!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "Valid secret + valid signature should not return 401"
    );
}
