//! Unit tests for UI variant selection and embedded asset serving.

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use std::collections::HashMap;

use rstest::rstest;

use super::*;
use crate::channels::web::test_helpers::TestGatewayBuilder;
use crate::test_support::ExpectValid;

fn app(variant: UiVariant) -> Router {
    routes_for(variant).with_state(TestGatewayBuilder::new().build())
}

async fn get_path(variant: UiVariant, path: &str) -> (StatusCode, String, String) {
    let request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect_valid("build asset request");
    let response = app(variant)
        .oneshot(request)
        .await
        .expect_valid("route asset request");
    let status = response.status();
    let content_type = response
        .headers()
        .get(header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or_default().to_string())
        .unwrap_or_default();
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect_valid("read asset response body");
    (
        status,
        content_type,
        String::from_utf8_lossy(&bytes).into_owned(),
    )
}

#[tokio::test]
async fn solid_variant_serves_spa_shell_at_root_and_app_routes() {
    for path in [
        "/",
        "/chat",
        "/memory",
        "/jobs",
        "/routines",
        "/extensions",
        "/skills",
        "/logs",
    ] {
        let (status, content_type, body) = get_path(UiVariant::Solid, path).await;
        assert_eq!(status, StatusCode::OK, "path {path}");
        assert!(content_type.starts_with("text/html"), "path {path}");
        assert!(body.contains("id=\"app\""), "SPA mount missing for {path}");
        assert!(
            body.contains("/assets/app.js"),
            "bundle ref missing for {path}"
        );
    }
}

#[tokio::test]
async fn solid_variant_serves_stable_asset_names() {
    let (status, content_type, body) = get_path(UiVariant::Solid, "/assets/app.js").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "application/javascript");
    assert!(!body.is_empty());

    let (status, content_type, _) = get_path(UiVariant::Solid, "/assets/index.css").await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(content_type, "text/css");
}

#[tokio::test]
async fn solid_variant_serves_locale_bundles() {
    let (status, content_type, body) =
        get_path(UiVariant::Solid, "/locales/en-GB/common.ftl").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/plain"));
    assert!(body.contains("route-chat-label"));

    let (status, _, _) = get_path(UiVariant::Solid, "/locales/xx/common.ftl").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn legacy_variant_still_serves_the_handwritten_shell() {
    let (status, content_type, body) = get_path(UiVariant::Legacy, "/").await;
    assert_eq!(status, StatusCode::OK);
    assert!(content_type.starts_with("text/html"));
    assert!(body.contains("app.js"));

    let (status, _, _) = get_path(UiVariant::Legacy, "/chat").await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[rstest]
#[case::stylesheet("/style.css", "text/css", "")]
#[case::script("/app.js", "application/javascript", "")]
#[case::favicon("/favicon.ico", "image/x-icon", "public, max-age=86400")]
#[tokio::test]
async fn legacy_variant_serves_its_assets(
    #[case] path: &str,
    #[case] expected_type: &str,
    #[case] expected_cache: &str,
) {
    let request = Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("build legacy asset request");
    let response = app(UiVariant::Legacy)
        .oneshot(request)
        .await
        .expect("route legacy asset request");
    assert_eq!(response.status(), StatusCode::OK, "path {path}");
    let header_text = |name| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_string()
    };
    assert_eq!(
        header_text(header::CONTENT_TYPE),
        expected_type,
        "path {path}"
    );
    let cache = header_text(header::CACHE_CONTROL);
    if expected_cache.is_empty() {
        assert_eq!(cache, "no-cache", "path {path}");
    } else {
        assert_eq!(cache, expected_cache, "path {path}");
    }
    let body = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("read legacy asset body");
    assert!(!body.is_empty(), "empty body for {path}");
}

fn env_with(value: Option<&str>) -> EnvContext {
    let vars = value
        .map(|value| HashMap::from([(UI_VARIANT_ENV.to_string(), value.to_string())]))
        .unwrap_or_default();
    EnvContext::for_testing(vars, HashMap::new())
}

#[rstest]
#[case::unset(None, UiVariant::Solid)]
#[case::empty(Some(""), UiVariant::Solid)]
#[case::legacy(Some("legacy"), UiVariant::Legacy)]
#[case::legacy_any_case(Some("LEGACY"), UiVariant::Legacy)]
#[case::solid(Some("solid"), UiVariant::Solid)]
#[case::unrecognized(Some("classic"), UiVariant::Solid)]
fn ui_variant_is_resolved_from_the_supplied_environment(
    #[case] value: Option<&str>,
    #[case] expected: UiVariant,
) {
    assert_eq!(ui_variant_from(&env_with(value)), expected);
}
