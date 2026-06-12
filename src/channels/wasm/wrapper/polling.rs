//! Polling loop for WASM channels.

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::oneshot;

use super::dispatch::DispatchContext;
use super::{WasmChannel, resolve_channel_host_credentials};

impl WasmChannel {
    /// instance (matching our "fresh instance per callback" pattern).
    pub(super) fn start_polling(&self, interval: Duration, shutdown_rx: oneshot::Receiver<()>) {
        let channel_name = self.name.clone();
        let runtime = Arc::clone(&self.runtime);
        let prepared = Arc::clone(&self.prepared);
        let poll_capabilities = self.capabilities.clone();
        let capabilities = Self::inject_workspace_reader(&self.capabilities, &self.workspace_store);
        let message_tx = self.message_tx.clone();
        let rate_limiter = self.rate_limiter.clone();
        let credentials = self.credentials.clone();
        let pairing_store = self.pairing_store.clone();
        let callback_timeout = self.runtime.config().callback_timeout;
        let workspace_store = self.workspace_store.clone();
        let last_broadcast_metadata = self.last_broadcast_metadata.clone();
        let settings_store = self.settings_store.clone();
        let poll_secrets_store = self.secrets_store.clone();

        tokio::spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            let mut shutdown = std::pin::pin!(shutdown_rx);

            loop {
                tokio::select! {
                    _ = interval_timer.tick() => {
                        tracing::debug!(
                            channel = %channel_name,
                            "Polling tick - calling on_poll"
                        );

                        // Pre-resolve host credentials for this tick
                        let host_credentials = resolve_channel_host_credentials(
                            &poll_capabilities,
                            poll_secrets_store.as_deref(),
                        )
                        .await;

                        // Execute on_poll with fresh WASM instance
                        let result = Self::execute_poll(
                            &channel_name,
                            &runtime,
                            &prepared,
                            &capabilities,
                            &credentials,
                            host_credentials,
                            pairing_store.clone(),
                            callback_timeout,
                            &workspace_store,
                        ).await;

                        match result {
                            Ok(emitted_messages) => {
                                // Process any emitted messages
                                if !emitted_messages.is_empty()
                                    && let Err(e) = Self::dispatch_emitted_messages(
                                        &channel_name,
                                        emitted_messages,
                                        DispatchContext {
                                            message_tx: &message_tx,
                                            rate_limiter: &rate_limiter,
                                            last_broadcast_metadata: &last_broadcast_metadata,
                                            settings_store: settings_store.as_ref(),
                                        },
                                    ).await {
                                        tracing::warn!(
                                            channel = %channel_name,
                                            error = %e,
                                            "Failed to dispatch emitted messages from poll"
                                        );
                                    }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    channel = %channel_name,
                                    error = %e,
                                    "Polling callback failed"
                                );
                            }
                        }
                    }
                    _ = &mut shutdown => {
                        tracing::info!(
                            channel = %channel_name,
                            "Polling stopped"
                        );
                        break;
                    }
                }
            }
        });
    }
}
