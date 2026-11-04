//! Fixed-label WASM channel metrics.
//!
//! Every label is a compile-time constant chosen at the call site. No secret
//! value, placeholder, user identifier, or raw error text is ever attached to
//! a metric.

/// Record one WASM channel credential-injection outcome.
///
/// Outcomes: `injected`, `missing`, `list_error`, `decryption_error`.
pub(crate) fn channel_credentials(outcome: &'static str) {
    tracing::debug!(outcome, "wasm.channel_credentials");
    #[cfg(feature = "metrics")]
    metrics::counter!("axinite.wasm.channel_credentials", "outcome" => outcome).increment(1);
}
