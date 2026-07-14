//! In-memory secrets store used for testing and development.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use secrecy::ExposeSecret;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::secrets::crypto::SecretsCrypto;
use crate::secrets::store::NativeSecretsStore;
use crate::secrets::types::{CreateSecretParams, DecryptedSecret, Secret, SecretError, SecretRef};

pub struct InMemorySecretsStore {
    secrets: RwLock<HashMap<(String, String), Secret>>,
    crypto: Arc<SecretsCrypto>,
}

impl InMemorySecretsStore {
    pub fn new(crypto: Arc<SecretsCrypto>) -> Self {
        Self {
            secrets: RwLock::new(HashMap::new()),
            crypto,
        }
    }
}

impl NativeSecretsStore for InMemorySecretsStore {
    async fn create<'a>(
        &'a self,
        user_id: &'a str,
        params: CreateSecretParams,
    ) -> Result<Secret, SecretError> {
        let plaintext = params.value.expose_secret().as_bytes();
        let (encrypted_value, key_salt) = self.crypto.encrypt(plaintext)?;

        let now = Utc::now();
        let secret = Secret {
            id: Uuid::new_v4(),
            user_id: user_id.to_string(),
            name: params.name.clone(),
            encrypted_value,
            key_salt,
            provider: params.provider,
            expires_at: params.expires_at,
            last_used_at: None,
            usage_count: 0,
            created_at: now,
            updated_at: now,
        };

        self.secrets
            .write()
            .await
            .insert((user_id.to_string(), params.name), secret.clone());
        Ok(secret)
    }

    async fn get<'a>(&'a self, user_id: &'a str, name: &'a str) -> Result<Secret, SecretError> {
        let name = name.to_lowercase();
        let secret = self
            .secrets
            .read()
            .await
            .get(&(user_id.to_string(), name.clone()))
            .cloned()
            .ok_or_else(|| SecretError::NotFound(name.clone()))?;

        if let Some(expires_at) = secret.expires_at
            && expires_at < Utc::now()
        {
            return Err(SecretError::Expired);
        }

        Ok(secret)
    }

    async fn get_decrypted<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> Result<DecryptedSecret, SecretError> {
        let secret = NativeSecretsStore::get(self, user_id, name).await?;
        self.crypto
            .decrypt(&secret.encrypted_value, &secret.key_salt)
    }

    async fn exists<'a>(&'a self, user_id: &'a str, name: &'a str) -> Result<bool, SecretError> {
        Ok(self
            .secrets
            .read()
            .await
            .contains_key(&(user_id.to_string(), name.to_lowercase())))
    }

    async fn list<'a>(&'a self, user_id: &'a str) -> Result<Vec<SecretRef>, SecretError> {
        Ok(self
            .secrets
            .read()
            .await
            .iter()
            .filter(|((uid, _), _)| uid == user_id)
            .map(|((_, _), s)| SecretRef {
                name: s.name.clone(),
                provider: s.provider.clone(),
            })
            .collect())
    }

    async fn delete<'a>(&'a self, user_id: &'a str, name: &'a str) -> Result<bool, SecretError> {
        Ok(self
            .secrets
            .write()
            .await
            .remove(&(user_id.to_string(), name.to_lowercase()))
            .is_some())
    }

    async fn record_usage(&self, _secret_id: Uuid) -> Result<(), SecretError> {
        Ok(())
    }

    async fn is_accessible<'a>(
        &'a self,
        user_id: &'a str,
        secret_name: &'a str,
        allowed_secrets: &'a [String],
    ) -> Result<bool, SecretError> {
        let secret_name_lower = secret_name.to_lowercase();
        if !NativeSecretsStore::exists(self, user_id, &secret_name_lower).await? {
            return Ok(false);
        }
        for pattern in allowed_secrets {
            let pattern_lower = pattern.to_lowercase();
            if pattern_lower == secret_name_lower {
                return Ok(true);
            }
            if let Some(prefix) = pattern_lower.strip_suffix('*')
                && secret_name_lower.starts_with(prefix)
            {
                return Ok(true);
            }
        }
        Ok(false)
    }
}
