//! Setup schema reporting and setup-secret persistence with hot activation.

use crate::extensions::{ExtensionError, ExtensionKind};
use crate::secrets::CreateSecretParams;

use super::ExtensionManager;
use super::SetupResult;

impl ExtensionManager {
    /// Get the setup schema for an extension (secret fields and their status).
    pub async fn get_setup_schema(
        &self,
        name: &str,
    ) -> Result<Vec<crate::channels::web::types::SecretFieldInfo>, ExtensionError> {
        let kind = self.determine_installed_kind(name).await?;
        match kind {
            ExtensionKind::WasmChannel => self.channel_setup_schema(name).await,
            ExtensionKind::WasmTool => Ok(self.tool_setup_schema(name).await),
            _ => Ok(Vec::new()),
        }
    }

    /// Whether a secret with this name is already stored for the user.
    async fn secret_provided(&self, secret_name: &str) -> bool {
        self.secrets
            .exists(&self.user_id, secret_name)
            .await
            .unwrap_or(false)
    }

    /// Setup fields for a WASM channel, from its capabilities file.
    async fn channel_setup_schema(
        &self,
        name: &str,
    ) -> Result<Vec<crate::channels::web::types::SecretFieldInfo>, ExtensionError> {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        if !cap_path.exists() {
            return Ok(Vec::new());
        }
        let cap_bytes = tokio::fs::read(&cap_path)
            .await
            .map_err(|e| ExtensionError::Other(e.to_string()))?;
        let cap_file = crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&cap_bytes)
            .map_err(|e| ExtensionError::Other(e.to_string()))?;

        let mut fields = Vec::new();
        for secret in &cap_file.setup.required_secrets {
            let provided = self.secret_provided(&secret.name).await;
            fields.push(crate::channels::web::types::SecretFieldInfo {
                name: secret.name.clone(),
                prompt: secret.prompt.clone(),
                optional: secret.optional,
                provided,
                auto_generate: secret.auto_generate.is_some(),
            });
        }
        Ok(fields)
    }

    /// Setup fields for a WASM tool, hiding OAuth client credentials that
    /// resolve automatically.
    async fn tool_setup_schema(
        &self,
        name: &str,
    ) -> Vec<crate::channels::web::types::SecretFieldInfo> {
        let Some(cap_file) = self.load_tool_capabilities(name).await else {
            return Vec::new();
        };
        let Some(setup) = &cap_file.setup else {
            return Vec::new();
        };

        let mut fields = Vec::new();
        for secret in &setup.required_secrets {
            // Skip OAuth client_id/secret fields that resolve automatically
            if Self::is_auto_resolved_oauth_field(&secret.name, &cap_file) {
                continue;
            }
            let provided = self.secret_provided(&secret.name).await;
            fields.push(crate::channels::web::types::SecretFieldInfo {
                name: secret.name.clone(),
                prompt: secret.prompt.clone(),
                optional: secret.optional,
                provided,
                auto_generate: false,
            });
        }
        fields
    }

    /// Save setup secrets for an extension, validating names against the capabilities schema.
    ///
    /// After saving, attempts to hot-activate the channel. Returns a [`SetupResult`]
    /// indicating whether activation succeeded (so the frontend can show appropriate UI).
    pub async fn save_setup_secrets(
        &self,
        name: &str,
        secrets: &std::collections::HashMap<String, String>,
    ) -> Result<SetupResult, ExtensionError> {
        let kind = self.determine_installed_kind(name).await?;

        let allowed = self.allowed_setup_secret_names(name, kind).await?;

        // For Telegram, validate the bot token against the API before storing it.
        // This catches bad tokens immediately (both on first setup and reconfigure),
        // before the channel activates and potentially shows as active with a bad token.
        if name == "telegram"
            && let Some(token_value) = secrets.get("telegram_bot_token")
        {
            validate_telegram_bot_token(token_value.trim()).await?;
        }

        self.store_submitted_secrets(name, &allowed, secrets)
            .await?;

        // Auto-generate any missing secrets (channel-only feature)
        if kind == ExtensionKind::WasmChannel {
            self.auto_generate_channel_secrets(name, secrets).await?;
        }

        // For tools, save and attempt auto-activation, then check auth.
        if kind == ExtensionKind::WasmTool {
            return Ok(self.finish_tool_setup(name).await);
        }

        // Try to hot-activate the channel now that secrets are saved
        Ok(self.finish_channel_setup(name).await)
    }

    /// Load the secret names an extension's capabilities file allows it to receive.
    async fn allowed_setup_secret_names(
        &self,
        name: &str,
        kind: ExtensionKind,
    ) -> Result<std::collections::HashSet<String>, ExtensionError> {
        match kind {
            ExtensionKind::WasmChannel => {
                let cap_path = self
                    .wasm_channels_dir
                    .join(format!("{}.capabilities.json", name));
                if !cap_path.exists() {
                    return Err(ExtensionError::Other(format!(
                        "Capabilities file not found for '{}'",
                        name
                    )));
                }
                let cap_bytes = tokio::fs::read(&cap_path)
                    .await
                    .map_err(|e| ExtensionError::Other(e.to_string()))?;
                let cap_file =
                    crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&cap_bytes)
                        .map_err(|e| ExtensionError::Other(e.to_string()))?;
                Ok(cap_file
                    .setup
                    .required_secrets
                    .iter()
                    .map(|s| s.name.clone())
                    .collect())
            }
            ExtensionKind::WasmTool => {
                let cap_file = self.load_tool_capabilities(name).await.ok_or_else(|| {
                    ExtensionError::Other(format!("Capabilities file not found for '{}'", name))
                })?;
                match cap_file.setup {
                    Some(s) => Ok(s.required_secrets.iter().map(|s| s.name.clone()).collect()),
                    None => Err(ExtensionError::Other(format!(
                        "Tool '{}' has no setup schema — no secrets to configure",
                        name
                    ))),
                }
            }
            _ => Err(ExtensionError::Other(
                "Setup is only supported for WASM channels and tools".to_string(),
            )),
        }
    }

    /// Validate each submitted secret name against the allowed set and store non-empty values.
    async fn store_submitted_secrets(
        &self,
        name: &str,
        allowed: &std::collections::HashSet<String>,
        secrets: &std::collections::HashMap<String, String>,
    ) -> Result<(), ExtensionError> {
        for (secret_name, secret_value) in secrets {
            if !allowed.contains(secret_name.as_str()) {
                return Err(ExtensionError::Other(format!(
                    "Unknown secret '{}' for extension '{}'",
                    secret_name, name
                )));
            }
            if secret_value.trim().is_empty() {
                continue;
            }
            let params =
                CreateSecretParams::new(secret_name, secret_value).with_provider(name.to_string());
            self.secrets
                .create(&self.user_id, params)
                .await
                .map_err(|e| ExtensionError::AuthFailed(e.to_string()))?;
        }
        Ok(())
    }

    /// Generate and store every auto-generated channel secret that is neither supplied nor stored.
    async fn auto_generate_channel_secrets(
        &self,
        name: &str,
        secrets: &std::collections::HashMap<String, String>,
    ) -> Result<(), ExtensionError> {
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let Ok(cap_bytes) = tokio::fs::read(&cap_path).await else {
            return Ok(());
        };
        let Ok(cap_file) = crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&cap_bytes)
        else {
            return Ok(());
        };
        for secret_def in &cap_file.setup.required_secrets {
            let Some(ref auto_gen) = secret_def.auto_generate else {
                continue;
            };
            let already_provided = secrets
                .get(&secret_def.name)
                .is_some_and(|v| !v.trim().is_empty());
            let already_stored = self
                .secrets
                .exists(&self.user_id, &secret_def.name)
                .await
                .unwrap_or(false);
            if already_provided || already_stored {
                continue;
            }
            self.store_generated_secret(name, &secret_def.name, auto_gen.length)
                .await?;
        }
        Ok(())
    }

    /// Store a random hex secret of `length` bytes for an extension.
    async fn store_generated_secret(
        &self,
        name: &str,
        secret_name: &str,
        length: usize,
    ) -> Result<(), ExtensionError> {
        use rand::RngCore;
        use rand::rngs::OsRng;
        let mut bytes = vec![0u8; length];
        OsRng.fill_bytes(&mut bytes);
        let hex_value: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let params =
            CreateSecretParams::new(secret_name, &hex_value).with_provider(name.to_string());
        self.secrets
            .create(&self.user_id, params)
            .await
            .map_err(|e| ExtensionError::AuthFailed(e.to_string()))?;
        tracing::info!(
            "Auto-generated secret '{}' for channel '{}'",
            secret_name,
            name
        );
        Ok(())
    }

    /// Activate a tool after its secrets are saved and report whether OAuth is still needed.
    async fn finish_tool_setup(&self, name: &str) -> SetupResult {
        match self.wasm_tool_activation.activate_wasm_tool(name).await {
            Ok(result) => {
                // Delete existing OAuth token so auth() starts a fresh flow.
                // Done AFTER activation succeeds to avoid losing tokens on failure.
                // This covers Reconfigure: user wants to re-auth (switch account, update creds).
                self.purge_tool_oauth_tokens(name).await;

                // Check if auth is needed (OAuth or manual token).
                // This is safe to call here — cancel-and-retry prevents port conflicts.
                let mut auth_url = None;
                if let Ok(auth_result) = self.auth(name, None).await {
                    auth_url = auth_result.auth_url().map(String::from);
                }
                let message = if auth_url.is_some() {
                    format!(
                        "Configuration saved and tool '{}' activated. Complete OAuth in your browser.",
                        name
                    )
                } else {
                    format!(
                        "Configuration saved and tool '{}' activated. {}",
                        name, result.message
                    )
                };
                SetupResult {
                    message,
                    activated: true,
                    auth_url,
                }
            }
            Err(e) => {
                tracing::debug!(
                    "Auto-activation of tool '{}' after setup failed: {}",
                    name,
                    e
                );
                SetupResult {
                    message: format!("Configuration saved for '{}'.", name),
                    activated: false,
                    auth_url: None,
                }
            }
        }
    }

    /// Hot-activate a channel after its secrets are saved and record the outcome.
    async fn finish_channel_setup(&self, name: &str) -> SetupResult {
        match self
            .wasm_channel_activation
            .activate_wasm_channel(name)
            .await
        {
            Ok(result) => {
                self.activation_errors.write().await.remove(name);
                self.broadcast_extension_status(name, "active", None).await;
                SetupResult {
                    message: format!(
                        "Configuration saved and channel '{}' activated. {}",
                        name, result.message
                    ),
                    activated: true,
                    auth_url: None,
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                tracing::warn!(
                    channel = name,
                    error = %e,
                    "Saved configuration but hot-activation failed"
                );
                self.activation_errors
                    .write()
                    .await
                    .insert(name.to_string(), error_msg.clone());
                self.broadcast_extension_status(name, "failed", Some(&error_msg))
                    .await;
                SetupResult {
                    message: format!(
                        "Configuration saved for '{}'. Activation failed: {}",
                        name, e
                    ),
                    activated: false,
                    auth_url: None,
                }
            }
        }
    }
}

/// Check a Telegram bot token against the API; an empty token is accepted unchecked.
async fn validate_telegram_bot_token(token: &str) -> Result<(), ExtensionError> {
    if token.is_empty() {
        return Ok(());
    }
    let encoded_token = url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>();
    let url = format!("https://api.telegram.org/bot{}/getMe", encoded_token);
    let resp = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| ExtensionError::Other(e.to_string()))?
        .get(&url)
        .send()
        .await
        .map_err(|e| ExtensionError::Other(format!("Failed to validate bot token: {}", e)))?;
    if !resp.status().is_success() {
        return Err(ExtensionError::Other(format!(
            "Invalid bot token (Telegram API returned {})",
            resp.status()
        )));
    }
    Ok(())
}
