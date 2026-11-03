//! Fixed-label reload metrics.
//!
//! Every label is a compile-time constant chosen at the call site. No path,
//! user identifier, URL, or raw error text is ever attached to a metric.

/// Record one hot-reload cycle outcome.
///
/// Outcomes: `success`, `http_removed`, `config_error`, `listener_error`.
pub(crate) fn reload_outcome(outcome: &'static str) {
    tracing::debug!(outcome, "reload.outcome");
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.reload.outcomes", "outcome" => outcome).increment(1);
}

/// Record one webhook-secret injection outcome.
///
/// Outcomes: `injected`, `missing`, `retained_after_error`, `error`.
pub(crate) fn secret_injection(outcome: &'static str) {
    tracing::debug!(outcome, "reload.secret_injection");
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.reload.secret_injection", "outcome" => outcome).increment(1);
}
