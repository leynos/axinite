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
            "FEATURE_FLAG_ACTION_MEMORY_EDIT" => Some("TRUE".to_string()),
            "FEATURE_FLAG_ROUTE_SKILLS" => Some("false".to_string()),
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

#[test]
fn non_true_values_disable_the_flag() {
    let flags = resolve_flags(
        |name| (name == "FEATURE_FLAG_ROUTE_CHAT").then(|| "1".to_string()),
        &no_overrides(),
        &[],
    );
    assert_eq!(flags.get("route_chat"), Some(&false));
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
        |name| (name == "FEATURE_FLAG_ROUTE_CHAT").then(|| "true".to_string()),
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
        |name| (name == "FEATURE_FLAG_ROUTE_JOBS").then(|| "true".to_string()),
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
