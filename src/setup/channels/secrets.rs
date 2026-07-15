//! Setup error type, secrets storage context, and secret generation helpers.

use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};

#[cfg(feature = "postgres")]
use crate::secrets::SecretsCrypto;
use crate::secrets::{CreateSecretParams, SecretsStore};

/// Typed errors for channel setup flows.
#[derive(Debug, thiserror::Error)]
pub enum ChannelSetupError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Network(String),

    #[error("{0}")]
    Secrets(String),

    #[error("{0}")]
    Validation(String),

    #[error("Setup cancelled by user")]
    Cancelled,
}

/// Context for saving secrets during setup.
///
/// Methods here take secret names and user identifiers as plain `&str`
/// deliberately: both are free-form identifiers originating from channel
/// capability schemas and the authentication layer, with no invariants
/// beyond non-emptiness that a newtype could enforce. Secret values are
/// already typed as [`SecretString`].
pub struct SecretsContext {
    store: Arc<dyn SecretsStore>,
    user_id: String,
}

impl SecretsContext {
    /// Create a new secrets context from a trait-object store.
    pub fn from_store(store: Arc<dyn SecretsStore>, user_id: &str) -> Self {
        Self {
            store,
            user_id: user_id.to_string(),
        }
    }

    /// Create a new secrets context from a PostgreSQL pool and crypto.
    #[cfg(feature = "postgres")]
    pub fn new(pool: deadpool_postgres::Pool, crypto: Arc<SecretsCrypto>, user_id: &str) -> Self {
        Self {
            store: Arc::new(crate::secrets::PostgresSecretsStore::new(pool, crypto)),
            user_id: user_id.to_string(),
        }
    }

    /// Save a secret to the database.
    pub async fn save_secret(
        &self,
        name: &str,
        value: &SecretString,
    ) -> Result<(), ChannelSetupError> {
        let params = CreateSecretParams::new(name, value.expose_secret());

        self.store
            .create(&self.user_id, params)
            .await
            .map_err(|e| ChannelSetupError::Secrets(format!("Failed to save secret: {}", e)))?;

        Ok(())
    }

    /// Check if a secret exists.
    pub async fn secret_exists(&self, name: &str) -> bool {
        match self.store.exists(&self.user_id, name).await {
            Ok(exists) => exists,
            Err(e) => {
                tracing::warn!(secret = name, error = %e, "Failed to check if secret exists, assuming absent");
                false
            }
        }
    }

    /// Read a secret from the database (decrypted).
    pub async fn get_secret(&self, name: &str) -> Result<SecretString, ChannelSetupError> {
        let decrypted = self
            .store
            .get_decrypted(&self.user_id, name)
            .await
            .map_err(|e| ChannelSetupError::Secrets(format!("Failed to read secret: {}", e)))?;
        Ok(SecretString::from(decrypted.expose().to_string()))
    }

    /// Delete a secret from the database.
    pub async fn delete_secret(&self, name: &str) -> Result<bool, ChannelSetupError> {
        self.store
            .delete(&self.user_id, name)
            .await
            .map_err(|e| ChannelSetupError::Secrets(format!("Failed to delete secret: {}", e)))
    }
}

/// Generate a random webhook secret.
pub fn generate_webhook_secret() -> String {
    generate_secret_with_length(32)
}

/// Generate a random secret of specified length (in bytes).
pub(super) fn generate_secret_with_length(length: usize) -> String {
    use rand::RngCore;
    use rand::rngs::OsRng;
    let mut bytes = vec![0u8; length];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
