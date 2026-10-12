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
