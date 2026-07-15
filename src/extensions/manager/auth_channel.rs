//! WASM channel and channel-relay authorization and stored-relay activation.

use crate::extensions::{AuthResult, ExtensionError, ExtensionKind};
use crate::secrets::CreateSecretParams;

use super::ExtensionManager;

/// The inputs needed to store the next missing secret for a WASM channel.
struct ChannelSecretRequest<'a> {
    name: &'a str,
    cap_file: &'a crate::channels::wasm::ChannelCapabilitiesFile,
    missing: &'a [&'a crate::channels::wasm::SecretSetupSchema],
    token_value: &'a str,
}

impl ExtensionManager {
    /// Load a WASM channel's capabilities file, returning `None` when the
    /// channel has no capabilities file at all.
    async fn load_channel_capabilities(
        &self,
        name: &str,
    ) -> Result<Option<crate::channels::wasm::ChannelCapabilitiesFile>, ExtensionError> {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));

        if !cap_path.exists() {
            return Ok(None);
        }

        let cap_bytes = tokio::fs::read(&cap_path)
            .await
            .map_err(|e| ExtensionError::Other(e.to_string()))?;

        crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&cap_bytes)
            .map(Some)
            .map_err(|e| ExtensionError::Other(e.to_string()))
    }

    /// Collect the non-optional secrets that are not yet stored.
    async fn missing_required_secrets<'s>(
        &self,
        required_secrets: &'s [crate::channels::wasm::SecretSetupSchema],
    ) -> Vec<&'s crate::channels::wasm::SecretSetupSchema> {
        let mut missing = Vec::new();
        for secret in required_secrets {
            if secret.optional {
                continue;
            }
            if !self
                .secrets
                .exists(&self.user_id, &secret.name)
                .await
                .unwrap_or(false)
            {
                missing.push(secret);
            }
        }
        missing
    }

    /// Store the provided token for the first missing secret, then either
    /// finish authentication or prompt for the next missing secret.
    async fn store_channel_secret(
        &self,
        request: ChannelSecretRequest<'_>,
    ) -> Result<AuthResult, ExtensionError> {
        let ChannelSecretRequest {
            name,
            cap_file,
            missing,
            token_value,
        } = request;
        let secret = &missing[0];
        let params =
            CreateSecretParams::new(&secret.name, token_value).with_provider(name.to_string());
        self.secrets
            .create(&self.user_id, params)
            .await
            .map_err(|e| ExtensionError::AuthFailed(e.to_string()))?;

        // Check if there are more missing secrets
        if missing.len() <= 1 {
            return Ok(AuthResult::authenticated(name, ExtensionKind::WasmChannel));
        }

        // More secrets needed; prompt for the next one
        let next = &missing[1];
        Ok(AuthResult::awaiting_token(
            name,
            ExtensionKind::WasmChannel,
            next.prompt.clone(),
            cap_file.setup.setup_url.clone(),
        ))
    }

    pub(super) async fn auth_wasm_channel(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<AuthResult, ExtensionError> {
        let Some(cap_file) = self.load_channel_capabilities(name).await? else {
            return Ok(AuthResult::no_auth_required(
                name,
                ExtensionKind::WasmChannel,
            ));
        };

        // Get required secrets from the setup section
        let required_secrets = &cap_file.setup.required_secrets;
        if required_secrets.is_empty() {
            return Ok(AuthResult::no_auth_required(
                name,
                ExtensionKind::WasmChannel,
            ));
        }

        // Find the non-optional secrets that aren't yet stored
        let missing = self.missing_required_secrets(required_secrets).await;
        if missing.is_empty() {
            return Ok(AuthResult::authenticated(name, ExtensionKind::WasmChannel));
        }

        // If a token was provided, store it for the first missing secret
        if let Some(token_value) = token {
            return self
                .store_channel_secret(ChannelSecretRequest {
                    name,
                    cap_file: &cap_file,
                    missing: &missing,
                    token_value,
                })
                .await;
        }

        // Prompt for the first missing secret
        let secret = &missing[0];
        Ok(AuthResult::awaiting_token(
            name,
            ExtensionKind::WasmChannel,
            secret.prompt.clone(),
            cap_file.setup.setup_url.clone(),
        ))
    }

    // ── Channel-relay extension methods ──────────────────────────────────

    /// Authenticate a channel-relay extension.
    ///
    /// For Slack: initiates OAuth flow (redirect-based).
    /// For Telegram: accepts a bot token, registers it with channel-relay,
    /// and stores the returned stream token.
    pub(super) async fn auth_channel_relay(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<AuthResult, ExtensionError> {
        // Check if already authenticated (stream token exists)
        let token_key = format!("relay:{}:stream_token", name);
        if self
            .secrets
            .exists(&self.user_id, &token_key)
            .await
            .unwrap_or(false)
        {
            return Ok(AuthResult::authenticated(name, ExtensionKind::ChannelRelay));
        }

        if let Some(token) = token {
            return self.store_relay_stream_token(name, &token_key, token).await;
        }

        self.initiate_relay_oauth(name).await
    }

    /// Store the caller-provided relay stream token, replacing any stale copy,
    /// and report the extension as authenticated.
    async fn store_relay_stream_token(
        &self,
        name: &str,
        token_key: &str,
        token: &str,
    ) -> Result<AuthResult, ExtensionError> {
        let _ = self.secrets.delete(&self.user_id, token_key).await;
        self.secrets
            .create(
                &self.user_id,
                CreateSecretParams::new(token_key, token).with_provider(name.to_string()),
            )
            .await
            .map_err(|e| ExtensionError::AuthFailed(format!("Failed to store relay token: {e}")))?;
        Ok(AuthResult::authenticated(name, ExtensionKind::ChannelRelay))
    }

    /// Resolve the OAuth callback base URL, preferring the active tunnel, then
    /// the relay's configured callback, then the gateway host/port env vars.
    fn relay_callback_base(&self, relay_config: &crate::config::RelayConfig) -> String {
        self.tunnel_url
            .clone()
            .or_else(|| relay_config.callback_url.clone())
            .unwrap_or_else(|| {
                let host = std::env::var("GATEWAY_HOST").unwrap_or_else(|_| "127.0.0.1".into());
                let port = std::env::var("GATEWAY_PORT").unwrap_or_else(|_| "3001".into());
                format!("http://{}:{}", host, port)
            })
    }

    /// Persist a fresh CSRF nonce for the relay OAuth state parameter,
    /// discarding any stale value first.
    async fn store_relay_oauth_state(
        &self,
        name: &str,
        state_nonce: &str,
    ) -> Result<(), ExtensionError> {
        let state_key = format!("relay:{}:oauth_state", name);
        let _ = self.secrets.delete(&self.user_id, &state_key).await;
        self.secrets
            .create(
                &self.user_id,
                CreateSecretParams::new(&state_key, state_nonce),
            )
            .await
            .map_err(|e| ExtensionError::AuthFailed(format!("Failed to store OAuth state: {e}")))?;
        Ok(())
    }

    /// Begin the relay OAuth redirect flow: build the callback URL, persist a
    /// CSRF nonce, and ask the relay for an authorization URL.
    async fn initiate_relay_oauth(&self, name: &str) -> Result<AuthResult, ExtensionError> {
        // Use relay config captured at startup
        let relay_config = self.relay_config()?;

        let instance_id = self.relay_instance_id(relay_config);
        let user_id_uuid =
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, self.user_id.as_bytes()).to_string();

        let client = crate::channels::relay::RelayClient::new(
            relay_config.url.clone(),
            relay_config.api_key.clone(),
            relay_config.request_timeout_secs,
        )
        .map_err(|e| ExtensionError::Config(e.to_string()))?;

        let callback_base = self.relay_callback_base(relay_config);

        // Generate CSRF nonce for OAuth state parameter
        let state_nonce = uuid::Uuid::new_v4().to_string();
        self.store_relay_oauth_state(name, &state_nonce).await?;

        let callback_url = format!(
            "{}/oauth/slack/callback?state={}",
            callback_base, state_nonce
        );

        match client
            .initiate_oauth(&instance_id, &user_id_uuid, &callback_url)
            .await
        {
            Ok(auth_url) => Ok(AuthResult::awaiting_authorization(
                name,
                ExtensionKind::ChannelRelay,
                auth_url,
                "redirect".to_string(),
            )),
            Err(e) => Err(ExtensionError::AuthFailed(e.to_string())),
        }
    }

    /// Activate a channel-relay extension from stored credentials (for startup reconnect).
    pub async fn activate_stored_relay(&self, name: &str) -> Result<(), ExtensionError> {
        self.installed_relay_extensions
            .write()
            .await
            .insert(name.to_string());
        self.wasm_channel_activation
            .activate_channel_relay(name)
            .await?;
        Ok(())
    }
}
