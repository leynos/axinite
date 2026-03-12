use axum::body::Body;
use rstest::rstest;
use tower::ServiceExt;

use super::super::*;
use super::fixtures::{
    TestGatewayStateFactory, TestRelayOAuthRouterFactory, build_test_ext_mgr,
    build_test_secrets_store, test_gateway_state, test_relay_oauth_router,
};

#[rstest]
#[tokio::test]
async fn test_relay_oauth_callback_missing_state_param(
    test_gateway_state: TestGatewayStateFactory,
    test_relay_oauth_router: TestRelayOAuthRouterFactory,
) {
    let ext_mgr = build_test_ext_mgr(build_test_secrets_store());
    let state = test_gateway_state.build(Some(ext_mgr), None);
    let app = test_relay_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri("/oauth/slack/callback?stream_token=tok123&team_id=T123&provider=slack")
        .body(Body::empty())
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("Invalid or expired authorization"),
        "Expected CSRF error, got: {}",
        &html[..html.len().min(300)]
    );
}

#[rstest]
#[tokio::test]
async fn test_relay_oauth_callback_wrong_state_param(
    test_gateway_state: TestGatewayStateFactory,
    test_relay_oauth_router: TestRelayOAuthRouterFactory,
) {
    let secrets = build_test_secrets_store();
    secrets
        .create(
            "test",
            crate::secrets::CreateSecretParams::new(
                format!("relay:{}:oauth_state", DEFAULT_RELAY_NAME),
                "correct-nonce-value",
            ),
        )
        .await
        .expect("store nonce");

    let ext_mgr = build_test_ext_mgr(secrets);
    let state = test_gateway_state.build(Some(ext_mgr), None);
    let app = test_relay_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri("/oauth/slack/callback?stream_token=tok123&team_id=T123&provider=slack&state=wrong-nonce")
        .body(Body::empty())
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("Invalid or expired authorization"),
        "Expected CSRF error for wrong nonce, got: {}",
        &html[..html.len().min(300)]
    );
}

#[rstest]
#[tokio::test]
async fn test_relay_oauth_callback_correct_state_proceeds(
    test_gateway_state: TestGatewayStateFactory,
    test_relay_oauth_router: TestRelayOAuthRouterFactory,
) {
    let secrets = build_test_secrets_store();
    let nonce = "valid-test-nonce-12345";

    secrets
        .create(
            "test",
            crate::secrets::CreateSecretParams::new(
                format!("relay:{}:oauth_state", DEFAULT_RELAY_NAME),
                nonce,
            ),
        )
        .await
        .expect("store nonce");

    let ext_mgr = build_test_ext_mgr(std::sync::Arc::clone(&secrets));
    let state = test_gateway_state.build(Some(ext_mgr), None);
    let app = test_relay_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri(format!(
            "/oauth/slack/callback?stream_token=tok123&team_id=T123&provider=slack&state={}",
            nonce
        ))
        .body(Body::empty())
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
    let html = String::from_utf8_lossy(&body);
    assert!(
        !html.contains("Invalid or expired authorization"),
        "Should have passed CSRF check, got: {}",
        &html[..html.len().min(300)]
    );

    let state_key = format!("relay:{}:oauth_state", DEFAULT_RELAY_NAME);
    let exists = secrets.exists("test", &state_key).await.unwrap_or(true);
    assert!(!exists, "CSRF nonce should be deleted after use");
}
