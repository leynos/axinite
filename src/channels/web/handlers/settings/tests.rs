//! Handler-level tests for the `feature_flag:` settings interception.

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use super::*;
use crate::channels::web::handlers::features;
use crate::channels::web::test_helpers::TestGatewayBuilder;

#[test]
fn valid_flag_name_accepts_lowercase_digits_underscore() {
    assert!(is_valid_flag_name("panel_logs"));
    assert!(is_valid_flag_name("route_chat2"));
    assert!(!is_valid_flag_name(""));
    assert!(!is_valid_flag_name("Panel_Logs"));
    assert!(!is_valid_flag_name("panel-logs"));
    assert!(!is_valid_flag_name("panel logs"));
}

#[test]
fn coerce_flag_value_accepts_bool_and_string_variants() {
    use serde_json::json;
    assert_eq!(coerce_flag_value(&json!(true)), Some(true));
    assert_eq!(coerce_flag_value(&json!(false)), Some(false));
    assert_eq!(coerce_flag_value(&json!("TRUE")), Some(true));
    assert_eq!(coerce_flag_value(&json!("False")), Some(false));
    assert_eq!(coerce_flag_value(&json!("1")), None);
    assert_eq!(coerce_flag_value(&json!(1)), None);
    assert_eq!(coerce_flag_value(&json!(null)), None);
}

fn app(state: Arc<GatewayState>) -> Router {
    super::routes().merge(features::routes()).with_state(state)
}

async fn body_string(response: axum::response::Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

#[tokio::test]
async fn put_feature_flag_without_deployment_header_returns_400() {
    let state = TestGatewayBuilder::new().build();
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings/feature_flag:route_memory")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"value":"false"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn put_feature_flag_with_invalid_name_returns_400() {
    let state = TestGatewayBuilder::new().build();
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings/feature_flag:Bad-Name")
                .header("content-type", "application/json")
                .header("x-deployment-id", "production")
                .body(Body::from(r#"{"value":true}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn put_feature_flag_with_uncoercible_value_returns_400() {
    // Store present so the failure is attributable to value coercion, not a
    // missing store.
    let backend = new_test_store().await;
    let state = TestGatewayBuilder::new().store(backend).build();
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings/feature_flag:route_memory")
                .header("content-type", "application/json")
                .header("x-deployment-id", "production")
                .body(Body::from(r#"{"value":"maybe"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_feature_flag_key_via_settings_is_rejected() {
    let state = TestGatewayBuilder::new().build();
    let response = app(state)
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/api/settings/feature_flag:route_memory")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn features_get_without_header_uses_default_deployment() {
    // No store: resolution falls back to compiled defaults.
    let state = TestGatewayBuilder::new().build();
    let response = app(state)
        .oneshot(
            Request::builder()
                .uri("/api/features")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response
            .headers()
            .get(super::super::features::VERSION_HEADER)
            .is_some(),
        "features response should carry the gateway version header"
    );
    let body = body_string(response).await;
    let flags: std::collections::BTreeMap<String, bool> =
        serde_json::from_str(&body).expect("valid JSON map");
    // Compiled defaults for the "default" deployment.
    assert_eq!(flags.get("route_chat"), Some(&true));
    // The bare test gateway wires no log broadcaster, so the
    // subsystem-availability layer forces the logs surfaces off.
    assert_eq!(flags.get("panel_logs"), Some(&false));
}

// --- libSQL-backed persistence proof (requires the libsql backend) ---

#[cfg(feature = "libsql")]
async fn new_test_store() -> Arc<dyn crate::db::Database> {
    use crate::db::Database as _;
    let backend = crate::db::libsql::LibSqlBackend::new_memory()
        .await
        .unwrap();
    backend.run_migrations().await.unwrap();
    Arc::new(backend)
}

#[cfg(not(feature = "libsql"))]
async fn new_test_store() -> Arc<dyn crate::db::Database> {
    // The postgres-only test build has no in-process store; the null
    // database satisfies the trait so value-validation tests can still run.
    Arc::new(crate::testing::null_db::NullDatabase::new())
}

#[cfg(feature = "libsql")]
#[tokio::test]
async fn put_feature_flag_then_get_reflects_override_without_restart() {
    // Guard against a leaked environment override from another test.
    // SAFETY: single-threaded test; no other thread reads the environment.
    unsafe {
        std::env::remove_var("FEATURE_FLAG_ROUTE_MEMORY");
    }

    let backend = new_test_store().await;
    let state = TestGatewayBuilder::new().store(backend).build();

    // Override route_memory=false for the "production" deployment
    // (route_memory has no subsystem gate, so the compiled default applies
    // elsewhere).
    let put = app(state.clone())
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings/feature_flag:route_memory")
                .header("content-type", "application/json")
                .header("x-deployment-id", "production")
                .body(Body::from(r#"{"value":"false"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(put.status(), StatusCode::OK);

    // The same deployment now reflects the override immediately.
    let get = app(state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/features")
                .header("x-deployment-id", "production")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(get.status(), StatusCode::OK);
    let flags: std::collections::BTreeMap<String, bool> =
        serde_json::from_str(&body_string(get).await).unwrap();
    assert_eq!(flags.get("route_memory"), Some(&false));

    // A different deployment is unaffected and keeps the compiled default.
    let other = app(state)
        .oneshot(
            Request::builder()
                .uri("/api/features")
                .header("x-deployment-id", "staging")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let other_flags: std::collections::BTreeMap<String, bool> =
        serde_json::from_str(&body_string(other).await).unwrap();
    assert_eq!(other_flags.get("route_memory"), Some(&true));
}
