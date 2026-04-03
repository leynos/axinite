//! Refresh and router-registration helpers for live WASM channels.

use std::sync::Arc;

use crate::channels::wasm::RegisteredEndpoint;
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};

use super::credentials::inject_channel_credentials_from_secrets;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct ChannelName(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct WebhookSecretName(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct SecretHeader(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct SigKeySecretName(pub String);

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) struct HmacSecretName(pub String);

impl AsRef<str> for ChannelName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for WebhookSecretName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for SecretHeader {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for SigKeySecretName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for HmacSecretName {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

pub(super) struct RegisterWebhookEndpointsParams {
    pub(super) channel_name: ChannelName,
    pub(super) webhook_secret: Option<WebhookSecretName>,
    pub(super) secret_header: Option<SecretHeader>,
    pub(super) sig_key_secret_name: Option<SigKeySecretName>,
    pub(super) hmac_secret_name: Option<HmacSecretName>,
}

impl super::LiveWasmChannelActivation {
    async fn register_sig_key_with_router(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        channel: &ChannelName,
        sig_key: &SigKeySecretName,
    ) {
        let key_secret = match self
            .secrets
            .get_decrypted(&self.user_id, sig_key.as_ref())
            .await
        {
            Ok(s) => s,
            Err(crate::secrets::SecretError::NotFound(_)) => {
                tracing::debug!(
                    user_id = %self.user_id,
                    channel = %channel.as_ref(),
                    sig_key = %sig_key.as_ref(),
                    "Signature key secret not found; skipping router registration"
                );
                return;
            }
            Err(e) => {
                tracing::warn!(
                    user_id = %self.user_id,
                    channel = %channel.as_ref(),
                    sig_key = %sig_key.as_ref(),
                    error = %e,
                    "Failed to load signature key for router registration"
                );
                return;
            }
        };
        match router
            .register_signature_key(channel.as_ref(), key_secret.expose())
            .await
        {
            Ok(()) => tracing::info!(
                channel = %channel.as_ref(),
                "Registered signature key for hot-activated channel"
            ),
            Err(e) => tracing::error!(
                channel = %channel.as_ref(),
                error = %e,
                "Failed to register signature key"
            ),
        }
    }

    async fn register_hmac_with_router(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        channel: &ChannelName,
        hmac: &HmacSecretName,
    ) {
        match self
            .secrets
            .get_decrypted(&self.user_id, hmac.as_ref())
            .await
        {
            Ok(secret) => {
                router
                    .register_hmac_secret(channel.as_ref(), secret.expose())
                    .await;
                tracing::info!(
                    channel = %channel.as_ref(),
                    "Registered HMAC signing secret for hot-activated channel"
                );
            }
            Err(e) => {
                tracing::warn!(channel = %channel.as_ref(), error = %e, "HMAC secret not found");
            }
        }
    }

    pub(super) async fn register_webhook_router_endpoints(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        channel: Arc<crate::channels::wasm::WasmChannel>,
        params: RegisterWebhookEndpointsParams,
    ) {
        let webhook_path = format!("/webhook/{}", params.channel_name.as_ref());
        let endpoints = vec![RegisteredEndpoint {
            channel_name: params.channel_name.0.clone(),
            path: webhook_path,
            methods: vec!["POST".to_string()],
            require_secret: params.webhook_secret.is_some(),
        }];
        router
            .register(
                channel,
                endpoints,
                params.webhook_secret.map(|secret| secret.0),
                params.secret_header.map(|header| header.0),
            )
            .await;
        tracing::info!(
            channel = %params.channel_name.as_ref(),
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
        name: &ChannelName,
    ) -> usize {
        match inject_channel_credentials_from_secrets(
            channel,
            Some(self.secrets.as_ref()),
            &self.user_id,
        )
        .await
        {
            Ok(count) => count,
            Err(e) => {
                tracing::warn!(
                    channel = %name.as_ref(),
                    error = %e,
                    "Failed to refresh credentials on already-active channel"
                );
                0
            }
        }
    }

    pub(super) async fn load_capabilities_secret_names(
        &self,
        name: &ChannelName,
    ) -> (
        WebhookSecretName,
        Option<SigKeySecretName>,
        Option<HmacSecretName>,
    ) {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name.as_ref()));
        let capabilities_file = match tokio::fs::read(&cap_path).await {
            Ok(bytes) => crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&bytes).ok(),
            Err(_) => None,
        };
        let webhook_secret_name = WebhookSecretName(
            capabilities_file
                .as_ref()
                .map(|f| f.webhook_secret_name())
                .unwrap_or_else(|| format!("{}_webhook_secret", name.as_ref())),
        );
        let sig_key_secret_name = capabilities_file
            .as_ref()
            .and_then(|f| f.signature_key_secret_name())
            .map(str::to_string)
            .map(SigKeySecretName);
        let hmac_secret_name = capabilities_file
            .as_ref()
            .and_then(|f| f.hmac_secret_name())
            .map(str::to_string)
            .map(HmacSecretName);
        (webhook_secret_name, sig_key_secret_name, hmac_secret_name)
    }

    pub(super) async fn refresh_webhook_secret(
        &self,
        router: &Arc<crate::channels::wasm::WasmChannelRouter>,
        name: &ChannelName,
        webhook_secret_name: &WebhookSecretName,
    ) {
        match self
            .secrets
            .get_decrypted(&self.user_id, webhook_secret_name.as_ref())
            .await
        {
            Ok(secret) => {
                router
                    .update_secret(name.as_ref(), secret.expose().to_string())
                    .await;
                tracing::info!(
                    channel = %name.as_ref(),
                    "Refreshed webhook secret for active channel"
                );
            }
            Err(crate::secrets::SecretError::NotFound(_)) => {}
            Err(e) => {
                tracing::warn!(
                    channel = %name.as_ref(),
                    webhook_secret_name = %webhook_secret_name.as_ref(),
                    error = %e,
                    "Failed to refresh webhook secret for active channel"
                );
            }
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

        let chan = ChannelName(name.to_string());
        let cred_count = self.reinject_credentials(&existing_channel, &chan).await;

        let (webhook_secret_name, sig_key_secret_name, hmac_secret_name) =
            self.load_capabilities_secret_names(&chan).await;

        self.refresh_webhook_secret(&router, &chan, &webhook_secret_name)
            .await;

        if let Some(ref sig) = sig_key_secret_name {
            self.refresh_sig_key(&router, name, sig.as_ref()).await;
        }

        if let Some(ref hmac) = hmac_secret_name {
            self.refresh_hmac_secret(&router, name, hmac.as_ref()).await;
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
