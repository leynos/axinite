//! Unix-only runtime-management wiring for startup hot reload.

use std::sync::Arc;

use ironclaw::{
    app::AppComponents,
    channels::{ChannelSecretUpdater, HttpChannelState, WebhookServer},
    secrets::SecretsStore,
};

use crate::startup::channels::spawn_sighup_handler;

/// Configures Unix-only runtime-management hooks such as SIGHUP-triggered
/// hot reload.
///
/// The shutdown sender must outlive the spawned signal handler so reload
/// listeners can exit cleanly during teardown.
#[cfg(unix)]
pub(crate) fn setup_runtime_management_unix(
    components: &AppComponents,
    webhook_server: &Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    http_channel_state: &Option<Arc<HttpChannelState>>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) {
    let sighup_settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>> = components
        .db
        .as_ref()
        .map(|db| Arc::clone(db) as Arc<dyn ironclaw::db::SettingsStore>);

    setup_sighup_reload(
        sighup_settings_store,
        webhook_server,
        components.secrets_store.clone(),
        http_channel_state,
        shutdown_tx,
    );
}

/// Creates the Unix SIGHUP hot-reload handler and registers any channel secret
/// updaters that need refresh support.
///
/// The spawned handler listens until the shared shutdown broadcast fires.
#[cfg(unix)]
fn setup_sighup_reload(
    sighup_settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>>,
    webhook_server: &Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    http_channel_state: &Option<Arc<HttpChannelState>>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) {
    let mut secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>> = Vec::new();
    if let Some(state) = http_channel_state {
        secret_updaters.push(Arc::clone(state) as Arc<dyn ChannelSecretUpdater>);
    }
    let reload_manager = Arc::new(ironclaw::reload::create_hot_reload_manager(
        sighup_settings_store.clone(),
        webhook_server.clone(),
        secrets_store,
        secret_updaters,
    ));
    spawn_sighup_handler(reload_manager, shutdown_tx);
}
