//! URL and address validation helpers for safe skill fetching.

use std::net::{IpAddr, SocketAddr};

use crate::tools::tool::ToolError;

/// Return `true` when the lowercased, normalised hostname is known to resolve
/// to an internal or metadata endpoint that must not be fetched.
fn is_blocked_hostname(host_lower: &str) -> bool {
    host_lower == "localhost"
        || host_lower == "metadata.google.internal"
        || host_lower.ends_with(".internal")
        || host_lower.ends_with(".local")
}

/// Validate that a URL is safe to fetch.
pub(super) fn validate_fetch_url(url_str: &str) -> Result<reqwest::Url, ToolError> {
    let parsed = reqwest::Url::parse(url_str)
        .map_err(|e| ToolError::ExecutionFailed(format!("Invalid URL '{}': {}", url_str, e)))?;

    if parsed.scheme() != "https" {
        return Err(ToolError::ExecutionFailed(format!(
            "Only HTTPS URLs are allowed for skill fetching, got scheme '{}'",
            parsed.scheme()
        )));
    }

    let host = parsed
        .host()
        .ok_or_else(|| ToolError::ExecutionFailed("URL has no host".to_string()))?;

    if let Some(ip) = host_ip_addr(&host) {
        validate_fetch_ip(&ip, &host.to_string())?;
    }

    let host_lower = normalize_domain(host.to_string().as_str()).to_lowercase();
    if is_blocked_hostname(&host_lower) {
        return Err(ToolError::ExecutionFailed(format!(
            "URL points to an internal hostname: {}",
            host
        )));
    }

    Ok(parsed)
}

fn host_ip_addr(host: &url::Host<&str>) -> Option<IpAddr> {
    match host {
        url::Host::Ipv4(v4) => Some(IpAddr::V4(*v4)),
        url::Host::Ipv6(v6) => Some(normalize_ip(IpAddr::V6(*v6))),
        url::Host::Domain(_) => None,
    }
}

fn normalize_ip(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(v6)),
        other => other,
    }
}

/// Return `true` when the IP address is loopback, unspecified, private,
/// or link-local, and therefore must not be fetched.
fn is_non_routable_ip(ip: &IpAddr) -> bool {
    ip.is_loopback() || ip.is_unspecified() || is_private_ip(ip) || is_link_local_ip(ip)
}

fn validate_fetch_ip(ip: &IpAddr, display_host: &str) -> Result<(), ToolError> {
    if is_non_routable_ip(ip) {
        return Err(ToolError::ExecutionFailed(format!(
            "URL points to a private/loopback/link-local address: {}",
            display_host
        )));
    }

    Ok(())
}

pub(super) fn normalize_domain(host: &str) -> &str {
    host.trim_end_matches('.')
}

pub(super) fn validate_resolved_addrs(host: &str, addrs: &[SocketAddr]) -> Result<(), ToolError> {
    if addrs.is_empty() {
        return Err(ToolError::ExecutionFailed(format!(
            "DNS resolution returned no addresses for {}",
            host
        )));
    }

    for addr in addrs {
        let ip = normalize_ip(addr.ip());
        validate_fetch_ip(&ip, host)?;
    }

    Ok(())
}

pub(super) fn is_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private(),
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            (segments[0] & 0xfe00) == 0xfc00
        }
    }
}

fn is_link_local_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_link_local(),
        IpAddr::V6(v6) => {
            let segments = v6.segments();
            (segments[0] & 0xffc0) == 0xfe80
        }
    }
}
