//! Outbound HTTP dispatch for WASM channels: request building, execution,
//! response normalisation, and error-chain formatting.

use std::collections::HashMap;
use std::error::Error;

use super::HttpMethod;
use super::near;
use crate::safety::LeakDetector;

/// Maps an HTTP method name to the corresponding `reqwest::RequestBuilder`.
pub(super) fn build_http_client_request(
    client: &reqwest::Client,
    method: HttpMethod,
    url: &str,
) -> Result<reqwest::RequestBuilder, String> {
    Ok(match method {
        HttpMethod::Get => client.get(url),
        HttpMethod::Post => client.post(url),
        HttpMethod::Put => client.put(url),
        HttpMethod::Delete => client.delete(url),
        HttpMethod::Patch => client.patch(url),
        HttpMethod::Head => client.head(url),
    })
}

/// Walks `reqwest`'s error source chain to produce a single diagnostic string.
pub(super) fn format_error_chain(e: &reqwest::Error) -> String {
    // Walk the full error chain so we get the actual root cause
    // (DNS, TLS, connection refused, etc.) instead of just
    // "error sending request for url (...)".
    let mut chain = format!("HTTP request failed: {}", e);
    let mut source: Option<&dyn Error> = e.source();
    while let Some(cause) = source {
        chain.push_str(&format!(" -> {}", cause));
        source = cause.source();
    }
    chain
}

/// Logs a truncated preview of a UTF-8 response body at DEBUG level.
pub(super) fn log_response_body(body: &[u8]) {
    // Log response body for debugging (truncated at char boundary)
    if let Ok(body_str) = std::str::from_utf8(body) {
        let truncated = if body_str.chars().count() > 500 {
            format!("{}...", body_str.chars().take(500).collect::<String>())
        } else {
            body_str.to_string()
        };
        tracing::debug!(body = %truncated, "Response body");
    }
}

/// Executes the outbound HTTP request and returns the normalised response.
pub(super) async fn send_http_request(
    method: HttpMethod,
    url: String,
    headers: HashMap<String, String>,
    body: Option<Vec<u8>>,
    timeout_ms: Option<u32>,
    max_response_bytes: usize,
    leak_detector: &LeakDetector,
) -> Result<near::agent::channel_host::HttpResponse, String> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

    let mut request = build_http_client_request(&client, method, &url)?;

    // Add headers
    for (key, value) in headers {
        request = request.header(&key, &value);
    }

    // Add body if present
    if let Some(body_bytes) = body {
        request = request.body(body_bytes);
    }

    // Send request with caller-specified timeout (default 30s, max 5min).
    let timeout_ms = timeout_ms.unwrap_or(30_000).min(300_000) as u64;
    let timeout = std::time::Duration::from_millis(timeout_ms);
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

    // Enforce max response body size to prevent memory exhaustion.
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

    tracing::info!(
        status = status,
        body_len = body.len(),
        "HTTP response received"
    );
    log_response_body(&body);

    // Leak detection on response body (best-effort)
    if let Ok(body_str) = std::str::from_utf8(&body) {
        leak_detector
            .scan_and_clean(body_str)
            .map_err(|e| format!("Potential secret leak in response: {}", e))?;
    }

    Ok(near::agent::channel_host::HttpResponse {
        status,
        headers_json,
        body,
    })
}
