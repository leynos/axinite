//! Unit tests for feature-flag resolution and override persistence.

use super::*;

fn no_overrides() -> HashMap<String, bool> {
    HashMap::new()
}

#[test]
fn defaults_apply_when_no_environment_or_override_exists() {
    let flags = resolve_flags(|_| None, &no_overrides(), &[]);
    assert_eq!(flags.get("route_chat"), Some(&true));
    assert_eq!(flags.get("panel_logs"), Some(&true));
    assert_eq!(flags.get("action_memory_edit"), Some(&false));
    assert_eq!(flags.len(), FLAG_DEFAULTS.len());
}

#[test]
fn environment_variables_override_defaults() {
    let flags = resolve_flags(
        |name| match name {
            "action_memory_edit" => Some(true),
            "route_skills" => Some(false),
            _ => None,
        },
        &no_overrides(),
        &[],
    );
    assert_eq!(flags.get("action_memory_edit"), Some(&true));
    assert_eq!(flags.get("route_skills"), Some(&false));
    // Untouched flags keep their compiled defaults.
    assert_eq!(flags.get("route_chat"), Some(&true));
}

#[rstest::rstest]
#[case::lowercase("true", true)]
#[case::uppercase("TRUE", true)]
#[case::mixed_case("TrUe", true)]
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

#[test]
fn deployment_override_beats_compiled_default() {
    let mut overrides = HashMap::new();
    overrides.insert("panel_logs".to_string(), false);
    overrides.insert("action_job_restart".to_string(), true);
    let flags = resolve_flags(|_| None, &overrides, &[]);
    assert_eq!(flags.get("panel_logs"), Some(&false));
    assert_eq!(flags.get("action_job_restart"), Some(&true));
    // A flag with no override keeps its default.
    assert_eq!(flags.get("route_chat"), Some(&true));
}

#[test]
fn environment_variable_beats_deployment_override() {
    let mut overrides = HashMap::new();
    overrides.insert("route_chat".to_string(), false);
    let flags = resolve_flags(
        |name| (name == "route_chat").then_some(true),
        &overrides,
        &[],
    );
    // Env var wins over the override.
    assert_eq!(flags.get("route_chat"), Some(&true));
}

#[test]
fn unknown_override_names_are_ignored() {
    let mut overrides = HashMap::new();
    overrides.insert("not_a_real_flag".to_string(), true);
    let flags = resolve_flags(|_| None, &overrides, &[]);
    assert!(!flags.contains_key("not_a_real_flag"));
    assert_eq!(flags.len(), FLAG_DEFAULTS.len());
}

#[test]
fn unavailable_subsystem_forces_a_flag_off() {
    let flags = resolve_flags(|_| None, &no_overrides(), &["route_routines"]);
    assert_eq!(flags.get("route_routines"), Some(&false));
    // Other flags keep their compiled defaults.
    assert_eq!(flags.get("route_jobs"), Some(&true));
}

#[test]
fn override_beats_subsystem_unavailability() {
    let overrides = HashMap::from([("route_routines".to_string(), true)]);
    let flags = resolve_flags(|_| None, &overrides, &["route_routines"]);
    assert_eq!(flags.get("route_routines"), Some(&true));
}

#[test]
fn environment_variable_beats_subsystem_unavailability() {
    let flags = resolve_flags(
        |name| (name == "route_jobs").then_some(true),
        &no_overrides(),
        &["route_jobs"],
    );
    assert_eq!(flags.get("route_jobs"), Some(&true));
}

#[test]
fn subsystem_layer_never_enables_a_flag() {
    // action flags default off; an available subsystem must not flip them.
    let flags = resolve_flags(|_| None, &no_overrides(), &[]);
    assert_eq!(flags.get("action_job_restart"), Some(&false));
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

    let result = apply_flag_override(&state, "production", "panel_logs", false).await;

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

    apply_flag_override(&state, "production", "panel_logs", false)
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
                .expect("build features request"),
        )
        .await
        .expect("route features request")
}

#[tokio::test]
async fn features_response_is_marked_degraded_when_hydration_fails() {
    use crate::testing::null_db::CapturingStore;

    let store = Arc::new(CapturingStore::failing_list_deployment_flags_once(
        crate::error::DatabaseError::Query("store unavailable".to_string()),
    ));
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(store)
        .build();

    let response = get_features(state).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(DEGRADED_HEADER)
            .and_then(|value| value.to_str().ok()),
        Some("true")
    );
    assert!(response.headers().contains_key(VERSION_HEADER));
}

#[tokio::test]
async fn features_response_is_not_degraded_when_hydration_succeeds() {
    use crate::testing::null_db::CapturingStore;

    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new()
        .store(Arc::new(CapturingStore::new()))
        .build();

    let response = get_features(state).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(DEGRADED_HEADER));
}

#[tokio::test]
async fn features_response_is_not_degraded_without_a_store() {
    let state = crate::channels::web::test_helpers::TestGatewayBuilder::new().build();

    let response = get_features(state).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(DEGRADED_HEADER));
}
