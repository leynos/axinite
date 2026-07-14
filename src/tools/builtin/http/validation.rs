//! URL, header, IP, and path validation helpers for the HTTP tool,
//! including SSRF blocklists and `save_to` path containment.

use std::net::{IpAddr, ToSocketAddrs};

#[cfg(feature = "html-to-markdown")]
use std::collections::HashMap;

use crate::tools::tool::ToolError;

/// Validate and resolve a `save_to` path, ensuring it stays under `/tmp/`.
///
/// Uses `path_utils::validate_path` with `/tmp` as the base directory to catch
/// traversal attacks like `/tmp/../../etc/passwd` and symlink escapes.
/// Creates parent directories only after validation succeeds.
pub(super) fn validate_save_to_path(save_to: &str) -> Result<std::path::PathBuf, ToolError> {
    // Quick prefix check before doing any fs work
    if !save_to.starts_with("/tmp/") {
        return Err(ToolError::InvalidParameters(
            "save_to path must be under /tmp/".to_string(),
        ));
    }
    // Validate path BEFORE creating directories to prevent traversal-based
    // directory creation outside /tmp (e.g. `/tmp/../../etc/passwd`).
    let tmp_base = std::path::Path::new("/tmp");
    let validated = crate::tools::builtin::path_utils::validate_path(save_to, Some(tmp_base))?;
    // Only create parent directories for the validated (safe) path
    if let Some(parent) = validated.parent() {
        ambient_fs::create_dir_all(parent).map_err(|e| {
            ToolError::ExecutionFailed(format!("failed to create directory: {}", e))
        })?;
    }
    Ok(validated)
}

pub(crate) fn validate_url(url: &str) -> Result<reqwest::Url, ToolError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| ToolError::InvalidParameters(format!("invalid URL: {}", e)))?;

    if parsed.scheme() != "https" {
        return Err(ToolError::NotAuthorized(
            "only https URLs are allowed".to_string(),
        ));
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| ToolError::InvalidParameters("URL missing host".to_string()))?;

    let host_lower = host.to_lowercase();
    if host_lower == "localhost" || host_lower.ends_with(".localhost") {
        return Err(ToolError::NotAuthorized(
            "localhost is not allowed".to_string(),
        ));
    }

    // Check literal IP addresses
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_disallowed_ip(&ip)
    {
        return Err(ToolError::NotAuthorized(
            "private or local IPs are not allowed".to_string(),
        ));
    }

    // Resolve hostname and check all resolved IPs against the blocklist.
    // This prevents DNS rebinding where a hostname resolves to a private IP.
    let port = parsed.port_or_known_default().unwrap_or(443);
    let socket_addr = format!("{}:{}", host, port);
    if let Ok(addrs) = socket_addr.to_socket_addrs() {
        for addr in addrs {
            if is_disallowed_ip(&addr.ip()) {
                return Err(ToolError::NotAuthorized(format!(
                    "hostname '{}' resolves to disallowed IP {}",
                    host,
                    addr.ip()
                )));
            }
        }
    }

    Ok(parsed)
}

pub(super) fn is_disallowed_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private()
                || v4.is_loopback()
                || v4.is_link_local()
                || v4.is_multicast()
                || v4.is_unspecified()
                || *v4 == std::net::Ipv4Addr::new(169, 254, 169, 254)
        }
        IpAddr::V6(v6) => {
            v6.is_loopback()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6.is_multicast()
                || v6.is_unspecified()
        }
    }
}

#[cfg(feature = "html-to-markdown")]
/// Heuristic: treat as HTML if the `Content-Type` header contains `text/html`.
pub(super) fn is_html_response(headers: &HashMap<String, String>) -> bool {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.to_lowercase().contains("text/html"))
        .unwrap_or(false)
}

pub(super) fn parse_headers_param(
    headers: Option<&serde_json::Value>,
) -> Result<Vec<(String, String)>, ToolError> {
    match headers {
        None => Ok(Vec::new()),
        Some(serde_json::Value::Object(map)) => {
            let mut out = Vec::with_capacity(map.len());
            for (k, v) in map {
                let value = v.as_str().ok_or_else(|| {
                    ToolError::InvalidParameters(format!("header '{}' must have a string value", k))
                })?;
                out.push((k.clone(), value.to_string()));
            }
            Ok(out)
        }
        Some(serde_json::Value::Array(items)) => {
            let mut out = Vec::with_capacity(items.len());
            for (idx, item) in items.iter().enumerate() {
                let obj = item.as_object().ok_or_else(|| {
                    ToolError::InvalidParameters(format!(
                        "headers[{}] must be an object with 'name' and 'value'",
                        idx
                    ))
                })?;
                let name = obj.get("name").and_then(|v| v.as_str()).ok_or_else(|| {
                    ToolError::InvalidParameters(format!("headers[{}].name must be a string", idx))
                })?;
                let value = obj.get("value").and_then(|v| v.as_str()).ok_or_else(|| {
                    ToolError::InvalidParameters(format!("headers[{}].value must be a string", idx))
                })?;
                out.push((name.to_string(), value.to_string()));
            }
            Ok(out)
        }
        Some(_) => Err(ToolError::InvalidParameters(
            "'headers' must be an object or an array of {name, value}".to_string(),
        )),
    }
}

/// Extract host from URL in params (for approval checks).
pub(super) fn extract_host_from_params(params: &serde_json::Value) -> Option<String> {
    params
        .get("url")
        .and_then(|u| u.as_str())
        .and_then(|u| reqwest::Url::parse(u).ok())
        .and_then(|u| u.host_str().map(|h| h.to_string()))
}

/// Parse the declared `Content-Length` header as a byte count, if present and valid.
pub(super) fn declared_content_length(response: &reqwest::Response) -> Option<usize> {
    let value = response.headers().get(reqwest::header::CONTENT_LENGTH)?;
    value.to_str().ok()?.parse::<usize>().ok()
}
