//! WASM store state for channel execution: WASI context, resource limits,
//! and pre-resolved host credentials injected into outbound requests.

use std::collections::HashMap;
use std::sync::Arc;

use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use super::credentials::extract_host_from_url;
use super::near;

mod host_impl;
mod http;
use super::types::{
    ChannelName, CredentialContext, HostPattern, HttpMethod, OutboundRequestSpec, SecretValue,
};
use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::host::{ChannelHostState, EmittedMessage};
use crate::pairing::PairingStore;
use crate::safety::LeakDetector;
use crate::tools::wasm::LogLevel;
use crate::tools::wasm::WasmResourceLimiter;
use crate::tools::wasm::credential_injector::host_matches_pattern;

/// Pre-resolved credential for host-based injection.
///
/// Built before each WASM execution by decrypting secrets from the store.
/// Applied per-request by matching the URL host against `host_patterns`.
/// WASM channels never see the raw secret values.
#[derive(Clone)]
pub(super) struct ResolvedHostCredential {
    /// Host patterns this credential applies to (e.g., "api.slack.com").
    pub(super) host_patterns: Vec<HostPattern>,
    /// Headers to add to matching requests (e.g., "Authorization: Bearer ...").
    pub(super) headers: HashMap<String, String>,
    /// Query parameters to add to matching requests.
    pub(super) query_params: HashMap<String, String>,
    /// Raw secret value for redaction in error messages.
    pub(super) secret_value: SecretValue,
}

/// Store data for WASM channel execution.
///
/// Contains the resource limiter, channel-specific host state, and WASI context.
pub(super) struct ChannelStoreData {
    pub(super) limiter: WasmResourceLimiter,
    pub(super) host_state: ChannelHostState,
    wasi: WasiCtx,
    table: ResourceTable,
    /// Injected credentials for URL substitution (e.g., bot tokens).
    /// Keys are placeholder names like "TELEGRAM_BOT_TOKEN".
    credentials: HashMap<String, SecretValue>,
    /// Pre-resolved credentials for automatic host-based injection.
    /// Applied per-request by matching the URL host against host_patterns.
    host_credentials: Vec<ResolvedHostCredential>,
    /// Pairing store for DM pairing (guest access control).
    pairing_store: Arc<PairingStore>,
    /// Dedicated tokio runtime for HTTP requests, lazily initialized.
    /// Reused across multiple `http_request` calls within one execution.
    http_runtime: Option<tokio::runtime::Runtime>,
}

impl ChannelStoreData {
    pub(super) fn new(
        memory_limit: u64,
        channel_name: &ChannelName,
        capabilities: ChannelCapabilities,
        credentials: HashMap<String, SecretValue>,
        host_credentials: Vec<ResolvedHostCredential>,
        pairing_store: Arc<PairingStore>,
    ) -> Self {
        // Create a minimal WASI context (no filesystem, no env vars for security)
        let wasi = WasiCtxBuilder::new().build();

        Self {
            limiter: WasmResourceLimiter::new(memory_limit),
            host_state: ChannelHostState::new(channel_name.as_str(), capabilities),
            wasi,
            table: ResourceTable::new(),
            credentials,
            host_credentials,
            pairing_store,
            http_runtime: None,
        }
    }
}

/// Replaces a single credential placeholder in `result` if it is present.
///
/// Logs at DEBUG level when a substitution is made.
fn replace_credential_placeholder(
    result: &mut String,
    name: &str,
    value: &SecretValue,
    context: CredentialContext<'_>,
) {
    let placeholder = format!("{{{}}}", name);
    if result.contains(&placeholder) {
        tracing::debug!(
            placeholder = %placeholder,
            context = %context,
            "Found and replacing credential placeholder"
        );
        *result = result.replace(&placeholder, value.as_str());
    }
}

/// Emits a WARN trace when `text` still contains an unresolved `{UPPER_CASE}`
/// credential placeholder after injection.
///
/// Brace pairs that look like JSON objects are ignored.
fn warn_if_unresolved_placeholders(text: &str, context: CredentialContext<'_>) {
    if !text.contains('{') || !text.contains('}') {
        return;
    }
    let brace_pattern = regex::Regex::new(r"\{[A-Z_]+\}").ok();
    if let Some(re) = brace_pattern
        && re.is_match(text)
    {
        tracing::warn!(
            context = %context,
            "String may contain unresolved credential placeholders"
        );
    }
}

impl ChannelStoreData {
    /// Inject credentials into a string by replacing placeholders.
    ///
    /// Replaces patterns like `{TELEGRAM_BOT_TOKEN}` or `{WHATSAPP_ACCESS_TOKEN}`
    /// with actual values from the injected credentials map. This allows WASM
    /// channels to reference credentials without ever seeing the actual values.
    ///
    /// Works on URLs, headers, or any string with credential placeholders.
    pub(super) fn inject_credentials(&self, input: &str, context: CredentialContext<'_>) -> String {
        let mut result = input.to_string();

        tracing::debug!(
            input_preview = %input.chars().take(100).collect::<String>(),
            context = %context,
            credential_count = self.credentials.len(),
            credential_names = ?self.credentials.keys().collect::<Vec<_>>(),
            "Injecting credentials"
        );

        for (name, value) in &self.credentials {
            replace_credential_placeholder(&mut result, name, value, context);
        }

        warn_if_unresolved_placeholders(&result, context);

        result
    }
}

/// Replaces every occurrence of `raw` — and its URL-encoded form — in
/// `result` with `tag`.
///
/// The URL-encoded form is only substituted when it differs from the raw
/// value, which avoids a redundant second pass for plain-ASCII secrets.
fn redact_value(result: &mut String, raw: &str, tag: &str) {
    *result = result.replace(raw, tag);
    let encoded = urlencoding::encode(raw);
    if encoded.as_ref() != raw {
        *result = result.replace(encoded.as_ref(), tag);
    }
}

impl ChannelStoreData {
    /// Replace injected credential values with `[REDACTED]` in text.
    ///
    /// Prevents credentials from leaking through error messages, logs, or
    /// return values to WASM. reqwest::Error includes the full URL in its
    /// Display output, so any error from an injected-URL request will
    /// contain the raw credential unless we scrub it.
    ///
    /// Scrubs raw and URL-encoded forms of each secret to prevent
    /// exfiltration via encoded representations in error strings.
    pub(super) fn redact_credentials(&self, text: &str) -> String {
        let mut result = text.to_string();

        for (name, value) in self.credentials.iter().filter(|(_, v)| !v.is_empty()) {
            redact_value(&mut result, value.as_str(), &format!("[REDACTED:{}]", name));
        }

        for cred in self
            .host_credentials
            .iter()
            .filter(|c| !c.secret_value.is_empty())
        {
            redact_value(
                &mut result,
                cred.secret_value.as_str(),
                "[REDACTED:host_credential]",
            );
        }

        result
    }

    /// Inject pre-resolved host credentials into the request.
    ///
    /// Matches the URL host against each resolved credential's host_patterns.
    /// Matching credentials have their headers merged and query params appended.
    pub(super) fn inject_host_credentials(
        &self,
        url_host: &HostPattern,
        headers: &mut HashMap<String, String>,
        url: &mut String,
    ) {
        for cred in &self.host_credentials {
            let matches = cred
                .host_patterns
                .iter()
                .any(|pattern| host_matches_pattern(url_host.as_str(), pattern.as_str()));

            if !matches {
                continue;
            }

            // Merge injected headers (host credentials take precedence)
            for (key, value) in &cred.headers {
                headers.insert(key.clone(), value.clone());
            }

            // Append query parameters to URL
            if !cred.query_params.is_empty() {
                if let Ok(mut parsed_url) = url::Url::parse(url) {
                    for (name, value) in &cred.query_params {
                        parsed_url.query_pairs_mut().append_pair(name, value);
                    }
                    *url = parsed_url.to_string();
                } else {
                    tracing::warn!(url = %url, "Could not parse URL to inject query parameters; skipping injection");
                }
            }
        }
    }

    /// Injects credentials, enforces access-control policy, runs the pre-flight
    /// leak scan, and returns the resolved URL and header map ready for dispatch.
    fn prepare_outbound_request(
        &mut self,
        req: OutboundRequestSpec<'_>,
    ) -> Result<(String, HashMap<String, String>, LeakDetector), String> {
        let OutboundRequestSpec {
            method,
            url,
            headers_json,
            body,
        } = req;
        // Preserve the raw request for leak scanning before any host-side
        // placeholder resolution. WASM only sees placeholders, not real secrets.
        let raw_url = url.clone();
        let injected_url = self.inject_credentials(&raw_url, CredentialContext::Url);

        // Log whether injection happened (without revealing the token)
        let url_changed = injected_url != url;
        tracing::info!(url_changed = url_changed, "URL after credential injection");

        // Check if HTTP is allowed for this URL
        self.host_state
            .check_http_allowed(&injected_url, method.as_str())
            .map_err(|e| {
                tracing::error!(error = %e, "HTTP not allowed");
                format!("HTTP not allowed: {}", e)
            })?;

        // Record the request for rate limiting
        self.host_state.record_http_request().map_err(|e| {
            tracing::error!(error = %e, "Rate limit exceeded");
            format!("Rate limit exceeded: {}", e)
        })?;

        // Parse the raw header values supplied by WASM.
        let raw_headers: HashMap<String, String> =
            serde_json::from_str(headers_json).unwrap_or_default();

        // Leak scan runs on WASM-provided values before any host-side
        // credential injection. This prevents false positives where the
        // resolved secret value would otherwise be attributed to the channel.
        let leak_detector = LeakDetector::new();
        let raw_header_vec: Vec<(String, String)> = raw_headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        leak_detector
            .scan_http_request(&raw_url, &raw_header_vec, body)
            .map_err(|e| format!("Potential secret leak blocked: {}", e))?;

        let mut headers: HashMap<String, String> = raw_headers
            .into_iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    self.inject_credentials(&v, CredentialContext::Header(&k)),
                )
            })
            .collect();

        let headers_changed = headers
            .values()
            .any(|v| v.contains("Bearer ") && !v.contains('{'));
        tracing::debug!(
            header_count = headers.len(),
            headers_changed = headers_changed,
            "Parsed and injected request headers"
        );

        let mut url = injected_url;

        // Inject pre-resolved host credentials (Bearer tokens, API keys, etc.)
        // after the leak scan so host-injected secrets don't trigger false positives.
        if let Some(host) = extract_host_from_url(&url)
            && let Some(host_pattern) = HostPattern::new(host)
        {
            self.inject_host_credentials(&host_pattern, &mut headers, &mut url);
        }

        Ok((url, headers, leak_detector))
    }
}

// Implement WasiView to provide WASI context and resource table
impl WasiView for ChannelStoreData {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}
