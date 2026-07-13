//! JSON schema for WASM tool capabilities files.
//!
//! External WASM tools declare their required capabilities via a sidecar JSON file
//! (e.g., `slack.capabilities.json`). This module defines the schema for those files
//! and provides conversion to runtime [`crate::tools::wasm::Capabilities`].
//!
//! The schema is split by concern: [`file`] holds the root `CapabilitiesFile`,
//! [`http`] the HTTP allowlist/credential/rate-limit schemas, [`sections`] the
//! simple secrets/tool-invoke/workspace sections, and [`auth`] the auth and
//! setup schemas.
//!
//! # Example Capabilities File
//!
//! ```json
//! {
//!   "http": {
//!     "allowlist": [
//!       { "host": "slack.com", "path_prefix": "/api/", "methods": ["GET", "POST"] }
//!     ],
//!     "credentials": {
//!       "slack_bot_token": {
//!         "secret_name": "slack_bot_token",
//!         "location": { "type": "bearer" },
//!         "host_patterns": ["slack.com"]
//!       }
//!     },
//!     "rate_limit": { "requests_per_minute": 50, "requests_per_hour": 1000 }
//!   },
//!   "secrets": {
//!     "allowed_names": ["slack_bot_token"]
//!   }
//! }
//! ```

mod auth;
mod file;
mod http;
mod sections;

#[cfg(test)]
mod tests;

pub use auth::{AuthCapabilitySchema, OAuthConfigSchema, ValidationEndpointSchema};
pub use file::CapabilitiesFile;
pub use http::RateLimitSchema;
