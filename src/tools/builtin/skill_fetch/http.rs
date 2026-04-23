//! HTTP transport helpers for safe skill downloads.

use futures::StreamExt;

use super::url_policy::{
    HttpsUrl, NormalizedDomain, redact_url, validate_fetch_url, validate_resolved_addrs,
};
use crate::tools::tool::ToolError;

const USER_AGENT: &str = concat!("ironclaw/", env!("CARGO_PKG_VERSION"));
const MAX_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024;

fn build_fetch_client_builder() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .user_agent(USER_AGENT)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
}

async fn build_safe_fetch_client(parsed: &reqwest::Url) -> Result<reqwest::Client, ToolError> {
    let host = parsed
        .host()
        .ok_or_else(|| ToolError::ExecutionFailed("URL has no host".to_string()))?;

    match host {
        url::Host::Ipv4(_) | url::Host::Ipv6(_) => build_fetch_client_builder()
            .build()
            .map_err(|e| ToolError::ExecutionFailed(format!("HTTP client error: {}", e))),
        url::Host::Domain(domain) => {
            let lookup_host = NormalizedDomain::new(domain);
            let port = parsed
                .port_or_known_default()
                .ok_or_else(|| ToolError::ExecutionFailed("URL has no valid port".to_string()))?;

            let addrs: Vec<std::net::SocketAddr> = tokio::time::timeout(
                std::time::Duration::from_secs(15),
                tokio::net::lookup_host((lookup_host.as_str(), port)),
            )
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!(
                    "DNS resolution timed out for {}",
                    lookup_host.as_str()
                ))
            })?
            .map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "DNS resolution failed for {}: {}",
                    lookup_host.as_str(),
                    e
                ))
            })?
            .collect();

            validate_resolved_addrs(&NormalizedDomain::new(domain), &addrs)?;

            build_fetch_client_builder()
                .resolve_to_addrs(domain, &addrs)
                .build()
                .map_err(|e| ToolError::ExecutionFailed(format!("HTTP client error: {}", e)))
        }
    }
}

/// Fetch raw skill bytes from a URL with SSRF protection.
pub(crate) async fn fetch_skill_bytes(url: &str) -> Result<Vec<u8>, ToolError> {
    let https = HttpsUrl::try_from(url)?;
    let parsed = validate_fetch_url(&https)?;
    let client = build_safe_fetch_client(&parsed).await?;
    let safe_url = redact_url(&parsed);

    let response = client.get(parsed.clone()).send().await.map_err(|e| {
        ToolError::ExecutionFailed(format!("Failed to fetch skill from {}: {}", safe_url, e))
    })?;

    if !response.status().is_success() {
        return Err(ToolError::ExecutionFailed(format!(
            "Skill fetch returned HTTP {}: {}",
            response.status(),
            safe_url
        )));
    }

    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| {
            ToolError::ExecutionFailed(format!("Failed to read response body: {}", e))
        })?;
        let next_len = bytes.len().saturating_add(chunk.len());
        if next_len > MAX_DOWNLOAD_BYTES {
            return Err(ToolError::ExecutionFailed(format!(
                "Response too large: {} bytes (max {} bytes)",
                next_len, MAX_DOWNLOAD_BYTES
            )));
        }
        bytes.extend_from_slice(&chunk);
    }

    Ok(bytes)
}
