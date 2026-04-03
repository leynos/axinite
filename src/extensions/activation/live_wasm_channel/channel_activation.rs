//! WASM channel activation flow for live activation.

use std::collections::HashMap;
use std::sync::Arc;

use crate::channels::ChannelManager;
use crate::channels::wasm::{SharedWasmChannel, WasmChannelRuntime};
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};
use crate::pairing::PairingStore;

use super::channel_refresh::RegisterWebhookEndpointsParams;
use super::{ToolAuthState, credentials::inject_channel_credentials_from_secrets};

impl super::LiveWasmChannelActivation {
    async fn inject_runtime_config(
        &self,
        channel: &Arc<crate::channels::wasm::WasmChannel>,
        channel_name: &str,
        owner_ids: &HashMap<String, i64>,
        webhook_secret: Option<&str>,
    ) {
        let mut config_updates = HashMap::new();

        if let Some(ref tunnel_url) = self.tunnel_url {
            config_updates.insert(
                "tunnel_url".to_string(),
                serde_json::Value::String(tunnel_url.clone()),
            );
        }
        if let Some(secret) = webhook_secret {
            config_updates.insert(
                "webhook_secret".to_string(),
                serde_json::Value::String(secret.to_string()),
            );
        }
        if let Some(&owner_id) = owner_ids.get(channel_name) {
            config_updates.insert("owner_id".to_string(), serde_json::json!(owner_id));
        }

        if !config_updates.is_empty() {
            channel.update_config(config_updates).await;
            tracing::info!(
                channel = %channel_name,
                has_tunnel = self.tunnel_url.is_some(),
                has_webhook_secret = webhook_secret.is_some(),
                "Injected runtime config into hot-activated channel"
            );
        }
    }

    async fn inject_and_log_credentials(
        &self,
        channel: &Arc<crate::channels::wasm::WasmChannel>,
        channel_name: &str,
    ) {
        match inject_channel_credentials_from_secrets(
            channel,
            Some(self.secrets.as_ref()),
            channel_name,
            &self.user_id,
        )
        .await
        {
            Ok(count) if count > 0 => {
                tracing::info!(
                    channel = %channel_name,
                    credentials_injected = count,
                    "Credentials injected into hot-activated channel"
                );
            }
            Ok(_) => {}
            Err(e) => {
                tracing::error!(
                    channel = %channel_name,
                    error = %e,
                    "Failed to inject credentials into hot-activated channel"
                );
            }
        }
    }

    async fn resolve_runtime_state(
        &self,
    ) -> Result<
        (
            Arc<WasmChannelRuntime>,
            Arc<ChannelManager>,
            Arc<PairingStore>,
            Arc<crate::channels::wasm::WasmChannelRouter>,
            HashMap<String, i64>,
        ),
        ExtensionError,
    > {
        let rt_guard = self.channel_runtime.read().await;
        let rt = rt_guard.as_ref().ok_or_else(|| {
            ExtensionError::ActivationFailed("WASM channel runtime not configured".to_string())
        })?;
        Ok((
            Arc::clone(&rt.wasm_channel_runtime),
            Arc::clone(&rt.channel_manager),
            Arc::clone(&rt.pairing_store),
            Arc::clone(&rt.wasm_channel_router),
            rt.wasm_channel_owner_ids.clone(),
        ))
    }

    async fn load_wasm_channel_from_disk(
        &self,
        name: &str,
        channel_runtime: &Arc<WasmChannelRuntime>,
        pairing_store: &Arc<PairingStore>,
    ) -> Result<crate::channels::wasm::LoadedChannel, ExtensionError> {
        let wasm_path = self.wasm_channels_dir.join(format!("{}.wasm", name));
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let cap_path_option = if cap_path.exists() {
            Some(cap_path.as_path())
        } else {
            None
        };

        let settings_store: Option<Arc<dyn crate::db::SettingsStore>> =
            self.store.as_ref().map(|db| Arc::clone(db) as _);
        let loader = crate::channels::wasm::WasmChannelLoader::new(
            Arc::clone(channel_runtime),
            Arc::clone(pairing_store),
            settings_store,
        )
        .with_secrets_store(Arc::clone(&self.secrets));
        loader
            .load_from_files(name, &wasm_path, cap_path_option)
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))
    }

    async fn complete_channel_activation(
        &self,
        channel_arc: Arc<crate::channels::wasm::WasmChannel>,
        channel_name: &str,
        channel_manager: &Arc<ChannelManager>,
    ) -> Result<ActivateResult, ExtensionError> {
        channel_manager
            .hot_add(Box::new(SharedWasmChannel::new(channel_arc)))
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        self.active_channel_names
            .write()
            .await
            .insert(channel_name.to_string());
        self.persist_active_channels().await;

        tracing::info!(channel = %channel_name, "Hot-activated WASM channel");

        self.activation_errors.write().await.remove(channel_name);
        self.broadcast_extension_status(channel_name, "active", None)
            .await;

        Ok(ActivateResult {
            name: channel_name.to_string(),
            kind: ExtensionKind::WasmChannel,
            tools_loaded: Vec::new(),
            message: format!("Channel '{}' activated and running", channel_name),
        })
    }

    /// Activate a WASM channel at runtime without restarting.
    ///
    /// Loads the channel from its WASM file, injects credentials and config,
    /// registers it with the webhook router, and hot-adds it to the channel
    /// manager so its stream feeds into the agent loop.
    pub(super) async fn activate_wasm_channel_inner(
        &self,
        name: &str,
    ) -> Result<ActivateResult, ExtensionError> {
        // This is a best-effort fast path: `active_channel_names` avoids most
        // duplicate work, while `hot_add` in `src/channels/manager.rs`
        // tolerates concurrent calls for the same `name` by replacing any
        // existing channel. Once `name` is published into
        // `active_channel_names`, later callers switch to
        // `refresh_active_channel`.
        {
            let active = self.active_channel_names.read().await;
            if active.contains(name) {
                return self.refresh_active_channel(name).await;
            }
        }

        let (
            channel_runtime,
            channel_manager,
            pairing_store,
            wasm_channel_router,
            wasm_channel_owner_ids,
        ) = self.resolve_runtime_state().await?;

        let auth_state = self.check_channel_auth_status(name).await?;
        if auth_state != ToolAuthState::Ready && auth_state != ToolAuthState::NoAuth {
            return Err(ExtensionError::ActivationFailed(format!(
                "Channel '{}' requires configuration. Use the setup form to provide credentials.",
                name
            )));
        }

        let loaded = self
            .load_wasm_channel_from_disk(name, &channel_runtime, &pairing_store)
            .await?;

        let channel_name = loaded.name().to_string();
        let webhook_secret_name = loaded.webhook_secret_name();
        let secret_header = loaded.webhook_secret_header().map(|s| s.to_string());
        let sig_key_secret_name = loaded.signature_key_secret_name();
        let hmac_secret_name = loaded.hmac_secret_name();

        let webhook_secret = match self
            .secrets
            .get_decrypted(&self.user_id, &webhook_secret_name)
            .await
        {
            Ok(secret) => Some(secret.expose().to_string()),
            Err(crate::secrets::SecretError::NotFound(_)) => None,
            Err(e) => {
                return Err(ExtensionError::ActivationFailed(format!(
                    "Failed to load webhook secret for '{}': {}",
                    channel_name, e
                )));
            }
        };

        let channel_arc = Arc::new(loaded.channel);

        self.inject_runtime_config(
            &channel_arc,
            &channel_name,
            &wasm_channel_owner_ids,
            webhook_secret.as_deref(),
        )
        .await;

        self.register_webhook_router_endpoints(
            &wasm_channel_router,
            Arc::clone(&channel_arc),
            RegisterWebhookEndpointsParams {
                channel_name: channel_name.clone(),
                webhook_secret,
                secret_header,
                sig_key_secret_name,
                hmac_secret_name,
            },
        )
        .await;

        self.inject_and_log_credentials(&channel_arc, &channel_name)
            .await;

        self.complete_channel_activation(channel_arc, &channel_name, &channel_manager)
            .await
    }
}
