//! Handler-level tests for the `feature_flag:` settings interception.

use axum::body::Body;
use axum::http::Request;
use tower::ServiceExt;

use rstest::rstest;

use super::*;
use crate::channels::web::handlers::features;
use crate::channels::web::test_helpers::TestGatewayBuilder;
use crate::test_support::ExpectValid;

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
        .expect_valid("read response body");
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Every malformed `feature_flag:` request and malformed deployment header is
/// rejected with 400 before it reaches the store or the registry.
#[rstest]
#[case::put_without_deployment_header(
    "PUT",
    "/api/settings/feature_flag:route_memory",
    None,
    r#"{"value":"false"}"#,
    false
)]
#[case::put_with_invalid_deployment_header(
    "PUT",
    "/api/settings/feature_flag:route_memory",
    Some("Production!"),
    r#"{"value":"false"}"#,
    false
)]
#[case::put_with_overlong_deployment_header(
    "PUT",
    "/api/settings/feature_flag:route_memory",
    Some(OVERLONG_DEPLOYMENT_ID),
    r#"{"value":"false"}"#,
    false
)]
#[case::put_with_invalid_flag_name(
    "PUT",
    "/api/settings/feature_flag:Bad-Name",
    Some("production"),
    r#"{"value":true}"#,
    false
)]
// Store present so the failure is attributable to value coercion, not a
// missing store.
#[case::put_with_uncoercible_value(
    "PUT",
    "/api/settings/feature_flag:route_memory",
    Some("production"),
    r#"{"value":"maybe"}"#,
    true
)]
#[case::get_flag_key_via_settings(
    "GET",
    "/api/settings/feature_flag:route_memory",
    None,
    "",
    false
)]
#[case::features_with_invalid_deployment_header("GET", "/api/features", Some("a/b"), "", false)]
#[tokio::test]
async fn malformed_feature_flag_requests_return_400(
    #[case] method: &str,
    #[case] uri: &str,
    #[case] deployment_id: Option<&str>,
    #[case] body: &'static str,
    #[case] with_store: bool,
) {
    let mut builder = TestGatewayBuilder::new();
    if with_store {
        builder = builder.store(new_test_store().await);
    }
    let mut request = Request::builder().method(method).uri(uri);
    if !body.is_empty() {
        request = request.header("content-type", "application/json");
    }
    if let Some(deployment_id) = deployment_id {
        request = request.header("x-deployment-id", deployment_id);
    }
    let response = app(builder.build())
        .oneshot(
            request
                .body(Body::from(body))
                .expect("build feature-flag request"),
        )
        .await
        .expect("route feature-flag request");
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

/// Sixty-five characters: one over `MAX_DEPLOYMENT_ID_LEN`.
const OVERLONG_DEPLOYMENT_ID: &str =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[test]
fn overlong_deployment_fixture_exceeds_the_bound() {
    assert_eq!(
        OVERLONG_DEPLOYMENT_ID.len(),
        crate::channels::web::handlers::feature_registry::MAX_DEPLOYMENT_ID_LEN + 1
    );
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
                .expect("build features request"),
        )
        .await
        .expect("route features request");
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
        .expect_valid("open in-memory libSQL backend");
    backend
        .run_migrations()
        .await
        .expect_valid("run libSQL migrations");
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
    // The environment layer outranks deployment overrides, so this test is
    // only meaningful when the process was started without it. Nothing in the
    // suite sets it; check rather than mutate the shared process environment.
    assert!(
        !features::env_flag_overlay().contains_key("FEATURE_FLAG_ROUTE_MEMORY"),
        "FEATURE_FLAG_ROUTE_MEMORY must be unset for this test"
    );

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
                .expect("build override PUT"),
        )
        .await
        .expect("route override PUT");
    assert_eq!(put.status(), StatusCode::OK);

    // The same deployment now reflects the override immediately.
    let get = app(state.clone())
        .oneshot(
            Request::builder()
                .uri("/api/features")
                .header("x-deployment-id", "production")
                .body(Body::empty())
                .expect("build production features GET"),
        )
        .await
        .expect("route production features GET");
    assert_eq!(get.status(), StatusCode::OK);
    let flags: std::collections::BTreeMap<String, bool> =
        serde_json::from_str(&body_string(get).await).expect("production flags JSON");
    assert_eq!(flags.get("route_memory"), Some(&false));

    // A different deployment is unaffected and keeps the compiled default.
    let other = app(state)
        .oneshot(
            Request::builder()
                .uri("/api/features")
                .header("x-deployment-id", "staging")
                .body(Body::empty())
                .expect("build staging features GET"),
        )
        .await
        .expect("route staging features GET");
    let other_flags: std::collections::BTreeMap<String, bool> =
        serde_json::from_str(&body_string(other).await).expect("staging flags JSON");
    assert_eq!(other_flags.get("route_memory"), Some(&true));
}
