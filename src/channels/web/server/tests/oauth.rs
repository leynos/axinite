//! Tests for the hosted gateway OAuth callback handler.

use anyhow::Context as _;
use axum::body::Body;
use rstest::rstest;
use tower::ServiceExt;

use super::fixtures::{
    TestGatewayStateFactory, TestOAuthRouterFactory, build_test_ext_mgr, build_test_secrets_store,
    expired_pending_oauth_flow, test_gateway_state, test_oauth_router,
};

/// Drive an OAuth callback request and return the rendered HTML page.
///
/// Request construction, dispatch, and body reading can all fail, so the
/// errors are propagated for the test body to unwrap. `request_context`
/// labels a request-construction failure.
async fn oauth_failure_html(
    app: axum::Router,
    uri: &str,
    request_context: &str,
) -> anyhow::Result<String> {
    let req = axum::http::Request::builder()
        .uri(uri)
        .body(Body::empty())
        .with_context(|| request_context.to_string())?;

    let resp = ServiceExt::<axum::http::Request<Body>>::oneshot(app, req)
        .await
        .context("send OAuth callback failure-path request")?;
    assert_eq!(resp.status(), axum::http::StatusCode::OK);

    let body = axum::body::to_bytes(resp.into_body(), 1024 * 64)
        .await
        .context("read OAuth callback failure-path response body")?;
    Ok(String::from_utf8_lossy(&body).into_owned())
}

#[rstest]
#[case::missing_params("/oauth/callback")]
#[case::provider_error("/oauth/callback?error=access_denied&error_description=access_denied")]
#[tokio::test]
async fn test_oauth_callback_basic_failure_paths(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
    #[case] uri: &str,
) {
    let state = test_gateway_state.build(None, None);
    let app = test_oauth_router.build(state);
    let html = oauth_failure_html(app, uri, "build OAuth callback failure-path request")
        .await
        .expect("OAuth callback failure path should render an HTML page");
    assert!(html.contains("Authorization Failed"));
}

#[rstest]
#[case::unknown_state("/oauth/callback?code=test_code&state=unknown_state_value", false)]
#[case::no_extension_manager("/oauth/callback?code=test_code&state=some_state", true)]
#[tokio::test]
async fn test_oauth_callback_stateful_failure_paths(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
    #[case] uri: &str,
    #[case] without_extension_manager: bool,
) {
    let extension_manager = if without_extension_manager {
        None
    } else {
        let secrets = build_test_secrets_store().expect("test secrets store should build");
        Some(build_test_ext_mgr(secrets))
    };
    let state = test_gateway_state.build(extension_manager, None);
    let app = test_oauth_router.build(state);
    let html = oauth_failure_html(
        app,
        uri,
        "build OAuth callback stateful failure-path request",
    )
    .await
    .expect("OAuth callback stateful failure path should render an HTML page");
    assert!(html.contains("Authorization Failed"));
}

#[rstest]
#[tokio::test]
async fn test_oauth_callback_expired_flow(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
) {
    let secrets = build_test_secrets_store().expect("test secrets store should build");
    let ext_mgr = build_test_ext_mgr(std::sync::Arc::clone(&secrets));

    let flow = expired_pending_oauth_flow(secrets).expect("expired OAuth flow should build");

    ext_mgr
        .pending_oauth_flows()
        .write()
        .await
        .insert("expired_state".to_string(), flow);

    let state = test_gateway_state.build(Some(ext_mgr), None);
    let app = test_oauth_router.build(state);

    let html = oauth_failure_html(
        app,
        "/oauth/callback?code=test_code&state=expired_state",
        "build OAuth callback request for expired flow",
    )
    .await
    .expect("OAuth callback expired-flow path should render an HTML page");
    assert!(html.contains("Authorization Failed"));
}

#[rstest]
#[tokio::test]
async fn test_oauth_callback_strips_instance_prefix(
    test_gateway_state: TestGatewayStateFactory,
    test_oauth_router: TestOAuthRouterFactory,
) {
    let secrets = build_test_secrets_store().expect("test secrets store should build");
    let ext_mgr = build_test_ext_mgr(std::sync::Arc::clone(&secrets));

    let flow = expired_pending_oauth_flow(secrets).expect("expired OAuth flow should build");

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
