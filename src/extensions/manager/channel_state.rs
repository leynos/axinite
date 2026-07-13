//! Channel runtime wiring, active-channel persistence, and auth-status checks.

use std::sync::Arc;

use crate::channels::ChannelManager;
use crate::channels::wasm::{WasmChannelRouter, WasmChannelRuntime};
use crate::db::{SettingKey, UserId};
use crate::extensions::activation::ChannelRuntimeState;
use crate::extensions::{ExtensionError, ToolAuthState};
use crate::pairing::PairingStore;
use crate::secrets::SecretsStore;

use super::ExtensionManager;

impl ExtensionManager {
    /// Get the relay config stored at startup.
    pub(super) fn relay_config(&self) -> Result<&crate::config::RelayConfig, ExtensionError> {
        self.relay_config.as_ref().ok_or_else(|| {
            ExtensionError::Config(
                "CHANNEL_RELAY_URL and CHANNEL_RELAY_API_KEY must be set".to_string(),
            )
        })
    }

    /// Generate a relay instance ID.
    pub(super) fn relay_instance_id(&self, config: &crate::config::RelayConfig) -> String {
        config.instance_id.clone().unwrap_or_else(|| {
            uuid::Uuid::new_v5(&uuid::Uuid::NAMESPACE_DNS, self.user_id.as_bytes()).to_string()
        })
    }

    /// Persist the list of active channels to the settings store.
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

    /// Broadcast an extension status change to the web UI via SSE.
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

    /// Determine the auth readiness of a WASM channel.
    pub(super) async fn check_channel_auth_status(&self, name: &str) -> ToolAuthState {
        use crate::channels::wasm::ChannelCapabilitiesFile;

        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let Ok(cap_bytes) = tokio::fs::read(&cap_path).await else {
            return ToolAuthState::NoAuth;
        };
        let Ok(cap_file) = ChannelCapabilitiesFile::from_bytes(&cap_bytes) else {
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
    /// Configure the channel runtime infrastructure for hot-activating WASM channels.
    ///
    /// Call after construction (and after wrapping in `Arc`) once the channel
    /// manager, WASM runtime, pairing store, and webhook router are available.
    /// Without this, channel activation returns an error.
    pub async fn set_channel_runtime(
        &self,
        channel_manager: Arc<ChannelManager>,
        wasm_channel_runtime: Arc<WasmChannelRuntime>,
        pairing_store: Arc<PairingStore>,
        wasm_channel_router: Arc<WasmChannelRouter>,
        wasm_channel_owner_ids: std::collections::HashMap<String, i64>,
    ) {
        // Also store the channel manager for relay channel activation.
        *self.relay_channel_manager.write().await = Some(Arc::clone(&channel_manager));
        *self.channel_runtime.write().await = Some(ChannelRuntimeState {
            channel_manager,
            wasm_channel_runtime,
            pairing_store,
            wasm_channel_router,
            wasm_channel_owner_ids,
        });
    }

    /// Set just the channel manager for relay channel hot-activation.
    ///
    /// Call this when WASM channel runtime is not available but relay channels
    /// still need to be hot-added.
    pub async fn set_relay_channel_manager(&self, channel_manager: Arc<ChannelManager>) {
        *self.relay_channel_manager.write().await = Some(channel_manager);
    }

    /// Check if a channel name corresponds to a relay extension (has stored stream token).
    pub async fn is_relay_channel(&self, name: &str) -> bool {
        self.secrets
            .exists(&self.user_id, &format!("relay:{}:stream_token", name))
            .await
            .unwrap_or(false)
    }

    /// Restore persisted relay channels after startup.
    ///
    /// Loads the persisted active channel list, filters to relay types (those with
    /// a stored stream token), and activates each via `activate_stored_relay()`.
    /// Skips channels that are already active. Call this after `set_relay_channel_manager()`.
    pub async fn restore_relay_channels(&self) {
        let persisted = self.load_persisted_active_channels().await;
        let already_active = self.active_channel_names.read().await.clone();

        for name in &persisted {
            if already_active.contains(name) {
                continue;
            }
            if !self.is_relay_channel(name).await {
                continue;
            }
            match self.activate_stored_relay(name).await {
                Ok(_) => {
                    tracing::debug!(channel = %name, "Restored persisted relay channel");
                }
                Err(e) => {
                    tracing::warn!(
                        channel = %name,
                        error = %e,
                        "Failed to restore persisted relay channel"
                    );
                }
            }
        }
    }

    /// Access the secrets store (used by OAuth callback handlers).
    pub fn secrets(&self) -> &Arc<dyn SecretsStore + Send + Sync> {
        &self.secrets
    }

    /// Register channel names that were loaded at startup.
    /// Called after WASM channels are loaded so `list()` reports accurate active status.
    pub async fn set_active_channels(&self, names: Vec<String>) {
        let mut active = self.active_channel_names.write().await;
        active.extend(names);
    }

    /// Load previously activated channel names from the settings store.
    ///
    /// Returns channel names that were activated in a prior session so they can
    /// be auto-activated at startup.
    pub async fn load_persisted_active_channels(&self) -> Vec<String> {
        let Some(ref store) = self.store else {
            return Vec::new();
        };
        match store
            .get_setting(
                UserId::from(self.user_id.as_str()),
                SettingKey::from("activated_channels"),
            )
            .await
        {
            Ok(Some(value)) => match serde_json::from_value(value) {
                Ok(names) => names,
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to deserialize activated_channels");
                    Vec::new()
                }
            },
            Ok(None) => Vec::new(),
            Err(e) => {
                tracing::warn!(error = %e, "Failed to load activated_channels setting");
                Vec::new()
            }
        }
    }

    /// Set the SSE broadcast sender for pushing extension status events to the web UI.
    pub async fn set_sse_sender(
        &self,
        sender: tokio::sync::broadcast::Sender<crate::channels::web::types::SseEvent>,
    ) {
        *self.sse_sender.write().await = Some(sender);
    }

    /// Returns the pending OAuth flow registry for sharing with the web gateway.
    ///
    /// The gateway's `/oauth/callback` handler uses this to look up pending flows
    /// by CSRF `state` parameter and complete the token exchange.
    pub fn pending_oauth_flows(&self) -> &crate::cli::oauth_defaults::PendingOAuthRegistry {
        &self.pending_oauth_flows
    }

    /// Determine the auth readiness of a WASM tool.
    pub(super) async fn check_tool_auth_status(&self, name: &str) -> ToolAuthState {
        let Some(cap_file) = self.load_tool_capabilities(name).await else {
            return ToolAuthState::NoAuth;
        };

        // If the tool declares an auth section, the access token is the
        // authoritative signal — setup secrets (client_id/secret) are
        // intermediate and may be auto-resolved via builtins.
        if let Some(ref auth) = cap_file.auth {
            let has_token = self
                .secrets
                .exists(&self.user_id, &auth.secret_name)
                .await
                .unwrap_or(false)
                || auth
                    .env_var
                    .as_ref()
                    .is_some_and(|v| std::env::var(v).is_ok());
            return if has_token {
                ToolAuthState::Ready
            } else if auth.oauth.is_some() {
                ToolAuthState::NeedsAuth
            } else {
                ToolAuthState::NeedsSetup
            };
        }

        // No auth section — fall back to checking setup.required_secrets.
        let Some(setup) = &cap_file.setup else {
            return ToolAuthState::NoAuth;
        };
        if setup.required_secrets.is_empty() {
            return ToolAuthState::NoAuth;
        }

        let all_provided = futures::future::join_all(
            setup
                .required_secrets
                .iter()
                .filter(|s| !s.optional)
                .filter(|s| !Self::is_auto_resolved_oauth_field(&s.name, &cap_file))
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
