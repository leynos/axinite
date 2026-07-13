//! HTTP router for WASM channel webhooks.
//!
//! Routes incoming HTTP requests to the appropriate WASM channel based on
//! registered paths. Handles secret validation at the host level.

mod oauth;
mod state;
mod webhook;

#[cfg(test)]
mod tests;

pub use state::RouterState;

use std::collections::HashMap;
use std::sync::Arc;

use axum::{
    Router,
    routing::{get, post},
};
use tokio::sync::RwLock;

use crate::channels::wasm::wrapper::WasmChannel;

use oauth::oauth_callback_handler;
use state::health_handler;
use webhook::webhook_handler;

/// A registered HTTP endpoint for a WASM channel.
#[derive(Debug, Clone)]
pub struct RegisteredEndpoint {
    /// Channel name that owns this endpoint.
    pub channel_name: String,
    /// HTTP path (e.g., "/webhook/slack").
    pub path: String,
    /// Allowed HTTP methods.
    pub methods: Vec<String>,
    /// Whether secret validation is required.
    pub require_secret: bool,
}

/// Router for WASM channel HTTP endpoints.
pub struct WasmChannelRouter {
    /// Registered channels by name.
    channels: RwLock<HashMap<String, Arc<WasmChannel>>>,
    /// Path to channel mapping for fast lookup.
    path_to_channel: RwLock<HashMap<String, String>>,
    /// Expected webhook secrets by channel name.
    secrets: RwLock<HashMap<String, String>>,
    /// Webhook secret header names by channel name (e.g., "X-Telegram-Bot-Api-Secret-Token").
    secret_headers: RwLock<HashMap<String, String>>,
    /// Ed25519 public keys for signature verification by channel name (hex-encoded).
    signature_keys: RwLock<HashMap<String, String>>,
    /// HMAC-SHA256 signing secrets for signature verification by channel name (Slack-style).
    hmac_secrets: RwLock<HashMap<String, String>>,
}

impl WasmChannelRouter {
    /// Create a new router.
    pub fn new() -> Self {
        Self {
            channels: RwLock::new(HashMap::new()),
            path_to_channel: RwLock::new(HashMap::new()),
            secrets: RwLock::new(HashMap::new()),
            secret_headers: RwLock::new(HashMap::new()),
            signature_keys: RwLock::new(HashMap::new()),
            hmac_secrets: RwLock::new(HashMap::new()),
        }
    }

    /// Register a channel with its endpoints.
    ///
    /// # Arguments
    /// * `channel` - The WASM channel to register
    /// * `endpoints` - HTTP endpoints to register for this channel
    /// * `secret` - Optional webhook secret for validation
    /// * `secret_header` - Optional HTTP header name for secret validation
    ///   (e.g., "X-Telegram-Bot-Api-Secret-Token"). Defaults to "X-Webhook-Secret".
    pub async fn register(
        &self,
        channel: Arc<WasmChannel>,
        endpoints: Vec<RegisteredEndpoint>,
        secret: Option<String>,
        secret_header: Option<String>,
    ) {
        let name = channel.channel_name().to_string();

        // Store the channel
        self.channels.write().await.insert(name.clone(), channel);

        // Register path mappings
        let mut path_map = self.path_to_channel.write().await;
        for endpoint in endpoints {
            path_map.insert(endpoint.path.clone(), name.clone());
            tracing::info!(
                channel = %name,
                path = %endpoint.path,
                methods = ?endpoint.methods,
                "Registered WASM channel HTTP endpoint"
            );
        }

        // Store secret if provided
        if let Some(s) = secret {
            self.secrets.write().await.insert(name.clone(), s);
        }

        // Store secret header if provided
        if let Some(h) = secret_header {
            self.secret_headers.write().await.insert(name, h);
        }
    }

    /// Get the secret header name for a channel.
    ///
    /// Returns the configured header or "X-Webhook-Secret" as default.
    pub async fn get_secret_header(&self, channel_name: &str) -> String {
        self.secret_headers
            .read()
            .await
            .get(channel_name)
            .cloned()
            .unwrap_or_else(|| "X-Webhook-Secret".to_string())
    }

    /// Update the webhook secret for an already-registered channel.
    ///
    /// This is used when credentials are saved after a channel was registered
    /// without a secret (e.g., loaded at startup before the user configured it).
    pub async fn update_secret(&self, channel_name: &str, secret: String) {
        self.secrets
            .write()
            .await
            .insert(channel_name.to_string(), secret);
        tracing::info!(
            channel = %channel_name,
            "Updated webhook secret for channel"
        );
    }

    /// Unregister a channel and its endpoints.
    pub async fn unregister(&self, channel_name: &str) {
        self.channels.write().await.remove(channel_name);
        self.secrets.write().await.remove(channel_name);
        self.secret_headers.write().await.remove(channel_name);
        self.signature_keys.write().await.remove(channel_name);
        self.hmac_secrets.write().await.remove(channel_name);

        // Remove all paths for this channel
        self.path_to_channel
            .write()
            .await
            .retain(|_, name| name != channel_name);

        tracing::info!(
            channel = %channel_name,
            "Unregistered WASM channel"
        );
    }

    /// Get the channel for a given path.
    pub async fn get_channel_for_path(&self, path: &str) -> Option<Arc<WasmChannel>> {
        let path_map = self.path_to_channel.read().await;
        let channel_name = path_map.get(path)?;

        self.channels.read().await.get(channel_name).cloned()
    }

    /// Validate a secret for a channel.
    pub async fn validate_secret(&self, channel_name: &str, provided: &str) -> bool {
        let secrets = self.secrets.read().await;
        match secrets.get(channel_name) {
            Some(expected) => expected == provided,
            None => true, // No secret required
        }
    }

    /// Check if a channel requires a secret.
    pub async fn requires_secret(&self, channel_name: &str) -> bool {
        self.secrets.read().await.contains_key(channel_name)
    }

    /// List all registered channels.
    pub async fn list_channels(&self) -> Vec<String> {
        self.channels.read().await.keys().cloned().collect()
    }

    /// List all registered paths.
    pub async fn list_paths(&self) -> Vec<String> {
        self.path_to_channel.read().await.keys().cloned().collect()
    }

    /// Register an Ed25519 public key for signature verification.
    ///
    /// Validates that the key is valid hex encoding of a 32-byte Ed25519 public key.
    /// Channels with a registered key will have Discord-style Ed25519
    /// signature validation performed before forwarding to WASM.
    pub async fn register_signature_key(
        &self,
        channel_name: &str,
        public_key_hex: &str,
    ) -> Result<(), String> {
        use ed25519_dalek::VerifyingKey;

        let key_bytes = hex::decode(public_key_hex).map_err(|e| format!("invalid hex: {e}"))?;
        VerifyingKey::try_from(key_bytes.as_slice())
            .map_err(|e| format!("invalid Ed25519 public key: {e}"))?;

        self.signature_keys
            .write()
            .await
            .insert(channel_name.to_string(), public_key_hex.to_string());
        Ok(())
    }

    /// Get the signature verification key for a channel.
    ///
    /// Returns `None` if no key is registered (no signature check needed).
    pub async fn get_signature_key(&self, channel_name: &str) -> Option<String> {
        self.signature_keys.read().await.get(channel_name).cloned()
    }

    /// Register an HMAC-SHA256 signing secret for signature verification.
    ///
    /// Channels with a registered secret will have Slack-style HMAC-SHA256
    /// signature validation performed before forwarding to WASM.
    pub async fn register_hmac_secret(&self, channel_name: &str, secret: &str) {
        self.hmac_secrets
            .write()
            .await
            .insert(channel_name.to_string(), secret.to_string());
    }

    /// Get the HMAC signing secret for a channel.
    ///
    /// Returns `None` if no secret is registered (no HMAC check needed).
    pub async fn get_hmac_secret(&self, channel_name: &str) -> Option<String> {
        self.hmac_secrets.read().await.get(channel_name).cloned()
    }
}

impl Default for WasmChannelRouter {
    fn default() -> Self {
        Self::new()
    }
}

/// Create an Axum router for WASM channel webhooks.
///
/// This router can be merged with the existing HTTP channel router.
pub fn create_wasm_channel_router(
    router: Arc<WasmChannelRouter>,
    extension_manager: Option<Arc<crate::extensions::ExtensionManager>>,
) -> Router {
    let mut state = RouterState::new(router);
    if let Some(manager) = extension_manager {
        state = state.with_extension_manager(manager);
    }

    Router::new()
        .route("/wasm-channels/health", get(health_handler))
        .route("/oauth/callback", get(oauth_callback_handler))
        // Catch-all for webhook paths
        .route("/webhook/{*path}", get(webhook_handler))
        .route("/webhook/{*path}", post(webhook_handler))
        .with_state(state)
}
