//! Helpers for platform-aware OAuth callback routing and state handling.

use crate::config::EnvContext;
use crate::llm::oauth_helpers::is_loopback_host;

/// Returns `true` if OAuth callbacks should be routed through the web gateway
/// instead of the local TCP listener.
pub fn use_gateway_callback() -> bool {
    use_gateway_callback_from(&EnvContext::capture_ambient())
}

/// Resolve gateway callback routing from an explicit environment snapshot.
pub fn use_gateway_callback_from(ctx: &EnvContext) -> bool {
    ctx.get("AXINITE_OAUTH_CALLBACK_URL")
        .map(|raw| {
            url::Url::parse(raw)
                .ok()
                .and_then(|u| u.host_str().map(String::from))
                .map(|host| !is_loopback_host(&host))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

fn platform_instance_name_from(ctx: &EnvContext) -> Option<String> {
    ctx.get("AXINITE_INSTANCE_NAME")
        .filter(|v| !v.contains(':'))
        .map(str::to_string)
        .or_else(|| {
            ctx.get("OPENCLAW_INSTANCE_NAME")
                .filter(|v| !v.contains(':'))
                .map(str::to_string)
        })
}

/// Prepend instance name to CSRF state for platform routing.
pub fn build_platform_state(nonce: &str) -> String {
    build_platform_state_from(nonce, &EnvContext::capture_ambient())
}

/// Build platform state using an explicit environment snapshot.
pub fn build_platform_state_from(nonce: &str, ctx: &EnvContext) -> String {
    match platform_instance_name_from(ctx) {
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
