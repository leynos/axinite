//! State persistence and status-refresh helpers for live WASM activation.

use std::sync::Arc;

impl super::LiveWasmChannelActivation {
    pub(super) async fn persist_active_channels(&self) {
        let Some(ref store) = self.store else {
            return;
        };
        let names: Vec<String> = self
            .active_channel_names
            .read()
            .await
            .iter()
            .cloned()
            .collect();
        let value = serde_json::json!(names);
        if let Err(e) = store
            .set_setting(
                self.user_id.as_str().into(),
                "activated_channels".into(),
                &value,
            )
            .await
        {
            tracing::warn!(error = %e, "Failed to persist activated_channels setting");
        }
    }

    pub(super) async fn broadcast_extension_status(
        &self,
        name: &str,
        status: &str,
        message: Option<&str>,
    ) {
        if let Some(ref sender) = *self.sse_sender.read().await {
            let _ = sender.send(crate::channels::web::types::SseEvent::ExtensionStatus {
                extension_name: name.to_string(),
                status: status.to_string(),
                message: message.map(|m| m.to_string()),
            });
        }
    }

    pub(super) async fn refresh_sig_key(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        name: &str,
        sig_key_name: &str,
    ) {
        match self
            .secrets
            .get_decrypted(&self.user_id, sig_key_name)
            .await
        {
            Ok(key_secret) => {
                if let Err(e) = router
                    .register_signature_key(name, key_secret.expose())
                    .await
                {
                    tracing::error!(
                        channel = %name,
                        error = %e,
                        "Failed to refresh signature key"
                    );
                } else {
                    tracing::info!(channel = %name, "Refreshed signature key for active channel");
                }
            }
            Err(_) => {
                tracing::debug!(
                    channel = %name,
                    "Signature key secret not found, skipping refresh"
                );
            }
        }
    }

    pub(super) async fn refresh_hmac_secret(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        name: &str,
        hmac_name: &str,
    ) {
        match self.secrets.get_decrypted(&self.user_id, hmac_name).await {
            Ok(hmac_secret) => {
                router
                    .register_hmac_secret(name, hmac_secret.expose())
                    .await;
                tracing::info!(channel = %name, "Refreshed HMAC secret for active channel");
            }
            Err(e) => {
                tracing::warn!(channel = %name, error = %e, "HMAC secret not found");
            }
        }
    }
}
