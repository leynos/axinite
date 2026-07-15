//! Network policy validation for outbound webhook targets: URL, header,
//! host, and IP checks plus DNS-pinned client construction.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::config::HookBundleError;

pub(super) fn validate_webhook_url(
    hook_name: &str,
    url: &str,
) -> Result<reqwest::Url, HookBundleError> {
    let parsed = reqwest::Url::parse(url).map_err(|_| HookBundleError::InvalidWebhookUrl {
        hook: hook_name.to_string(),
        url: url.to_string(),
    })?;

    if parsed.scheme() != "https" {
        return Err(HookBundleError::InvalidWebhookScheme {
            hook: hook_name.to_string(),
            scheme: parsed.scheme().to_string(),
        });
    }

    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err(HookBundleError::InvalidWebhookUrl {
            hook: hook_name.to_string(),
            url: url.to_string(),
        });
    }

    if let Some(host) = parsed.host_str() {
        ensure_webhook_host_allowed(hook_name, host)?;
    }

    Ok(parsed)
}

/// Reject a webhook host that is either a forbidden IP literal or a forbidden
/// hostname (loopback aliases, cloud metadata endpoints, and similar).
fn ensure_webhook_host_allowed(hook_name: &str, host: &str) -> Result<(), HookBundleError> {
    let normalized_host = normalize_host(host);
    let forbidden = match normalized_host.parse::<IpAddr>() {
        Ok(ip) => is_forbidden_ip(ip),
        Err(_) => is_forbidden_webhook_host(normalized_host),
    };
    if forbidden {
        return Err(HookBundleError::ForbiddenWebhookHost {
            hook: hook_name.to_string(),
            host: normalized_host.to_string(),
        });
    }
    Ok(())
}

pub(super) async fn dispatch_client_for_target(
    base_client: &reqwest::Client,
    url: &str,
    timeout: Duration,
) -> Result<reqwest::Client, String> {
    let parsed = reqwest::Url::parse(url).map_err(|e| format!("Invalid URL: {e}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "Webhook URL has no host".to_string())?;
    let normalized_host = normalize_host(host);

    // Literal IP targets need no DNS pinning; just screen the address.
    if let Ok(ip) = normalized_host.parse::<IpAddr>() {
        if is_forbidden_ip(ip) {
            return Err(format!("Webhook target resolves to blocked IP {ip}"));
        }
        return Ok(base_client.clone());
    }

    let port = parsed
        .port_or_known_default()
        .ok_or_else(|| "Webhook URL has no valid port".to_string())?;

    let addrs = resolve_and_screen(normalized_host, port).await?;
    build_pinned_client(timeout, normalized_host, &addrs)
}

/// Resolve a host to addresses and reject empty or forbidden results.
async fn resolve_and_screen(host: &str, port: u16) -> Result<Vec<SocketAddr>, String> {
    let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("DNS resolution failed: {e}"))?
        .collect();

    if addrs.is_empty() {
        return Err("DNS resolution returned no addresses".to_string());
    }

    for addr in &addrs {
        if is_forbidden_ip(addr.ip()) {
            return Err(format!(
                "Webhook target resolves to blocked IP {}",
                addr.ip()
            ));
        }
    }
    Ok(addrs)
}

/// Build a redirect-free client pinned to the pre-screened addresses.
fn build_pinned_client(
    timeout: Duration,
    host: &str,
    addrs: &[SocketAddr],
) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::none())
        .resolve_to_addrs(host, addrs)
        .build()
        .map_err(|e| format!("Failed to build resolved webhook client: {e}"))
}

fn normalize_host(host: &str) -> &str {
    host.trim_start_matches('[').trim_end_matches(']')
}

pub(super) fn validate_webhook_headers(
    hook_name: &str,
    headers: &HashMap<String, String>,
) -> Result<HeaderMap, HookBundleError> {
    let mut validated = HeaderMap::new();

    for (name, value) in headers {
        let header_name = HeaderName::from_bytes(name.as_bytes()).map_err(|e| {
            HookBundleError::InvalidWebhookHeader {
                hook: hook_name.to_string(),
                header: name.clone(),
                reason: e.to_string(),
            }
        })?;

        if is_forbidden_header(header_name.as_str()) {
            return Err(HookBundleError::ForbiddenWebhookHeader {
                hook: hook_name.to_string(),
                header: name.clone(),
            });
        }

        let header_value =
            HeaderValue::from_str(value).map_err(|e| HookBundleError::InvalidWebhookHeader {
                hook: hook_name.to_string(),
                header: name.clone(),
                reason: e.to_string(),
            })?;

        validated.insert(header_name, header_value);
    }

    Ok(validated)
}

fn is_forbidden_webhook_host(host: &str) -> bool {
    let lower = host.to_ascii_lowercase();
    lower == "localhost"
        || lower.ends_with(".localhost")
        || lower == "host.docker.internal"
        || lower == "metadata.google.internal"
        || lower == "metadata.aws.internal"
}

fn is_forbidden_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_forbidden_ipv4(v4),
        IpAddr::V6(v6) => {
            if let Some(mapped) = ipv6_mapped_ipv4(v6) {
                return is_forbidden_ipv4(mapped);
            }

            if is_local_scope_ipv6(v6) {
                return true;
            }

            // Documentation range (2001:db8::/32).
            let segments = v6.segments();
            segments[0] == 0x2001 && segments[1] == 0x0db8
        }
    }
}

/// Whether an IPv6 address is loopback, unspecified, or otherwise
/// local in scope (unique-local, link-local, or multicast).
fn is_local_scope_ipv6(v6: Ipv6Addr) -> bool {
    let unbound = v6.is_loopback() || v6.is_unspecified();
    unbound || is_link_or_multicast_ipv6(v6)
}

/// Whether an IPv6 address is unique-local, unicast link-local, or multicast.
fn is_link_or_multicast_ipv6(v6: Ipv6Addr) -> bool {
    let local = v6.is_unique_local() || v6.is_unicast_link_local();
    local || v6.is_multicast()
}

fn ipv6_mapped_ipv4(v6: Ipv6Addr) -> Option<Ipv4Addr> {
    let segments = v6.segments();
    // An IPv4-mapped address is ::ffff:a.b.c.d — five zero segments
    // followed by 0xffff.
    if segments[..5] == [0u16; 5] && segments[5] == 0xffff {
        Some(Ipv4Addr::new(
            (segments[6] >> 8) as u8,
            segments[6] as u8,
            (segments[7] >> 8) as u8,
            segments[7] as u8,
        ))
    } else {
        None
    }
}

/// Whether an IPv4 address falls in a standard non-public class
/// (private, loopback, link-local, unspecified, broadcast,
/// documentation, or multicast).
fn is_nonpublic_ipv4_class(v4: Ipv4Addr) -> bool {
    let internal = v4.is_private() || v4.is_loopback();
    let local = v4.is_link_local() || v4.is_unspecified();
    let special = v4.is_broadcast() || v4.is_documentation();
    let scoped = internal || local;
    let reserved = special || v4.is_multicast();
    scoped || reserved
}

fn is_forbidden_ipv4(v4: Ipv4Addr) -> bool {
    if is_nonpublic_ipv4_class(v4) {
        return true;
    }

    let octets = v4.octets();

    // Carrier-grade NAT range (100.64.0.0/10).
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        return true;
    }

    // Benchmark testing range (198.18.0.0/15).
    if octets[0] == 198 && matches!(octets[1], 18 | 19) {
        return true;
    }

    false
}

fn is_forbidden_header(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let exact = matches!(
        lower.as_str(),
        "host"
            | "authorization"
            | "cookie"
            | "proxy-authorization"
            | "forwarded"
            | "x-real-ip"
            | "transfer-encoding"
            | "connection"
    );
    exact || lower.starts_with("x-forwarded-")
}
