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
    use std::sync::Arc;

    use mockall::automock;
    use secrecy::SecretString;

    use super::*;
    use crate::config::helpers::{EnvKey, optional_env};
    use crate::secrets::{
        CreateSecretParams, DecryptedSecret, InMemorySecretsStore, NativeSecretsStore, Secret,
        SecretError, SecretRef, SecretsCrypto, SecretsStore,
    };
    use crate::testing::credentials::TEST_CRYPTO_KEY;
    use crate::testing::test_utils::EnvVarsGuard;

    struct OverlayResetGuard(&'static str);

    impl Drop for OverlayResetGuard {
        fn drop(&mut self) {
            crate::config::remove_single_var(self.0);
        }
    }

    /// Synchronous mirror of `NativeSecretsStore` for mockall compatibility.
    ///
    /// This trait exists as a workaround for mockall's inability to mock async
    /// traits with lifetime parameters directly. `SyncSecretsStore` mirrors
    /// `NativeSecretsStore` but uses owned `String` types instead of `&str`
    /// references, allowing mockall to generate `MockSyncSecretsStore`.
    ///
    /// The `MockSecretsStore` wrapper implements `NativeSecretsStore` by
    /// delegating to the generated mock, converting `&str` to `String` at
    /// each call boundary. This keeps tests synchronous and avoids the
    /// complexity of boxed futures or the `async-trait` crate.
    #[automock]
    trait SyncSecretsStore {
        fn create(
            &self,
            user_id: String,
            params: CreateSecretParams,
        ) -> Result<Secret, SecretError>;
        fn get(&self, user_id: String, name: String) -> Result<Secret, SecretError>;
        fn get_decrypted(
            &self,
            user_id: String,
            name: String,
        ) -> Result<DecryptedSecret, SecretError>;
        fn exists(&self, user_id: String, name: String) -> Result<bool, SecretError>;
        fn list(&self, user_id: String) -> Result<Vec<SecretRef>, SecretError>;
        fn delete(&self, user_id: String, name: String) -> Result<bool, SecretError>;
        fn record_usage(&self, secret_id: uuid::Uuid) -> Result<(), SecretError>;
        fn is_accessible(
            &self,
            user_id: String,
            secret_name: String,
            allowed_secrets: Vec<String>,
        ) -> Result<bool, SecretError>;
    }

    struct MockSecretsStore {
        inner: MockSyncSecretsStore,
    }

    impl MockSecretsStore {
        fn new(inner: MockSyncSecretsStore) -> Self {
            Self { inner }
        }
    }

    impl NativeSecretsStore for MockSecretsStore {
        async fn create(
            &self,
            user_id: &str,
            params: CreateSecretParams,
        ) -> Result<Secret, SecretError> {
            self.inner.create(user_id.to_string(), params)
        }

        async fn get(&self, user_id: &str, name: &str) -> Result<Secret, SecretError> {
            self.inner.get(user_id.to_string(), name.to_string())
        }

        async fn get_decrypted(
            &self,
            user_id: &str,
            name: &str,
        ) -> Result<DecryptedSecret, SecretError> {
            self.inner
                .get_decrypted(user_id.to_string(), name.to_string())
        }

        async fn exists(&self, user_id: &str, name: &str) -> Result<bool, SecretError> {
            self.inner.exists(user_id.to_string(), name.to_string())
        }

        async fn list(&self, user_id: &str) -> Result<Vec<SecretRef>, SecretError> {
            self.inner.list(user_id.to_string())
        }

        async fn delete(&self, user_id: &str, name: &str) -> Result<bool, SecretError> {
            self.inner.delete(user_id.to_string(), name.to_string())
        }

        async fn record_usage(&self, secret_id: uuid::Uuid) -> Result<(), SecretError> {
            self.inner.record_usage(secret_id)
        }

        async fn is_accessible(
            &self,
            user_id: &str,
            secret_name: &str,
            allowed_secrets: &[String],
        ) -> Result<bool, SecretError> {
            self.inner.is_accessible(
                user_id.to_string(),
                secret_name.to_string(),
                allowed_secrets.to_vec(),
            )
        }
    }

    fn make_error_store() -> Arc<dyn SecretsStore + Send + Sync> {
        let mut mock = MockSyncSecretsStore::new();
        mock.expect_get_decrypted()
            .returning(|_, _| Err(SecretError::Database("simulated store failure".to_string())));
        Arc::new(MockSecretsStore::new(mock))
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
                .expect("test crypto should initialise"),
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

        let store: Arc<dyn SecretsStore + Send + Sync> = make_error_store();
        let injector = DbSecretInjector::new(store, UserId::from("test_user"));

        NativeSecretInjector::inject(&injector).await;

        assert_eq!(
            optional_env(EnvKey(HTTP_WEBHOOK_SECRET_KEY)).expect("overlay lookup should succeed"),
            Some("existing-overlay".to_string()),
            "inject() should preserve the existing overlay when the store returns a non-NotFound error"
        );
    }
}
