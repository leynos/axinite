//! PostgreSQL implementation of the secrets store.

use std::sync::Arc;

use chrono::Utc;
use deadpool_postgres::Pool;
use secrecy::ExposeSecret;
use uuid::Uuid;

use crate::secrets::crypto::SecretsCrypto;
use crate::secrets::types::{CreateSecretParams, DecryptedSecret, Secret, SecretError, SecretRef};

use super::NativeSecretsStore;
use super::common::{db_err, get_decrypted_via, is_accessible_via, require_live_secret};

/// PostgreSQL implementation of SecretsStore.
pub struct PostgresSecretsStore {
    pool: Pool,
    crypto: Arc<SecretsCrypto>,
}

impl PostgresSecretsStore {
    /// Create a new store with the given database pool and crypto instance.
    pub fn new(pool: Pool, crypto: Arc<SecretsCrypto>) -> Self {
        Self { pool, crypto }
    }
}

impl NativeSecretsStore for PostgresSecretsStore {
    async fn create<'a>(
        &'a self,
        user_id: &'a str,
        params: CreateSecretParams,
    ) -> Result<Secret, SecretError> {
        let client = self.pool.get().await.map_err(db_err)?;

        // Encrypt the secret value
        let plaintext = params.value.expose_secret().as_bytes();
        let (encrypted_value, key_salt) = self.crypto.encrypt(plaintext)?;

        let id = Uuid::new_v4();
        let now = Utc::now();

        let row = client
            .query_one(
                r#"
                INSERT INTO secrets (id, user_id, name, encrypted_value, key_salt, provider, expires_at, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8)
                ON CONFLICT (user_id, name) DO UPDATE SET
                    encrypted_value = EXCLUDED.encrypted_value,
                    key_salt = EXCLUDED.key_salt,
                    provider = EXCLUDED.provider,
                    expires_at = EXCLUDED.expires_at,
                    updated_at = NOW()
                RETURNING id, user_id, name, encrypted_value, key_salt, provider, expires_at,
                          last_used_at, usage_count, created_at, updated_at
                "#,
                &[
                    &id,
                    &user_id,
                    &params.name,
                    &encrypted_value,
                    &key_salt,
                    &params.provider,
                    &params.expires_at,
                    &now,
                ],
            )
            .await
            .map_err(db_err)?;

        Ok(row_to_secret(&row))
    }

    async fn get<'a>(&'a self, user_id: &'a str, name: &'a str) -> Result<Secret, SecretError> {
        let name = name.to_lowercase();
        let client = self.pool.get().await.map_err(db_err)?;

        let row = client
            .query_opt(
                r#"
                SELECT id, user_id, name, encrypted_value, key_salt, provider, expires_at,
                       last_used_at, usage_count, created_at, updated_at
                FROM secrets
                WHERE user_id = $1 AND name = $2
                "#,
                &[&user_id, &name],
            )
            .await
            .map_err(db_err)?;

        require_live_secret(row.map(|r| row_to_secret(&r)), &name)
    }

    async fn get_decrypted<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> Result<DecryptedSecret, SecretError> {
        get_decrypted_via(self, &self.crypto, user_id, name).await
    }

    async fn exists<'a>(&'a self, user_id: &'a str, name: &'a str) -> Result<bool, SecretError> {
        let name = name.to_lowercase();
        let client = self.pool.get().await.map_err(db_err)?;

        let row = client
            .query_one(
                "SELECT EXISTS(SELECT 1 FROM secrets WHERE user_id = $1 AND name = $2)",
                &[&user_id, &name],
            )
            .await
            .map_err(db_err)?;

        Ok(row.get(0))
    }

    async fn list<'a>(&'a self, user_id: &'a str) -> Result<Vec<SecretRef>, SecretError> {
        let client = self.pool.get().await.map_err(db_err)?;

        let rows = client
            .query(
                "SELECT name, provider FROM secrets WHERE user_id = $1 ORDER BY name",
                &[&user_id],
            )
            .await
            .map_err(db_err)?;

        Ok(rows
            .into_iter()
            .map(|r| SecretRef {
                name: r.get(0),
                provider: r.get(1),
            })
            .collect())
    }

    async fn delete<'a>(&'a self, user_id: &'a str, name: &'a str) -> Result<bool, SecretError> {
        let name = name.to_lowercase();
        let client = self.pool.get().await.map_err(db_err)?;

        let result = client
            .execute(
                "DELETE FROM secrets WHERE user_id = $1 AND name = $2",
                &[&user_id, &name],
            )
            .await
            .map_err(db_err)?;

        Ok(result > 0)
    }

    async fn record_usage(&self, secret_id: Uuid) -> Result<(), SecretError> {
        let client = self.pool.get().await.map_err(db_err)?;

        client
            .execute(
                r#"
                UPDATE secrets
                SET last_used_at = NOW(), usage_count = usage_count + 1
                WHERE id = $1
                "#,
                &[&secret_id],
            )
            .await
            .map_err(db_err)?;

        Ok(())
    }

    async fn is_accessible<'a>(
        &'a self,
        user_id: &'a str,
        secret_name: &'a str,
        allowed_secrets: &'a [String],
    ) -> Result<bool, SecretError> {
        is_accessible_via(self, user_id, secret_name, allowed_secrets).await
    }
}

fn row_to_secret(row: &tokio_postgres::Row) -> Secret {
    Secret {
        id: row.get("id"),
        user_id: row.get("user_id"),
        name: row.get("name"),
        encrypted_value: row.get("encrypted_value"),
        key_salt: row.get("key_salt"),
        provider: row.get("provider"),
        expires_at: row.get("expires_at"),
        last_used_at: row.get("last_used_at"),
        usage_count: row.get("usage_count"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}
