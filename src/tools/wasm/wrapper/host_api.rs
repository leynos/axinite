//! Implementation of the generated `near:agent/host` import interface
//! for WASM tool stores.

use super::http::{reject_private_ip, send_http_request};
use super::store::{PreparedHttpRequest, StoreData};
use super::*;

// This registers all 6 host functions under the `near:agent/host` namespace:
// log, now-millis, workspace-read, http-request, secret-exists, tool-invoke
impl near::agent::host::Host for StoreData {
    fn log(&mut self, level: near::agent::host::LogLevel, message: String) {
        let log_level = match level {
            near::agent::host::LogLevel::Trace => LogLevel::Trace,
            near::agent::host::LogLevel::Debug => LogLevel::Debug,
            near::agent::host::LogLevel::Info => LogLevel::Info,
            near::agent::host::LogLevel::Warn => LogLevel::Warn,
            near::agent::host::LogLevel::Error => LogLevel::Error,
        };
        let _ = self.host_state.log(log_level, message);
    }

    fn now_millis(&mut self) -> u64 {
        self.host_state.now_millis()
    }

    fn workspace_read(&mut self, path: String) -> Option<String> {
        self.host_state.workspace_read(&path).ok().flatten()
    }

    fn http_request(
        &mut self,
        params: near::agent::host::HttpRequestParams,
    ) -> Result<near::agent::host::HttpResponse, String> {
        let near::agent::host::HttpRequestParams {
            method,
            url,
            headers_json,
            body,
            timeout_ms,
        } = params;
        let leak_detector = LeakDetector::new();
        let PreparedHttpRequest { url, headers } = self.prepare_http_request_with_detector(
            &method,
            &url,
            &headers_json,
            body.as_deref(),
            &leak_detector,
        )?;

        // SSRF pre-check: reject private IPs before even creating the runtime.
        reject_private_ip(&url)?;

        // Get the max response size from capabilities (default 10MB).
        let max_response_bytes = self
            .host_state
            .capabilities()
            .http
            .as_ref()
            .map(|h| h.max_response_bytes)
            .unwrap_or(10 * 1024 * 1024);

        // Make HTTP request using a dedicated single-threaded runtime.
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
        // Populated just above; propagate rather than panic if that ever changes.
        let rt = self
            .http_runtime
            .as_ref()
            .ok_or_else(|| "HTTP runtime unavailable after initialization".to_string())?;
        let result = rt.block_on(send_http_request(
            &method,
            url,
            headers,
            body,
            timeout_ms,
            max_response_bytes,
            &leak_detector,
        ));

        // Redact credentials from error messages before returning to WASM.
        result.map_err(|e| self.redact_credentials(&e))
    }

    fn tool_invoke(&mut self, alias: String, _params_json: String) -> Result<String, String> {
        // Validate capability and resolve alias
        let _real_name = self.host_state.check_tool_invoke_allowed(&alias)?;
        self.host_state.record_tool_invoke()?;

        // Tool invocation requires async context and access to the tool registry,
        // which aren't available inside a synchronous WASM callback.
        Err("Tool invocation from WASM tools is not yet supported".to_string())
    }

    fn secret_exists(&mut self, name: String) -> bool {
        self.host_state.secret_exists(&name)
    }
}
