//! WASM channel setup and credential injection.
//!
//! Encapsulates the logic for loading WASM channels, registering their
//! webhook routes, and injecting credentials from the secrets store.

use std::collections::HashMap;
use std::sync::Arc;

pub use super::credentials::{inject_channel_credentials, inject_channel_credentials_from_context};
use crate::channels::wasm::{
    LoadedChannel, RegisteredEndpoint, SharedWasmChannel, WasmChannelLoader, WasmChannelRouter,
    WasmChannelRuntime, WasmChannelRuntimeConfig, create_wasm_channel_router,
};
use crate::config::{Config, EnvContext};
use crate::db::Database;
use crate::extensions::ExtensionManager;
use crate::pairing::PairingStore;
use crate::secrets::SecretsStore;

/// Result of WASM channel setup.
pub struct WasmChannelSetup {
    pub channels: Vec<(String, Box<dyn crate::channels::Channel>)>,
    pub channel_names: Vec<String>,
    pub webhook_routes: Option<axum::Router>,
    /// Runtime objects needed for hot-activation via ExtensionManager.
    pub wasm_channel_runtime: Arc<WasmChannelRuntime>,
    pub pairing_store: Arc<PairingStore>,
    pub wasm_channel_router: Arc<WasmChannelRouter>,
}
/// Inputs for loading WASM channels with an explicit environment snapshot.
pub struct WasmChannelSetupRequest<'a> {
    pub config: &'a Config,
    pub secrets_store: &'a Option<Arc<dyn SecretsStore + Send + Sync>>,
    pub extension_manager: Option<&'a Arc<ExtensionManager>>,
    pub database: Option<&'a Arc<dyn Database>>,
    pub env_context: &'a EnvContext,
}

/// Load WASM channels and register their webhook routes.
pub async fn setup_wasm_channels(
    config: &Config,
    secrets_store: &Option<Arc<dyn SecretsStore + Send + Sync>>,
    extension_manager: Option<&Arc<ExtensionManager>>,
    database: Option<&Arc<dyn Database>>,
) -> Option<WasmChannelSetup> {
    let env_context = EnvContext::capture_ambient();
    setup_wasm_channels_from_context(WasmChannelSetupRequest {
        config,
        secrets_store,
        extension_manager,
        database,
        env_context: &env_context,
    })
    .await
}

/// Load WASM channels using the supplied environment snapshot for credential
/// fallback.
pub async fn setup_wasm_channels_from_context(
    request: WasmChannelSetupRequest<'_>,
) -> Option<WasmChannelSetup> {
    let runtime = match WasmChannelRuntime::new(WasmChannelRuntimeConfig::default()) {
        Ok(r) => Arc::new(r),
        Err(e) => {
            tracing::warn!("Failed to initialize WASM channel runtime: {}", e);
            return None;
        }
    };

    let pairing_store = Arc::new(PairingStore::new());
    let settings_store: Option<Arc<dyn crate::db::SettingsStore>> = request
        .database
        .map(|db| Arc::clone(db) as Arc<dyn crate::db::SettingsStore>);
    let mut loader = WasmChannelLoader::new(
        Arc::clone(&runtime),
        Arc::clone(&pairing_store),
        settings_store,
    );
    if let Some(secrets) = request.secrets_store {
        loader = loader.with_secrets_store(Arc::clone(secrets));
    }

    let results = match loader
        .load_from_dir(&request.config.channels.wasm_channels_dir)
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("Failed to scan WASM channels directory: {}", e);
            return None;
        }
    };

    let wasm_router = Arc::new(WasmChannelRouter::new());
    let mut channels: Vec<(String, Box<dyn crate::channels::Channel>)> = Vec::new();
    let mut channel_names: Vec<String> = Vec::new();

    for loaded in results.loaded {
        let (name, channel) = register_channel(loaded, &request, &wasm_router).await;
        channel_names.push(name.clone());
        channels.push((name, channel));
    }

    for (path, err) in &results.errors {
        tracing::warn!("Failed to load WASM channel {}: {}", path.display(), err);
    }

    // Always create webhook routes (even with no channels loaded) so that
    // channels hot-added at runtime can receive webhooks without a restart.
    let webhook_routes = {
        Some(create_wasm_channel_router(
            Arc::clone(&wasm_router),
            request.extension_manager.map(Arc::clone),
        ))
    };

    Some(WasmChannelSetup {
        channels,
        channel_names,
        webhook_routes,
        wasm_channel_runtime: runtime,
        pairing_store,
        wasm_channel_router: wasm_router,
    })
}

/// Process a single loaded WASM channel: retrieve secrets, inject config,
/// register with the router, and set up signing keys and credentials.
async fn register_channel(
    loaded: LoadedChannel,
    request: &WasmChannelSetupRequest<'_>,
    wasm_router: &Arc<WasmChannelRouter>,
) -> (String, Box<dyn crate::channels::Channel>) {
    let channel_name = loaded.name().to_string();
    tracing::info!("Loaded WASM channel: {}", channel_name);

    let secret_name = loaded.webhook_secret_name();
    let sig_key_secret_name = loaded.signature_key_secret_name();
    let hmac_secret_name = loaded.hmac_secret_name();

    let webhook_secret = if let Some(secrets) = request.secrets_store {
        secrets
            .get_decrypted("default", &secret_name)
            .await
            .ok()
            .map(|s| s.expose().to_string())
    } else {
        None
    };

    let secret_header = loaded.webhook_secret_header().map(|s| s.to_string());

    let webhook_path = format!("/webhook/{}", channel_name);
    let endpoints = vec![RegisteredEndpoint {
        channel_name: channel_name.clone(),
        path: webhook_path,
        methods: vec!["POST".to_string()],
        require_secret: webhook_secret.is_some(),
    }];

    let channel_arc = Arc::new(loaded.channel);

    let config_updates =
        runtime_config_updates(request.config, &channel_name, webhook_secret.as_deref());
    if !config_updates.is_empty() {
        channel_arc.update_config(config_updates).await;
        tracing::info!(
            channel = %channel_name,
            has_tunnel = request.config.tunnel.public_url.is_some(),
            has_webhook_secret = webhook_secret.is_some(),
            "Injected runtime config into channel"
        );
    }

    tracing::info!(
        channel = %channel_name,
        has_webhook_secret = webhook_secret.is_some(),
        secret_header = ?secret_header,
        "Registering channel with router"
    );

    wasm_router
        .register(
            Arc::clone(&channel_arc),
            endpoints,
            crate::channels::wasm::router::WebhookSecrets {
                secret: webhook_secret.clone(),
                header: secret_header,
            },
        )
        .await;

    // Register Ed25519 signature key if declared in capabilities.
    if let Some(key_secret) =
        resolve_declared_secret(request.secrets_store, sig_key_secret_name.as_deref()).await
    {
        match wasm_router
            .register_signature_key(&channel_name, key_secret.expose())
            .await
        {
            Ok(()) => {
                tracing::info!(channel = %channel_name, "Registered Ed25519 signature key")
            }
            Err(e) => {
                tracing::error!(channel = %channel_name, error = %e, "Invalid signature key in secrets store")
            }
        }
    }

    // Register HMAC signing secret if declared in capabilities.
    if let Some(secret) =
        resolve_declared_secret(request.secrets_store, hmac_secret_name.as_deref()).await
    {
        wasm_router
            .register_hmac_secret(&channel_name, secret.expose())
            .await;
        tracing::info!(channel = %channel_name, "Registered HMAC signing secret");
    }

    // Inject credentials from secrets store / environment.
    match inject_channel_credentials_from_context(
        &channel_arc,
        request
            .secrets_store
            .as_ref()
            .map(|s| s.as_ref() as &dyn SecretsStore),
        &channel_name,
        request.env_context,
    )
    .await
    {
        Ok(count) => {
            if count > 0 {
                tracing::info!(
                    channel = %channel_name,
                    credentials_injected = count,
                    "Channel credentials injected"
                );
            }
        }
        Err(e) => {
            tracing::error!(
                channel = %channel_name,
                error = %e,
                "Failed to inject channel credentials"
            );
        }
    }

    (channel_name, Box::new(SharedWasmChannel::new(channel_arc)))
}

fn runtime_config_updates(
    config: &Config,
    channel_name: &str,
    webhook_secret: Option<&str>,
) -> HashMap<String, serde_json::Value> {
    [
        config
            .tunnel
            .public_url
            .as_ref()
            .map(|url| ("tunnel_url", serde_json::Value::String(url.clone()))),
        webhook_secret.map(|secret| {
            (
                "webhook_secret",
                serde_json::Value::String(secret.to_string()),
            )
        }),
        config
            .channels
            .wasm_channel_owner_ids
            .get(channel_name)
            .map(|owner_id| ("owner_id", serde_json::json!(owner_id))),
    ]
    .into_iter()
    .flatten()
    .map(|(key, value)| (key.to_string(), value))
    .collect()
}

/// Fetch a decrypted secret when both a secret name and a store are available.
///
/// Returns `None` when the channel declares no secret, no store is configured,
/// or decryption fails; callers treat all three cases as "nothing to register".
async fn resolve_declared_secret(
    secrets_store: &Option<Arc<dyn SecretsStore + Send + Sync>>,
    secret_name: Option<&str>,
) -> Option<crate::secrets::DecryptedSecret> {
    let name = secret_name?;
    let secrets = secrets_store.as_ref()?;
    secrets.get_decrypted("default", name).await.ok()
}
