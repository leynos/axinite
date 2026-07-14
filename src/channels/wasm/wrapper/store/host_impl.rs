//! Implementation of the generated channel-host `Host` trait for
//! `ChannelStoreData`: logging, workspace access, HTTP, and pairing.

use super::http::send_http_request;
use super::near;
use super::{ChannelStoreData, EmittedMessage, HttpMethod, LogLevel, OutboundRequestSpec};

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
        params: near::agent::channel_host::HttpRequestParams,
    ) -> Result<near::agent::channel_host::HttpResponse, String> {
        let near::agent::channel_host::HttpRequestParams {
            method,
            url,
            headers_json,
            body,
            timeout_ms,
        } = params;
        tracing::info!(
            method = %method,
            original_url = %url,
            body_len = body.as_ref().map(|b| b.len()).unwrap_or(0),
            "WASM http_request called"
        );

        let http_method = HttpMethod::from_str(&method)
            .ok_or_else(|| format!("Unsupported HTTP method: {}", method))?;

        let (url, headers, leak_detector) = self.prepare_outbound_request(OutboundRequestSpec {
            method: http_method,
            url,
            headers_json: &headers_json,
            body: body.as_deref(),
        })?;

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
        let rt = self
            .http_runtime
            .as_ref()
            .ok_or_else(|| "HTTP runtime missing despite being just initialized".to_string())?;

        let result = rt
            .block_on(send_http_request(
                http_method,
                url,
                headers,
                body,
                timeout_ms,
                max_response_bytes,
                &leak_detector,
            ))
            .map_err(|e| self.redact_credentials(&e));

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
        params: near::agent::channel_host::PairingUpsertParams,
    ) -> Result<near::agent::channel_host::PairingUpsertResult, String> {
        let near::agent::channel_host::PairingUpsertParams {
            identity,
            meta_json,
        } = params;
        let meta = if meta_json.is_empty() {
            None
        } else {
            serde_json::from_str(&meta_json).ok()
        };
        match self
            .pairing_store
            .upsert_request(&identity.channel, &identity.id, meta)
        {
            Ok(r) => Ok(near::agent::channel_host::PairingUpsertResult {
                code: r.code,
                created: r.created,
            }),
            Err(e) => Err(e.to_string()),
        }
    }

    fn pairing_is_allowed(
        &mut self,
        identity: near::agent::channel_host::PairingIdentity,
        username: Option<String>,
    ) -> Result<bool, String> {
        self.pairing_store
            .is_sender_allowed(&identity.channel, &identity.id, username.as_deref())
            .map_err(|e| e.to_string())
    }

    fn pairing_read_allow_from(&mut self, channel: String) -> Result<Vec<String>, String> {
        self.pairing_store
            .read_allow_from(&channel)
            .map_err(|e| e.to_string())
    }
}
