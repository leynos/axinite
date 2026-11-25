//! Tests for a channel that registers both an Ed25519 key and an HMAC secret:
//! the Ed25519 check runs first, and the HMAC check still gates the request.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use ed25519_dalek::Signer;
use http_body_util::BodyExt;
use tower::ServiceExt;

use super::helpers::{setup_slack_router, slack_signature, test_signing_key};

const SIGNING_SECRET: &str = "my-signing-secret";
const PAYLOAD: &str = "payload";

/// Send a POST to the Slack route with the given headers; return the status and `error` text.
async fn post_with(app: axum::Router, headers: &[(&str, String)]) -> (StatusCode, String) {
    let mut builder = Request::builder()
        .method("POST")
        .uri("/webhook/slack")
        .header("content-type", "application/json");
    for (name, value) in headers {
        builder = builder.header(*name, value);
    }
    let resp = app
        .oneshot(builder.body(Body::from(PAYLOAD)).expect("request"))
        .await
        .expect("response");
    let status = resp.status();
    let bytes = resp.into_body().collect().await.expect("body").to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    let error = json["error"].as_str().unwrap_or_default().to_string();
    (status, error)
}

fn now_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock")
        .as_secs()
        .to_string()
}

async fn router_with_both_checks() -> axum::Router {
    let (wasm_router, app) = setup_slack_router().await;
    let pub_key_hex = hex::encode(test_signing_key().verifying_key().to_bytes());
    wasm_router
        .register_signature_key("slack", &pub_key_hex)
        .await
        .expect("key registers");
    wasm_router
        .register_hmac_secret("slack", SIGNING_SECRET)
        .await;
    app
}

#[tokio::test]
async fn ed25519_is_checked_before_hmac() {
    let app = router_with_both_checks().await;
    let ts = now_secs();
    let hmac = slack_signature(SIGNING_SECRET, &ts, PAYLOAD.as_bytes());

    // A valid HMAC cannot rescue a request that lacks the Ed25519 headers.
    let (status, error) = post_with(
        app,
        &[
            ("x-slack-request-timestamp", ts),
            ("x-slack-signature", hmac),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error, "Missing signature headers");
}

#[tokio::test]
async fn valid_ed25519_still_needs_a_valid_hmac() {
    let app = router_with_both_checks().await;
    let ts = now_secs();
    let mut signed = ts.clone().into_bytes();
    signed.extend_from_slice(PAYLOAD.as_bytes());
    let sig = hex::encode(test_signing_key().sign(&signed).to_bytes());

    let (status, error) = post_with(
        app,
        &[
            ("x-signature-ed25519", sig),
            ("x-signature-timestamp", ts.clone()),
            ("x-slack-request-timestamp", ts),
            ("x-slack-signature", "v0=deadbeef".to_string()),
        ],
    )
    .await;

    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(error, "Invalid Slack signature");
}
