//! Unit tests for feature-flag resolution and override persistence.

use super::*;
use crate::feature_flags::{DeploymentId, FlagName};
use crate::test_support::ExpectValid;

#[rstest::rstest]
#[case::lowercase("true", true)]
#[case::uppercase("TRUE", true)]
#[case::mixed_case("tRUE", true)]
#[case::false_word("false", false)]
#[case::one("1", false)]
#[case::yes("yes", false)]
#[case::empty("", false)]
#[case::padded(" true", false)]
fn environment_values_enable_a_flag_only_when_true(#[case] raw: &str, #[case] expected: bool) {
    assert_eq!(parse_env_flag(raw), expected);
}

#[test]
fn environment_variable_names_are_the_upper_snake_flag_name() {
    assert_eq!(flag_env_var("route_chat"), "FEATURE_FLAG_ROUTE_CHAT");
    assert_eq!(
        flag_env_var("surface_tee_attestation"),
        "FEATURE_FLAG_SURFACE_TEE_ATTESTATION"
    );
}

#[tokio::test]
async fn apply_flag_override_fails_without_writing_when_hydration_fails() {
    use crate::testing::null_db::CapturingStore;

    let store = Arc::new(CapturingStore::failing_list_deployment_flags_once(
        crate::error::DatabaseError::Query("store unavailable".to_string()),
    ));
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(store.clone())
        .build();

    let result = apply_flag_override(
        &state,
        &DeploymentId::parse("production").expect("valid deployment"),
        &FlagName::parse("panel_logs").expect("valid flag"),
        false,
    )
    .await;

    assert!(result.is_err(), "hydration failure must propagate");
    assert!(
        store.calls().deployment_flag_writes.lock().await.is_empty(),
        "the override must not be persisted after a failed hydration"
    );
    let registry = state.feature_flags.read().await;
    assert!(!registry.is_hydrated("production"));
    assert_eq!(registry.get("production", "panel_logs"), None);
}

#[tokio::test]
async fn apply_flag_override_persists_and_caches_after_hydration() {
    use crate::testing::null_db::CapturingStore;

    let store = Arc::new(CapturingStore::new());
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(store.clone())
        .build();

    apply_flag_override(
        &state,
        &DeploymentId::parse("production").expect("valid deployment"),
        &FlagName::parse("panel_logs").expect("valid flag"),
        false,
    )
    .await
    .expect("override applies when the store is healthy");

    assert_eq!(
        *store.calls().deployment_flag_writes.lock().await,
        vec![("production".to_string(), "panel_logs".to_string(), false)]
    );
    assert_eq!(
        state
            .feature_flags
            .read()
            .await
            .get("production", "panel_logs"),
        Some(false)
    );
}

#[tokio::test]
async fn bare_test_state_reports_all_gated_subsystems_unavailable() {
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new().build();
    let unavailable = unavailable_subsystem_flags(&state).await;
    for flag in [
        "route_jobs",
        "route_routines",
        "route_extensions",
        "route_skills",
        "route_logs",
        "panel_logs",
    ] {
        assert!(unavailable.contains(&flag), "missing {flag}");
    }
}

async fn get_features(state: Arc<GatewayState>) -> axum::response::Response {
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    routes()
        .with_state(state)
        .oneshot(
            Request::builder()
                .uri("/api/features")
                .body(Body::empty())
                .expect_valid("build features request"),
        )
        .await
        .expect_valid("route features request")
}

#[tokio::test]
async fn features_returns_503_on_snapshot_failure_and_retries_next_request() {
    use crate::testing::null_db::CapturingStore;
    let store = Arc::new(CapturingStore::failing_list_deployment_flags_once(
        crate::error::DatabaseError::Query("store unavailable".to_string()),
    ));
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(store)
        .build();
    let response = get_features(state.clone()).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!state.feature_flags.read().await.is_hydrated("default"));
    let retry = get_features(state.clone()).await;
    assert_eq!(retry.status(), StatusCode::OK);
    assert!(!state.feature_flags.read().await.is_hydrated("default"));
    assert_eq!(
        state.feature_flags.read().await.resources(),
        Default::default()
    );
}

#[tokio::test]
async fn features_response_carries_version_when_snapshot_load_succeeds() {
    use crate::testing::null_db::CapturingStore;

    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(Arc::new(CapturingStore::new()))
        .build();

    let response = get_features(state).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key(VERSION_HEADER));
}

#[tokio::test]
async fn features_response_succeeds_without_a_store() {
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new().build();

    let response = get_features(state).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().contains_key(VERSION_HEADER));
}

#[tokio::test]
async fn repeated_features_get_does_not_publish_cache_admission_or_retry_entries() {
    use crate::testing::null_db::CapturingStore;
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(Arc::new(CapturingStore::new()))
        .build();
    let before = state.feature_flags.read().await.resources();
    for _ in 0..2 {
        assert_eq!(get_features(state.clone()).await.status(), StatusCode::OK);
        assert_eq!(state.feature_flags.read().await.resources(), before);
        assert!(!state.feature_flags.read().await.is_hydrated("default"));
        assert!(!state.feature_flags.read().await.is_retry("default"));
    }
}
