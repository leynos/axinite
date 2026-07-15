//! Per-execution store state: resource limits, WASI context, injected
//! credentials, and credential injection/redaction helpers.

use super::http::extract_host_from_url;
use super::*;

/// Pre-resolved credential for host-based injection.
///
/// Built before each WASM execution by decrypting secrets from the store.
/// Applied per-request by matching the URL host against `host_patterns`.
/// WASM tools never see the raw secret values.
pub(super) struct ResolvedHostCredential {
    /// Host patterns this credential applies to (e.g., "www.googleapis.com").
    pub(super) host_patterns: Vec<String>,
    /// Headers to add to matching requests (e.g., "Authorization: Bearer ...").
    pub(super) headers: HashMap<String, String>,
    /// Query parameters to add to matching requests.
    pub(super) query_params: HashMap<String, String>,
    /// Raw secret value for redaction in error messages.
    pub(super) secret_value: String,
}

/// Store data for WASM tool execution.
///
/// Contains the resource limiter, host state, WASI context, and injected
/// credentials. Fresh instance created per execution (NEAR pattern).
pub(super) struct StoreData {
    pub(super) limiter: WasmResourceLimiter,
    pub(super) host_state: HostState,
    wasi: WasiCtx,
    table: ResourceTable,
    /// Injected credentials for URL/header placeholder substitution.
    /// Keys are placeholder names like "TELEGRAM_BOT_TOKEN".
    credentials: HashMap<String, String>,
    /// Pre-resolved credentials for automatic host-based injection.
    /// Applied by matching URL host against each credential's host_patterns.
    host_credentials: Vec<ResolvedHostCredential>,
    /// Dedicated tokio runtime for HTTP requests, lazily initialized.
    /// Reused across multiple `http_request` calls within one execution.
    pub(super) http_runtime: Option<tokio::runtime::Runtime>,
}

pub(super) struct PreparedHttpRequest {
    pub(super) url: String,
    pub(super) headers: HashMap<String, String>,
}

/// Borrowed view of an outbound HTTP request awaiting preparation.
///
/// Groups the WASM-supplied request parts so the prepare functions do not
/// take an excess of positional arguments.
pub(super) struct HttpRequestInputs<'a> {
    /// HTTP method (e.g. "GET").
    pub(super) method: &'a str,
    /// Request URL, possibly containing credential placeholders.
    pub(super) url: &'a str,
    /// JSON-encoded map of request headers.
    pub(super) headers_json: &'a str,
    /// Optional request body.
    pub(super) body: Option<&'a [u8]>,
}

/// Compute the encoding variants of a secret that must all be redacted.
///
/// Covers the raw value, its percent-encoded form, and the `+`-for-space
/// form used in query strings; longest variants first so substrings of a
/// longer variant do not survive redaction.
fn secret_redaction_variants(secret: &str) -> Vec<String> {
    let mut variants = vec![secret.to_string()];
    let percent_encoded = urlencoding::encode(secret).into_owned();
    if percent_encoded != secret {
        variants.push(percent_encoded.clone());
        let plus_encoded = percent_encoded.replace("%20", "+");
        if plus_encoded != percent_encoded {
            variants.push(plus_encoded);
        }
    }
    variants.sort_by_key(|variant| std::cmp::Reverse(variant.len()));
    variants.dedup();
    variants
}

/// Replace every encoding variant of `secret` in `text` with `replacement`.
// The subject text, the secret needle, and its redaction placeholder are all free-form strings with no invariant a newtype could enforce.
// @codescene(disable:"String Heavy Function Arguments")
fn redact_secret_value(text: String, secret: &str, replacement: &str) -> String {
    let mut result = text;
    for variant in secret_redaction_variants(secret) {
        result = result.replace(&variant, replacement);
    }
    result
}

/// Redact one secret value from `text`, leaving it untouched when the
/// secret is empty (an empty needle would match everywhere).
fn redact_nonempty_secret(text: String, secret: &str, replacement: &str) -> String {
    if secret.is_empty() {
        return text;
    }
    redact_secret_value(text, secret, replacement)
}

impl StoreData {
    pub(super) fn new(
        memory_limit: u64,
        capabilities: Capabilities,
        credentials: HashMap<String, String>,
        host_credentials: Vec<ResolvedHostCredential>,
    ) -> Self {
        // Minimal WASI context: no filesystem, no env vars (security)
        let wasi = WasiCtxBuilder::new().build();

        Self {
            limiter: WasmResourceLimiter::new(memory_limit),
            host_state: HostState::new(capabilities),
            wasi,
            table: ResourceTable::new(),
            credentials,
            host_credentials,
            http_runtime: None,
        }
    }

    /// Inject credentials into a string by replacing placeholders.
    ///
    /// Replaces patterns like `{GOOGLE_ACCESS_TOKEN}` with actual values.
    /// WASM tools reference credentials by placeholder, never seeing real values.
    pub(super) fn inject_credentials(&self, input: &str, context: &str) -> String {
        let mut result = input.to_string();

        for (name, value) in &self.credentials {
            let placeholder = format!("{{{}}}", name);
            if result.contains(&placeholder) {
                tracing::debug!(
                    placeholder = %placeholder,
                    context = %context,
                    "Replacing credential placeholder in tool request"
                );
                result = result.replace(&placeholder, value);
            }
        }

        result
    }

    /// Replace injected credential values with `[REDACTED]` in text.
    ///
    /// Prevents credentials from leaking through error messages or logs.
    /// reqwest::Error includes the full URL in its Display output, so any
    /// error from an injected-URL request will contain the raw credential
    /// unless we scrub it.
    pub(super) fn redact_credentials(&self, text: &str) -> String {
        let redacted = self
            .credentials
            .iter()
            .fold(text.to_string(), |acc, (name, value)| {
                redact_nonempty_secret(acc, value, &format!("[REDACTED:{}]", name))
            });
        self.host_credentials.iter().fold(redacted, |acc, cred| {
            redact_nonempty_secret(acc, &cred.secret_value, "[REDACTED:host_credential]")
        })
    }

    /// Inject pre-resolved host credentials into the request.
    ///
    /// Matches the URL host against each resolved credential's host_patterns.
    /// Matching credentials have their headers merged and query params appended.
    pub(super) fn inject_host_credentials(
        &self,
        url_host: &str,
        headers: &mut HashMap<String, String>,
        url: &mut String,
    ) {
        for cred in &self.host_credentials {
            let matches = cred
                .host_patterns
                .iter()
                .any(|pattern| host_matches_pattern(url_host, pattern));

            if !matches {
                continue;
            }

            // Merge injected headers (host credentials take precedence)
            for (key, value) in &cred.headers {
                headers.insert(key.clone(), value.clone());
            }

            // Append query parameters to URL (insert before fragment if present)
            if !cred.query_params.is_empty() {
                let (base, fragment) = match url.find('#') {
                    Some(i) => (url[..i].to_string(), Some(url[i..].to_string())),
                    None => (url.clone(), None),
                };
                *url = base;

                let separator = if url.contains('?') { '&' } else { '?' };
                for (i, (name, value)) in cred.query_params.iter().enumerate() {
                    if i == 0 {
                        url.push(separator);
                    } else {
                        url.push('&');
                    }
                    url.push_str(&urlencoding::encode(name));
                    url.push('=');
                    url.push_str(&urlencoding::encode(value));
                }

                if let Some(frag) = fragment {
                    url.push_str(&frag);
                }
            }
        }
    }

    #[cfg(test)]
    pub(super) fn prepare_http_request(
        &mut self,
        request: &HttpRequestInputs<'_>,
    ) -> Result<PreparedHttpRequest, String> {
        let leak_detector = LeakDetector::new();
        self.prepare_http_request_with_detector(request, &leak_detector)
    }

    pub(super) fn prepare_http_request_with_detector(
        &mut self,
        request: &HttpRequestInputs<'_>,
        leak_detector: &LeakDetector,
    ) -> Result<PreparedHttpRequest, String> {
        // Preserve the raw request for leak scanning before any host-side
        // placeholder resolution. WASM only sees placeholders, not real secrets.
        let raw_url = request.url;
        let injected_url = self.inject_credentials(raw_url, "url");

        // Check HTTP allowlist
        self.host_state
            .check_http_allowed(&injected_url, request.method)
            .map_err(|e| format!("HTTP not allowed: {}", e))?;

        // Record for rate limiting
        self.host_state
            .record_http_request()
            .map_err(|e| format!("Rate limit exceeded: {}", e))?;

        // Parse the raw header values supplied by WASM.
        let raw_headers: HashMap<String, String> = serde_json::from_str(request.headers_json)
            .map_err(|e| format!("invalid headers_json payload: {e}"))?;

        // Leak scan runs on WASM-provided values before any host-side
        // credential injection. This prevents false positives where the
        // resolved secret value would otherwise be attributed to the tool.
        let raw_header_vec: Vec<(String, String)> = raw_headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        leak_detector
            .scan_http_request(raw_url, &raw_header_vec, request.body)
            .map_err(|e| format!("Potential secret leak blocked: {}", e))?;

        let mut headers: HashMap<String, String> = raw_headers
            .into_iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    self.inject_credentials(&v, &format!("header:{}", k)),
                )
            })
            .collect();

        let mut url = injected_url;

        // Inject pre-resolved host credentials (Bearer tokens, API keys, etc.)
        // after the leak scan so host-injected secrets don't trigger false positives.
        if let Some(host) = extract_host_from_url(&url) {
            self.inject_host_credentials(&host, &mut headers, &mut url);
        }

        Ok(PreparedHttpRequest { url, headers })
    }
}

// Provide WASI context for the WASM component.
// Required because tools are compiled with wasm32-wasip2 target.

// Provide WASI context for the WASM component.
// Required because tools are compiled with wasm32-wasip2 target.
impl WasiView for StoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

// Implement the generated Host trait from bindgen.
//
// This registers all 6 host functions under the `near:agent/host` namespace:
