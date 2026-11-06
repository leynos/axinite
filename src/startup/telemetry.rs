//! Fixed-label startup metrics.
//!
//! Every label is a compile-time constant chosen at the call site. No path,
//! user identifier, URL, or raw error text is ever attached to a metric.

/// Record one startup configuration-load outcome.
///
/// Outcomes: `success`, `settings_error`, `config_error`.
///
/// Non-success outcomes are logged at `info` so a default production build
/// reports a startup failure even when the `metrics` feature is off and the
/// log filter excludes `debug`. Success stays at `debug` to keep routine
/// startup quiet.
pub(crate) fn config_load(outcome: &'static str) {
    if outcome == "success" {
        tracing::debug!(outcome, "startup.config_load");
    } else {
        tracing::info!(outcome, "startup.config_load");
    }
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.startup.config_load", "outcome" => outcome).increment(1);
}
