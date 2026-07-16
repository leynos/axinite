//! HTTP request tool.
//!
//! Submodules:
//! - [`validation`]: URL/header/IP/path validation and SSRF blocklists
//! - [`execute`]: the `NativeTool` implementation (request execution)

mod execute;
#[cfg(test)]
mod tests;
mod validation;

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use reqwest::Client;

use crate::secrets::SecretsStore;
use crate::tools::tool::ToolError;
use crate::tools::wasm::SharedCredentialRegistry;

use validation::{extract_host_from_params, is_disallowed_ip};

/// Maximum response body size for text responses (5 MB).
///
/// 5 MB is large enough for typical JSON API responses and moderate HTML pages,
/// but small enough to prevent OOM from malicious or runaway servers.  The WASM
/// HTTP wrapper uses the same limit for consistency.
const MAX_RESPONSE_SIZE: usize = 5 * 1024 * 1024;

/// Maximum response body size when saving to disk via `save_to` (50 MB).
///
/// Larger limit for file downloads since the body is written to disk, not held
/// in memory for LLM context. Matches the WASM attachment size cap.
const MAX_SAVE_TO_SIZE: usize = 50 * 1024 * 1024;

/// Tool for making HTTP requests.
pub struct HttpTool {
    client: Client,
    credential_registry: Option<Arc<SharedCredentialRegistry>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
}

impl HttpTool {
    /// Create a new HTTP tool.
    ///
    /// # Errors
    ///
    /// Returns [`ToolError::ExecutionFailed`] when the underlying reqwest
    /// client cannot be built.
    pub fn new() -> Result<Self, ToolError> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 10 {
                    return attempt.error("too many redirects");
                }
                // Reject scheme downgrades (https → http)
                if attempt.url().scheme() != "https" {
                    return attempt.error("redirect to non-HTTPS URL is not allowed");
                }
                // Extract host info before consuming attempt
                let host_owned = attempt.url().host_str().map(|h| h.to_owned());
                let port = attempt.url().port_or_known_default().unwrap_or(443);

                if let Some(host) = host_owned {
                    let host_lower = host.to_lowercase();
                    if host_lower == "localhost" || host_lower.ends_with(".localhost") {
                        return attempt.error("redirect to localhost is not allowed");
                    }
                    if let Ok(ip) = host.parse::<IpAddr>()
                        && is_disallowed_ip(&ip)
                    {
                        return attempt.error("redirect to private/local IP is not allowed");
                    }
                    // Resolve hostname and check all IPs
                    let socket_addr = format!("{}:{}", host, port);
                    if let Ok(addrs) = std::net::ToSocketAddrs::to_socket_addrs(&socket_addr) {
                        for addr in addrs {
                            if is_disallowed_ip(&addr.ip()) {
                                let msg = format!(
                                    "redirect target '{}' resolves to disallowed IP {}",
                                    host,
                                    addr.ip()
                                );
                                return attempt.error(msg);
                            }
                        }
                    }
                }
                attempt.follow()
            }))
            .build()
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to create HTTP client: {e}"))
            })?;

        Ok(Self {
            client,
            credential_registry: None,
            secrets_store: None,
        })
    }

    /// Attach a credential registry and secrets store for auto-injection.
    pub fn with_credentials(
        mut self,
        registry: Arc<SharedCredentialRegistry>,
        secrets_store: Arc<dyn SecretsStore + Send + Sync>,
    ) -> Self {
        self.credential_registry = Some(registry);
        self.secrets_store = Some(secrets_store);
        self
    }

    /// Return `true` when credentials would be auto-injected for the request's target host.
    fn host_has_mapped_credentials(&self, params: &serde_json::Value) -> bool {
        let Some(ref registry) = self.credential_registry else {
            return false;
        };
        let Some(host) = extract_host_from_params(params) else {
            return false;
        };
        registry.has_credentials_for_host(&host)
    }
}
