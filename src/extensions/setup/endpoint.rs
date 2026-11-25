//! Offline endpoint validation.
//!
//! Validation never resolves DNS, opens a socket, or contacts the provider.
//! It inspects the submitted text and any IP literal directly, so a
//! declaration cannot make the gateway probe a private network.

use url::{Host, Url};

use super::error::EndpointRejection;
use super::model::{ENDPOINT_VALIDATION_POLICY_VERSION, EndpointUrl, MAX_ENDPOINT_URL_BYTES};

/// Parse and validate an absolute HTTPS endpoint URL.
///
/// Rules, in order:
///
/// 1. The value must be non-empty and within [`MAX_ENDPOINT_URL_BYTES`].
/// 2. It must not contain control characters, whitespace, or backslashes.
/// 3. It must parse as an absolute URL with the `https` scheme.
/// 4. It must carry a host, and no userinfo, query, or fragment.
/// 5. The host must not be a local-only name or a non-public IP literal.
///
/// The returned value is canonical: parsing its exposed form again yields an
/// identical value.
pub fn validate_endpoint_url(raw: &str) -> Result<EndpointUrl, EndpointRejection> {
    let url = parse_and_check_shape(raw)?;
    check_host(&url)?;
    Ok(EndpointUrl::from_canonical(url.to_string()))
}

/// Reject a value that cannot be a confidential HTTPS endpoint before parsing.
fn parse_and_check_shape(raw: &str) -> Result<Url, EndpointRejection> {
    if raw.trim().is_empty() {
        return Err(EndpointRejection::Empty);
    }
    if raw.len() > MAX_ENDPOINT_URL_BYTES {
        return Err(EndpointRejection::TooLong);
    }
    if raw.chars().any(is_forbidden_character) {
        return Err(EndpointRejection::InvalidCharacter);
    }

    let url = Url::parse(raw).map_err(|error| match error {
        url::ParseError::RelativeUrlWithoutBase => EndpointRejection::NotAbsolute,
        _ => EndpointRejection::Malformed,
    })?;
    check_scheme_and_components(&url)?;
    Ok(url)
}

/// Whether a character cannot appear in a submitted endpoint URL.
///
/// Control characters and whitespace could smuggle a second value past a
/// reviewer, and a backslash is rewritten by some parsers before the host is
/// examined, so all three are refused before parsing.
fn is_forbidden_character(character: char) -> bool {
    character.is_control() || character.is_whitespace() || character == '\\'
}

/// Reject a non-HTTPS scheme and any component that could leak or confuse.
fn check_scheme_and_components(url: &Url) -> Result<(), EndpointRejection> {
    if url.scheme() != "https" {
        return Err(EndpointRejection::InsecureScheme);
    }
    if url.host().is_none() {
        return Err(EndpointRejection::MissingHost);
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(EndpointRejection::CredentialsNotAllowed);
    }
    if url.query().is_some() {
        return Err(EndpointRejection::QueryNotAllowed);
    }
    if url.fragment().is_some() {
        return Err(EndpointRejection::FragmentNotAllowed);
    }
    Ok(())
}

/// Reject a host that is local-only or not publicly routable.
fn check_host(url: &Url) -> Result<(), EndpointRejection> {
    let rejection = match url.host() {
        Some(Host::Domain(domain)) => domain_rejection(domain),
        Some(Host::Ipv4(address)) => address_rejection(is_public_ipv4(address)),
        Some(Host::Ipv6(address)) => address_rejection(is_public_ipv6(address)),
        None => Some(EndpointRejection::MissingHost),
    };
    match rejection {
        Some(rejection) => Err(rejection),
        None => Ok(()),
    }
}

/// The rejection for a hostname, when it names a local-only destination.
fn domain_rejection(domain: &str) -> Option<EndpointRejection> {
    if is_local_hostname(domain) {
        Some(EndpointRejection::LocalHostname)
    } else {
        None
    }
}

/// The rejection for an address literal that is not publicly routable.
fn address_rejection(is_public: bool) -> Option<EndpointRejection> {
    if is_public {
        None
    } else {
        Some(EndpointRejection::NonPublicAddress)
    }
}

/// Validation policy version applied to newly stored bindings.
pub fn validation_policy_version() -> u16 {
    ENDPOINT_VALIDATION_POLICY_VERSION
}

fn is_local_hostname(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if host.is_empty() {
        return true;
    }
    if !host.contains('.') {
        // Single-label names resolve through local search domains.
        return true;
    }
    [
        "localhost",
        "local",
        "localdomain",
        "internal",
        "home.arpa",
        "lan",
    ]
    .iter()
    .any(|suffix| host.ends_with(&format!(".{suffix}")))
}

fn is_public_ipv4(address: std::net::Ipv4Addr) -> bool {
    let octets = address.octets();
    let routable = !is_local_ipv4(address) && !is_special_use_ipv4(address);
    // 0.0.0.0/8 "this network" and 240.0.0.0/4 reserved.
    routable && octets[0] != 0 && octets[0] < 240
}

fn is_local_ipv4(address: std::net::Ipv4Addr) -> bool {
    address.is_private() || address.is_loopback() || address.is_link_local()
}

fn is_special_use_ipv4(address: std::net::Ipv4Addr) -> bool {
    address.is_broadcast()
        || address.is_documentation()
        || address.is_unspecified()
        || address.is_multicast()
        || is_shared_or_benchmark_ipv4(address.octets())
}

// 100.64.0.0/10 carrier-grade NAT and 198.18.0.0/15 benchmarking.
fn is_shared_or_benchmark_ipv4(octets: [u8; 4]) -> bool {
    let carrier_grade_nat = octets[0] == 100 && (64..=127).contains(&octets[1]);
    let benchmarking = octets[0] == 198 && matches!(octets[1], 18 | 19);
    carrier_grade_nat || benchmarking
}

fn is_public_ipv6(address: std::net::Ipv6Addr) -> bool {
    let segments = address.segments();
    let routable = !address.is_loopback()
        && !address.is_unspecified()
        && !address.is_multicast()
        && !is_unique_local_or_link_local(segments);
    routable && !is_documentation_ipv6(segments) && embedded_ipv4_is_public(address)
}

// Unique local fc00::/7 and link-local fe80::/10.
fn is_unique_local_or_link_local(segments: [u16; 8]) -> bool {
    let unique_local = segments[0] & 0xfe00 == 0xfc00;
    let link_local = segments[0] & 0xffc0 == 0xfe80;
    unique_local || link_local
}

// Documentation 2001:db8::/32.
fn is_documentation_ipv6(segments: [u16; 8]) -> bool {
    segments[0] == 0x2001 && segments[1] == 0x0db8
}

// An embedded IPv4 address must itself be public.
fn embedded_ipv4_is_public(address: std::net::Ipv6Addr) -> bool {
    match address.to_ipv4_mapped().or_else(|| address.to_ipv4()) {
        Some(embedded) => is_public_ipv4(embedded),
        None => true,
    }
}
