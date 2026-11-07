//! Regression tests for the reload-to-channel webhook-secret boundary.
//!
//! These drive the production [`DbSecretInjector`] through a full
//! `perform_reload` cycle and assert what the manager hands to the live
//! channel instances. The invariant under test is that a webhook secret the
//! operator deleted never returns to a channel, even when the startup
//! snapshot still contains it or a later store read fails.

use std::collections::VecDeque;
use std::sync::Arc;

use secrecy::SecretString;
use tokio::sync::Mutex;

use super::super::*;
use super::common::{http_config, test_config_with_http};
use crate::channels::ChannelSecretUpdater;
use crate::config::{Config, EnvContext};
use crate::db::UserId;
use crate::error::ConfigError;
use crate::reload::config_loader::NativeConfigLoader;
use crate::reload::secret_injector::DbSecretInjector;
use crate::reload::test_stubs::SpySecretUpdater;
use crate::secrets::{
    CreateSecretParams, DecryptedSecret, NativeSecretsStore, Secret, SecretError, SecretRef,
};

/// Snapshot key the production injector writes the store value into.
const WEBHOOK_SECRET_KEY: &str = "HTTP_WEBHOOK_SECRET";

/// Config loader that resolves the webhook secret from the reload snapshot.
///
/// This mirrors the production resolution rule for `channels.http`: the
/// credential is read from the snapshot the injector populated for this
/// cycle, so a cleared snapshot entry yields a channel with no secret.
struct SnapshotConfigLoader {
    template: Config,
}

impl NativeConfigLoader for SnapshotConfigLoader {
    async fn load(&self, ctx: EnvContext) -> Result<Config, ConfigError> {
        let mut config = self.template.clone();
        if let Some(http) = config.channels.http.as_mut() {
            http.webhook_secret = ctx
                .get(WEBHOOK_SECRET_KEY)
                .map(|value| SecretString::from(value.to_string()));
        }
        Ok(config)
    }
}

/// Secrets store whose `get_decrypted` result is scripted per reload cycle.
struct ScriptedSecretsStore {
    results: Mutex<VecDeque<Result<DecryptedSecret, SecretError>>>,
}

impl ScriptedSecretsStore {
    fn new(results: Vec<Result<DecryptedSecret, SecretError>>) -> Self {
        Self {
            results: Mutex::new(results.into()),
        }
    }
}

/// An error for the store operations this regression never exercises.
fn unused_operation() -> SecretError {
    SecretError::Database("operation not used by this test".to_string())
}

impl NativeSecretsStore for ScriptedSecretsStore {
    async fn create(
        &self,
        _user_id: &str,
        _params: CreateSecretParams,
    ) -> Result<Secret, SecretError> {
        Err(unused_operation())
    }

    async fn get(&self, _user_id: &str, _name: &str) -> Result<Secret, SecretError> {
        Err(unused_operation())
    }

    async fn get_decrypted(
        &self,
        _user_id: &str,
        _name: &str,
    ) -> Result<DecryptedSecret, SecretError> {
        self.results
            .lock()
            .await
            .pop_front()
            .unwrap_or_else(|| Err(SecretError::Database("script exhausted".to_string())))
    }

    async fn exists(&self, _user_id: &str, _name: &str) -> Result<bool, SecretError> {
        Err(unused_operation())
    }

    async fn list(&self, _user_id: &str) -> Result<Vec<SecretRef>, SecretError> {
        Err(unused_operation())
    }

    async fn delete(&self, _user_id: &str, _name: &str) -> Result<bool, SecretError> {
        Err(unused_operation())
    }

    async fn record_usage(&self, _secret_id: uuid::Uuid) -> Result<(), SecretError> {
        Err(unused_operation())
    }

    async fn is_accessible(
        &self,
        _user_id: &str,
        _secret_name: &str,
        _allowed_secrets: &[String],
    ) -> Result<bool, SecretError> {
        Err(unused_operation())
    }
}

fn decrypted(value: &str) -> DecryptedSecret {
    DecryptedSecret::from_bytes(value.as_bytes().to_vec()).expect("valid UTF-8 secret bytes")
}

fn not_found() -> SecretError {
    SecretError::NotFound("http_webhook_secret".to_string())
}

/// Build a manager whose injector reads from `store` and whose snapshot
/// resolver publishes `channels.http` to `updater`.
fn manager_with_store(
    config: Config,
    env_context: EnvContext,
    store: Arc<ScriptedSecretsStore>,
    updater: &Arc<SpySecretUpdater>,
) -> HotReloadManager {
    HotReloadManager::new_with_context(HotReloadManagerDependencies {
        env_context,
        config_loader: Arc::new(SnapshotConfigLoader { template: config }) as Arc<dyn ConfigLoader>,
        listener_controller: None,
        secret_injector: Some(
            Arc::new(DbSecretInjector::new(store, UserId::from("test_user")))
                as Arc<dyn SecretInjector>,
        ),
        secret_updaters: vec![updater.clone() as Arc<dyn ChannelSecretUpdater>],
    })
}

/// A secret deleted from the store must not be restored from the startup
/// snapshot that still contains it.
#[tokio::test]
async fn deleted_webhook_secret_is_not_restored_from_the_startup_snapshot()
-> Result<(), Box<dyn std::error::Error>> {
    let (_temp_dir, config) =
        test_config_with_http(Some(http_config("127.0.0.1", 8080, Some("startup-secret")))).await?;
    let store = Arc::new(ScriptedSecretsStore::new(vec![Err(not_found())]));
    let updater = Arc::new(SpySecretUpdater::new());
    let manager = manager_with_store(
        config,
        EnvContext::default().with_secret(WEBHOOK_SECRET_KEY, "startup-secret"),
        store,
        &updater,
    );

    manager.perform_reload().await?;

    assert_eq!(
        updater.last_secret().await,
        Some(None),
        "a confirmed deletion must reach the channel as no secret"
    );
    Ok(())
}

/// A store error after a confirmed deletion must keep the secret deleted
/// instead of re-applying the last successfully read value.
#[tokio::test]
async fn confirmed_deletion_survives_a_later_store_error() -> Result<(), Box<dyn std::error::Error>>
{
    let (_temp_dir, config) =
        test_config_with_http(Some(http_config("127.0.0.1", 8080, Some("startup-secret")))).await?;
    let store = Arc::new(ScriptedSecretsStore::new(vec![
        Ok(decrypted("live-secret")),
        Err(not_found()),
        Err(SecretError::Database("transient failure".to_string())),
    ]));
    let updater = Arc::new(SpySecretUpdater::new());
    let manager = manager_with_store(
        config,
        EnvContext::default().with_secret(WEBHOOK_SECRET_KEY, "startup-secret"),
        store,
        &updater,
    );

    manager.perform_reload().await?;
    assert_eq!(
        updater.last_secret().await,
        Some(Some("live-secret".to_string())),
        "the store value must reach the channel"
    );

    manager.perform_reload().await?;
    assert_eq!(
        updater.last_secret().await,
        Some(None),
        "the deletion must reach the channel"
    );

    manager.perform_reload().await?;

    assert_eq!(updater.call_count().await, 3, "every cycle must publish");
    assert_eq!(
        updater.last_secret().await,
        Some(None),
        "a later store error must not resurrect the deleted secret"
    );
    Ok(())
}
