//! PostgreSQL adapter for the endpoint-binding repository port.

use deadpool_postgres::Pool;

use crate::extensions::setup::{
    BindingRecord, BindingScope, BindingStoreError, NativeEndpointBindingRepository, StoredBinding,
};

/// PostgreSQL-backed endpoint-binding storage.
pub struct PostgresEndpointBindingRepository {
    pool: Pool,
}

impl PostgresEndpointBindingRepository {
    /// Create an adapter over a connection pool.
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    async fn client(&self) -> Result<deadpool_postgres::Client, BindingStoreError> {
        self.pool
            .get()
            .await
            .map_err(|_| BindingStoreError::Unavailable)
    }
}

impl NativeEndpointBindingRepository for PostgresEndpointBindingRepository {
    async fn upsert<'a>(&'a self, record: &'a BindingRecord) -> Result<(), BindingStoreError> {
        let client = self.client().await?;
        let envelope_version = i32::from(record.envelope_version);
        let policy_version = i32::from(record.validation_policy_version);

        client
            .execute(
                r#"
                INSERT INTO endpoint_bindings (
                    user_id, extension_name, endpoint_name, endpoint_kind,
                    envelope_version, validation_policy_version,
                    encrypted_value, key_salt
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                ON CONFLICT (user_id, extension_name, endpoint_name) DO UPDATE SET
                    endpoint_kind = EXCLUDED.endpoint_kind,
                    envelope_version = EXCLUDED.envelope_version,
                    validation_policy_version = EXCLUDED.validation_policy_version,
                    encrypted_value = EXCLUDED.encrypted_value,
                    key_salt = EXCLUDED.key_salt,
                    updated_at = NOW()
                "#,
                &[
                    &record.scope.user_id(),
                    &record.scope.extension_name(),
                    &record.endpoint_name.as_str(),
                    &record.endpoint_kind.as_str(),
                    &envelope_version,
                    &policy_version,
                    &record.encrypted_value,
                    &record.key_salt,
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
        let client = self.client().await?;
        let affected = client
            .execute(
                r#"
                DELETE FROM endpoint_bindings
                WHERE user_id = $1 AND extension_name = $2 AND endpoint_name = $3
                "#,
                &[&scope.user_id(), &scope.extension_name(), &endpoint_name],
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
        let client = self.client().await?;
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);

        let rows = client
            .query(
                r#"
                SELECT endpoint_name, endpoint_kind, envelope_version,
                       validation_policy_version, encrypted_value, key_salt
                FROM endpoint_bindings
                WHERE user_id = $1 AND extension_name = $2
                ORDER BY endpoint_name
                LIMIT $3
                "#,
                &[&scope.user_id(), &scope.extension_name(), &limit],
            )
            .await
            .map_err(|_| BindingStoreError::Unavailable)?;

        rows.iter().map(decode_row).collect()
    }

    async fn delete_scope<'a>(&'a self, scope: &'a BindingScope) -> Result<u64, BindingStoreError> {
        let client = self.client().await?;
        client
            .execute(
                "DELETE FROM endpoint_bindings WHERE user_id = $1 AND extension_name = $2",
                &[&scope.user_id(), &scope.extension_name()],
            )
            .await
            .map_err(|_| BindingStoreError::Unavailable)
    }
}

fn decode_row(row: &tokio_postgres::Row) -> Result<StoredBinding, BindingStoreError> {
    let endpoint_name = row
        .try_get::<_, String>(0)
        .map_err(|_| BindingStoreError::Corrupt)?;
    let endpoint_kind = row
        .try_get::<_, String>(1)
        .map_err(|_| BindingStoreError::Corrupt)?;
    let envelope_version = row
        .try_get::<_, i32>(2)
        .map_err(|_| BindingStoreError::Corrupt)?;
    let envelope_version =
        u16::try_from(envelope_version).map_err(|_| BindingStoreError::Corrupt)?;
    let validation_policy_version = row
        .try_get::<_, i32>(3)
        .map_err(|_| BindingStoreError::Corrupt)?;
    let validation_policy_version =
        u16::try_from(validation_policy_version).map_err(|_| BindingStoreError::Corrupt)?;
    let encrypted_value = row
        .try_get::<_, Vec<u8>>(4)
        .map_err(|_| BindingStoreError::Corrupt)?;
    let key_salt = row
        .try_get::<_, Vec<u8>>(5)
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
