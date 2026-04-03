//! WASM channel activation and refresh logic for live activation.

use std::collections::HashMap;
use std::sync::Arc;

use crate::channels::ChannelManager;
use crate::channels::wasm::{RegisteredEndpoint, SharedWasmChannel, WasmChannelRuntime};
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};
use crate::pairing::PairingStore;

use super::{ToolAuthState, credentials::inject_channel_credentials_from_secrets};

struct RegisterWebhookEndpointsParams {
    channel_name: String,
    webhook_secret: Option<String>,
    secret_header: Option<String>,
    sig_key_secret_name: Option<String>,
    hmac_secret_name: Option<String>,
}

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

    async fn register_sig_key_with_router(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        channel_name: &str,
        sig_key_name: &str,
    ) {
        let key_secret = match self
            .secrets
            .get_decrypted(&self.user_id, sig_key_name)
            .await
        {
            Ok(s) => s,
            Err(_) => return,
        };
        match router
            .register_signature_key(channel_name, key_secret.expose())
            .await
        {
            Ok(()) => tracing::info!(
                channel = %channel_name,
                "Registered signature key for hot-activated channel"
            ),
            Err(e) => tracing::error!(
                channel = %channel_name,
                error = %e,
                "Failed to register signature key"
            ),
        }
    }

    async fn register_hmac_with_router(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        channel_name: &str,
        hmac_name: &str,
    ) {
        match self.secrets.get_decrypted(&self.user_id, hmac_name).await {
            Ok(secret) => {
                router
                    .register_hmac_secret(channel_name, secret.expose())
                    .await;
                tracing::info!(
                    channel = %channel_name,
                    "Registered HMAC signing secret for hot-activated channel"
                );
            }
            Err(e) => {
                tracing::warn!(channel = %channel_name, error = %e, "HMAC secret not found");
            }
        }
    }

    async fn register_webhook_router_endpoints(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        channel: Arc<crate::channels::wasm::WasmChannel>,
        params: RegisterWebhookEndpointsParams,
    ) {
        let webhook_path = format!("/webhook/{}", params.channel_name);
        let endpoints = vec![RegisteredEndpoint {
            channel_name: params.channel_name.clone(),
            path: webhook_path,
            methods: vec!["POST".to_string()],
            require_secret: params.webhook_secret.is_some(),
        }];
        router
            .register(
                channel,
                endpoints,
                params.webhook_secret,
                params.secret_header,
            )
            .await;
        tracing::info!(
            channel = %params.channel_name,
            "Registered hot-activated channel with webhook router"
        );

        if let Some(sig_key_name) = params.sig_key_secret_name {
            self.register_sig_key_with_router(router, &params.channel_name, &sig_key_name)
                .await;
        }

        if let Some(hmac_name) = params.hmac_secret_name {
            self.register_hmac_with_router(router, &params.channel_name, &hmac_name)
                .await;
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

        let auth_state = self.check_channel_auth_status(name).await;
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

        let webhook_secret = self
            .secrets
            .get_decrypted(&self.user_id, &webhook_secret_name)
            .await
            .ok()
            .map(|s| s.expose().to_string());

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

    async fn reinject_credentials(
        &self,
        channel: &Arc<crate::channels::wasm::WasmChannel>,
        name: &str,
    ) -> usize {
        match inject_channel_credentials_from_secrets(
            channel,
            Some(self.secrets.as_ref()),
            name,
            &self.user_id,
        )
        .await
        {
            Ok(count) => count,
            Err(e) => {
                tracing::warn!(
                    channel = %name,
                    error = %e,
                    "Failed to refresh credentials on already-active channel"
                );
                0
            }
        }
    }

    async fn load_capabilities_secret_names(
        &self,
        name: &str,
    ) -> (String, Option<String>, Option<String>) {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let capabilities_file = match tokio::fs::read(&cap_path).await {
            Ok(bytes) => crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&bytes).ok(),
            Err(_) => None,
        };
        let webhook_secret_name = capabilities_file
            .as_ref()
            .map(|f| f.webhook_secret_name())
            .unwrap_or_else(|| format!("{}_webhook_secret", name));
        let sig_key_secret_name = capabilities_file
            .as_ref()
            .and_then(|f| f.signature_key_secret_name())
            .map(str::to_string);
        let hmac_secret_name = capabilities_file
            .as_ref()
            .and_then(|f| f.hmac_secret_name())
            .map(str::to_string);
        (webhook_secret_name, sig_key_secret_name, hmac_secret_name)
    }

    async fn refresh_webhook_secret(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        name: &str,
        webhook_secret_name: &str,
    ) {
        if let Ok(secret) = self
            .secrets
            .get_decrypted(&self.user_id, webhook_secret_name)
            .await
        {
            router
                .update_secret(name, secret.expose().to_string())
                .await;
            tracing::info!(channel = %name, "Refreshed webhook secret for active channel");
        }
    }

    /// Refresh credentials and webhook secret on an already-active channel.
    ///
    /// Called when the user saves new secrets via the setup form for a channel
    /// that was loaded at startup (possibly without credentials).
    pub(super) async fn refresh_active_channel(
        &self,
        name: &str,
    ) -> Result<ActivateResult, ExtensionError> {
        let router = {
            let rt_guard = self.channel_runtime.read().await;
            match rt_guard.as_ref() {
                Some(rt) => Arc::clone(&rt.wasm_channel_router),
                None => {
                    return Ok(ActivateResult {
                        name: name.to_string(),
                        kind: ExtensionKind::WasmChannel,
                        tools_loaded: Vec::new(),
                        message: format!("Channel '{}' is already active", name),
                    });
                }
            }
        };

        let webhook_path = format!("/webhook/{}", name);
        let existing_channel = match router.get_channel_for_path(&webhook_path).await {
            Some(ch) => ch,
            None => {
                return Ok(ActivateResult {
                    name: name.to_string(),
                    kind: ExtensionKind::WasmChannel,
                    tools_loaded: Vec::new(),
                    message: format!("Channel '{}' is already active", name),
                });
            }
        };

        let cred_count = self.reinject_credentials(&existing_channel, name).await;

        let (webhook_secret_name, sig_key_secret_name, hmac_secret_name) =
            self.load_capabilities_secret_names(name).await;

        self.refresh_webhook_secret(&router, name, &webhook_secret_name)
            .await;

        if let Some(sig_key_name) = sig_key_secret_name {
            self.refresh_sig_key(&router, name, &sig_key_name).await;
        }

        if let Some(hmac_name) = hmac_secret_name {
            self.refresh_hmac_secret(&router, name, &hmac_name).await;
        }

        self.activation_errors.write().await.remove(name);
        self.broadcast_extension_status(name, "active", None).await;

        let message = if cred_count > 0 {
            format!(
                "Channel '{}' is active (refreshed {} credentials)",
                name, cred_count
            )
        } else {
            format!("Channel '{}' is already active", name)
        };

        Ok(ActivateResult {
            name: name.to_string(),
            kind: ExtensionKind::WasmChannel,
            tools_loaded: Vec::new(),
            message,
        })
    }

    /// Check the authentication status of a WASM channel.
    pub(super) async fn check_channel_auth_status(&self, name: &str) -> ToolAuthState {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let Ok(cap_bytes) = tokio::fs::read(&cap_path).await else {
            return ToolAuthState::NoAuth;
        };
        let Ok(cap_file) = crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&cap_bytes)
        else {
            return ToolAuthState::NoAuth;
        };

        let required: Vec<_> = cap_file
            .setup
            .required_secrets
            .iter()
            .filter(|s| !s.optional)
            .collect();
        if required.is_empty() {
            return ToolAuthState::NoAuth;
        }

        let all_provided = futures::future::join_all(
            required
                .iter()
                .map(|s| self.secrets.exists(&self.user_id, &s.name)),
        )
        .await
        .into_iter()
        .all(|r| r.unwrap_or(false));

        if all_provided {
            ToolAuthState::Ready
        } else {
            ToolAuthState::NeedsSetup
        }
    }
}
