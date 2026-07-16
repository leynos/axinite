//! HTTP capability schema: endpoint allowlists, credential mappings and
//! injection locations, and rate limits.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::secrets::{CredentialLocation, CredentialMapping};
use crate::tools::wasm::{EndpointPattern, HttpCapability, RateLimitConfig};

/// HTTP capability schema.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpCapabilitySchema {
    /// Allowed endpoint patterns.
    #[serde(default)]
    pub allowlist: Vec<EndpointPatternSchema>,

    /// Credential mappings (key is an identifier, not the secret name).
    #[serde(default)]
    pub credentials: HashMap<String, CredentialMappingSchema>,

    /// Rate limiting configuration.
    #[serde(default)]
    pub rate_limit: Option<RateLimitSchema>,

    /// Maximum request body size in bytes.
    #[serde(default)]
    pub max_request_bytes: Option<usize>,

    /// Maximum response body size in bytes.
    #[serde(default)]
    pub max_response_bytes: Option<usize>,

    /// Request timeout in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

impl HttpCapabilitySchema {
    pub(super) fn to_http_capability(&self) -> HttpCapability {
        let mut cap = HttpCapability {
            allowlist: self
                .allowlist
                .iter()
                .map(|p| p.to_endpoint_pattern())
                .collect(),
            credentials: self
                .credentials
                .values()
                .map(|m| (m.secret_name.clone(), m.to_credential_mapping()))
                .collect(),
            rate_limit: self
                .rate_limit
                .as_ref()
                .map(|r| r.to_rate_limit_config())
                .unwrap_or_default(),
            ..Default::default()
        };

        if let Some(max) = self.max_request_bytes {
            cap.max_request_bytes = max;
        }
        if let Some(max) = self.max_response_bytes {
            cap.max_response_bytes = max;
        }
        if let Some(secs) = self.timeout_secs {
            cap.timeout = Duration::from_secs(secs);
        }

        cap
    }
}

/// Endpoint pattern schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointPatternSchema {
    /// Hostname (e.g., "api.slack.com" or "*.slack.com").
    pub host: String,

    /// Optional path prefix (e.g., "/api/").
    #[serde(default)]
    pub path_prefix: Option<String>,

    /// Allowed HTTP methods (empty = all).
    #[serde(default)]
    pub methods: Vec<String>,
}

impl EndpointPatternSchema {
    fn to_endpoint_pattern(&self) -> EndpointPattern {
        EndpointPattern {
            host: self.host.clone(),
            path_prefix: self.path_prefix.clone(),
            methods: self.methods.clone(),
        }
    }
}

/// Credential mapping schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CredentialMappingSchema {
    /// Name of the secret to inject.
    pub secret_name: String,

    /// Where to inject the credential.
    pub location: CredentialLocationSchema,

    /// Host patterns this credential applies to.
    #[serde(default)]
    pub host_patterns: Vec<String>,
}

impl CredentialMappingSchema {
    fn to_credential_mapping(&self) -> CredentialMapping {
        CredentialMapping {
            secret_name: self.secret_name.clone(),
            location: self.location.to_credential_location(),
            host_patterns: self.host_patterns.clone(),
        }
    }
}

/// Credential injection location schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CredentialLocationSchema {
    /// Bearer token in Authorization header.
    Bearer,

    /// Basic auth (password from secret, username in config).
    Basic { username: String },

    /// Custom header.
    Header {
        #[serde(alias = "header_name")]
        name: String,
        #[serde(default)]
        prefix: Option<String>,
    },

    /// Query parameter.
    QueryParam { name: String },

    /// URL/path placeholder replacement.
    UrlPath { placeholder: String },
}

impl CredentialLocationSchema {
    fn to_credential_location(&self) -> CredentialLocation {
        match self {
            CredentialLocationSchema::Bearer => CredentialLocation::AuthorizationBearer,
            CredentialLocationSchema::Basic { username } => {
                CredentialLocation::AuthorizationBasic {
                    username: username.clone(),
                }
            }
            CredentialLocationSchema::Header { name, prefix } => CredentialLocation::Header {
                name: name.clone(),
                prefix: prefix.clone(),
            },
            CredentialLocationSchema::QueryParam { name } => {
                CredentialLocation::QueryParam { name: name.clone() }
            }
            CredentialLocationSchema::UrlPath { placeholder } => CredentialLocation::UrlPath {
                placeholder: placeholder.clone(),
            },
        }
    }
}

/// Rate limit schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitSchema {
    /// Maximum requests per minute.
    #[serde(default = "default_requests_per_minute")]
    pub requests_per_minute: u32,

    /// Maximum requests per hour.
    #[serde(default = "default_requests_per_hour")]
    pub requests_per_hour: u32,
}

fn default_requests_per_minute() -> u32 {
    60
}

fn default_requests_per_hour() -> u32 {
    1000
}

impl RateLimitSchema {
    pub(super) fn to_rate_limit_config(&self) -> RateLimitConfig {
        RateLimitConfig {
            requests_per_minute: self.requests_per_minute,
            requests_per_hour: self.requests_per_hour,
        }
    }
}
