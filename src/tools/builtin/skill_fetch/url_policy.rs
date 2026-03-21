//! URL and address validation helpers for safe skill fetching.

use std::net::{IpAddr, SocketAddr};

use crate::tools::tool::ToolError;

pub(super) struct HttpsUrl(reqwest::Url);

impl HttpsUrl {
    pub(super) fn as_url(&self) -> &reqwest::Url {
        &self.0
    }
}

impl TryFrom<&str> for HttpsUrl {
    type Error = ToolError;

    fn try_from(url_str: &str) -> Result<Self, Self::Error> {
        let parsed = reqwest::Url::parse(url_str)
            .map_err(|e| ToolError::ExecutionFailed(format!("Invalid URL: {}", e)))?;

        if parsed.scheme() != "https" {
            return Err(ToolError::InvalidParameters(format!(
                "Only HTTPS URLs are allowed for skill fetching, got scheme '{}'",
                parsed.scheme()
            )));
        }

        if parsed.host().is_none() {
            return Err(ToolError::InvalidParameters("URL has no host".to_string()));
        }

        Ok(Self(parsed))
    }
}

pub(super) struct NormalizedDomain(String);

impl NormalizedDomain {
    pub(super) fn new(host: &str) -> Self {
        Self(host.trim_end_matches('.').to_lowercase())
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

enum Host {
    Domain(NormalizedDomain),
    Ip(IpAddr),
}

/// Return a URL string with userinfo, query, and fragment stripped, safe to
/// include in error messages.
pub(super) fn redact_url(url: &reqwest::Url) -> String {
    let mut redacted = url.clone();
    let _ = redacted.set_username("");
    let _ = redacted.set_password(None);
    redacted.set_query(None);
    redacted.set_fragment(None);
    redacted.to_string()
}

/// Return `true` when the lowercased, normalised hostname is known to resolve
/// to an internal or metadata endpoint that must not be fetched.
fn is_blocked_hostname(host: &NormalizedDomain) -> bool {
    host.as_str() == "localhost"
        || host.as_str() == "metadata.google.internal"
        || host.as_str().ends_with(".internal")
        || host.as_str().ends_with(".local")
}

/// Validate that a URL is safe to fetch.
pub(super) fn validate_fetch_url(url: &HttpsUrl) -> Result<reqwest::Url, ToolError> {
    let parsed = url.as_url().clone();
    let display_host = parsed
        .host()
        .expect("HttpsUrl guarantees that validated URLs always have a host")
        .to_string();
    match parse_host(&parsed) {
        Host::Ip(ip) => validate_fetch_ip(&ip, &display_host)?,
        Host::Domain(domain) => {
            if is_blocked_hostname(&domain) {
                return Err(ToolError::InvalidParameters(format!(
                    "URL points to an internal hostname: {}",
                    display_host
                )));
            }
        }
    }

    Ok(parsed)
}

fn parse_host(url: &reqwest::Url) -> Host {
    match url
        .host()
        .expect("HttpsUrl guarantees that validated URLs always have a host")
    {
        url::Host::Ipv4(v4) => Host::Ip(IpAddr::V4(v4)),
        url::Host::Ipv6(v6) => Host::Ip(normalize_ip(IpAddr::V6(v6))),
        url::Host::Domain(domain) => Host::Domain(NormalizedDomain::new(domain)),
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
        return Err(ToolError::InvalidParameters(format!(
            "URL points to a private/loopback/link-local address: {}",
            display_host
        )));
    }

    Ok(())
}

pub(super) fn validate_resolved_addrs(
    host: &NormalizedDomain,
    addrs: &[SocketAddr],
) -> Result<(), ToolError> {
    if addrs.is_empty() {
        return Err(ToolError::InvalidParameters(format!(
            "DNS resolution returned no addresses for {}",
            host.as_str()
        )));
    }

    for addr in addrs {
        let ip = normalize_ip(addr.ip());
        validate_fetch_ip(&ip, host.as_str())?;
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
