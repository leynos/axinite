use axum::body::Body;
use rstest::rstest;
use tower::ServiceExt;

use super::fixtures::{
    TestGatewayStateFactory, TestOAuthRouterFactory, build_test_ext_mgr, build_test_secrets_store,
    test_gateway_state, test_oauth_router,
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
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
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
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
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
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
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

    let flow = crate::cli::oauth_defaults::PendingOAuthFlow {
        extension_name: "test_tool".to_string(),
        display_name: "Test Tool".to_string(),
        token_url: "https://example.com/token".to_string(),
        client_id: "client123".to_string(),
        client_secret: None,
        redirect_uri: "https://example.com/oauth/callback".to_string(),
        code_verifier: None,
        access_token_field: "access_token".to_string(),
        secret_name: "test_token".to_string(),
        provider: None,
        validation_endpoint: None,
        scopes: vec![],
        user_id: "test".to_string(),
        secrets,
        sse_sender: None,
        gateway_token: None,
        created_at: std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(600))
            .expect("System uptime is too low to run expired flow test"),
    };

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
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
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
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
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

    let flow = crate::cli::oauth_defaults::PendingOAuthFlow {
        extension_name: "test_tool".to_string(),
        display_name: "Test Tool".to_string(),
        token_url: "https://example.com/token".to_string(),
        client_id: "client123".to_string(),
        client_secret: None,
        redirect_uri: "https://example.com/oauth/callback".to_string(),
        code_verifier: None,
        access_token_field: "access_token".to_string(),
        secret_name: "test_token".to_string(),
        provider: None,
        validation_endpoint: None,
        scopes: vec![],
        user_id: "test".to_string(),
        secrets,
        sse_sender: None,
        gateway_token: None,
        created_at: std::time::Instant::now()
            .checked_sub(std::time::Duration::from_secs(600))
            .expect("System uptime is too low to run expired flow test"),
    };

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
        .expect("request");

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .expect("response");
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .expect("body");
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
