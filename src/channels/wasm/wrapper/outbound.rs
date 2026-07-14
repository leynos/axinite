//! Outbound callbacks for the WASM channel: on_respond, on_broadcast, and
//! on_status.
//!
//! These callbacks push agent output back through the WASM guest — replies to
//! incoming messages, proactive broadcasts, and status updates such as typing
//! indicators.

use std::sync::Arc;

use uuid::Uuid;

use crate::channels::StatusUpdate;
use crate::channels::wasm::error::WasmChannelError;

use super::{
    WasmChannel, read_attachments, resolve_channel_host_credentials, status_to_wit, wit_channel,
};

impl WasmChannel {
    /// Execute the on_respond callback.
    ///
    /// Called when the agent has a response to send back.
    pub async fn call_on_respond(
        &self,
        message_id: Uuid,
        content: &str,
        thread_id: Option<&str>,
        metadata_json: &str,
        attachments: &[String],
    ) -> Result<(), WasmChannelError> {
        tracing::info!(
            channel = %self.name,
            message_id = %message_id,
            content_len = content.len(),
            thread_id = ?thread_id,
            attachment_count = attachments.len(),
            "call_on_respond invoked"
        );

        // Log credentials state (without values)
        let creds = self.get_credentials().await;
        tracing::info!(
            credential_count = creds.len(),
            credential_names = ?creds.keys().collect::<Vec<_>>(),
            "Credentials available for on_respond"
        );

        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                message_id = %message_id,
                "WASM channel on_respond called (no WASM module)"
            );
            return Ok(());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = self.capabilities.clone();
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();

        // Prepare response data
        let message_id_str = message_id.to_string();
        let content = content.to_string();
        let thread_id = thread_id.map(|s| s.to_string());
        let metadata_json = metadata_json.to_string();
        let attachments = attachments.to_vec();

        // Execute in blocking task with timeout
        tracing::info!(channel = %channel_name, "Starting on_respond WASM execution");

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                // Read attachment files from disk before entering WASM
                let wit_attachments = read_attachments(&attachments).map_err(|e| {
                    WasmChannelError::CallbackFailed {
                        name: prepared.name.clone(),
                        reason: e,
                    }
                })?;

                tracing::info!("Creating WASM store for on_respond");
                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;

                tracing::info!("Instantiating WASM component for on_respond");
                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                // Build the WIT response type
                let wit_response = wit_channel::AgentResponse {
                    message_id: message_id_str,
                    content: content.clone(),
                    thread_id,
                    metadata_json,
                    attachments: wit_attachments,
                };

                // Truncate at char boundary for logging (avoid panic on multi-byte UTF-8)
                let content_preview: String = content.chars().take(50).collect();
                tracing::info!(
                    content_preview = %content_preview,
                    "Calling WASM on_respond"
                );

                // Call on_respond using the generated typed interface
                let channel_iface = instance.near_agent_channel();
                let wasm_result = channel_iface
                    .call_on_respond(&mut store, &wit_response)
                    .map_err(|e| {
                        tracing::error!(error = %e, "WASM on_respond call failed");
                        Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel)
                    })?;

                tracing::info!(wasm_result = ?wasm_result, "WASM on_respond returned");

                // Check for WASM-level errors
                if let Err(ref err_msg) = wasm_result {
                    tracing::error!(error = %err_msg, "WASM on_respond returned error");
                    return Err(WasmChannelError::CallbackFailed {
                        name: prepared.name.clone(),
                        reason: err_msg.clone(),
                    });
                }

                let host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);
                tracing::info!("on_respond WASM execution completed successfully");
                Ok(((), host_state))
            })
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "spawn_blocking panicked");
                WasmChannelError::ExecutionPanicked {
                    name: channel_name.clone(),
                    reason: e.to_string(),
                }
            })?
        })
        .await;

        let channel_name = self.name.clone();
        match result {
            Ok(Ok(((), _host_state))) => {
                tracing::debug!(
                    channel = %channel_name,
                    message_id = %message_id,
                    "WASM channel on_respond completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name,
                callback: "on_respond".to_string(),
            }),
        }
    }

    /// Execute the on_broadcast callback.
    ///
    /// Called to send a proactive message to a user without a prior incoming message.
    pub async fn call_on_broadcast(
        &self,
        user_id: &str,
        content: &str,
        thread_id: Option<&str>,
        attachments: &[String],
    ) -> Result<(), WasmChannelError> {
        tracing::info!(
            channel = %self.name,
            user_id = %user_id,
            content_len = content.len(),
            attachment_count = attachments.len(),
            "call_on_broadcast invoked"
        );

        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                "WASM channel on_broadcast called (no WASM module)"
            );
            return Ok(());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = self.capabilities.clone();
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();

        let user_id = user_id.to_string();
        let content = content.to_string();
        let thread_id = thread_id.map(|s| s.to_string());
        let attachments = attachments.to_vec();

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                // Read attachment files from disk
                let wit_attachments = read_attachments(&attachments).map_err(|e| {
                    WasmChannelError::CallbackFailed {
                        name: prepared.name.clone(),
                        reason: e,
                    }
                })?;

                let mut store = Self::create_store(
                    &runtime,
                    &prepared,
                    &capabilities,
                    credentials,
                    host_credentials,
                    pairing_store,
                )?;

                let instance = Self::instantiate_component(&runtime, &prepared, &mut store)?;

                let wit_response = wit_channel::AgentResponse {
                    message_id: String::new(),
                    content: content.clone(),
                    thread_id,
                    metadata_json: String::new(),
                    attachments: wit_attachments,
                };

                let channel_iface = instance.near_agent_channel();
                let wasm_result = channel_iface
                    .call_on_broadcast(&mut store, &user_id, &wit_response)
                    .map_err(|e| {
                        tracing::error!(error = %e, "WASM on_broadcast call failed");
                        Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel)
                    })?;

                if let Err(ref err_msg) = wasm_result {
                    tracing::error!(error = %err_msg, "WASM on_broadcast returned error");
                    return Err(WasmChannelError::CallbackFailed {
                        name: prepared.name.clone(),
                        reason: err_msg.clone(),
                    });
                }

                let host_state =
                    Self::extract_host_state(&mut store, &prepared.name, &capabilities);
                tracing::info!("on_broadcast WASM execution completed successfully");
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
            Ok(Ok(((), _host_state))) => {
                tracing::debug!(
                    channel = %channel_name,
                    "WASM channel on_broadcast completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: channel_name,
                callback: "on_broadcast".to_string(),
            }),
        }
    }

    /// Execute the on_status callback.
    ///
    /// Called to notify the WASM channel of agent status changes (e.g., typing).
    pub async fn call_on_status(
        &self,
        status: &StatusUpdate,
        metadata: &serde_json::Value,
    ) -> Result<(), WasmChannelError> {
        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            return Ok(());
        }

        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let capabilities = self.capabilities.clone();
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();
        let credentials = self.credentials.read().await.clone();
        let host_credentials =
            resolve_channel_host_credentials(&self.capabilities, self.secrets_store.as_deref())
                .await;
        let pairing_store = self.pairing_store.clone();

        let wit_update = status_to_wit(status, metadata);

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

                let channel_iface = instance.near_agent_channel();
                channel_iface
                    .call_on_status(&mut store, &wit_update)
                    .map_err(|e| Self::map_wasm_error(e, &prepared.name, prepared.limits.fuel))?;

                Ok(())
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        match result {
            Ok(Ok(())) => {
                tracing::debug!(
                    channel = %self.name,
                    "WASM channel on_status completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: self.name.clone(),
                callback: "on_status".to_string(),
            }),
        }
    }
}
