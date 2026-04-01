//! Live WASM channel and channel-relay activation adapter.
//!
//! Contains the full implementation for activating WASM channels and channel-relay
//! extensions, decoupled from the [`ExtensionManager`] via direct state injection.
//!
//! The port seam is in place so that tests can inject
//! [`NoOpWasmChannelActivation`](super::NoOpWasmChannelActivation) without
//! triggering real channel infrastructure.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};

use crate::channels::ChannelManager;
use crate::channels::wasm::WasmChannelRuntime;
use crate::channels::wasm::{RegisteredEndpoint, SharedWasmChannel};
use crate::extensions::activation::{ActivationFuture, WasmChannelActivationPort};
use crate::extensions::{ActivateResult, ExtensionError, ExtensionKind};
use crate::pairing::PairingStore;
use crate::secrets::SecretsStore;

/// Configuration bundle for [`LiveWasmChannelActivation`].
///
/// All fields are required for construction; production callers obtain these
/// from the same sources that previously constructed [`ExtensionManager`].
pub struct LiveWasmChannelActivationConfig {
    /// Names of WASM channels that are currently active.
    pub active_channel_names: Arc<RwLock<HashSet<String>>>,
    /// Last activation error for each WASM channel (ephemeral, cleared on success).
    pub activation_errors: Arc<RwLock<HashMap<String, String>>>,
    /// SSE broadcast sender for pushing extension status events to the web UI.
    pub sse_sender: Arc<RwLock<Option<broadcast::Sender<crate::channels::web::types::SseEvent>>>>,
    /// Directory containing installed WASM channels.
    pub wasm_channels_dir: PathBuf,
    /// Secrets store for credential injection.
    pub secrets: Arc<dyn SecretsStore + Send + Sync>,
    /// Channel runtime state for WASM channel activation (optional until set).
    pub(crate) channel_runtime: Arc<RwLock<Option<ChannelRuntimeState>>>,
    /// Channel manager for hot-adding relay channels (optional until set).
    pub relay_channel_manager: Arc<RwLock<Option<Arc<ChannelManager>>>>,
    /// Tunnel URL for webhook configuration and remote OAuth callbacks.
    pub tunnel_url: Option<String>,
    /// User identifier for namespacing secrets and configuration.
    pub user_id: String,
    /// Database store for persistence.
    pub store: Option<Arc<dyn crate::db::Database>>,
    /// Relay configuration for channel-relay extensions.
    pub relay_config: Option<crate::config::RelayConfig>,
    /// Gateway authentication token for platform token exchange proxy.
    pub gateway_token: Option<String>,
    /// Installed channel-relay extensions (no on-disk artifact, tracked in memory).
    pub installed_relay_extensions: Arc<RwLock<HashSet<String>>>,
}

/// Runtime state for WASM channel hot-activation.
///
/// This is a pub(crate) re-export from the manager module to allow
/// `LiveWasmChannelActivation` to access the channel runtime.
pub(crate) struct ChannelRuntimeState {
    pub(crate) channel_manager: Arc<ChannelManager>,
    pub(crate) wasm_channel_runtime: Arc<WasmChannelRuntime>,
    pub(crate) pairing_store: Arc<PairingStore>,
    pub(crate) wasm_channel_router: Arc<crate::channels::wasm::WasmChannelRouter>,
    pub(crate) wasm_channel_owner_ids: HashMap<String, i64>,
}

/// Live adapter that implements WASM channel and channel-relay activation.
///
/// State is injected at construction time via [`LiveWasmChannelActivationConfig`],
/// eliminating the need for post-construction wiring or weak references.
pub struct LiveWasmChannelActivation {
    active_channel_names: Arc<RwLock<HashSet<String>>>,
    activation_errors: Arc<RwLock<HashMap<String, String>>>,
    sse_sender: Arc<RwLock<Option<broadcast::Sender<crate::channels::web::types::SseEvent>>>>,
    wasm_channels_dir: PathBuf,
    secrets: Arc<dyn SecretsStore + Send + Sync>,
    channel_runtime: Arc<RwLock<Option<ChannelRuntimeState>>>,
    relay_channel_manager: Arc<RwLock<Option<Arc<ChannelManager>>>>,
    tunnel_url: Option<String>,
    user_id: String,
    store: Option<Arc<dyn crate::db::Database>>,
    relay_config: Option<crate::config::RelayConfig>,
    #[allow(dead_code)]
    gateway_token: Option<String>,
    #[allow(dead_code)]
    installed_relay_extensions: Arc<RwLock<HashSet<String>>>,
}

impl LiveWasmChannelActivation {
    /// Create a new activation adapter with the provided configuration.
    pub fn new(config: LiveWasmChannelActivationConfig) -> Self {
        Self {
            active_channel_names: config.active_channel_names,
            activation_errors: config.activation_errors,
            sse_sender: config.sse_sender,
            wasm_channels_dir: config.wasm_channels_dir,
            secrets: config.secrets,
            channel_runtime: config.channel_runtime,
            relay_channel_manager: config.relay_channel_manager,
            tunnel_url: config.tunnel_url,
            user_id: config.user_id,
            store: config.store,
            relay_config: config.relay_config,
            gateway_token: config.gateway_token,
            installed_relay_extensions: config.installed_relay_extensions,
        }
    }

    /// Activate a WASM channel at runtime without restarting.
    ///
    /// Loads the channel from its WASM file, injects credentials and config,
    /// registers it with the webhook router, and hot-adds it to the channel manager
    /// so its stream feeds into the agent loop.
    async fn activate_wasm_channel_inner(
        &self,
        name: &str,
    ) -> Result<ActivateResult, ExtensionError> {
        // If already active, re-inject credentials and refresh webhook secret.
        // Handles the case where a channel was loaded at startup before the
        // user saved secrets via the web UI.
        {
            let active = self.active_channel_names.read().await;
            if active.contains(name) {
                return self.refresh_active_channel(name).await;
            }
        }

        // Verify runtime infrastructure is available and clone Arcs so we don't
        // hold the RwLock guard across awaits.
        let (
            channel_runtime,
            channel_manager,
            pairing_store,
            wasm_channel_router,
            wasm_channel_owner_ids,
        ) = {
            let rt_guard = self.channel_runtime.read().await;
            let rt = rt_guard.as_ref().ok_or_else(|| {
                ExtensionError::ActivationFailed("WASM channel runtime not configured".to_string())
            })?;
            (
                Arc::clone(&rt.wasm_channel_runtime),
                Arc::clone(&rt.channel_manager),
                Arc::clone(&rt.pairing_store),
                Arc::clone(&rt.wasm_channel_router),
                rt.wasm_channel_owner_ids.clone(),
            )
        };

        // Check auth status first
        let auth_state = self.check_channel_auth_status(name).await;
        if auth_state != ToolAuthState::Ready && auth_state != ToolAuthState::NoAuth {
            return Err(ExtensionError::ActivationFailed(format!(
                "Channel '{}' requires configuration. Use the setup form to provide credentials.",
                name
            )));
        }

        // Load the channel from files
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
            Arc::clone(&channel_runtime),
            Arc::clone(&pairing_store),
            settings_store,
        )
        .with_secrets_store(Arc::clone(&self.secrets));
        let loaded = loader
            .load_from_files(name, &wasm_path, cap_path_option)
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        let channel_name = loaded.name().to_string();
        let webhook_secret_name = loaded.webhook_secret_name();
        let secret_header = loaded.webhook_secret_header().map(|s| s.to_string());
        let sig_key_secret_name = loaded.signature_key_secret_name();
        let hmac_secret_name = loaded.hmac_secret_name();

        // Get webhook secret from secrets store
        let webhook_secret = self
            .secrets
            .get_decrypted(&self.user_id, &webhook_secret_name)
            .await
            .ok()
            .map(|s| s.expose().to_string());

        let channel_arc = Arc::new(loaded.channel);

        // Inject runtime config (tunnel_url, webhook_secret, owner_id)
        {
            let mut config_updates = HashMap::new();

            if let Some(ref tunnel_url) = self.tunnel_url {
                config_updates.insert(
                    "tunnel_url".to_string(),
                    serde_json::Value::String(tunnel_url.clone()),
                );
            }

            if let Some(ref secret) = webhook_secret {
                config_updates.insert(
                    "webhook_secret".to_string(),
                    serde_json::Value::String(secret.clone()),
                );
            }

            if let Some(&owner_id) = wasm_channel_owner_ids.get(channel_name.as_str()) {
                config_updates.insert("owner_id".to_string(), serde_json::json!(owner_id));
            }

            if !config_updates.is_empty() {
                channel_arc.update_config(config_updates).await;
                tracing::info!(
                    channel = %channel_name,
                    has_tunnel = self.tunnel_url.is_some(),
                    has_webhook_secret = webhook_secret.is_some(),
                    "Injected runtime config into hot-activated channel"
                );
            }
        }

        // Register with webhook router
        {
            let webhook_path = format!("/webhook/{}", channel_name);
            let endpoints = vec![RegisteredEndpoint {
                channel_name: channel_name.clone(),
                path: webhook_path,
                methods: vec!["POST".to_string()],
                require_secret: webhook_secret.is_some(),
            }];

            wasm_channel_router
                .register(
                    Arc::clone(&channel_arc),
                    endpoints,
                    webhook_secret,
                    secret_header,
                )
                .await;
            tracing::info!(channel = %channel_name, "Registered hot-activated channel with webhook router");

            // Register Ed25519 signature key if declared in capabilities
            if let Some(ref sig_key_name) = sig_key_secret_name
                && let Ok(key_secret) = self
                    .secrets
                    .get_decrypted(&self.user_id, sig_key_name)
                    .await
            {
                match wasm_channel_router
                    .register_signature_key(&channel_name, key_secret.expose())
                    .await
                {
                    Ok(()) => {
                        tracing::info!(channel = %channel_name, "Registered signature key for hot-activated channel")
                    }
                    Err(e) => {
                        tracing::error!(channel = %channel_name, error = %e, "Failed to register signature key")
                    }
                }
            }

            // Register HMAC signing secret if declared in capabilities
            if let Some(hmac_name) = &hmac_secret_name {
                match self.secrets.get_decrypted(&self.user_id, hmac_name).await {
                    Ok(secret) => {
                        wasm_channel_router
                            .register_hmac_secret(&channel_name, secret.expose())
                            .await;
                        tracing::info!(channel = %channel_name, "Registered HMAC signing secret for hot-activated channel");
                    }
                    Err(e) => {
                        tracing::warn!(channel = %channel_name, error = %e, "HMAC secret not found");
                    }
                }
            }
        }

        // Inject credentials
        match inject_channel_credentials_from_secrets(
            &channel_arc,
            Some(self.secrets.as_ref()),
            &channel_name,
            &self.user_id,
        )
        .await
        {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(
                        channel = %channel_name,
                        credentials_injected = count,
                        "Credentials injected into hot-activated channel"
                    );
                }
            }
            Err(e) => {
                tracing::error!(
                    channel = %channel_name,
                    error = %e,
                    "Failed to inject credentials into hot-activated channel"
                );
            }
        }

        // Hot-add the channel to the running agent
        channel_manager
            .hot_add(Box::new(SharedWasmChannel::new(channel_arc)))
            .await
            .map_err(|e| ExtensionError::ActivationFailed(e.to_string()))?;

        // Mark as active
        self.active_channel_names
            .write()
            .await
            .insert(channel_name.clone());

        // Persist activation state so the channel auto-activates on restart
        self.persist_active_channels().await;

        tracing::info!(channel = %channel_name, "Hot-activated WASM channel");

        // Clear any previous activation error and broadcast success
        self.activation_errors.write().await.remove(&channel_name);
        self.broadcast_extension_status(&channel_name, "active", None)
            .await;

        Ok(ActivateResult {
            name: channel_name,
            kind: ExtensionKind::WasmChannel,
            tools_loaded: Vec::new(),
            message: format!("Channel '{}' activated and running", name),
        })
    }

    /// Refresh credentials and webhook secret on an already-active channel.
    ///
    /// Called when the user saves new secrets via the setup form for a channel
    /// that was loaded at startup (possibly without credentials).
    async fn refresh_active_channel(&self, name: &str) -> Result<ActivateResult, ExtensionError> {
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

        // Re-inject credentials from secrets store into the running channel
        let cred_count = match inject_channel_credentials_from_secrets(
            &existing_channel,
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
        };

        // Load capabilities file once to extract all secret names
        let cap_path = self
            .wasm_channels_dir
            .join(format!("{}.capabilities.json", name));
        let capabilities_file = match tokio::fs::read(&cap_path).await {
            Ok(bytes) => crate::channels::wasm::ChannelCapabilitiesFile::from_bytes(&bytes).ok(),
            Err(_) => None,
        };

        // Extract all secret names from the capabilities file
        let webhook_secret_name = capabilities_file
            .as_ref()
            .map(|f| f.webhook_secret_name())
            .unwrap_or_else(|| format!("{}_webhook_secret", name));

        let sig_key_secret_name = capabilities_file
            .as_ref()
            .and_then(|f| f.signature_key_secret_name());

        let hmac_secret_name = capabilities_file
            .as_ref()
            .and_then(|f| f.hmac_secret_name());

        // Refresh webhook secret
        if let Ok(secret) = self
            .secrets
            .get_decrypted(&self.user_id, &webhook_secret_name)
            .await
        {
            let secret_value = secret.expose().to_string();
            router.update_secret(name, secret_value).await;
            tracing::info!(channel = %name, "Refreshed webhook secret for active channel");
        }

        // Refresh signature key if present
        if let Some(sig_key_name) = sig_key_secret_name {
            match self
                .secrets
                .get_decrypted(&self.user_id, sig_key_name)
                .await
            {
                Ok(key_secret) => {
                    if let Err(e) = router
                        .register_signature_key(name, key_secret.expose())
                        .await
                    {
                        tracing::error!(
                            channel = %name,
                            error = %e,
                            "Failed to refresh signature key"
                        );
                    } else {
                        tracing::info!(channel = %name, "Refreshed signature key for active channel");
                    }
                }
                Err(_) => {
                    tracing::debug!(channel = %name, "Signature key secret not found, skipping refresh");
                }
            }
        }

        // Refresh HMAC secret if present
        if let Some(hmac_name) = hmac_secret_name {
            match self.secrets.get_decrypted(&self.user_id, hmac_name).await {
                Ok(hmac_secret) => {
                    router
                        .register_hmac_secret(name, hmac_secret.expose())
                        .await;
                    tracing::info!(channel = %name, "Refreshed HMAC secret for active channel");
                }
                Err(e) => {
                    tracing::warn!(channel = %name, error = %e, "HMAC secret not found");
                }
            }
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
    async fn check_channel_auth_status(&self, name: &str) -> ToolAuthState {
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

    /// Persist the list of active channels to the settings store.
    async fn persist_active_channels(&self) {
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
    async fn broadcast_extension_status(&self, name: &str, status: &str, message: Option<&str>) {
        if let Some(ref sender) = *self.sse_sender.read().await {
            let _ = sender.send(crate::channels::web::types::SseEvent::ExtensionStatus {
                extension_name: name.to_string(),
                status: status.to_string(),
                message: message.map(|m| m.to_string()),
            });
        }
    }

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
    /// Activates a channel-relay extension (e.g., Slack) using the relay service.
    async fn activate_channel_relay_inner(
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

impl Default for LiveWasmChannelActivation {
    fn default() -> Self {
        // Create default empty configuration for testing
        use crate::secrets::{InMemorySecretsStore, SecretsCrypto};
        use secrecy::SecretString;

        let master_key = SecretString::from(crate::secrets::keychain::generate_master_key_hex());
        let crypto = Arc::new(SecretsCrypto::new(master_key).expect("ephemeral crypto"));

        Self {
            active_channel_names: Arc::new(RwLock::new(HashSet::new())),
            activation_errors: Arc::new(RwLock::new(HashMap::new())),
            sse_sender: Arc::new(RwLock::new(None)),
            wasm_channels_dir: PathBuf::new(),
            secrets: Arc::new(InMemorySecretsStore::new(crypto)),
            channel_runtime: Arc::new(RwLock::new(None)),
            relay_channel_manager: Arc::new(RwLock::new(None)),
            tunnel_url: None,
            user_id: "default".to_string(),
            store: None,
            relay_config: None,
            gateway_token: None,
            installed_relay_extensions: Arc::new(RwLock::new(HashSet::new())),
        }
    }
}

impl WasmChannelActivationPort for LiveWasmChannelActivation {
    fn activate_wasm_channel<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        let name = name.to_owned();
        Box::pin(async move { self.activate_wasm_channel_inner(&name).await })
    }

    fn activate_channel_relay<'a>(&'a self, name: &'a str) -> ActivationFuture<'a> {
        let name = name.to_owned();
        Box::pin(async move { self.activate_channel_relay_inner(&name).await })
    }
}

/// Inject channel credentials from secrets store and environment variables.
///
/// Looks for secrets matching the pattern `{channel_name}_*` and injects them
/// as credential placeholders (e.g., `telegram_bot_token` -> `{TELEGRAM_BOT_TOKEN}`).
///
/// Falls back to environment variables starting with the uppercase channel name
/// prefix (e.g., `TELEGRAM_` for channel `telegram`) for missing credentials.
///
/// Returns the number of credentials injected.
async fn inject_channel_credentials_from_secrets(
    channel: &Arc<crate::channels::wasm::WasmChannel>,
    secrets: Option<&dyn SecretsStore>,
    channel_name: &str,
    user_id: &str,
) -> Result<usize, String> {
    let mut count = 0;
    let mut injected_placeholders = HashSet::new();

    // 1. Try injecting from persistent secrets store if available
    if let Some(secrets) = secrets {
        let all_secrets = secrets
            .list(user_id)
            .await
            .map_err(|e| format!("Failed to list secrets: {}", e))?;

        let prefix = format!("{}_", channel_name.to_ascii_lowercase());

        for secret_meta in all_secrets {
            if !secret_meta.name.to_ascii_lowercase().starts_with(&prefix) {
                continue;
            }

            let decrypted = match secrets.get_decrypted(user_id, &secret_meta.name).await {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(
                        secret = %secret_meta.name,
                        error = %e,
                        "Failed to decrypt secret for channel credential injection"
                    );
                    continue;
                }
            };

            let placeholder = secret_meta.name.to_uppercase();
            channel
                .set_credential(&placeholder, decrypted.expose().to_string())
                .await;
            injected_placeholders.insert(placeholder);
            count += 1;
        }
    }

    // 2. Fallback to environment variables for missing credentials
    count += inject_env_credentials(channel, channel_name, &injected_placeholders).await;

    Ok(count)
}

/// Inject missing credentials from environment variables.
///
/// Only environment variables starting with the uppercase channel name prefix
/// (e.g., `TELEGRAM_` for channel `telegram`) are considered for security.
async fn inject_env_credentials(
    channel: &Arc<crate::channels::wasm::WasmChannel>,
    channel_name: &str,
    already_injected: &HashSet<String>,
) -> usize {
    if channel_name.trim().is_empty() {
        return 0;
    }

    let caps = channel.capabilities();
    let Some(ref http_cap) = caps.tool_capabilities.http else {
        return 0;
    };

    let placeholders: Vec<String> = http_cap
        .credentials
        .values()
        .map(|m| m.secret_name.to_uppercase())
        .collect();

    let resolved = resolve_env_credentials(&placeholders, channel_name, already_injected);
    let count = resolved.len();
    for (placeholder, value) in resolved {
        channel.set_credential(&placeholder, value).await;
    }
    count
}

/// Pure helper: from a list of credential placeholder names, return those that
/// pass the channel-prefix security check and have a non-empty env var value.
///
/// Placeholders already covered by the secrets store (`already_injected`) are
/// skipped. Only names starting with `{CHANNEL_NAME}_` are allowed to prevent
/// a WASM channel from reading unrelated host credentials (e.g. `AWS_SECRET_ACCESS_KEY`).
fn resolve_env_credentials(
    placeholders: &[String],
    channel_name: &str,
    already_injected: &HashSet<String>,
) -> Vec<(String, String)> {
    if channel_name.trim().is_empty() {
        return Vec::new();
    }

    let prefix = format!("{}_", channel_name.to_ascii_uppercase());
    let mut out = Vec::new();

    for placeholder in placeholders {
        if already_injected.contains(placeholder) {
            continue;
        }
        if !placeholder.starts_with(&prefix) {
            tracing::warn!(
                channel = %channel_name,
                placeholder = %placeholder,
                "Ignoring non-prefixed credential placeholder in environment fallback"
            );
            continue;
        }
        if let Ok(value) = std::env::var(placeholder)
            && !value.is_empty()
        {
            out.push((placeholder.clone(), value));
        }
    }
    out
}

/// Auth readiness states for WASM channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolAuthState {
    /// No authentication required.
    NoAuth,
    /// All required secrets are present.
    Ready,
    /// Missing required secrets.
    NeedsSetup,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_security_prefix_check() {
        // Placeholders that don't start with the channel prefix must be rejected.
        // All env var names are prefixed with ICTEST1_ to avoid CI collisions.
        let placeholders = vec![
            "ICTEST1_BOT_TOKEN".to_string(), // valid: matches channel prefix
            "ICTEST2_TOKEN".to_string(),     // invalid: wrong channel prefix
            "ICTEST1_UNRELATED_OTHER".to_string(), // valid prefix, but env var not set — not injected
        ];
        let already_injected = std::collections::HashSet::new();

        unsafe { std::env::set_var("ICTEST1_BOT_TOKEN", "good-secret") };
        unsafe { std::env::set_var("ICTEST2_TOKEN", "bad-secret") };
        // ICTEST1_UNRELATED_OTHER intentionally not set — tests both prefix rejection and absence

        let resolved = resolve_env_credentials(&placeholders, "ictest1", &already_injected);

        // Only ICTEST1_BOT_TOKEN passes the prefix check for channel "ictest1"
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].0, "ICTEST1_BOT_TOKEN");
        assert_eq!(resolved[0].1, "good-secret");

        unsafe { std::env::remove_var("ICTEST1_BOT_TOKEN") };
        unsafe { std::env::remove_var("ICTEST2_TOKEN") };
    }

    #[test]
    fn test_already_injected_skipped() {
        // Use unique env var names (ictest3_*) to avoid interference with other tests.
        let placeholders = vec!["ICTEST3_TOKEN".to_string()];
        let mut already_injected = std::collections::HashSet::new();
        already_injected.insert("ICTEST3_TOKEN".to_string());

        unsafe { std::env::set_var("ICTEST3_TOKEN", "secret") };

        let resolved = resolve_env_credentials(&placeholders, "ictest3", &already_injected);

        // Already covered by secrets store — env var must be skipped
        assert!(resolved.is_empty());

        unsafe { std::env::remove_var("ICTEST3_TOKEN") };
    }

    #[test]
    fn test_missing_env_var_not_injected() {
        // Use unique env var names (ictest4_*) to avoid interference with other tests.
        let placeholders = vec!["ICTEST4_TOKEN".to_string()];
        let already_injected = std::collections::HashSet::new();

        unsafe { std::env::remove_var("ICTEST4_TOKEN") };

        let resolved = resolve_env_credentials(&placeholders, "ictest4", &already_injected);

        assert!(resolved.is_empty());
    }

    #[test]
    fn test_empty_env_var_not_injected() {
        // An env var that exists but is empty must not be injected.
        // Use unique env var names (ictest5_*) to avoid interference with other tests.
        let placeholders = vec!["ICTEST5_TOKEN".to_string()];
        let already_injected = std::collections::HashSet::new();

        unsafe { std::env::set_var("ICTEST5_TOKEN", "") };

        let resolved = resolve_env_credentials(&placeholders, "ictest5", &already_injected);

        assert!(resolved.is_empty());

        unsafe { std::env::remove_var("ICTEST5_TOKEN") };
    }

    #[test]
    fn test_empty_channel_name_returns_nothing() {
        // An empty channel name must never match any env var (prefix would be "_").
        let placeholders = vec!["_TOKEN".to_string(), "ICTEST6_TOKEN".to_string()];
        let already_injected = std::collections::HashSet::new();

        unsafe { std::env::set_var("_TOKEN", "bad") };
        unsafe { std::env::set_var("ICTEST6_TOKEN", "bad") };

        let resolved = resolve_env_credentials(&placeholders, "", &already_injected);

        assert!(resolved.is_empty(), "empty channel name must match nothing");

        unsafe { std::env::remove_var("_TOKEN") };
        unsafe { std::env::remove_var("ICTEST6_TOKEN") };
    }
}
