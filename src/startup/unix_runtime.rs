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

    let ReloadManagerArgs {
        settings_store,
        webhook_server,
        secrets_store,
        secret_updaters,
    } = build_reload_manager_args(
        sighup_settings_store,
        webhook_server.clone(),
        components.secrets_store.clone(),
        http_channel_state.clone(),
    );

    setup_sighup_reload(
        settings_store,
        &webhook_server,
        secrets_store,
        &secret_updaters,
        shutdown_tx,
    );
}

fn build_reload_manager_args(
    settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>>,
    webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    http_channel_state: Option<Arc<HttpChannelState>>,
) -> ReloadManagerArgs {
    let mut secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>> = Vec::new();
    if let Some(state) = http_channel_state {
        secret_updaters.push(state as Arc<dyn ChannelSecretUpdater>);
    }
    ReloadManagerArgs {
        settings_store,
        webhook_server,
        secrets_store,
        secret_updaters,
    }
}

struct ReloadManagerArgs {
    settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>>,
    webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    secrets_store: Option<Arc<dyn SecretsStore + Send + Sync>>,
    secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>>,
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
    secret_updaters: &[Arc<dyn ChannelSecretUpdater>],
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) {
    let reload_manager = Arc::new(ironclaw::reload::create_hot_reload_manager(
        sighup_settings_store.clone(),
        webhook_server.clone(),
        secrets_store,
        secret_updaters.to_vec(),
    ));
    spawn_sighup_handler(reload_manager, shutdown_tx);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use ironclaw::{
        channels::{HttpChannel, HttpChannelState, WebhookServer, WebhookServerConfig},
        config::HttpConfig,
        db::{NativeSettingsStore, SettingKey, SettingsStore, UserId},
        history::SettingRow,
        secrets::{InMemorySecretsStore, SecretsCrypto, SecretsStore},
    };
    use rstest::rstest;
    use secrecy::SecretString;

    use super::{ReloadManagerArgs, build_reload_manager_args};

    struct DummySettingsStore;

    impl NativeSettingsStore for DummySettingsStore {
        async fn get_setting(
            &self,
            _user_id: UserId,
            _key: SettingKey,
        ) -> Result<Option<serde_json::Value>, ironclaw::error::DatabaseError> {
            Ok(None)
        }

        async fn get_setting_full(
            &self,
            _user_id: UserId,
            _key: SettingKey,
        ) -> Result<Option<SettingRow>, ironclaw::error::DatabaseError> {
            Ok(None)
        }

        async fn set_setting(
            &self,
            _user_id: UserId,
            _key: SettingKey,
            _value: &serde_json::Value,
        ) -> Result<(), ironclaw::error::DatabaseError> {
            Ok(())
        }

        async fn delete_setting(
            &self,
            _user_id: UserId,
            _key: SettingKey,
        ) -> Result<bool, ironclaw::error::DatabaseError> {
            Ok(false)
        }

        async fn list_settings(
            &self,
            _user_id: UserId,
        ) -> Result<Vec<SettingRow>, ironclaw::error::DatabaseError> {
            Ok(vec![])
        }

        async fn get_all_settings(
            &self,
            _user_id: UserId,
        ) -> Result<HashMap<String, serde_json::Value>, ironclaw::error::DatabaseError> {
            Ok(HashMap::new())
        }

        async fn set_all_settings(
            &self,
            _user_id: UserId,
            _settings: &HashMap<String, serde_json::Value>,
        ) -> Result<(), ironclaw::error::DatabaseError> {
            Ok(())
        }

        async fn has_settings(
            &self,
            _user_id: UserId,
        ) -> Result<bool, ironclaw::error::DatabaseError> {
            Ok(false)
        }
    }

    fn test_http_channel_state() -> Arc<HttpChannelState> {
        HttpChannel::new(HttpConfig {
            host: "127.0.0.1".to_string(),
            port: 0,
            webhook_secret: Some(SecretString::from("secret".to_string())),
            user_id: "http".to_string(),
        })
        .shared_state()
    }

    fn test_settings_store() -> Arc<dyn SettingsStore> {
        Arc::new(DummySettingsStore)
    }

    fn test_secrets_store() -> Arc<dyn SecretsStore + Send + Sync> {
        let crypto = Arc::new(
            SecretsCrypto::new(SecretString::from(
                "0123456789abcdef0123456789abcdef".to_string(),
            ))
            .expect("valid key"),
        );
        Arc::new(InMemorySecretsStore::new(crypto))
    }

    #[rstest]
    #[case(false, false, false, false, 0)]
    #[case(true, false, false, false, 0)]
    #[case(false, true, false, false, 0)]
    #[case(false, false, true, false, 0)]
    #[case(false, false, false, true, 1)]
    #[case(true, true, true, true, 1)]
    #[case(true, false, true, true, 1)]
    #[case(true, true, false, true, 1)]
    fn build_reload_manager_args_handles_option_permutations(
        #[case] has_settings_store: bool,
        #[case] has_webhook_server: bool,
        #[case] has_secrets_store: bool,
        #[case] has_http_channel_state: bool,
        #[case] expected_updaters: usize,
    ) {
        let settings_store = has_settings_store.then(test_settings_store);
        let webhook_server = has_webhook_server.then(|| {
            Arc::new(tokio::sync::Mutex::new(WebhookServer::new(
                WebhookServerConfig {
                    addr: std::net::SocketAddr::from(([127, 0, 0, 1], 0)),
                },
            )))
        });
        let secrets_store = has_secrets_store.then(test_secrets_store);
        let http_channel_state = has_http_channel_state.then(test_http_channel_state);

        let ReloadManagerArgs {
            settings_store: settings_store_out,
            webhook_server: webhook_server_out,
            secrets_store: secrets_store_out,
            secret_updaters,
        } = build_reload_manager_args(
            settings_store,
            webhook_server,
            secrets_store,
            http_channel_state,
        );

        assert_eq!(settings_store_out.is_some(), has_settings_store);
        assert_eq!(webhook_server_out.is_some(), has_webhook_server);
        assert_eq!(secrets_store_out.is_some(), has_secrets_store);
        assert_eq!(secret_updaters.len(), expected_updaters);
    }
}
