//! Refresh and router-registration helpers for live WASM channels.

use std::sync::Arc;

use crate::channels::wasm::RegisteredEndpoint;
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};

use super::credentials::inject_channel_credentials_from_secrets;

pub(super) struct RegisterWebhookEndpointsParams {
    pub(super) channel_name: String,
    pub(super) webhook_secret: Option<String>,
    pub(super) secret_header: Option<String>,
    pub(super) sig_key_secret_name: Option<String>,
    pub(super) hmac_secret_name: Option<String>,
}

impl super::LiveWasmChannelActivation {
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

    pub(super) async fn register_webhook_router_endpoints(
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

    pub(super) async fn reinject_credentials(
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

    pub(super) async fn load_capabilities_secret_names(
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

    pub(super) async fn refresh_webhook_secret(
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
}
