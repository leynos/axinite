//! HTTP transport helpers for safe skill downloads.

use futures::StreamExt;

use super::url_policy::{normalize_domain, validate_fetch_url, validate_resolved_addrs};
use super::zip_extract::extract_skill_from_zip;
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
            let lookup_host = normalize_domain(domain);
            let port = parsed
                .port_or_known_default()
                .ok_or_else(|| ToolError::ExecutionFailed("URL has no valid port".to_string()))?;

            let addrs: Vec<std::net::SocketAddr> = tokio::net::lookup_host((lookup_host, port))
                .await
                .map_err(|e| {
                    ToolError::ExecutionFailed(format!(
                        "DNS resolution failed for {}: {}",
                        lookup_host, e
                    ))
                })?
                .collect();

            validate_resolved_addrs(domain, &addrs)?;

            build_fetch_client_builder()
                .resolve_to_addrs(domain, &addrs)
                .build()
                .map_err(|e| ToolError::ExecutionFailed(format!("HTTP client error: {}", e)))
        }
    }
}

/// Fetch SKILL.md content from a URL with SSRF protection.
pub(crate) async fn fetch_skill_content(url: &str) -> Result<String, ToolError> {
    let parsed = validate_fetch_url(url)?;
    let client = build_safe_fetch_client(&parsed).await?;

    let response = client.get(parsed.clone()).send().await.map_err(|e| {
        ToolError::ExecutionFailed(format!("Failed to fetch skill from {}: {}", url, e))
    })?;

    if !response.status().is_success() {
        return Err(ToolError::ExecutionFailed(format!(
            "Skill fetch returned HTTP {}: {}",
            response.status(),
            url
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

    if bytes.starts_with(b"PK\x03\x04") {
        extract_skill_from_zip(&bytes)
    } else {
        String::from_utf8(bytes)
            .map_err(|e| ToolError::ExecutionFailed(format!("Response is not valid UTF-8: {}", e)))
    }
}
