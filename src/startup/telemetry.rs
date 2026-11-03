//! Fixed-label startup metrics.
//!
//! Every label is a compile-time constant chosen at the call site. No path,
//! user identifier, URL, or raw error text is ever attached to a metric.

/// Record one startup configuration-load outcome.
///
/// Outcomes: `success`, `settings_error`, `config_error`.
pub(crate) fn config_load(outcome: &'static str) {
    tracing::debug!(outcome, "startup.config_load");
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.startup.config_load", "outcome" => outcome).increment(1);
}
