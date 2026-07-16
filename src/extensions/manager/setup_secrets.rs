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

        // Load allowed secret names from the extension's capabilities file
        let allowed: std::collections::HashSet<String> = match kind {
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
                cap_file
                    .setup
                    .required_secrets
                    .iter()
                    .map(|s| s.name.clone())
                    .collect()
            }
            ExtensionKind::WasmTool => {
                let cap_file = self.load_tool_capabilities(name).await.ok_or_else(|| {
                    ExtensionError::Other(format!("Capabilities file not found for '{}'", name))
                })?;
                match cap_file.setup {
                    Some(s) => s.required_secrets.iter().map(|s| s.name.clone()).collect(),
                    None => {
                        return Err(ExtensionError::Other(format!(
                            "Tool '{}' has no setup schema — no secrets to configure",
                            name
                        )));
                    }
                }
            }
            _ => {
                return Err(ExtensionError::Other(
                    "Setup is only supported for WASM channels and tools".to_string(),
                ));
            }
        };

        // For Telegram, validate the bot token against the API before storing it.
        // This catches bad tokens immediately (both on first setup and reconfigure),
        // before the channel activates and potentially shows as active with a bad token.
        if name == "telegram"
            && let Some(token_value) = secrets.get("telegram_bot_token")
        {
            let token = token_value.trim();
            if !token.is_empty() {
                let encoded_token =
                    url::form_urlencoded::byte_serialize(token.as_bytes()).collect::<String>();
                let url = format!("https://api.telegram.org/bot{}/getMe", encoded_token);
                let resp = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(10))
                    .build()
                    .map_err(|e| ExtensionError::Other(e.to_string()))?
                    .get(&url)
                    .send()
                    .await
                    .map_err(|e| {
                        ExtensionError::Other(format!("Failed to validate bot token: {}", e))
                    })?;
                if !resp.status().is_success() {
                    return Err(ExtensionError::Other(format!(
                        "Invalid bot token (Telegram API returned {})",
                        resp.status()
                    )));
                }
            }
        }

        // Validate and store each submitted secret
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

        // Auto-generate any missing secrets (channel-only feature)
        if kind == ExtensionKind::WasmChannel {
            let cap_path = self
                .wasm_channels_dir
                .join(format!("{}.capabilities.json", name));
            if let Ok(cap_bytes) = tokio::fs::read(&cap_path).await
                && let Ok(cap_file) =
                    crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&cap_bytes)
            {
                for secret_def in &cap_file.setup.required_secrets {
                    if let Some(ref auto_gen) = secret_def.auto_generate {
                        let already_provided = secrets
                            .get(&secret_def.name)
                            .is_some_and(|v| !v.trim().is_empty());
                        let already_stored = self
                            .secrets
                            .exists(&self.user_id, &secret_def.name)
                            .await
                            .unwrap_or(false);
                        if !already_provided && !already_stored {
                            use rand::RngCore;
                            use rand::rngs::OsRng;
                            let mut bytes = vec![0u8; auto_gen.length];
                            OsRng.fill_bytes(&mut bytes);
                            let hex_value: String =
                                bytes.iter().map(|b| format!("{b:02x}")).collect();
                            let params = CreateSecretParams::new(&secret_def.name, &hex_value)
                                .with_provider(name.to_string());
                            self.secrets
                                .create(&self.user_id, params)
                                .await
                                .map_err(|e| ExtensionError::AuthFailed(e.to_string()))?;
                            tracing::info!(
                                "Auto-generated secret '{}' for channel '{}'",
                                secret_def.name,
                                name
                            );
                        }
                    }
                }
            }
        }

        // For tools, save and attempt auto-activation, then check auth.
        if kind == ExtensionKind::WasmTool {
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
                    return Ok(SetupResult {
                        message,
                        activated: true,
                        auth_url,
                    });
                }
                Err(e) => {
                    tracing::debug!(
                        "Auto-activation of tool '{}' after setup failed: {}",
                        name,
                        e
                    );
                    return Ok(SetupResult {
                        message: format!("Configuration saved for '{}'.", name),
                        activated: false,
                        auth_url: None,
                    });
                }
            }
        }

        // Try to hot-activate the channel now that secrets are saved
        match self
            .wasm_channel_activation
            .activate_wasm_channel(name)
            .await
        {
            Ok(result) => {
                self.activation_errors.write().await.remove(name);
                self.broadcast_extension_status(name, "active", None).await;
                Ok(SetupResult {
                    message: format!(
                        "Configuration saved and channel '{}' activated. {}",
                        name, result.message
                    ),
                    activated: true,
                    auth_url: None,
                })
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
                Ok(SetupResult {
                    message: format!(
                        "Configuration saved for '{}'. Activation failed: {}",
                        name, e
                    ),
                    activated: false,
                    auth_url: None,
                })
            }
        }
    }
}
