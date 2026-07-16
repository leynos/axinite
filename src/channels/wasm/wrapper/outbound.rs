//! Outbound callbacks for the WASM channel: on_respond, on_broadcast, and
//! on_status.
//!
//! These callbacks push agent output back through the WASM guest — replies to
//! incoming messages, proactive broadcasts, and status updates such as typing
//! indicators.

use uuid::Uuid;

use crate::channels::StatusUpdate;
use crate::channels::wasm::error::WasmChannelError;

use super::guest_calls::{BroadcastPayload, RespondPayload};
use super::{WasmChannel, status_to_wit};

/// Borrowed view of an on_respond invocation: the sole parameter to
/// [`WasmChannel::call_on_respond`], also used for its structured logging.
pub struct RespondInvocation<'a> {
    /// Identifier of the incoming message being replied to.
    pub message_id: Uuid,
    /// Reply body forwarded to the guest.
    pub content: &'a str,
    /// Optional thread identifier for reply chaining.
    pub thread_id: Option<&'a str>,
    /// Channel-specific routing metadata (JSON), taken from the original message.
    pub metadata_json: &'a str,
    /// Attachment references sent alongside the reply.
    pub attachments: &'a [String],
}

impl WasmChannel {
    /// Emit the invocation and credential-state logs for an on_respond call.
    async fn log_respond_invocation(&self, invocation: &RespondInvocation<'_>) {
        tracing::info!(
            channel = %self.name,
            message_id = %invocation.message_id,
            content_len = invocation.content.len(),
            thread_id = ?invocation.thread_id,
            attachment_count = invocation.attachments.len(),
            "call_on_respond invoked"
        );

        // Log credentials state (without values)
        let creds = self.get_credentials().await;
        tracing::info!(
            credential_count = creds.len(),
            credential_names = ?creds.keys().collect::<Vec<_>>(),
            "Credentials available for on_respond"
        );
    }

    /// Interpret the outcome of a timed guest callback, mapping join panics and
    /// timeouts to the corresponding channel errors and logging completion.
    fn interpret_callback_result(
        &self,
        result: Result<Result<(), WasmChannelError>, tokio::time::error::Elapsed>,
        callback: &str,
    ) -> Result<(), WasmChannelError> {
        match result {
            Ok(Ok(())) => {
                tracing::debug!(
                    channel = %self.name,
                    callback = callback,
                    "WASM channel callback completed"
                );
                Ok(())
            }
            Ok(Err(e)) => Err(e),
            Err(_) => Err(WasmChannelError::Timeout {
                name: self.name.clone(),
                callback: callback.to_string(),
            }),
        }
    }

    /// Execute the on_respond callback.
    ///
    /// Called when the agent has a response to send back.
    pub async fn call_on_respond(
        &self,
        invocation: RespondInvocation<'_>,
    ) -> Result<(), WasmChannelError> {
        self.log_respond_invocation(&invocation).await;

        let RespondInvocation {
            message_id,
            content,
            thread_id,
            metadata_json,
            attachments,
        } = invocation;

        // If no WASM bytes, do nothing (for testing)
        if self.prepared.component().is_none() {
            tracing::debug!(
                channel = %self.name,
                message_id = %message_id,
                "WASM channel on_respond called (no WASM module)"
            );
            return Ok(());
        }

        let env = self.guest_call_env().await;
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();

        // Prepare response data
        let payload = RespondPayload {
            message_id: message_id.to_string(),
            content: content.to_string(),
            thread_id: thread_id.map(|s| s.to_string()),
            metadata_json: metadata_json.to_string(),
            attachments: attachments.to_vec(),
        };

        // Execute in blocking task with timeout
        tracing::info!(channel = %channel_name, "Starting on_respond WASM execution");

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || Self::run_respond_guest(env, payload))
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

        self.interpret_callback_result(result, "on_respond")
    }

    /// Execute the on_broadcast callback.
    ///
    /// Called to send a proactive message to a user without a prior incoming message.
    pub(super) async fn call_on_broadcast(
        &self,
        payload: BroadcastPayload,
    ) -> Result<(), WasmChannelError> {
        tracing::info!(
            channel = %self.name,
            user_id = %payload.user_id,
            content_len = payload.content.len(),
            attachment_count = payload.attachments.len(),
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

        let env = self.guest_call_env().await;
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || Self::run_broadcast_guest(env, payload))
                .await
                .map_err(|e| WasmChannelError::ExecutionPanicked {
                    name: channel_name.clone(),
                    reason: e.to_string(),
                })?
        })
        .await;

        self.interpret_callback_result(result, "on_broadcast")
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

        let env = self.guest_call_env().await;
        let timeout = self.runtime.config().callback_timeout;
        let channel_name = self.name.clone();

        let wit_update = status_to_wit(status, metadata);

        let result = tokio::time::timeout(timeout, async move {
            tokio::task::spawn_blocking(move || {
                let mut store = Self::create_store(
                    &env.runtime,
                    &env.prepared,
                    &env.capabilities,
                    env.credentials,
                    env.host_credentials,
                    env.pairing_store,
                )?;
                let instance =
                    Self::instantiate_component(&env.runtime, &env.prepared, &mut store)?;

                let channel_iface = instance.near_agent_channel();
                channel_iface
                    .call_on_status(&mut store, &wit_update)
                    .map_err(|e| {
                        Self::map_wasm_error(e, &env.prepared.name, env.prepared.limits.fuel)
                    })?;

                Ok(())
            })
            .await
            .map_err(|e| WasmChannelError::ExecutionPanicked {
                name: channel_name.clone(),
                reason: e.to_string(),
            })?
        })
        .await;

        self.interpret_callback_result(result, "on_status")
    }
}
