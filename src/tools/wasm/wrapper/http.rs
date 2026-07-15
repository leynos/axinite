//! Outbound HTTP for WASM tools: request construction, private-IP
//! (DNS-rebinding) guards, and response normalisation.

use super::*;

/// Maps an HTTP method string to the corresponding `reqwest::RequestBuilder`.
fn build_http_client_request(
    client: &reqwest::Client,
    method: &str,
    url: &str,
) -> Result<reqwest::RequestBuilder, String> {
    match method.to_uppercase().as_str() {
        "GET" => Ok(client.get(url)),
        "POST" => Ok(client.post(url)),
        "PUT" => Ok(client.put(url)),
        "DELETE" => Ok(client.delete(url)),
        "PATCH" => Ok(client.patch(url)),
        "HEAD" => Ok(client.head(url)),
        other => Err(format!("Unsupported HTTP method: {}", other)),
    }
}

/// Walks a `reqwest::Error`'s source chain into a single diagnostic string.
fn format_error_chain(e: &reqwest::Error) -> String {
    let mut chain = format!("HTTP request failed: {}", e);
    let mut source: Option<&dyn std::error::Error> = std::error::Error::source(e);
    while let Some(cause) = source {
        chain.push_str(&format!(" -> {}", cause));
        source = cause.source();
    }
    chain
}

/// A fully resolved outbound HTTP request, ready to send.
///
/// Groups the request parts (method, URL, headers, body, timeout) that
/// travel together from preparation to execution.
pub(super) struct OutboundHttpRequest {
    /// HTTP method (e.g. "GET").
    pub(super) method: String,
    /// Request URL with all credentials injected.
    pub(super) url: String,
    /// Request headers with all credentials injected.
    pub(super) headers: HashMap<String, String>,
    /// Optional request body.
    pub(super) body: Option<Vec<u8>>,
    /// Caller-specified timeout in milliseconds, when given.
    pub(super) timeout_ms: Option<u32>,
}

/// Build the `reqwest` request for an outbound call: fresh redirect-disabled
/// client, method dispatch, and injected headers and body.
fn build_outbound_request(
    method: &str,
    url: &str,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
) -> Result<reqwest::RequestBuilder, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let mut request = build_http_client_request(&client, method, url)?;
    for (key, value) in headers {
        request = request.header(&key, &value);
    }
    if let Some(body_bytes) = body {
        request = request.body(body_bytes);
    }
    Ok(request)
}

/// Serialise the response headers into the JSON string the guest expects.
fn collect_response_headers(response: &reqwest::Response) -> String {
    let response_headers: HashMap<String, String> = response
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_string(), v.to_string()))
        })
        .collect();
    serde_json::to_string(&response_headers).unwrap_or_default()
}

/// Read the response body, rejecting it if it exceeds `max_response_bytes`
/// (by Content-Length before buffering, then again after reading).
async fn read_response_body_within_limit(
    response: reqwest::Response,
    max_response_bytes: usize,
) -> Result<Vec<u8>, String> {
    let too_large = |size: usize| {
        format!(
            "Response body too large: {} bytes exceeds limit of {} bytes",
            size, max_response_bytes
        )
    };

    // Early rejection on Content-Length to avoid streaming large bodies.
    if let Some(cl) = response.content_length()
        && cl as usize > max_response_bytes
    {
        return Err(too_large(cl as usize));
    }

    let body = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;
    if body.len() > max_response_bytes {
        return Err(too_large(body.len()));
    }
    Ok(body.to_vec())
}

/// Executes the outbound HTTP request and returns the normalised response.
///
/// Includes per-request DNS rebinding protection via `reject_private_ip`.
pub(super) async fn send_http_request(
    outbound: OutboundHttpRequest,
    max_response_bytes: usize,
    leak_detector: &LeakDetector,
) -> Result<near::agent::host::HttpResponse, String> {
    let OutboundHttpRequest {
        method,
        url,
        headers,
        body,
        timeout_ms,
    } = outbound;

    // Reject private/internal IPs to prevent DNS rebinding attacks.
    // Must run inside the runtime so DNS resolution can be performed asynchronously
    // when the host is a domain name; the sync outer path already called
    // `reject_private_ip` but this second check catches any race.
    reject_private_ip(&url)?;

    let request = build_outbound_request(&method, &url, headers, body)?;

    // Caller-specified timeout (default 30s, max 5min).
    let timeout_ms = timeout_ms.unwrap_or(30_000).min(300_000) as u64;
    let timeout = Duration::from_millis(timeout_ms);
    let response = request
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format_error_chain(&e))?;

    let status = response.status().as_u16();
    let headers_json = collect_response_headers(&response);
    let body = read_response_body_within_limit(response, max_response_bytes).await?;

    // Leak detection on response body (best-effort).
    if let Ok(body_str) = std::str::from_utf8(&body) {
        leak_detector
            .scan_and_clean(body_str)
            .map_err(|e| format!("Potential secret leak in response: {}", e))?;
    }

    Ok(near::agent::host::HttpResponse {
        status,
        headers_json,
        body,
    })
}

/// Extract the hostname from a URL string.
///
/// Handles `https://host:port/path`, stripping scheme, port, and path.
/// Also handles IPv6 bracket notation like `http://[::1]:8080/path`.
/// Returns None for malformed URLs.
pub(super) fn extract_host_from_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.host_str().map(|h| {
        h.strip_prefix('[')
            .and_then(|v| v.strip_suffix(']'))
            .unwrap_or(h)
            .to_lowercase()
    })
}

/// Parse and validate a URL for outbound requests (scheme and userinfo).
fn parse_outbound_url(url: &str) -> Result<url::Url, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Failed to parse URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("Unsupported URL scheme: {}", parsed.scheme()));
    }
    let has_userinfo = !parsed.username().is_empty() || parsed.password().is_some();
    if has_userinfo {
        return Err("URL contains userinfo (@) which is not allowed".to_string());
    }
    Ok(parsed)
}

/// Extract the host from a parsed URL, stripping IPv6 brackets.
fn outbound_url_host(parsed: &url::Url) -> Result<&str, String> {
    parsed
        .host_str()
        .map(|h| {
            h.strip_prefix('[')
                .and_then(|v| v.strip_suffix(']'))
                .unwrap_or(h)
        })
        .ok_or_else(|| "Failed to parse host from URL".to_string())
}

/// Reject an IP-literal host that falls in a private/internal range.
fn reject_private_ip_literal(ip: std::net::IpAddr) -> Result<(), String> {
    if is_private_ip(ip) {
        return Err(format!(
            "HTTP request to private/internal IP {} is not allowed",
            ip
        ));
    }
    Ok(())
}

/// Resolve a hostname and reject it if any resolved address is private.
fn reject_private_resolved_addresses(host: &str) -> Result<(), String> {
    use std::net::ToSocketAddrs;
    // Port 0 is a placeholder; ToSocketAddrs needs host:port but the port
    // doesn't affect which IPs the hostname resolves to.
    let addrs: Vec<_> = format!("{}:0", host)
        .to_socket_addrs()
        .map_err(|e| format!("DNS resolution failed for {}: {}", host, e))?
        .collect();

    if addrs.is_empty() {
        return Err(format!("DNS resolution returned no addresses for {}", host));
    }

    for addr in &addrs {
        if is_private_ip(addr.ip()) {
            return Err(format!(
                "DNS rebinding detected: {} resolved to private IP {}",
                host,
                addr.ip()
            ));
        }
    }

    Ok(())
}

/// Resolve the URL's hostname and reject connections to private/internal IP addresses.
/// This prevents DNS rebinding attacks where an attacker's domain resolves to an
/// internal IP after passing the allowlist check.
pub(super) fn reject_private_ip(url: &str) -> Result<(), String> {
    let parsed = parse_outbound_url(url)?;
    let host = outbound_url_host(&parsed)?;

    // If the host is already an IP, check it directly; otherwise resolve DNS
    // and check every address.
    match host.parse::<std::net::IpAddr>() {
        Ok(ip) => reject_private_ip_literal(ip),
        Err(_) => reject_private_resolved_addresses(host),
    }
}

/// Check if an IP address belongs to a private/internal range.
pub(super) fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => is_private_ipv4(v4),
        std::net::IpAddr::V6(v6) => is_private_ipv6(v6),
    }
}

/// Whether an IPv4 address falls in a private or internal range.
fn is_private_ipv4(v4: std::net::Ipv4Addr) -> bool {
    // 127.0.0.0/8; 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
    if v4.is_loopback() || v4.is_private() {
        return true;
    }
    // 169.254.0.0/16; 0.0.0.0
    if v4.is_link_local() || v4.is_unspecified() {
        return true;
    }
    // 100.64.0.0/10 (CGNAT)
    v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64
}

/// Whether an IPv6 address falls in a private or internal range.
fn is_private_ipv6(v6: std::net::Ipv6Addr) -> bool {
    // ::1; ::
    if v6.is_loopback() || v6.is_unspecified() {
        return true;
    }
    // fc00::/7 (unique local) or fe80::/10 (link-local)
    (v6.segments()[0] & 0xFE00) == 0xFC00 || (v6.segments()[0] & 0xFFC0) == 0xFE80
}
