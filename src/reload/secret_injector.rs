//! Secret injection abstraction for hot-reload.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::db::UserId;
use crate::secrets::{SecretError, SecretsStore};

const HTTP_WEBHOOK_SECRET_KEY: &str = "HTTP_WEBHOOK_SECRET";
const SECRETS_STORE_KEY: &str = "http_webhook_secret";

/// Boxed future used at the dyn secret-injector boundary.
pub type SecretInjectorFuture<'a> = Pin<Box<dyn Future<Output = ()> + Send + 'a>>;

/// Trait for injecting secrets into the environment overlay.
///
/// Implementations fetch secrets from storage and inject them
/// into the thread-safe overlay used by config loading.
pub trait SecretInjector: Send + Sync {
    /// Inject secrets into the environment overlay.
    ///
    /// Errors are logged internally and do not fail the reload.
    fn inject<'a>(&'a self) -> SecretInjectorFuture<'a>;
}

/// Native async sibling trait for concrete secret-injector implementations.
pub trait NativeSecretInjector: Send + Sync {
    /// See [`SecretInjector::inject`].
    fn inject(&self) -> impl Future<Output = ()> + Send + '_;
}

impl<T> SecretInjector for T
where
    T: NativeSecretInjector + Send + Sync,
{
    fn inject<'a>(&'a self) -> SecretInjectorFuture<'a> {
        Box::pin(NativeSecretInjector::inject(self))
    }
}

/// Secret injector that reads from a database-backed secrets store.
pub struct DbSecretInjector {
    secrets_store: Arc<dyn SecretsStore + Send + Sync>,
    user_id: UserId,
}

impl DbSecretInjector {
    /// Create a new database-backed secret injector.
    ///
    /// On each hot-reload cycle, `secrets_store` is queried for secrets belonging to `user_id`.
    ///
    /// `secrets_store` — database-backed store for encrypted secrets.
    /// `user_id` — identifier of the user whose secrets should be loaded.
    pub fn new(secrets_store: Arc<dyn SecretsStore + Send + Sync>, user_id: UserId) -> Self {
        Self {
            secrets_store,
            user_id,
        }
    }
}

impl NativeSecretInjector for DbSecretInjector {
    async fn inject(&self) {
        self.inject_webhook_secret().await;
    }
}

impl DbSecretInjector {
    /// Inject HTTP webhook secret from encrypted store.
    ///
    /// If the secret does not exist, logs and clears the overlay entry — absence is not an error.
    /// Other errors are logged but do not fail the reload.
    async fn inject_webhook_secret(&self) {
        let webhook_secret = match self
            .secrets_store
            .get_decrypted(self.user_id.as_str(), SECRETS_STORE_KEY)
            .await
        {
            Ok(s) => s,
            Err(SecretError::NotFound(_)) => {
                crate::config::remove_single_var(HTTP_WEBHOOK_SECRET_KEY);
                tracing::debug!(
                    "{HTTP_WEBHOOK_SECRET_KEY} not found in secrets store; cleared overlay entry"
                );
                return;
            }
            Err(e) => {
                tracing::error!("Failed to inject {HTTP_WEBHOOK_SECRET_KEY}: {e}");
                return;
            }
        };

        // Thread-safe: Uses INJECTED_VARS mutex instead of unsafe std::env::set_var.
        // Config::from_env() will read from the overlay via optional_env().
        crate::config::inject_single_var(HTTP_WEBHOOK_SECRET_KEY, webhook_secret.expose());
        tracing::debug!("Injected {HTTP_WEBHOOK_SECRET_KEY} from secrets store");
    }
}

#[cfg(test)]
mod tests {
    use secrecy::SecretString;
    use uuid::Uuid;

    use super::*;
    use crate::config::helpers::{EnvKey, optional_env};
    use crate::secrets::{
        CreateSecretParams, InMemorySecretsStore, NativeSecretsStore, Secret, SecretRef,
        SecretsCrypto, SecretsStore,
    };
    use crate::testing::credentials::TEST_CRYPTO_KEY;
    use crate::testing::test_utils::EnvVarsGuard;

    struct OverlayResetGuard(&'static str);

    impl Drop for OverlayResetGuard {
        fn drop(&mut self) {
            crate::config::remove_single_var(self.0);
        }
    }

    struct ErrorSecretsStore;

    impl NativeSecretsStore for ErrorSecretsStore {
        async fn create(
            &self,
            _user_id: &str,
            _params: CreateSecretParams,
        ) -> Result<Secret, SecretError> {
            panic!("create() should not be called in this test")
        }

        async fn get(&self, _user_id: &str, _name: &str) -> Result<Secret, SecretError> {
            panic!("get() should not be called in this test")
        }

        async fn get_decrypted(
            &self,
            _user_id: &str,
            _name: &str,
        ) -> Result<crate::secrets::DecryptedSecret, SecretError> {
            Err(SecretError::Database("simulated store failure".to_string()))
        }

        async fn exists(&self, _user_id: &str, _name: &str) -> Result<bool, SecretError> {
            panic!("exists() should not be called in this test")
        }

        async fn list(&self, _user_id: &str) -> Result<Vec<SecretRef>, SecretError> {
            panic!("list() should not be called in this test")
        }

        async fn delete(&self, _user_id: &str, _name: &str) -> Result<bool, SecretError> {
            panic!("delete() should not be called in this test")
        }

        async fn record_usage(&self, _secret_id: Uuid) -> Result<(), SecretError> {
            panic!("record_usage() should not be called in this test")
        }

        async fn is_accessible(
            &self,
            _user_id: &str,
            _secret_name: &str,
            _allowed_secrets: &[String],
        ) -> Result<bool, SecretError> {
            panic!("is_accessible() should not be called in this test")
        }
    }

    /// Test the HTTP_WEBHOOK_SECRET_KEY constant value.
    #[test]
    fn http_webhook_secret_key_has_correct_value() {
        assert_eq!(HTTP_WEBHOOK_SECRET_KEY, "HTTP_WEBHOOK_SECRET");
    }

    #[tokio::test]
    async fn db_secret_injector_injects_and_clears_webhook_secret() {
        let mut env_guard = EnvVarsGuard::new(&[HTTP_WEBHOOK_SECRET_KEY]);
        env_guard.remove(HTTP_WEBHOOK_SECRET_KEY);
        let _overlay_guard = OverlayResetGuard(HTTP_WEBHOOK_SECRET_KEY);
        crate::config::remove_single_var(HTTP_WEBHOOK_SECRET_KEY);

        let crypto = Arc::new(
            SecretsCrypto::new(SecretString::from(TEST_CRYPTO_KEY.to_string()))
                .expect("test crypto should initialize"),
        );
        let store: Arc<dyn SecretsStore + Send + Sync> =
            Arc::new(InMemorySecretsStore::new(crypto));
        store
            .create(
                "test_user",
                CreateSecretParams::new(SECRETS_STORE_KEY, "super-secret-value"),
            )
            .await
            .expect("secret should be created");

        let injector = DbSecretInjector::new(Arc::clone(&store), UserId::from("test_user"));

        NativeSecretInjector::inject(&injector).await;
        assert_eq!(
            optional_env(EnvKey(HTTP_WEBHOOK_SECRET_KEY)).expect("overlay lookup should succeed"),
            Some("super-secret-value".to_string()),
            "inject() should populate the overlay from the secrets store"
        );

        store
            .delete("test_user", SECRETS_STORE_KEY)
            .await
            .expect("secret should be deleted");
        env_guard.remove(HTTP_WEBHOOK_SECRET_KEY);
        NativeSecretInjector::inject(&injector).await;

        assert_eq!(
            optional_env(EnvKey(HTTP_WEBHOOK_SECRET_KEY)).expect("overlay lookup should succeed"),
            None,
            "inject() should clear the overlay when the secret is removed"
        );
    }

    #[tokio::test]
    async fn db_secret_injector_preserves_overlay_on_store_error() {
        let mut env_guard = EnvVarsGuard::new(&[HTTP_WEBHOOK_SECRET_KEY]);
        env_guard.remove(HTTP_WEBHOOK_SECRET_KEY);
        let _overlay_guard = OverlayResetGuard(HTTP_WEBHOOK_SECRET_KEY);
        crate::config::remove_single_var(HTTP_WEBHOOK_SECRET_KEY);
        crate::config::inject_single_var(HTTP_WEBHOOK_SECRET_KEY, "existing-overlay");

        let store: Arc<dyn SecretsStore + Send + Sync> = Arc::new(ErrorSecretsStore);
        let injector = DbSecretInjector::new(store, UserId::from("test_user"));

        NativeSecretInjector::inject(&injector).await;

        assert_eq!(
            optional_env(EnvKey(HTTP_WEBHOOK_SECRET_KEY)).expect("overlay lookup should succeed"),
            Some("existing-overlay".to_string()),
            "inject() should preserve the existing overlay when the store returns a non-NotFound error"
        );
    }
}
