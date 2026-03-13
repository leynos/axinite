//! Helpers for platform-aware OAuth callback routing and state handling.

use crate::llm::oauth_helpers::is_loopback_host;

/// Returns `true` if OAuth callbacks should be routed through the web gateway
/// instead of the local TCP listener.
pub fn use_gateway_callback() -> bool {
    std::env::var("IRONCLAW_OAUTH_CALLBACK_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .map(|raw| {
            url::Url::parse(&raw)
                .ok()
                .and_then(|u| u.host_str().map(String::from))
                .map(|host| !is_loopback_host(&host))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// Prepend instance name to CSRF state for platform routing.
pub fn build_platform_state(nonce: &str) -> String {
    let instance = std::env::var("IRONCLAW_INSTANCE_NAME")
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| {
            std::env::var("OPENCLAW_INSTANCE_NAME")
                .ok()
                .filter(|v| !v.is_empty())
        });
    match instance {
        Some(name) => format!("{name}:{nonce}"),
        None => nonce.to_string(),
    }
}

/// Strip the instance prefix from a state parameter to recover the lookup nonce.
pub fn strip_instance_prefix(state: &str) -> &str {
    state
        .split_once(':')
        .map(|(_, nonce)| nonce)
        .unwrap_or(state)
}
