//! libSQL/Turso adapter for the endpoint-binding repository port.

use std::sync::Arc;

use chrono::Utc;
use uuid::Uuid;

use crate::db::libsql::LibSqlDatabase;
use crate::extensions::setup::{
    BindingRecord, BindingScope, BindingStoreError, NativeEndpointBindingRepository, StoredBinding,
};

/// libSQL-backed endpoint-binding storage.
///
/// Holds the shared database handle and opens a fresh connection per
/// operation, matching the other libSQL satellite stores.
pub struct LibSqlEndpointBindingRepository {
    db: Arc<LibSqlDatabase>,
}

impl LibSqlEndpointBindingRepository {
    /// Create an adapter over a shared libSQL database handle.
    pub fn new(db: Arc<LibSqlDatabase>) -> Self {
        Self { db }
    }

    async fn connect(&self) -> Result<libsql::Connection, BindingStoreError> {
        self.db
            .connect()
            .await
            .map_err(|_| BindingStoreError::Unavailable)
    }
}

impl NativeEndpointBindingRepository for LibSqlEndpointBindingRepository {
    async fn upsert<'a>(&'a self, record: &'a BindingRecord) -> Result<(), BindingStoreError> {
        let conn = self.connect().await?;
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        conn.execute(
            r#"
            INSERT INTO endpoint_bindings (
                id, user_id, extension_name, endpoint_name, endpoint_kind,
                envelope_version, validation_policy_version,
                encrypted_value, key_salt, created_at, updated_at
            )
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
            ON CONFLICT (user_id, extension_name, endpoint_name) DO UPDATE SET
                endpoint_kind = excluded.endpoint_kind,
                envelope_version = excluded.envelope_version,
                validation_policy_version = excluded.validation_policy_version,
                encrypted_value = excluded.encrypted_value,
                key_salt = excluded.key_salt,
                updated_at = excluded.updated_at
            "#,
            libsql::params![
                Uuid::new_v4().to_string(),
                record.scope.user_id(),
                record.scope.extension_name(),
                record.endpoint_name.as_str(),
                record.endpoint_kind.as_str(),
                i64::from(record.envelope_version),
                i64::from(record.validation_policy_version),
                libsql::Value::Blob(record.encrypted_value.clone()),
                libsql::Value::Blob(record.key_salt.clone()),
                now.as_str(),
            ],
        )
        .await
        .map_err(|_| BindingStoreError::Unavailable)?;

        Ok(())
    }

    async fn delete<'a>(
        &'a self,
        scope: &'a BindingScope,
        endpoint_name: &'a str,
    ) -> Result<bool, BindingStoreError> {
        let conn = self.connect().await?;
        let affected = conn
            .execute(
                r#"
                DELETE FROM endpoint_bindings
                WHERE user_id = ?1 AND extension_name = ?2 AND endpoint_name = ?3
                "#,
                libsql::params![scope.user_id(), scope.extension_name(), endpoint_name],
            )
            .await
            .map_err(|_| BindingStoreError::Unavailable)?;

        Ok(affected > 0)
    }

    async fn list<'a>(
        &'a self,
        scope: &'a BindingScope,
        limit: usize,
    ) -> Result<Vec<StoredBinding>, BindingStoreError> {
        let conn = self.connect().await?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);

        let mut rows = conn
            .query(
                r#"
                SELECT endpoint_name, endpoint_kind, envelope_version,
                       validation_policy_version, encrypted_value, key_salt
                FROM endpoint_bindings
                WHERE user_id = ?1 AND extension_name = ?2
                ORDER BY endpoint_name
                LIMIT ?3
                "#,
                libsql::params![scope.user_id(), scope.extension_name(), limit],
            )
            .await
            .map_err(|_| BindingStoreError::Unavailable)?;

        let mut bindings = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|_| BindingStoreError::Unavailable)?
        {
            bindings.push(decode_row(&row)?);
        }
        Ok(bindings)
    }

    async fn delete_scope<'a>(&'a self, scope: &'a BindingScope) -> Result<u64, BindingStoreError> {
        let conn = self.connect().await?;
        conn.execute(
            "DELETE FROM endpoint_bindings WHERE user_id = ?1 AND extension_name = ?2",
            libsql::params![scope.user_id(), scope.extension_name()],
        )
        .await
        .map_err(|_| BindingStoreError::Unavailable)
    }
}

fn decode_row(row: &libsql::Row) -> Result<StoredBinding, BindingStoreError> {
    let endpoint_name = row
        .get::<String>(0)
        .map_err(|_| BindingStoreError::Corrupt)?;
    let endpoint_kind = row
        .get::<String>(1)
        .map_err(|_| BindingStoreError::Corrupt)?;
    let envelope_version = row.get::<i64>(2).map_err(|_| BindingStoreError::Corrupt)?;
    let envelope_version =
        u16::try_from(envelope_version).map_err(|_| BindingStoreError::Corrupt)?;
    let validation_policy_version = row.get::<i64>(3).map_err(|_| BindingStoreError::Corrupt)?;
    let validation_policy_version =
        u16::try_from(validation_policy_version).map_err(|_| BindingStoreError::Corrupt)?;
    let encrypted_value = row
        .get::<Vec<u8>>(4)
        .map_err(|_| BindingStoreError::Corrupt)?;
    let key_salt = row
        .get::<Vec<u8>>(5)
        .map_err(|_| BindingStoreError::Corrupt)?;

    Ok(StoredBinding {
        endpoint_name,
        endpoint_kind,
        envelope_version,
        validation_policy_version,
        encrypted_value,
        key_salt,
    })
}
