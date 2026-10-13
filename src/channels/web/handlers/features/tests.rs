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

#[rstest::rstest]
#[case::without_store(false)]
#[case::failing_store(true)]
#[tokio::test]
async fn cached_features_queries_return_detached_values_before_storage_fallback(
    #[case] with_store: bool,
) {
    use crate::testing::null_db::CapturingStore;
    let builder = crate::channels::web::test_helpers::TestGatewayBuilder::new();
    let state = if with_store {
        builder
            .store(Arc::new(
                CapturingStore::failing_list_deployment_flags_once(
                    crate::error::DatabaseError::Query("store unavailable".to_string()),
                ),
            ))
            .build()
    } else {
        builder.build()
    };
    let deployment = DeploymentId::parse("default").expect("valid deployment");
    let flag = FlagName::parse("panel_logs").expect("valid flag");
    let setup_store = CapturingStore::new();
    let adapter = SettingsFlagStore {
        store: &setup_store,
    };
    application::set_override(
        &state.feature_flags,
        &adapter,
        FlagOverrideCommand {
            deployment: deployment.clone(),
            flag: flag.clone(),
            enabled: false,
        },
    )
    .await
    .expect("prepare cached value through the owning command");
    let before = state.feature_flags.read().await.resources();
    let cached = state.feature_flags.read().await.snapshot(&deployment);
    let mut snapshot = query_deployment_snapshot(&state, cached, &deployment)
        .await
        .expect("cached value bypasses storage");
    assert_eq!(snapshot.get("panel_logs"), Some(false));
    snapshot.set(flag, true);
    assert_eq!(state.feature_flags.read().await.resources(), before);
    assert_eq!(
        state
            .feature_flags
            .read()
            .await
            .get("default", "panel_logs"),
        Some(false)
    );
    if with_store {
        let uncached = DeploymentId::parse("preview").expect("valid deployment");
        assert_eq!(
            query_deployment_snapshot(&state, None, &uncached).await,
            Err(application::FlagStoreError::Load)
        );
    }
}

#[tokio::test]
async fn read_failures_have_safe_request_correlation_and_bounded_category() {
    use super::telemetry::test_support::RecordingObserver;
    use crate::testing::null_db::CapturingStore;
    use tracing::instrument::WithSubscriber;

    let observer = RecordingObserver::default();
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(Arc::new(
            CapturingStore::failing_list_deployment_flags_once(crate::error::DatabaseError::Query(
                "private database contents".to_string(),
            )),
        ))
        .build();
    let mut headers = HeaderMap::new();
    headers.insert(
        "x-deployment-id",
        HeaderValue::from_static("private_deployment"),
    );
    let result = features_handler(State(state), headers)
        .with_subscriber(observer.clone().subscriber())
        .await;
    assert!(matches!(result, Err((StatusCode::SERVICE_UNAVAILABLE, _))));
    let records = observer.observations();
    let failures = records
        .iter()
        .filter(|record| record.0["message"] == "feature_flags.failure")
        .collect::<Vec<_>>();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0["operation"], "read_request");
    assert_eq!(failures[0].0["category"], "load");
    assert!(uuid::Uuid::parse_str(&failures[0].0["operation_id"]).is_ok());
    assert_eq!(
        records
            .iter()
            .filter(|record| record.0["message"] == "feature_flags.store_latency")
            .count(),
        1
    );
    assert!(records.iter().all(|record| {
        !record.0.values().any(|value| {
            value.contains("private_deployment") || value.contains("private database contents")
        })
    }));
}

#[rstest::rstest]
#[case::healthy_store(false)]
#[case::failing_store(true)]
#[tokio::test]
async fn snapshot_query_with_settings_adapter_emits_no_observations(#[case] fails: bool) {
    use super::telemetry::test_support::RecordingObserver;
    use crate::testing::null_db::CapturingStore;
    use tracing::instrument::WithSubscriber;

    let observer = RecordingObserver::default();
    let store = if fails {
        CapturingStore::failing_list_deployment_flags_once(crate::error::DatabaseError::Query(
            "private store failure".to_string(),
        ))
    } else {
        CapturingStore::new()
    };
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(Arc::new(store))
        .build();
    let deployment = DeploymentId::parse("production").expect("valid deployment");
    let result = query_deployment_snapshot(&state, None, &deployment)
        .with_subscriber(observer.clone().subscriber())
        .await;
    assert_eq!(
        result,
        if fails {
            Err(application::FlagStoreError::Load)
        } else {
            Ok(FlagSnapshot::default())
        }
    );
    assert!(
        observer.observations().is_empty(),
        "the real adapter query must remain silent"
    );
    assert_eq!(
        state.feature_flags.read().await.resources(),
        Default::default()
    );
}
