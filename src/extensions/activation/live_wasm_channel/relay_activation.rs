//! Channel-relay activation logic for live activation.

use crate::channels::relay::RelayChannel;
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};

impl super::LiveWasmChannelActivation {
    /// Get the relay configuration, returning an error if not set.
    fn fetch_relay_config(&self) -> Result<&crate::config::RelayConfig, ExtensionError> {
        self.relay_config.as_ref().ok_or_else(|| {
            ExtensionError::Config(
                "CHANNEL_RELAY_URL and CHANNEL_RELAY_API_KEY must be set".to_string(),
            )
        })
    }

    /// Generate a relay instance ID.
    fn derive_relay_instance_id(&self, cfg: &crate::config::RelayConfig) -> String {
        cfg.instance_id.clone().unwrap_or_else(|| {
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, self.user_id.as_bytes()).to_string()
        })
    }

    async fn decrypt_stream_token(&self, secret_name: &str) -> Result<String, ExtensionError> {
        match self.secrets.get_decrypted(&self.user_id, secret_name).await {
            Ok(secret) => Ok(secret.expose().to_string()),
            Err(_) => Err(ExtensionError::AuthRequired),
        }
    }

    async fn maybe_fetch_team_id(&self, name: &str) -> Option<String> {
        if let Some(ref store) = self.store {
            let team_id_key = format!("relay:{}:team_id", name);
            store
                .get_setting(self.user_id.as_str().into(), team_id_key.as_str().into())
                .await
                .ok()
                .flatten()
                .and_then(|v| v.as_str().map(|s| s.to_string()))
        } else {
            None
        }
    }

    fn build_relay_channel(
        &self,
        _name: &str,
        cfg: &crate::config::RelayConfig,
        instance_id: &str,
        stream_token: &str,
        team_id: Option<&str>,
    ) -> Result<RelayChannel, ExtensionError> {
        let client = crate::channels::relay::RelayClient::new(
            cfg.url.clone(),
            cfg.api_key.clone(),
            cfg.request_timeout_secs,
        )
        .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        Ok(RelayChannel::new_with_provider(
            client,
            crate::channels::relay::channel::RelayProvider::Slack,
            stream_token.to_string(),
            team_id.unwrap_or("").to_string(),
            instance_id.to_string(),
            self.user_id.clone(),
        )
        .with_timeouts(
            cfg.stream_timeout_secs,
            cfg.backoff_initial_ms,
            cfg.backoff_max_ms,
        ))
    }

    async fn complete_relay_activation(
        &self,
        name: &str,
        channel: RelayChannel,
    ) -> Result<ActivateResult, ExtensionError> {
        let cm_guard = self.relay_channel_manager.read().await;
        let channel_mgr = cm_guard.as_ref().ok_or_else(|| {
            ExtensionError::ActivationFailed("Channel manager not initialized".to_string())
        })?;

        channel_mgr
            .hot_add(Box::new(channel))
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        self.active_channel_names
            .write()
            .await
            .insert(name.to_string());
        self.persist_active_channels().await;

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

    /// Activate a channel-relay extension.
    ///
    /// Activates a channel-relay extension (e.g., Slack) using the relay
    /// service.
    pub(super) async fn activate_channel_relay_inner(
        &self,
        name: &str,
    ) -> Result<ActivateResult, ExtensionError> {
        let token_key = format!("relay:{}:stream_token", name);
        let cfg = self.fetch_relay_config()?;
        let instance_id = self.derive_relay_instance_id(cfg);
        let stream_token = self.decrypt_stream_token(&token_key).await?;
        let team_id = self.maybe_fetch_team_id(name).await;
        let channel =
            self.build_relay_channel(name, cfg, &instance_id, &stream_token, team_id.as_deref())?;
        self.complete_relay_activation(name, channel).await
    }
}
