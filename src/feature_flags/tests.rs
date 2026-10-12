//! Pure resolution tests; no HTTP framework, process environment or database.

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

#[rstest::rstest]
#[case::empty("")]
#[case::uppercase("Production")]
#[case::punctuation("eu-west")]
#[case::embedded_whitespace("eu west")]
#[case::non_ascii("déployment")]
fn deployment_validation_rejects_invalid_domain_inputs(#[case] raw: &str) {
    assert_eq!(DeploymentId::parse(raw), Err(InvalidDeploymentId));
}

#[test]
fn deployment_validation_enforces_the_length_boundary() {
    assert!(DeploymentId::parse(&"a".repeat(MAX_DEPLOYMENT_ID_LEN)).is_ok());
    assert_eq!(
        DeploymentId::parse(&"a".repeat(MAX_DEPLOYMENT_ID_LEN + 1)),
        Err(InvalidDeploymentId)
    );
}

#[test]
fn unavailable_subsystems_hide_both_routes_and_actions_without_enabling_defaults() {
    let unavailable = availability::SubsystemAvailability::default().unavailable_flags();
    let flags = resolve_flags(|_| None, &no_overrides(), &unavailable);
    for name in [
        "route_jobs",
        "action_job_restart",
        "route_routines",
        "action_routine_trigger",
        "route_extensions",
        "action_extension_install",
        "route_skills",
        "action_skill_install",
        "route_logs",
        "panel_logs",
    ] {
        assert_eq!(flags.get(name), Some(&false), "{name}");
    }
    assert_eq!(flags.get("route_chat"), Some(&true));
}
