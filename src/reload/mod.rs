//! Hot-reload orchestration for configuration, listeners, and secrets.
//!
//! Separates reload policy from I/O by defining three trait boundaries:
//! - [`ConfigLoader`] — load configuration from DB or environment
//! - [`ListenerController`] — restart HTTP listeners
//! - [`SecretInjector`] — inject secrets into the environment overlay
//!
//! The [`HotReloadManager`] orchestrates these boundaries without knowing
//! implementation details, making reload logic testable via hand-rolled stubs.

pub(crate) mod config_loader;
mod error;
pub(crate) mod listener_controller;
mod manager;
pub(crate) mod secret_injector;

#[cfg(test)]
mod test_stubs;

pub use config_loader::{ConfigLoader, DbConfigLoader, EnvConfigLoader};
pub use error::ReloadError;
pub use listener_controller::{ListenerController, WebhookListenerController};
pub use manager::HotReloadManager;
pub use secret_injector::{DbSecretInjector, SecretInjector};

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::channels::{ChannelSecretUpdater, WebhookServer};
use crate::db::SettingsStore;
use crate::secrets::SecretsStore;

/// Factory function to create a HotReloadManager with default implementations.
///
/// Constructs the manager by instantiating:
/// - DbConfigLoader or EnvConfigLoader based on settings_store presence
/// - WebhookListenerController if webhook_server is provided
/// - DbSecretInjector if secrets_store is provided (with "default" user_id)
///
/// This centralizes the wiring logic and keeps main.rs free of policy details.
pub fn create_hot_reload_manager(
    settings_store: Option<Arc<dyn SettingsStore>>,
    webhook_server: Option<Arc<Mutex<WebhookServer>>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>>,
) -> HotReloadManager {
    // Instantiate config loader based on whether settings store is present
    let config_loader: Arc<dyn ConfigLoader> = match settings_store {
        Some(store) => Arc::new(DbConfigLoader::new(store, "default".to_string())),
        None => Arc::new(EnvConfigLoader::new()),
    };

    // Wrap webhook server in listener controller
    let listener_controller: Option<Arc<dyn ListenerController>> = webhook_server
        .map(|ws| Arc::new(WebhookListenerController::new(ws)) as Arc<dyn ListenerController>);

    // Wrap secrets store in secret injector with "default" user_id
    let secret_injector: Option<Arc<dyn SecretInjector>> = secrets_store.map(|ss| {
        Arc::new(DbSecretInjector::new(ss, "default".to_string())) as Arc<dyn SecretInjector>
    });

    HotReloadManager::new(
        config_loader,
        listener_controller,
        secret_injector,
        secret_updaters,
    )
}
