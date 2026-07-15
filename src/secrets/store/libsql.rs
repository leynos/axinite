//! libSQL/Turso implementation of the secrets store.

use std::sync::Arc;

use chrono::Utc;
use secrecy::ExposeSecret;
use uuid::Uuid;

use crate::secrets::crypto::SecretsCrypto;
use crate::secrets::types::{CreateSecretParams, DecryptedSecret, Secret, SecretError, SecretRef};

use super::NativeSecretsStore;
use super::common::{db_err, get_decrypted_via, is_accessible_via, require_live_secret};

// ==================== libSQL implementation ====================

/// libSQL/Turso implementation of SecretsStore.
///
/// Holds an `Arc<Database>` handle and creates a fresh connection per operation,
/// matching the connection-per-request pattern used by the main `LibSqlBackend`.
pub struct LibSqlSecretsStore {
    db: Arc<crate::db::libsql::LibSqlDatabase>,
    crypto: Arc<SecretsCrypto>,
}

impl LibSqlSecretsStore {
    /// Create a new store with the given shared libsql database handle and crypto instance.
    pub fn new(db: Arc<crate::db::libsql::LibSqlDatabase>, crypto: Arc<SecretsCrypto>) -> Self {
        Self { db, crypto }
    }

    async fn connect(&self) -> Result<libsql::Connection, SecretError> {
        let conn = self
            .db
            .connect()
            .await
            .map_err(|e| SecretError::Database(format!("Connection failed: {}", e)))?;
        Ok(conn)
    }
}

impl NativeSecretsStore for LibSqlSecretsStore {
    async fn create<'a>(
        &'a self,
        user_id: &'a str,
        params: CreateSecretParams,
    ) -> Result<Secret, SecretError> {
        let plaintext = params.value.expose_secret().as_bytes();
        let (encrypted_value, key_salt) = self.crypto.encrypt(plaintext)?;

        let id = Uuid::new_v4();
        let now = Utc::now();
        let now_str = now.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let expires_at_str = params
            .expires_at
            .map(|dt| dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true));

        // Start transaction for atomic upsert + read-back
        let conn = self.connect().await?;
        let tx = conn.transaction().await.map_err(db_err)?;

        tx.execute(
                r#"
                INSERT INTO secrets (id, user_id, name, encrypted_value, key_salt, provider, expires_at, created_at, updated_at)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                ON CONFLICT (user_id, name) DO UPDATE SET
                    encrypted_value = excluded.encrypted_value,
                    key_salt = excluded.key_salt,
                    provider = excluded.provider,
                    expires_at = excluded.expires_at,
                    updated_at = ?8
                "#,
                libsql::params![
                    id.to_string(),
                    user_id,
                    params.name.as_str(),
                    libsql::Value::Blob(encrypted_value.clone()),
                    libsql::Value::Blob(key_salt.clone()),
                    libsql_opt_text(params.provider.as_deref()),
                    libsql_opt_text(expires_at_str.as_deref()),
                    now_str.as_str(),
                ],
            )
            .await
            .map_err(db_err)?;

        // Read back the row (may have been upserted)
        let mut rows = tx
            .query(
                r#"
                SELECT id, user_id, name, encrypted_value, key_salt, provider, expires_at,
                       last_used_at, usage_count, created_at, updated_at
                FROM secrets
                WHERE user_id = ?1 AND name = ?2
                "#,
                libsql::params![user_id, params.name.as_str()],
            )
            .await
            .map_err(db_err)?;

        let row =
            rows.next().await.map_err(db_err)?.ok_or_else(|| {
                SecretError::Database("Insert succeeded but row not found".into())
            })?;

        let secret = libsql_row_to_secret(&row)?;

        tx.commit().await.map_err(db_err)?;

        Ok(secret)
    }

    async fn get<'a>(&'a self, user_id: &'a str, name: &'a str) -> Result<Secret, SecretError> {
        let name = name.to_lowercase();
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, user_id, name, encrypted_value, key_salt, provider, expires_at,
                       last_used_at, usage_count, created_at, updated_at
                FROM secrets
                WHERE user_id = ?1 AND name = ?2
                "#,
                libsql::params![user_id, name.as_str()],
            )
            .await
            .map_err(db_err)?;

        let secret = rows
            .next()
            .await
            .map_err(db_err)?
            .map(|row| libsql_row_to_secret(&row))
            .transpose()?;
        require_live_secret(secret, &name)
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
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT 1 FROM secrets WHERE user_id = ?1 AND name = ?2",
                libsql::params![user_id, name.as_str()],
            )
            .await
            .map_err(db_err)?;

        Ok(rows.next().await.map_err(db_err)?.is_some())
    }

    async fn list<'a>(&'a self, user_id: &'a str) -> Result<Vec<SecretRef>, SecretError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                "SELECT name, provider FROM secrets WHERE user_id = ?1 ORDER BY name",
                libsql::params![user_id],
            )
            .await
            .map_err(db_err)?;

        let mut refs = Vec::new();
        while let Some(row) = rows.next().await.map_err(db_err)? {
            refs.push(SecretRef {
                name: row.get::<String>(0).unwrap_or_default(),
                provider: row.get::<String>(1).ok(),
            });
        }
        Ok(refs)
    }

    async fn delete<'a>(&'a self, user_id: &'a str, name: &'a str) -> Result<bool, SecretError> {
        let name = name.to_lowercase();
        let conn = self.connect().await?;
        let affected = conn
            .execute(
                "DELETE FROM secrets WHERE user_id = ?1 AND name = ?2",
                libsql::params![user_id, name.as_str()],
            )
            .await
            .map_err(db_err)?;

        Ok(affected > 0)
    }

    async fn record_usage(&self, secret_id: Uuid) -> Result<(), SecretError> {
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let conn = self.connect().await?;

        conn.execute(
            r#"
                UPDATE secrets
                SET last_used_at = ?1, usage_count = usage_count + 1
                WHERE id = ?2
                "#,
            libsql::params![now.as_str(), secret_id.to_string()],
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

fn libsql_opt_text(s: Option<&str>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s.to_string()),
        None => libsql::Value::Null,
    }
}

fn libsql_parse_timestamp(s: &str) -> Result<chrono::DateTime<Utc>, SecretError> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(ndt.and_utc());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(ndt.and_utc());
    }
    Err(SecretError::Database(format!(
        "unparseable timestamp: {:?}",
        s
    )))
}

fn libsql_row_to_secret(row: &libsql::Row) -> Result<Secret, SecretError> {
    let id_str: String = row.get(0).map_err(db_err)?;
    let user_id: String = row.get(1).map_err(db_err)?;
    let name: String = row.get(2).map_err(db_err)?;
    let encrypted_value: Vec<u8> = row.get(3).map_err(db_err)?;
    let key_salt: Vec<u8> = row.get(4).map_err(db_err)?;
    let provider: Option<String> = row.get::<String>(5).ok().filter(|s| !s.is_empty());
    let expires_at = row
        .get::<String>(6)
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| libsql_parse_timestamp(&s).ok());
    let last_used_at = row
        .get::<String>(7)
        .ok()
        .filter(|s| !s.is_empty())
        .and_then(|s| libsql_parse_timestamp(&s).ok());
    let usage_count: i64 = row.get::<i64>(8).unwrap_or(0);
    let created_at_str: String = row.get(9).map_err(db_err)?;
    let updated_at_str: String = row.get(10).map_err(db_err)?;

    Ok(Secret {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| SecretError::Database(e.to_string()))?,
        user_id,
        name,
        encrypted_value,
        key_salt,
        provider,
        expires_at,
        last_used_at,
        usage_count,
        created_at: libsql_parse_timestamp(&created_at_str)?,
        updated_at: libsql_parse_timestamp(&updated_at_str)?,
    })
}
