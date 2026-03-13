//! Tests for the hosted gateway OAuth callback handler.

use axum::body::Body;
use rstest::rstest;
use tower::ServiceExt;

use super::fixtures::{
    TestGatewayStateFactory, TestOAuthRouterFactory, build_test_ext_mgr, build_test_secrets_store,
    expired_pending_oauth_flow, test_gateway_state, test_oauth_router,
};

#[rstest]
#[tokio::test]
async fn test_oauth_callback_missing_params(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
) {
    let state = test_gateway_state.build(None, None);
    let app = test_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri("/oauth/callback")
        .body(Body::empty())
        .expect("build OAuth callback request without query params");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("send OAuth callback request without query params");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("read OAuth callback response body without query params");
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Authorization Failed"));
}

#[rstest]
#[tokio::test]
async fn test_oauth_callback_error_from_provider(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
) {
    let state = test_gateway_state.build(None, None);
    let app = test_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri("/oauth/callback?error=access_denied&error_description=access_denied")
        .body(Body::empty())
        .expect("build OAuth callback request with provider error");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("send OAuth callback request with provider error");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("read OAuth callback provider error response body");
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Authorization Failed"));
}

#[rstest]
#[tokio::test]
async fn test_oauth_callback_unknown_state(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
) {
    let ext_mgr = build_test_ext_mgr(build_test_secrets_store());
    let state = test_gateway_state.build(Some(ext_mgr), None);
    let app = test_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri("/oauth/callback?code=test_code&state=unknown_state_value")
        .body(Body::empty())
        .expect("build OAuth callback request with unknown state");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("send OAuth callback request with unknown state");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("read OAuth callback unknown-state response body");
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Authorization Failed"));
}

#[rstest]
#[tokio::test]
async fn test_oauth_callback_expired_flow(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
) {
    let secrets = build_test_secrets_store();
    let ext_mgr = build_test_ext_mgr(std::sync::Arc::clone(&secrets));

    let flow = expired_pending_oauth_flow(secrets);

    ext_mgr
        .pending_oauth_flows()
        .write()
        .await
        .insert("expired_state".to_string(), flow);

    let state = test_gateway_state.build(Some(ext_mgr), None);
    let app = test_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri("/oauth/callback?code=test_code&state=expired_state")
        .body(Body::empty())
        .expect("build OAuth callback request for expired flow");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("send OAuth callback request for expired flow");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("read OAuth callback expired-flow response body");
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Authorization Failed"));
}

#[rstest]
#[tokio::test]
async fn test_oauth_callback_no_extension_manager(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
) {
    let state = test_gateway_state.build(None, None);
    let app = test_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri("/oauth/callback?code=test_code&state=some_state")
        .body(Body::empty())
        .expect("build OAuth callback request without extension manager");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("send OAuth callback request without extension manager");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("read OAuth callback no-manager response body");
    let html = String::from_utf8_lossy(&body);
    assert!(html.contains("Authorization Failed"));
}

#[rstest]
#[tokio::test]
async fn test_oauth_callback_strips_instance_prefix(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
) {
    let secrets = build_test_secrets_store();
    let ext_mgr = build_test_ext_mgr(std::sync::Arc::clone(&secrets));

    let flow = expired_pending_oauth_flow(secrets);

    ext_mgr
        .pending_oauth_flows()
        .write()
        .await
        .insert("test_nonce".to_string(), flow);

    let state = test_gateway_state.build(Some(std::sync::Arc::clone(&ext_mgr)), None);
    let app = test_oauth_router.build(state);

    let req = axum::http::Request::builder()
        .uri("/oauth/callback?code=fake_code&state=myinstance:test_nonce")
        .body(Body::empty())
        .expect("build OAuth callback request with instance-prefixed state");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("send OAuth callback request with instance-prefixed state");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("read OAuth callback instance-prefix response body");
    let html = String::from_utf8_lossy(&body);
    assert!(
        html.contains("Authorization Failed"),
        "Expected error page, html was: {}",
        &html[..html.len().min(500)]
    );

    assert!(
        ext_mgr
            .pending_oauth_flows()
            .read()
            .await
            .get("test_nonce")
            .is_none()
    );
}
