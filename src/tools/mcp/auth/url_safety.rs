//! URL construction and safety checks: well-known URI building (RFC 8414 /
//! RFC 9728), canonical resource URIs (RFC 8707), and SSRF protection for
//! server-side requests.

use std::net::IpAddr;

use super::types::AuthError;

// ---------------------------------------------------------------------------
// Well-known URI construction (RFC 8414 / RFC 9728)
// ---------------------------------------------------------------------------

/// Build a well-known URI according to RFC 8414 / RFC 9728.
///
/// The path component of the base URL is placed *after* the well-known suffix:
/// ```text
/// https://example.com/path + oauth-authorization-server
///   -> https://example.com/.well-known/oauth-authorization-server/path
/// ```
pub fn build_well_known_uri(base_url: &str, suffix: &str) -> Result<String, AuthError> {
    let parsed = reqwest::Url::parse(base_url)
        .map_err(|e| AuthError::DiscoveryFailed(format!("Invalid URL: {}", e)))?;
    let origin = parsed.origin().ascii_serialization();
    let path = parsed.path().trim_end_matches('/');
    Ok(format!("{}/.well-known/{}{}", origin, suffix, path))
}

// ---------------------------------------------------------------------------
// RFC 8707 resource parameter
// ---------------------------------------------------------------------------

/// Compute the canonical resource URI for RFC 8707.
///
/// Strips fragments and trailing slashes from the server URL.
pub fn canonical_resource_uri(server_url: &str) -> String {
    match reqwest::Url::parse(server_url) {
        Ok(mut parsed) => {
            parsed.set_fragment(None);
            let s = parsed.to_string();
            s.trim_end_matches('/').to_string()
        }
        Err(_) => server_url.trim_end_matches('/').to_string(),
    }
}

// ---------------------------------------------------------------------------
// SSRF protection
// ---------------------------------------------------------------------------

/// Check if an IP address is dangerous (loopback, link-local, private, etc.)
pub(super) fn is_dangerous_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_dangerous_ipv4(v4),
        IpAddr::V6(v6) => is_dangerous_ipv6(v6),
    }
}

/// Whether an IPv4 address falls in a loopback, private, or reserved range.
fn is_dangerous_ipv4(v4: std::net::Ipv4Addr) -> bool {
    // Link-local 169.254.0.0/16 is covered by `is_link_local`.
    if v4.is_loopback() || v4.is_private() {
        return true;
    }
    if v4.is_link_local() || v4.is_broadcast() {
        return true;
    }
    v4.is_unspecified() || is_cgnat_ipv4(v4)
}

/// Whether an IPv4 address falls in the CGNAT range (100.64.0.0/10).
fn is_cgnat_ipv4(v4: std::net::Ipv4Addr) -> bool {
    v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64
}

/// Whether an IPv6 address falls in a loopback, local, or reserved range.
fn is_dangerous_ipv6(v6: std::net::Ipv6Addr) -> bool {
    if v6.is_loopback() || v6.is_unspecified() {
        return true;
    }
    if is_reserved_ipv6_block(v6) {
        return true;
    }
    // IPv4-mapped IPv6 (::ffff:x.x.x.x) inherits the IPv4 verdict.
    v6.to_ipv4_mapped().is_some_and(is_dangerous_ipv4)
}

/// Whether an IPv6 address belongs to a link-local, site-local, unique-local,
/// or documentation prefix.
fn is_reserved_ipv6_block(v6: std::net::Ipv6Addr) -> bool {
    let segs = v6.segments();
    // Link-local (fe80::/10) or site-local / deprecated (fec0::/10).
    let local = (segs[0] & 0xffc0) == 0xfe80 || (segs[0] & 0xffc0) == 0xfec0;
    if local {
        return true;
    }
    // Unique local (fc00::/7).
    if (segs[0] & 0xfe00) == 0xfc00 {
        return true;
    }
    // Documentation (2001:db8::/32).
    segs[0] == 0x2001 && segs[1] == 0x0db8
}

/// Validate that a URL is safe for server-side requests (SSRF protection).
pub(super) async fn validate_url_safe(url: &str) -> Result<(), AuthError> {
    let parsed = reqwest::Url::parse(url)
        .map_err(|e| AuthError::DiscoveryFailed(format!("Invalid URL: {}", e)))?;

    // Must be HTTPS. HTTP is only allowed for localhost/loopback (dev scenarios).
    let scheme = parsed.scheme();
    if scheme != "https" && scheme != "http" {
        return Err(AuthError::DiscoveryFailed(format!(
            "Unsupported scheme: {}",
            scheme
        )));
    }
    if scheme == "http" {
        // Localhost HTTP is allowed for dev — skip SSRF checks since the host
        // is already validated as localhost/loopback.
        return validate_http_host_is_localhost(&parsed);
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| AuthError::DiscoveryFailed("URL has no host".to_string()))?;

    match host.parse::<IpAddr>() {
        Ok(ip) => check_ip_literal(ip, host),
        Err(_) => {
            let port = parsed.port_or_known_default().unwrap_or(443);
            check_resolved_addresses(host, port).await
        }
    }
}

/// Whether a host string names the local machine (loopback aliases).
fn is_localhost_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]")
}

/// Reject plain-HTTP URLs whose host is not localhost/loopback.
fn validate_http_host_is_localhost(parsed: &reqwest::Url) -> Result<(), AuthError> {
    let host = parsed.host_str().unwrap_or("");
    if is_localhost_host(host) {
        return Ok(());
    }
    Err(AuthError::DiscoveryFailed(format!(
        "HTTP is only allowed for localhost; use HTTPS for '{}'",
        host
    )))
}

/// Reject IP-literal hosts that fall in a restricted range.
fn check_ip_literal(ip: IpAddr, host: &str) -> Result<(), AuthError> {
    if is_dangerous_ip(ip) {
        return Err(AuthError::DiscoveryFailed(format!(
            "URL points to a restricted IP address: {}",
            host
        )));
    }
    Ok(())
}

/// Resolve a hostname and reject it if any resolved address is restricted.
///
/// This prevents DNS-based SSRF where a hostname resolves to an internal IP
/// (e.g., 169.254.169.254 for cloud metadata endpoints). DNS failures fail
/// closed (the request is not allowed).
async fn check_resolved_addresses(host: &str, port: u16) -> Result<(), AuthError> {
    let addr = format!("{}:{}", host, port);
    let addrs = tokio::net::lookup_host(&addr).await.map_err(|e| {
        AuthError::DiscoveryFailed(format!("DNS resolution failed for '{}': {}", host, e))
    })?;
    for socket_addr in addrs {
        if is_dangerous_ip(socket_addr.ip()) {
            return Err(AuthError::DiscoveryFailed(format!(
                "URL hostname '{}' resolves to restricted IP address: {}",
                host,
                socket_addr.ip()
            )));
        }
    }
    Ok(())
}
