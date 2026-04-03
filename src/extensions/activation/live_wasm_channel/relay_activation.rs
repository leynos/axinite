//! Channel-relay activation logic for live activation.

use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};

impl super::LiveWasmChannelActivation {
    /// Get the relay configuration, returning an error if not set.
    fn relay_config(&self) -> Result<&crate::config::RelayConfig, ExtensionError> {
        self.relay_config.as_ref().ok_or_else(|| {
            ExtensionError::Config(
                "CHANNEL_RELAY_URL and CHANNEL_RELAY_API_KEY must be set".to_string(),
            )
        })
    }

    /// Generate a relay instance ID.
    fn relay_instance_id(&self, config: &crate::config::RelayConfig) -> String {
        config.instance_id.clone().unwrap_or_else(|| {
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, self.user_id.as_bytes()).to_string()
        })
    }

    /// Activate a channel-relay extension.
    ///
    /// Activates a channel-relay extension (e.g., Slack) using the relay
    /// service.
    pub(super) async fn activate_channel_relay_inner(
        &self,
        name: &str,
    ) -> Result<ActivateResult, ExtensionError> {
        let token_key = format!("relay:{}:stream_token", name);
        let team_id_key = format!("relay:{}:team_id", name);

        // Check if we have a stream token
        let stream_token = match self.secrets.get_decrypted(&self.user_id, &token_key).await {
            Ok(secret) => secret.expose().to_string(),
            Err(_) => {
                return Err(ExtensionError::AuthRequired);
            }
        };

        // Get team_id from settings
        let team_id = if let Some(ref store) = self.store {
            store
                .get_setting(self.user_id.as_str().into(), team_id_key.as_str().into())
                .await
                .ok()
                .flatten()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default()
        } else {
            String::new()
        };

        // Use relay config captured at startup
        let relay_config = self.relay_config()?;

        let instance_id = self.relay_instance_id(relay_config);

        let client = crate::channels::relay::RelayClient::new(
            relay_config.url.clone(),
            relay_config.api_key.clone(),
            relay_config.request_timeout_secs,
        )
        .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        let channel = crate::channels::relay::RelayChannel::new_with_provider(
            client,
            crate::channels::relay::channel::RelayProvider::Slack,
            stream_token,
            team_id,
            instance_id,
            self.user_id.clone(),
        )
        .with_timeouts(
            relay_config.stream_timeout_secs,
            relay_config.backoff_initial_ms,
            relay_config.backoff_max_ms,
        );

        // Hot-add to channel manager
        let cm_guard = self.relay_channel_manager.read().await;
        let channel_mgr = cm_guard.as_ref().ok_or_else(|| {
            ExtensionError::ActivationFailed("Channel manager not initialized".to_string())
        })?;

        channel_mgr
            .hot_add(Box::new(channel))
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        // Mark as active
        self.active_channel_names
            .write()
            .await
            .insert(name.to_string());
        self.persist_active_channels().await;

        // Clear any previous activation error and broadcast success
        self.activation_errors.write().await.remove(name);
        let status_msg = "Slack connected via channel relay".to_string();
        self.broadcast_extension_status(name, "active", Some(&status_msg))
            .await;

        Ok(ActivateResult {
            name: name.to_string(),
            kind: ExtensionKind::ChannelRelay,
            tools_loaded: Vec::new(),
            message: status_msg,
        })
    }
}
