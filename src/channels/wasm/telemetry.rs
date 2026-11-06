//! Fixed-label WASM channel metrics.
//!
//! Every label is a compile-time constant chosen at the call site. No secret
//! value, placeholder, user identifier, or raw error text is ever attached to
//! a metric.

/// Record one WASM channel credential-injection outcome.
///
/// Outcomes: `injected`, `partial`, `missing`, `list_error`, `decryption_error`.
///
/// Non-success outcomes are logged at `info` so a default production build
/// reports them even when the `metrics` feature is off and the log filter
/// excludes `debug`.
pub(crate) fn channel_credentials(outcome: &'static str) {
    if outcome == "injected" {
        tracing::debug!(outcome, "wasm.channel_credentials");
    } else {
        tracing::info!(outcome, "wasm.channel_credentials");
    }
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.wasm.channel_credentials", "outcome" => outcome).increment(1);
}
