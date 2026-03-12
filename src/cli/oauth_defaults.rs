//! Shared OAuth infrastructure: built-in credentials, callback server, landing pages.
//!
//! Every OAuth flow in the codebase uses the same callback port, landing page,
//! built-in credentials, and token exchange helpers from this module.

mod oauth_credentials;
mod oauth_flow;
mod oauth_gateway;
mod oauth_platform;

pub use crate::llm::oauth_helpers::{
    OAUTH_CALLBACK_PORT, OAuthCallbackError, bind_callback_listener, callback_host, callback_url,
    is_loopback_host, landing_html, wait_for_callback,
};
pub use oauth_credentials::{OAuthCredentials, builtin_credentials};
pub use oauth_flow::{
    OAuthTokenResponse, OAuthUrlResult, build_oauth_url, exchange_oauth_code, store_oauth_tokens,
    validate_oauth_token,
};
pub use oauth_gateway::{
    OAUTH_FLOW_EXPIRY, PendingOAuthFlow, PendingOAuthRegistry, exchange_via_proxy,
    new_pending_oauth_registry, sweep_expired_flows,
};
pub use oauth_platform::{build_platform_state, strip_instance_prefix, use_gateway_callback};

#[cfg(test)]
mod tests;
