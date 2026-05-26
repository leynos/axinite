use std::collections::HashMap;
use std::sync::Arc;

use wasmtime_wasi::{ResourceTable, WasiCtx, WasiCtxBuilder, WasiCtxView, WasiView};

use super::credentials::extract_host_from_url;
use super::near;
use super::types::{ChannelName, HostPattern, SecretValue};
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

    /// Inject credentials into a string by replacing placeholders.
    ///
    /// Replaces patterns like `{TELEGRAM_BOT_TOKEN}` or `{WHATSAPP_ACCESS_TOKEN}`
    /// with actual values from the injected credentials map. This allows WASM
    /// channels to reference credentials without ever seeing the actual values.
    ///
    /// Works on URLs, headers, or any string with credential placeholders.
    pub(super) fn inject_credentials(&self, input: &str, context: &str) -> String {
        let mut result = input.to_string();

        tracing::debug!(
            input_preview = %input.chars().take(100).collect::<String>(),
            context = %context,
            credential_count = self.credentials.len(),
            credential_names = ?self.credentials.keys().collect::<Vec<_>>(),
            "Injecting credentials"
        );

        // Replace all known placeholders from the credentials map
        for (name, value) in &self.credentials {
            let placeholder = format!("{{{}}}", name);
            if result.contains(&placeholder) {
                tracing::debug!(
                    placeholder = %placeholder,
                    context = %context,
                    "Found and replacing credential placeholder"
                );
                result = result.replace(&placeholder, value.as_str());
            }
        }

        // Check if any placeholders remain (indicates missing credential)
        if result.contains('{') && result.contains('}') {
            // Only warn if it looks like an unresolved placeholder (not JSON braces)
            let brace_pattern = regex::Regex::new(r"\{[A-Z_]+\}").ok();
            if let Some(re) = brace_pattern
                && re.is_match(&result)
            {
                tracing::warn!(
                    context = %context,
                    "String may contain unresolved credential placeholders"
                );
            }
        }

        result
    }

    /// Replace injected credential values with `[REDACTED]` in text.
    ///
    /// Prevents credentials from leaking through error messages, logs, or
    /// return values to WASM. reqwest::Error includes the full URL in its
    /// Display output, so any error from an injected-URL request will
    /// contain the raw credential unless we scrub it.
    ///
    /// Scrubs raw, URL-encoded, and Base64-encoded forms of each secret
    /// to prevent exfiltration via encoded representations in error strings.
    pub(super) fn redact_credentials(&self, text: &str) -> String {
        let mut result = text.to_string();
        for (name, value) in &self.credentials {
            if !value.is_empty() {
                let tag = format!("[REDACTED:{}]", name);
                result = result.replace(value.as_str(), &tag);
                // Also redact URL-encoded form (covers secrets in query strings)
                let encoded = urlencoding::encode(value.as_str());
                if encoded.as_ref() != value.as_str() {
                    result = result.replace(encoded.as_ref(), &tag);
                }
            }
        }
        for cred in &self.host_credentials {
            if !cred.secret_value.is_empty() {
                let tag = "[REDACTED:host_credential]";
                result = result.replace(cred.secret_value.as_str(), tag);
                // Also redact URL-encoded form (covers secrets injected as query params)
                let encoded = urlencoding::encode(cred.secret_value.as_str());
                if encoded.as_ref() != cred.secret_value.as_str() {
                    result = result.replace(encoded.as_ref(), tag);
                }
            }
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

// Implement the generated Host trait for channel-host interface
impl near::agent::channel_host::Host for ChannelStoreData {
    fn log(&mut self, level: near::agent::channel_host::LogLevel, message: String) {
        let log_level = match level {
            near::agent::channel_host::LogLevel::Trace => LogLevel::Trace,
            near::agent::channel_host::LogLevel::Debug => LogLevel::Debug,
            near::agent::channel_host::LogLevel::Info => LogLevel::Info,
            near::agent::channel_host::LogLevel::Warn => LogLevel::Warn,
            near::agent::channel_host::LogLevel::Error => LogLevel::Error,
        };
        let _ = self.host_state.log(log_level, message);
    }

    fn now_millis(&mut self) -> u64 {
        self.host_state.now_millis()
    }

    fn workspace_read(&mut self, path: String) -> Option<String> {
        self.host_state.workspace_read(&path).ok().flatten()
    }

    fn workspace_write(&mut self, path: String, content: String) -> Result<(), String> {
        self.host_state
            .workspace_write(&path, content)
            .map_err(|e| e.to_string())
    }

    fn http_request(
        &mut self,
        method: String,
        url: String,
        headers_json: String,
        body: Option<Vec<u8>>,
        timeout_ms: Option<u32>,
    ) -> Result<near::agent::channel_host::HttpResponse, String> {
        tracing::info!(
            method = %method,
            original_url = %url,
            body_len = body.as_ref().map(|b| b.len()).unwrap_or(0),
            "WASM http_request called"
        );

        // Preserve the raw request for leak scanning before any host-side
        // placeholder resolution. WASM only sees placeholders, not real secrets.
        let raw_url = url.clone();
        let injected_url = self.inject_credentials(&raw_url, "url");

        // Log whether injection happened (without revealing the token)
        let url_changed = injected_url != url;
        tracing::info!(url_changed = url_changed, "URL after credential injection");

        // Check if HTTP is allowed for this URL
        self.host_state
            .check_http_allowed(&injected_url, &method)
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
        let raw_headers: std::collections::HashMap<String, String> =
            serde_json::from_str(&headers_json).unwrap_or_default();

        // Leak scan runs on WASM-provided values before any host-side
        // credential injection. This prevents false positives where the
        // resolved secret value would otherwise be attributed to the channel.
        let leak_detector = LeakDetector::new();
        let raw_header_vec: Vec<(String, String)> = raw_headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        leak_detector
            .scan_http_request(&raw_url, &raw_header_vec, body.as_deref())
            .map_err(|e| format!("Potential secret leak blocked: {}", e))?;

        let mut headers: std::collections::HashMap<String, String> = raw_headers
            .into_iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    self.inject_credentials(&v, &format!("header:{}", k)),
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

        // Get the max response size from capabilities (default 10MB).
        let max_response_bytes = self
            .host_state
            .capabilities()
            .tool_capabilities
            .http
            .as_ref()
            .map(|h| h.max_response_bytes)
            .unwrap_or(10 * 1024 * 1024);

        // Make the HTTP request using a dedicated single-threaded runtime.
        // We're inside spawn_blocking, so we can't rely on the main runtime's
        // I/O driver (it may be busy with WASM compilation or other startup work).
        // A dedicated runtime gives us our own I/O driver and avoids contention.
        // The runtime is lazily created and reused across calls within one execution.
        if self.http_runtime.is_none() {
            self.http_runtime = Some(
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|e| format!("Failed to create HTTP runtime: {e}"))?,
            );
        }
        let rt = self.http_runtime.as_ref().expect("just initialized");
        let result = rt.block_on(async {
            let client = reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .map_err(|e| format!("Failed to build HTTP client: {e}"))?;

            let mut request = match method.to_uppercase().as_str() {
                "GET" => client.get(&url),
                "POST" => client.post(&url),
                "PUT" => client.put(&url),
                "DELETE" => client.delete(&url),
                "PATCH" => client.patch(&url),
                "HEAD" => client.head(&url),
                _ => return Err(format!("Unsupported HTTP method: {}", method)),
            };

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
            let response = request.timeout(timeout).send().await.map_err(|e| {
                // Walk the full error chain so we get the actual root cause
                // (DNS, TLS, connection refused, etc.) instead of just
                // "error sending request for url (...)".
                let mut chain = format!("HTTP request failed: {}", e);
                let mut source = std::error::Error::source(&e);
                while let Some(cause) = source {
                    chain.push_str(&format!(" -> {}", cause));
                    source = cause.source();
                }
                chain
            })?;

            let status = response.status().as_u16();
            let response_headers: std::collections::HashMap<String, String> = response
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
            let max_response = max_response_bytes;
            if let Some(cl) = response.content_length()
                && cl as usize > max_response
            {
                return Err(format!(
                    "Response body too large: {} bytes exceeds limit of {} bytes",
                    cl, max_response
                ));
            }
            let body = response
                .bytes()
                .await
                .map_err(|e| format!("Failed to read response body: {}", e))?;
            if body.len() > max_response {
                return Err(format!(
                    "Response body too large: {} bytes exceeds limit of {} bytes",
                    body.len(),
                    max_response
                ));
            }
            let body = body.to_vec();

            tracing::info!(
                status = status,
                body_len = body.len(),
                "HTTP response received"
            );

            // Log response body for debugging (truncated at char boundary)
            if let Ok(body_str) = std::str::from_utf8(&body) {
                let truncated = if body_str.chars().count() > 500 {
                    format!("{}...", body_str.chars().take(500).collect::<String>())
                } else {
                    body_str.to_string()
                };
                tracing::debug!(body = %truncated, "Response body");
            }

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
        });

        // Scrub credential values from error messages before logging or returning
        // to WASM. reqwest::Error includes the full URL (with injected credentials)
        // in its Display output.
        let result = result.map_err(|e| self.redact_credentials(&e));

        match &result {
            Ok(resp) => {
                tracing::info!(status = resp.status, "http_request completed successfully");
            }
            Err(e) => {
                tracing::error!(error = %e, "http_request failed");
            }
        }

        result
    }

    fn secret_exists(&mut self, name: String) -> bool {
        self.host_state.secret_exists(&name)
    }

    fn emit_message(&mut self, msg: near::agent::channel_host::EmittedMessage) {
        tracing::info!(
            user_id = %msg.user_id,
            user_name = ?msg.user_name,
            content_len = msg.content.len(),
            attachment_count = msg.attachments.len(),
            "WASM emit_message called"
        );

        let attachments: Vec<crate::channels::wasm::host::Attachment> = msg
            .attachments
            .into_iter()
            .map(|a| {
                // Parse extras-json for well-known fields
                let extras: serde_json::Value = if a.extras_json.is_empty() {
                    serde_json::Value::Null
                } else {
                    serde_json::from_str(&a.extras_json).unwrap_or(serde_json::Value::Null)
                };
                let duration_secs = extras
                    .get("duration_secs")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);

                // Merge stored binary data (from store-attachment-data host call)
                let data = self
                    .host_state
                    .remove_attachment_data(&a.id)
                    .unwrap_or_default();

                crate::channels::wasm::host::Attachment {
                    id: a.id,
                    mime_type: a.mime_type,
                    filename: a.filename,
                    size_bytes: a.size_bytes,
                    source_url: a.source_url,
                    storage_key: a.storage_key,
                    extracted_text: a.extracted_text,
                    data,
                    duration_secs,
                }
            })
            .collect();

        let mut emitted = EmittedMessage::new(msg.user_id.clone(), msg.content.clone());
        if let Some(name) = msg.user_name {
            emitted = emitted.with_user_name(name);
        }
        if let Some(tid) = msg.thread_id {
            emitted = emitted.with_thread_id(tid);
        }
        emitted = emitted.with_metadata(msg.metadata_json);
        emitted = emitted.with_attachments(attachments);

        match self.host_state.emit_message(emitted) {
            Ok(()) => {
                tracing::info!("Message emitted to host state successfully");
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to emit message to host state");
            }
        }
    }

    fn store_attachment_data(
        &mut self,
        attachment_id: String,
        data: Vec<u8>,
    ) -> Result<(), String> {
        tracing::debug!(
            attachment_id = %attachment_id,
            size = data.len(),
            "WASM store_attachment_data called"
        );
        self.host_state
            .store_attachment_data(&attachment_id, data)
            .map_err(|e| e.to_string())
    }

    fn pairing_upsert_request(
        &mut self,
        channel: String,
        id: String,
        meta_json: String,
    ) -> Result<near::agent::channel_host::PairingUpsertResult, String> {
        let meta = if meta_json.is_empty() {
            None
        } else {
            serde_json::from_str(&meta_json).ok()
        };
        match self.pairing_store.upsert_request(&channel, &id, meta) {
            Ok(r) => Ok(near::agent::channel_host::PairingUpsertResult {
                code: r.code,
                created: r.created,
            }),
            Err(e) => Err(e.to_string()),
        }
    }

    fn pairing_is_allowed(
        &mut self,
        channel: String,
        id: String,
        username: Option<String>,
    ) -> Result<bool, String> {
        self.pairing_store
            .is_sender_allowed(&channel, &id, username.as_deref())
            .map_err(|e| e.to_string())
    }

    fn pairing_read_allow_from(&mut self, channel: String) -> Result<Vec<String>, String> {
        self.pairing_store
            .read_allow_from(&channel)
            .map_err(|e| e.to_string())
    }
}
