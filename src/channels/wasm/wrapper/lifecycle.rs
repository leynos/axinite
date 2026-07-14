//! Lifecycle callbacks for the WASM channel: on_start, on_http_request, and
//! on_poll.
//!
//! Each callback creates a fresh WASM instance, executes the guest export in a
//! blocking task with a timeout, then commits pending workspace writes and
//! processes any emitted messages.

use std::collections::HashMap;
use std::sync::Arc;

use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::schema::ChannelConfig;

use super::{
    HttpResponse, WasmChannel, convert_channel_config, convert_http_response,
    resolve_channel_host_credentials, wit_channel,
};

impl WasmChannel {
    /// Default configuration returned when no WASM module is present
    /// (testing mode).
    fn default_start_config(&self) -> ChannelConfig {
        ChannelConfig {
            display_name: self.prepared.description.clone(),
            http_endpoints: Vec::new(),
            poll: None,
        }
    }

    /// Surface WASM guest logs (errors/warnings from webhook setup, etc.)
    /// at the matching tracing level.
    fn surface_guest_logs(&self, host_state: &mut crate::channels::wasm::host::ChannelHostState) {
        for entry in host_state.take_logs() {
            match entry.level {
                crate::tools::wasm::LogLevel::Error => {
                    tracing::error!(channel = %self.name, "{}", entry.message);
                }
                crate::tools::wasm::LogLevel::Warn => {
                    tracing::warn!(channel = %self.name, "{}", entry.message);
                }
                _ => {
                    tracing::debug!(channel = %self.name, "{}", entry.message);
                }
            }
        }
    }

    /// Execute the on_start callback.
    ///
    /// Returns the channel configuration for HTTP endpoint registration.
    /// Call the WASM module's `on_start` callback.
    ///
    /// Typically called once during `start()`, but can be called again after
    /// credentials are refreshed to re-trigger webhook registration and
    /// other one-time setup that depends on credentials.
    pub async fn call_on_start(&self) -> Result<ChannelConfig, WasmChannelError> {
        // If no WASM bytes, return default config (for testing)
        if self.prepared.component().is_none() {
            tracing::info!(
                channel = %self.name,
                "WASM channel on_start called (no WASM module, returning defaults)"
            );
            return Ok(self.default_start_config());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = Self::inject_workspace_reader(&self.capabilities, &self.workspace_store);
        let config_json = self.config_json.read().await.clone();
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();
        let workspace_store = self.workspace_store.clone();

        // Execute in blocking task with timeout
        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                // Call on_start using the generated typed interface
                let channel_iface = instance.near_agent_channel();
                let wasm_result = channel_iface
                    .call_on_start(&mut store, &config_json)
                    .map_err(|e| Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel))?;

                // Convert the result
                let config = match wasm_result {
                    Ok(wit_config) => convert_channel_config(wit_config),
                    Err(err_msg) => {
                        return Err(WasmChannelError::CallbackFailed {
                            name: prepared.name.clone(),
                            reason: err_msg,
                        });
                    }
                };

                let mut host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);

                // Commit pending workspace writes to the persistent store
                let pending_writes = host_state.take_pending_writes();
                workspace_store.commit_writes(&pending_writes);

                Ok((config, host_state))
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        match result {
            Ok(Ok((config, mut host_state))) => {
                self.surface_guest_logs(&mut host_state);
                tracing::info!(
                    channel = %self.name,
                    display_name = %config.display_name,
                    endpoints = config.http_endpoints.len(),
                    "WASM channel on_start completed"
                );
                Ok(config)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: self.name.clone(),
                callback: "on_start".to_string(),
            }),
        }
    }

    /// Execute the on_http_request callback.
    ///
    /// Called when an HTTP request arrives at a registered endpoint.
    pub async fn call_on_http_request(
        &self,
        method: &str,
        path: &str,
        headers: &HashMap<String, String>,
        query: &HashMap<String, String>,
        body: &[u8],
        secret_validated: bool,
    ) -> Result<HttpResponse, WasmChannelError> {
        tracing::info!(
            channel = %self.name,
            method = method,
            path = path,
            body_len = body.len(),
            secret_validated = secret_validated,
            "call_on_http_request invoked (webhook received)"
        );

        // Log the body for debugging (truncated at char boundary)
        if let Ok(body_str) = std::str::from_utf8(body) {
            let truncated = if body_str.chars().count() > 1000 {
                format!("{}...", body_str.chars().take(1000).collect::<String>())
            } else {
                body_str.to_string()
            };
            tracing::debug!(body = %truncated, "Webhook request body");
        }

        // Log credentials state (without values)
        let creds = self.get_credentials().await;
        tracing::info!(
            credential_count = creds.len(),
            credential_names = ?creds.keys().collect::<Vec<_>>(),
            "Credentials available for on_http_request"
        );

        // If no WASM bytes, return 200 OK (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                method = method,
                path = path,
                "WASM channel on_http_request called (no WASM module)"
            );
            return Ok(HttpResponse::ok());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = Self::inject_workspace_reader(&self.capabilities, &self.workspace_store);
        let timeout = self.runtime.config().callback_timeout;
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();
        let workspace_store = self.workspace_store.clone();

        // Prepare request data
        let method = method.to_string();
        let path = path.to_string();
        let headers_json = serde_json::to_string(&headers).unwrap_or_default();
        let query_json = serde_json::to_string(&query).unwrap_or_default();
        let body = body.to_vec();

        let channel_name = self.name.clone();

        // Execute in blocking task with timeout
        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                // Build the WIT request type
                let wit_request = wit_channel::IncomingHttpRequest {
                    method,
                    path,
                    headers_json,
                    query_json,
                    body,
                    secret_validated,
                };

                // Call on_http_request using the generated typed interface
                let channel_iface = instance.near_agent_channel();
                let wit_response = channel_iface
                    .call_on_http_request(&mut store, &wit_request)
                    .map_err(|e| Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel))?;

                let response = convert_http_response(wit_response);
                let mut host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);

                // Commit pending workspace writes to the persistent store
                let pending_writes = host_state.take_pending_writes();
                workspace_store.commit_writes(&pending_writes);

                Ok((response, host_state))
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        let channel_name = self.name.clone();
        match result {
            Ok(Ok((response, mut host_state))) => {
                // Process emitted messages
                let emitted = host_state.take_emitted_messages();
                self.process_emitted_messages(emitted).await?;

                tracing::debug!(
                    channel = %channel_name,
                    status = response.status,
                    "WASM channel on_http_request completed"
                );
                Ok(response)
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name,
                callback: "on_http_request".to_string(),
            }),
        }
    }

    /// Execute the on_poll callback.
    ///
    /// Called periodically if polling is configured.
    pub async fn call_on_poll(&self) -> Result<(), WasmChannelError> {
        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                "WASM channel on_poll called (no WASM module)"
            );
            return Ok(());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = Self::inject_workspace_reader(&self.capabilities, &self.workspace_store);
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();
        let workspace_store = self.workspace_store.clone();

        // Execute in blocking task with timeout
        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                // Call on_poll using the generated typed interface
                let channel_iface = instance.near_agent_channel();
                channel_iface
                    .call_on_poll(&mut store)
                    .map_err(|e| Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel))?;

                let mut host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);

                // Commit pending workspace writes to the persistent store
                let pending_writes = host_state.take_pending_writes();
                workspace_store.commit_writes(&pending_writes);

                Ok(((), host_state))
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        let channel_name = self.name.clone();
        match result {
            Ok(Ok(((), mut host_state))) => {
                // Process emitted messages
                let emitted = host_state.take_emitted_messages();
                self.process_emitted_messages(emitted).await?;

                tracing::debug!(
                    channel = %channel_name,
                    "WASM channel on_poll completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name,
                callback: "on_poll".to_string(),
            }),
        }
    }
}
