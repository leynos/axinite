//! OAuth 2.1 authentication for MCP servers.
//!
//! Implements the MCP Authorization specification using OAuth 2.1 with PKCE.
//! See: https://spec.modelcontextprotocol.io/specification/2025-03-26/basic/authorization/
//!
//! The module is split by concern: [`types`] holds the shared OAuth types,
//! [`url_safety`] URL construction and SSRF protection, [`discovery`] the
//! endpoint discovery and dynamic client registration logic, [`flow`] the
//! interactive authorization flow, and [`tokens`] token storage and refresh.

mod discovery;
mod flow;
mod tokens;
mod types;
mod url_safety;

#[cfg(test)]
mod tests;

pub use discovery::{
    discover_authorization_server, discover_full_oauth_metadata, discover_oauth_endpoints,
    discover_protected_resource, register_client,
};
pub use flow::{
    TokenExchangeRequest, authorize_mcp_server, build_authorization_url, exchange_code_for_token,
    find_available_port, wait_for_authorization_callback,
};
pub use tokens::{
    get_access_token, is_authenticated, refresh_access_token, store_client_id, store_tokens,
};
pub use types::{
    AccessToken, AuthError, AuthorizationServerMetadata, ClientRegistrationRequest,
    ClientRegistrationResponse, PkceChallenge, ProtectedResourceMetadata,
};
pub use url_safety::{build_well_known_uri, canonical_resource_uri};
