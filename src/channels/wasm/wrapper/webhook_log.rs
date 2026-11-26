//! Debug logging of inbound webhook bodies.

/// Log the webhook body for debugging, truncated at a char boundary.
pub(super) fn log_webhook_body(body: &[u8]) {
    let Ok(body_str) = std::str::from_utf8(body) else {
        return;
    };
    let truncated = if body_str.chars().count() > 1000 {
        format!("{}...", body_str.chars().take(1000).collect::<String>())
    } else {
        body_str.to_string()
    };
    tracing::debug!(body = %truncated, "Webhook request body");
}
