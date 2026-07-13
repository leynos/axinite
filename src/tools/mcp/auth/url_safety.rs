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
        IpAddr::V4(v4) => {
            v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_unspecified()
                || (v4.octets()[0] == 169 && v4.octets()[1] == 254) // link-local
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64) // CGNAT 100.64/10
        }
        IpAddr::V6(v6) => {
            let segs = v6.segments();
            v6.is_loopback()
                || v6.is_unspecified()
                // Link-local (fe80::/10)
                || (segs[0] & 0xffc0) == 0xfe80
                // Site-local / deprecated (fec0::/10)
                || (segs[0] & 0xffc0) == 0xfec0
                // Unique local (fc00::/7)
                || (segs[0] & 0xfe00) == 0xfc00
                // Documentation (2001:db8::/32)
                || (segs[0] == 0x2001 && segs[1] == 0x0db8)
                // Check for IPv4-mapped IPv6 (::ffff:x.x.x.x)
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| is_dangerous_ip(IpAddr::V4(v4)))
        }
    }
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
        let host = parsed.host_str().unwrap_or("");
        let is_localhost =
            host == "localhost" || host == "127.0.0.1" || host == "::1" || host == "[::1]";
        if !is_localhost {
            return Err(AuthError::DiscoveryFailed(format!(
                "HTTP is only allowed for localhost; use HTTPS for '{}'",
                host
            )));
        }
        // Localhost HTTP is allowed for dev — skip SSRF checks since we've
        // already validated the host is localhost/loopback.
        return Ok(());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| AuthError::DiscoveryFailed("URL has no host".to_string()))?;

    // For IP literals, parse directly and check.
    if let Ok(ip) = host.parse::<IpAddr>()
        && is_dangerous_ip(ip)
    {
        return Err(AuthError::DiscoveryFailed(format!(
            "URL points to a restricted IP address: {}",
            host
        )));
    }

    // For hostnames, resolve DNS and check each resolved address.
    // This prevents DNS-based SSRF where a hostname resolves to an internal IP
    // (e.g., 169.254.169.254 for cloud metadata endpoints).
    if host.parse::<IpAddr>().is_err() {
        let addr = format!("{}:{}", host, parsed.port_or_known_default().unwrap_or(443));
        match tokio::net::lookup_host(&addr).await {
            Ok(addrs) => {
                for socket_addr in addrs {
                    if is_dangerous_ip(socket_addr.ip()) {
                        return Err(AuthError::DiscoveryFailed(format!(
                            "URL hostname '{}' resolves to restricted IP address: {}",
                            host,
                            socket_addr.ip()
                        )));
                    }
                }
            }
            Err(e) => {
                // DNS failure = fail closed (do not allow the request)
                return Err(AuthError::DiscoveryFailed(format!(
                    "DNS resolution failed for '{}': {}",
                    host, e
                )));
            }
        }
    }

    Ok(())
}
