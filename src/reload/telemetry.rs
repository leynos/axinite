//! Fixed-label reload metrics.
//!
//! Every label is a compile-time constant chosen at the call site. No path,
//! user identifier, URL, or raw error text is ever attached to a metric.

/// Record one hot-reload cycle outcome.
///
/// Outcomes: `success`, `http_removed`, `config_error`, `listener_error`.
///
/// Non-success outcomes are logged at `info` so a default production build
/// reports them even when the `metrics` feature is off and the log filter
/// excludes `debug`.
pub(crate) fn reload_outcome(outcome: &'static str) {
    if outcome == "success" {
        tracing::debug!(outcome, "reload.outcome");
    } else {
        tracing::info!(outcome, "reload.outcome");
    }
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.reload.outcomes", "outcome" => outcome).increment(1);
}

/// Record one webhook-secret injection outcome.
///
/// Outcomes: `injected`, `missing`, `retained_after_error`, `error`.
///
/// Non-success outcomes are logged at `info` rather than `debug` for the same
/// reason as [`reload_outcome`].
pub(crate) fn secret_injection(outcome: &'static str) {
    if outcome == "injected" {
        tracing::debug!(outcome, "reload.secret_injection");
    } else {
        tracing::info!(outcome, "reload.secret_injection");
    }
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.reload.secret_injection", "outcome" => outcome).increment(1);
}
