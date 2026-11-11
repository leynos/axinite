//! Unix-only runtime-management wiring for startup hot reload.

use std::sync::Arc;

use axinite::{
    channels::{ChannelSecretUpdater, HttpChannelState, WebhookServer},
    config::EnvContext,
    db::SettingsStore,
    reload::HotReloadManagerFactoryContext,
    secrets::SecretsStore,
};

use crate::startup::RuntimeManagementContext;

/// Configures Unix-only runtime-management hooks such as SIGHUP-triggered
/// hot reload.
///
/// The shutdown sender must outlive the spawned signal handler so reload
/// listeners can exit cleanly during teardown.
#[cfg(unix)]
pub(crate) fn setup_runtime_management_unix(
    context: &RuntimeManagementContext<'_>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) {
    setup_sighup_reload(build_reload_manager_args(context, shutdown_tx));
}

fn build_reload_manager_args(
    context: &RuntimeManagementContext<'_>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) -> ReloadManagerArgs {
    let settings_store: Option<Arc<dyn SettingsStore>> = context
        .components
        .db
        .as_ref()
        .map(|db| Arc::clone(db) as Arc<dyn SettingsStore>);
    build_reload_manager_args_from_inputs(ReloadManagerInputs {
        env_context: context.env_context.clone(),
        settings_store,
        webhook_server: context.webhook_server.clone(),
        secrets_store: context.components.secrets_store.clone(),
        http_channel_state: context.http_channel_state.clone(),
        shutdown_tx: shutdown_tx.clone(),
    })
}

struct ReloadManagerInputs {
    env_context: EnvContext,
    settings_store: Option<Arc<dyn SettingsStore>>,
    webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    http_channel_state: Option<Arc<HttpChannelState>>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
}

/// Convert reload wiring inputs into reload manager arguments, promoting an
/// HTTP channel state into a channel secret updater when present.
fn build_reload_manager_args_from_inputs(inputs: ReloadManagerInputs) -> ReloadManagerArgs {
    let mut secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>> = Vec::new();
    if let Some(state) = inputs.http_channel_state {
        secret_updaters.push(state as Arc<dyn ChannelSecretUpdater>);
    }
    ReloadManagerArgs {
        env_context: inputs.env_context,
        settings_store: inputs.settings_store,
        webhook_server: inputs.webhook_server,
        secrets_store: inputs.secrets_store,
        secret_updaters,
        shutdown_tx: inputs.shutdown_tx,
    }
}

struct ReloadManagerArgs {
    env_context: EnvContext,
    settings_store: Option<Arc<dyn SettingsStore>>,
    webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>>,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
}

/// Creates the Unix SIGHUP hot-reload handler and registers any channel secret
/// updaters that need refresh support.
///
/// The spawned handler listens until the shared shutdown broadcast fires.
#[cfg(unix)]
fn setup_sighup_reload(args: ReloadManagerArgs) {
    let reload_manager = Arc::new(axinite::reload::create_hot_reload_manager_from_context(
        HotReloadManagerFactoryContext {
            env_context: args.env_context,
            settings_store: args.settings_store,
            webhook_server: args.webhook_server,
            secrets_store: args.secrets_store,
            secret_updaters: args.secret_updaters,
        },
    ));
    spawn_sighup_handler(reload_manager, &args.shutdown_tx);
}

/// Spawns a Tokio task that listens for `SIGHUP` and triggers a hot-reload.
///
/// The task exits cleanly when the shutdown broadcast fires. Only compiled on
/// Unix targets.
#[cfg(unix)]
pub(crate) fn spawn_sighup_handler(
    reload_manager: Arc<axinite::reload::HotReloadManager>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) {
    let mut shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to register SIGHUP handler: {e}");
                return;
            }
        };
        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::debug!("SIGHUP handler shutting down");
                    break;
                }
                _ = sighup.recv() => {
                    tracing::info!("SIGHUP received — reloading HTTP webhook config");
                    if let Err(e) = reload_manager.perform_reload().await {
                        tracing::error!("Hot-reload failed: {e}");
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests;
