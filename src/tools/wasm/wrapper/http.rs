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

/// Executes the outbound HTTP request and returns the normalised response.
///
/// Includes per-request DNS rebinding protection via `reject_private_ip`.
pub(super) async fn send_http_request(
    method: &str,
    url: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    timeout_ms: Option<u32>,
    max_response_bytes: usize,
    leak_detector: &LeakDetector,
) -> Result<near::agent::host::HttpResponse, String> {
    // Reject private/internal IPs to prevent DNS rebinding attacks.
    // Must run inside the runtime so DNS resolution can be performed asynchronously
    // when the host is a domain name; the sync outer path already called
    // `reject_private_ip` but this second check catches any race.
    reject_private_ip(&url)?;

    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let mut request = build_http_client_request(&client, method, &url)?;

    for (key, value) in headers {
        request = request.header(&key, &value);
    }

    if let Some(body_bytes) = body {
        request = request.body(body_bytes);
    }

    // Caller-specified timeout (default 30s, max 5min).
    let timeout_ms = timeout_ms.unwrap_or(30_000).min(300_000) as u64;
    let timeout = Duration::from_millis(timeout_ms);
    let response = request
        .timeout(timeout)
        .send()
        .await
        .map_err(|e| format_error_chain(&e))?;

    let status = response.status().as_u16();
    let response_headers: HashMap<String, String> = response
        .headers()
        .iter()
        .filter_map(|(k, v)| {
            v.to_str()
                .ok()
                .map(|v| (k.as_str().to_string(), v.to_string()))
        })
        .collect();
    let headers_json = serde_json::to_string(&response_headers).unwrap_or_default();

    // Early rejection on Content-Length to avoid streaming large bodies.
    if let Some(cl) = response.content_length()
        && cl as usize > max_response_bytes
    {
        return Err(format!(
            "Response body too large: {} bytes exceeds limit of {} bytes",
            cl, max_response_bytes
        ));
    }

    let body = response
        .bytes()
        .await
        .map_err(|e| format!("Failed to read response body: {}", e))?;
    if body.len() > max_response_bytes {
        return Err(format!(
            "Response body too large: {} bytes exceeds limit of {} bytes",
            body.len(),
            max_response_bytes
        ));
    }
    let body = body.to_vec();

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

/// Resolve the URL's hostname and reject connections to private/internal IP addresses.
/// This prevents DNS rebinding attacks where an attacker's domain resolves to an
/// internal IP after passing the allowlist check.
pub(super) fn reject_private_ip(url: &str) -> Result<(), String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("Failed to parse URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("Unsupported URL scheme: {}", parsed.scheme()));
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("URL contains userinfo (@) which is not allowed".to_string());
    }

    let host = parsed
        .host_str()
        .map(|h| {
            h.strip_prefix('[')
                .and_then(|v| v.strip_suffix(']'))
                .unwrap_or(h)
        })
        .ok_or_else(|| "Failed to parse host from URL".to_string())?;

    // If the host is already an IP, check it directly
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return if is_private_ip(ip) {
            Err(format!(
                "HTTP request to private/internal IP {} is not allowed",
                ip
            ))
        } else {
            Ok(())
        };
    }

    // Resolve DNS and check all addresses
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

/// Check if an IP address belongs to a private/internal range.
pub(super) fn is_private_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            v4.is_loopback()           // 127.0.0.0/8
            || v4.is_private()         // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
            || v4.is_link_local()      // 169.254.0.0/16
            || v4.is_unspecified()     // 0.0.0.0
            || v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64 // 100.64.0.0/10 (CGNAT)
        }
        std::net::IpAddr::V6(v6) => {
            v6.is_loopback()           // ::1
            || v6.is_unspecified()     // ::
            // fc00::/7 (unique local)
            || (v6.segments()[0] & 0xFE00) == 0xFC00
            // fe80::/10 (link-local)
            || (v6.segments()[0] & 0xFFC0) == 0xFE80
        }
    }
}
