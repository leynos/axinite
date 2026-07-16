//! Shared machinery for the outbound WASM guest calls (on_respond and
//! on_broadcast): the collaborator snapshot moved into the blocking task, the
//! owned per-call payloads, and the blocking guest bodies themselves.

use std::collections::HashMap;
use std::sync::Arc;

use crate::channels::wasm::capabilities::ChannelCapabilities;
use crate::channels::wasm::error::WasmChannelError;
use crate::channels::wasm::runtime::{PreparedChannelModule, WasmChannelRuntime};
use crate::pairing::PairingStore;

use super::types::SecretValue;
use super::{
    ResolvedHostCredential, WasmChannel, read_attachments, resolve_channel_host_credentials,
    wit_channel,
};

/// Snapshot of the collaborators an outbound guest call moves into its
/// blocking task.
pub(super) struct GuestCallEnv {
    pub runtime: Arc<WasmChannelRuntime>,
    pub prepared: Arc<PreparedChannelModule>,
    pub capabilities: ChannelCapabilities,
    pub credentials: HashMap<String, SecretValue>,
    pub host_credentials: Vec<ResolvedHostCredential>,
    pub pairing_store: Arc<PairingStore>,
}

/// Owned response payload for the on_respond guest call.
pub(super) struct RespondPayload {
    pub message_id: String,
    pub content: String,
    pub thread_id: Option<String>,
    pub metadata_json: String,
    pub attachments: Vec<String>,
}

/// Owned broadcast payload for the on_broadcast guest call.
pub(super) struct BroadcastPayload {
    pub user_id: String,
    pub content: String,
    pub thread_id: Option<String>,
    pub attachments: Vec<String>,
}

impl WasmChannel {
    /// Capture the shared collaborators an outbound callback needs to move
    /// into its blocking task.
    pub(super) async fn guest_call_env(&self) -> GuestCallEnv {
        GuestCallEnv {
            runtime: Arc::clone(&self.runtime),
            prepared: Arc::clone(&self.prepared),
            capabilities: self.capabilities.clone(),
            credentials: self.credentials.read().await.clone(),
            host_credentials: resolve_channel_host_credentials(
                &self.capabilities,
                self.secrets_store.as_deref(),
            )
            .await,
            pairing_store: self.pairing_store.clone(),
        }
    }

    /// Blocking body of the on_respond guest call: read attachments, create a
    /// fresh instance, invoke the guest export, and surface guest errors.
    pub(super) fn run_respond_guest(
        env: GuestCallEnv,
        payload: RespondPayload,
    ) -> Result<(), WasmChannelError> {
        // Read attachment files from disk before entering WASM
        let wit_attachments = read_attachments(&payload.attachments).map_err(|e| {
            WasmChannelError::CallbackFailed {
                name: env.prepared.name.clone(),
                reason: e,
            }
        })?;

        tracing::info!("Creating WASM store for on_respond");
        let mut store = Self::create_store(
            &env.runtime,
            &env.prepared,
            &env.capabilities,
            env.credentials,
            env.host_credentials,
            env.pairing_store,
        )?;

        tracing::info!("Instantiating WASM component for on_respond");
        let instance = Self::instantiate_component(&env.runtime, &env.prepared, &mut store)?;

        // Build the WIT response type
        let wit_response = wit_channel::AgentResponse {
            message_id: payload.message_id,
            content: payload.content.clone(),
            thread_id: payload.thread_id,
            metadata_json: payload.metadata_json,
            attachments: wit_attachments,
        };

        // Truncate at char boundary for logging (avoid panic on multi-byte UTF-8)
        let content_preview: String = payload.content.chars().take(50).collect();
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
                Self::map_wasm_error(e, &env.prepared.name, env.prepared.limits.fuel)
            })?;

        tracing::info!(wasm_result = ?wasm_result, "WASM on_respond returned");

        // Check for WASM-level errors
        if let Err(ref err_msg) = wasm_result {
            tracing::error!(error = %err_msg, "WASM on_respond returned error");
            return Err(WasmChannelError::CallbackFailed {
                name: env.prepared.name.clone(),
                reason: err_msg.clone(),
            });
        }

        let _host_state =
            Self::extract_host_state(&mut store, &env.prepared.name, &env.capabilities);
        tracing::info!("on_respond WASM execution completed successfully");
        Ok(())
    }

    /// Blocking body of the on_broadcast guest call: read attachments, create a
    /// fresh instance, invoke the guest export, and surface guest errors.
    pub(super) fn run_broadcast_guest(
        env: GuestCallEnv,
        payload: BroadcastPayload,
    ) -> Result<(), WasmChannelError> {
        // Read attachment files from disk before entering WASM
        let wit_attachments = read_attachments(&payload.attachments).map_err(|e| {
            WasmChannelError::CallbackFailed {
                name: env.prepared.name.clone(),
                reason: e,
            }
        })?;

        let mut store = Self::create_store(
            &env.runtime,
            &env.prepared,
            &env.capabilities,
            env.credentials,
            env.host_credentials,
            env.pairing_store,
        )?;

        let instance = Self::instantiate_component(&env.runtime, &env.prepared, &mut store)?;

        let wit_response = wit_channel::AgentResponse {
            message_id: String::new(),
            content: payload.content,
            thread_id: payload.thread_id,
            metadata_json: String::new(),
            attachments: wit_attachments,
        };

        let channel_iface = instance.near_agent_channel();
        let wasm_result = channel_iface
            .call_on_broadcast(&mut store, &payload.user_id, &wit_response)
            .map_err(|e| {
                tracing::error!(error = %e, "WASM on_broadcast call failed");
                Self::map_wasm_error(e, &env.prepared.name, env.prepared.limits.fuel)
            })?;

        if let Err(ref err_msg) = wasm_result {
            tracing::error!(error = %err_msg, "WASM on_broadcast returned error");
            return Err(WasmChannelError::CallbackFailed {
                name: env.prepared.name.clone(),
                reason: err_msg.clone(),
            });
        }

        let _host_state =
            Self::extract_host_state(&mut store, &env.prepared.name, &env.capabilities);
        tracing::info!("on_broadcast WASM execution completed successfully");
        Ok(())
    }
}
