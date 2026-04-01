//! Secret injection abstraction for hot-reload.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::secrets::{SecretError, SecretsStore};

const HTTP_WEBHOOK_SECRET_KEY: &str = "HTTP_WEBHOOK_SECRET";
const SECRETS_STORE_KEY: &str = "http_webhook_secret";

/// Boxed future used at the dyn secret-injector boundary.
pub type SecretInjectorFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), SecretError>> + Send + 'a>>;

/// Trait for injecting secrets into the environment overlay.
///
/// Implementations fetch secrets from storage and inject them
/// into the thread-safe overlay used by config loading.
pub trait SecretInjector: Send + Sync {
    /// Inject secrets into the environment overlay.
    ///
    /// Failures should be logged but not treated as fatal reload errors.
    fn inject<'a>(&'a self) -> SecretInjectorFuture<'a>;
}

/// Native async sibling trait for concrete secret-injector implementations.
pub trait NativeSecretInjector: Send + Sync {
    /// See [`SecretInjector::inject`].
    fn inject(&self) -> impl Future<Output = Result<(), SecretError>> + Send + '_;
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
    user_id: String,
}

impl DbSecretInjector {
    /// Create a new database-backed secret injector.
    ///
    /// On each hot-reload cycle, `secrets_store` is queried for secrets belonging to `user_id`.
    ///
    /// `secrets_store` — database-backed store for encrypted secrets.
    /// `user_id` — identifier of the user whose secrets should be loaded.
    pub fn new(secrets_store: Arc<dyn SecretsStore + Send + Sync>, user_id: String) -> Self {
        Self {
            secrets_store,
            user_id,
        }
    }
}

impl NativeSecretInjector for DbSecretInjector {
    async fn inject(&self) -> Result<(), SecretError> {
        self.inject_webhook_secret().await
    }
}

impl DbSecretInjector {
    /// Inject HTTP webhook secret from encrypted store.
    ///
    /// If the secret does not exist, logs and returns Ok — absence is not an error.
    async fn inject_webhook_secret(&self) -> Result<(), SecretError> {
        let result = self
            .secrets_store
            .get_decrypted(&self.user_id, SECRETS_STORE_KEY)
            .await;

        let is_missing = matches!(result, Err(SecretError::NotFound(_)));

        if is_missing {
            crate::config::remove_injected_var(HTTP_WEBHOOK_SECRET_KEY);
            tracing::debug!(
                "{HTTP_WEBHOOK_SECRET_KEY} not found in secrets store; cleared overlay entry"
            );
            return Ok(());
        }

        let webhook_secret = result?;
        // Thread-safe: Uses INJECTED_VARS mutex instead of unsafe std::env::set_var.
        // Config::from_env() will read from the overlay via optional_env().
        crate::config::inject_single_var(HTTP_WEBHOOK_SECRET_KEY, webhook_secret.expose());
        tracing::debug!("Injected {HTTP_WEBHOOK_SECRET_KEY} from secrets store");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test the HTTP_WEBHOOK_SECRET_KEY constant value.
    #[test]
    fn http_webhook_secret_key_has_correct_value() {
        assert_eq!(HTTP_WEBHOOK_SECRET_KEY, "HTTP_WEBHOOK_SECRET");
    }

    /// Test that DbSecretInjector::new has the correct signature.
    ///
    /// Verifies the constructor accepts Arc<dyn SecretsStore + Send + Sync>
    /// and a user_id String as expected.
    #[test]
    fn db_secret_injector_constructor_signature_is_valid() {
        fn _type_check_new(
            store: Arc<dyn crate::secrets::SecretsStore + Send + Sync>,
            user_id: String,
        ) -> DbSecretInjector {
            DbSecretInjector::new(store, user_id)
        }

        // The type check above ensures the constructor accepts the right types
        let _ = _type_check_new;
    }
}
