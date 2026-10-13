//! Fixed-label gateway flag metrics; deployment IDs and flag names are never labels.

/// Report one operational outcome using labels fixed at adapter call sites.
pub(crate) fn count(operation: &'static str, outcome: &'static str) {
    tracing::debug!(operation, outcome, "feature_flags.counter");
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.feature_flags.events", "operation" => operation, "outcome" => outcome)
        .increment(1);
}

/// Report persistence duration and success without exposing storage identifiers.
pub(crate) fn store_result(operation: &'static str, started: std::time::Instant, succeeded: bool) {
    let outcome = if succeeded { "success" } else { "error" };
    count(operation, outcome);
    let elapsed_seconds = started.elapsed().as_secs_f64();
    tracing::debug!(
        operation,
        outcome,
        elapsed_seconds,
        "feature_flags.store_latency"
    );
    #[cfg(feature = "metrics")]
    metrics::histogram!("axinite.feature_flags.store_latency_seconds",
        "operation" => operation, "outcome" => outcome)
    .record(elapsed_seconds);
}

/// Observe gateway resource growth without creating query-visible cache state.
pub(crate) fn cache_resources(resources: super::registry::CacheResources) {
    tracing::debug!(
        cached_deployments = resources.deployments,
        cached_overrides = resources.overrides,
        failed_hydrations = resources.failed_hydrations,
        admission_gates = resources.admission_gates,
        "feature_flags.cache_resources"
    );
    #[cfg(feature = "metrics")]
    {
        metrics::gauge!("axinite.feature_flags.cached_deployments")
            .set(resources.deployments as f64);
        metrics::gauge!("axinite.feature_flags.cached_overrides").set(resources.overrides as f64);
        metrics::gauge!("axinite.feature_flags.failed_hydration_entries")
            .set(resources.failed_hydrations as f64);
        metrics::gauge!("axinite.feature_flags.admission_gates")
            .set(resources.admission_gates as f64);
    }
}

/// Observe the request's captured source selection outside the query service.
pub(crate) fn read_source(cached: bool, has_store: bool) {
    let outcome = if cached { "hit" } else { "miss" };
    count("cache", outcome);
    if !cached && !has_store {
        count("read_snapshot", "no_store_defaults");
    }
}

/// Record only fixed categories for errors; the enclosing span carries correlation.
pub(crate) fn failure(operation: &'static str, error: super::application::FlagStoreError) {
    let category = error_category(Some(error));
    tracing::error!(operation, category, "feature_flags.failure");
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.feature_flags.failures", "operation" => operation,
        "category" => category)
    .increment(1);
}

/// Normalize semantic persistence errors to a closed set of operational categories.
fn error_category(error: Option<super::application::FlagStoreError>) -> &'static str {
    use super::application::FlagStoreError;
    match error {
        None => "success",
        Some(FlagStoreError::Load) => "load",
        Some(FlagStoreError::Save) => "save",
        Some(FlagStoreError::InvalidSnapshot) => "invalid_snapshot",
    }
}

/// Measure admission separately, including the brief registry lock acquisition.
pub(crate) fn admission_wait(started: std::time::Instant) {
    let elapsed_seconds = started.elapsed().as_secs_f64();
    tracing::debug!(elapsed_seconds, "feature_flags.admission_wait");
    #[cfg(feature = "metrics")]
    metrics::histogram!("axinite.feature_flags.admission_wait_seconds").record(elapsed_seconds);
}

/// Observe the complete admitted command, including hydration, save and publication.
pub(crate) fn command_result(
    started: std::time::Instant,
    error: Option<super::application::FlagStoreError>,
) {
    let category = error_category(error);
    let elapsed_seconds = started.elapsed().as_secs_f64();
    tracing::debug!(category, elapsed_seconds, "feature_flags.command_latency");
    #[cfg(feature = "metrics")]
    metrics::histogram!("axinite.feature_flags.command_latency_seconds", "category" => category)
        .record(elapsed_seconds);
    if let Some(error) = error {
        failure("override_command", error);
    }
}

/// Observe absent subsystems independently of overrides. Attribute disables only
/// when the availability layer is reached below explicit override values.
pub(crate) fn availability_decisions(
    unavailable: &[&str],
    environment: &std::collections::HashMap<&str, bool>,
    overrides: &crate::feature_flags::FlagSnapshot,
) {
    // This fixed mapping is adapter metadata, never a dynamic metric label.
    for (subsystem, surfaces) in [
        (
            "jobs",
            [("route_jobs", "route"), ("action_job_restart", "action")],
        ),
        (
            "routines",
            [
                ("route_routines", "route"),
                ("action_routine_trigger", "action"),
            ],
        ),
        (
            "extensions",
            [
                ("route_extensions", "route"),
                ("action_extension_install", "action"),
            ],
        ),
        (
            "skills",
            [
                ("route_skills", "route"),
                ("action_skill_install", "action"),
            ],
        ),
        ("logs", [("route_logs", "route"), ("panel_logs", "panel")]),
    ] {
        if !surfaces.iter().any(|(flag, _)| unavailable.contains(flag)) {
            continue;
        }
        let disabled_surfaces = surfaces.into_iter().filter_map(|(flag, surface)| {
            availability_applies(flag, unavailable, environment, overrides).then_some(surface)
        });
        observe_unavailable_subsystem(subsystem, disabled_surfaces);
    }
}

/// Report subsystem absence before observing surfaces disabled by availability.
fn observe_unavailable_subsystem(
    subsystem: &'static str,
    disabled_surfaces: impl Iterator<Item = &'static str>,
) {
    tracing::debug!(subsystem, "feature_flags.unavailable_subsystem");
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.feature_flags.unavailable_subsystems",
        "subsystem" => subsystem)
    .increment(1);
    for surface in disabled_surfaces {
        tracing::debug!(subsystem, surface, "feature_flags.availability_disable");
        #[cfg(feature = "metrics")]
        metrics::counter!("axinite.feature_flags.availability_disables",
            "subsystem" => subsystem, "surface" => surface)
        .increment(1);
    }
}

/// Availability is consulted only after both explicit override layers fall through.
fn availability_applies(
    flag: &str,
    unavailable: &[&str],
    environment: &std::collections::HashMap<&str, bool>,
    overrides: &crate::feature_flags::FlagSnapshot,
) -> bool {
    unavailable.contains(&flag) && !environment.contains_key(flag) && overrides.get(flag).is_none()
}

#[cfg(test)]
pub(crate) mod test_support;
#[cfg(test)]
mod tests;
