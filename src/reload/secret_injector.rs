//! Secret injection abstraction for hot-reload.

use std::sync::Arc;

use async_trait::async_trait;

use crate::secrets::{SecretError, SecretsStore};

/// Trait for injecting secrets into the environment overlay.
///
/// Implementations fetch secrets from storage and inject them
/// into the thread-safe overlay used by config loading.
#[async_trait]
pub trait SecretInjector: Send + Sync {
    /// Inject secrets into the environment overlay.
    ///
    /// Failures should be logged but not treated as fatal reload errors.
    async fn inject(&self) -> Result<(), SecretError>;
}

/// Secret injector that reads from a database-backed secrets store.
pub struct DbSecretInjector {
    secrets_store: Arc<dyn SecretsStore + Send + Sync>,
    user_id: String,
}

impl DbSecretInjector {
    pub fn new(secrets_store: Arc<dyn SecretsStore + Send + Sync>, user_id: String) -> Self {
        Self {
            secrets_store,
            user_id,
        }
    }
}

#[async_trait]
impl SecretInjector for DbSecretInjector {
    async fn inject(&self) -> Result<(), SecretError> {
        self.inject_webhook_secret().await
    }
}

impl DbSecretInjector {
    /// Inject HTTP webhook secret from encrypted store.
    ///
    /// If the secret does not exist, logs and returns Ok — absence is not an error.
    async fn inject_webhook_secret(&self) -> Result<(), SecretError> {
        match self
            .secrets_store
            .get_decrypted(&self.user_id, "http_webhook_secret")
            .await
        {
            Ok(webhook_secret) => {
                // Thread-safe: Uses INJECTED_VARS mutex instead of unsafe std::env::set_var.
                // Config::from_env() will read from the overlay via optional_env().
                crate::config::inject_single_var("HTTP_WEBHOOK_SECRET", webhook_secret.expose());
                tracing::debug!("Injected HTTP_WEBHOOK_SECRET from secrets store");
                Ok(())
            }
            Err(SecretError::NotFound(_)) => {
                crate::config::remove_injected_var("HTTP_WEBHOOK_SECRET");
                tracing::debug!(
                    "HTTP_WEBHOOK_SECRET not found in secrets store; cleared overlay entry"
                );
                Ok(())
            }
            Err(e) => Err(e),
        }
    }
}
