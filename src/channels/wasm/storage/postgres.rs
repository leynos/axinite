//! PostgreSQL backend for WASM channel storage.

use chrono::Utc;
use deadpool_postgres::Pool;
use uuid::Uuid;

use crate::tools::wasm::storage::compute_binary_hash;

use super::{
    CHANNEL_COLUMNS, CHANNEL_COLUMNS_WITH_BINARY, ChannelKey, StoreChannelParams,
    StoredWasmChannel, StoredWasmChannelWithBinary, WasmChannelStore, WasmChannelStoreError,
    check_binary_integrity,
};

use super::PostgresWasmChannelStore;

impl PostgresWasmChannelStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }

    /// Fetch a pooled client, mapping pool errors to store errors.
    async fn client(&self) -> Result<deadpool_postgres::Object, WasmChannelStoreError> {
        self.pool
            .get()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))
    }
}

impl WasmChannelStore for PostgresWasmChannelStore {
    async fn store(
        &self,
        params: StoreChannelParams,
    ) -> Result<StoredWasmChannel, WasmChannelStoreError> {
        let mut client = self.client().await?;

        let binary_hash = compute_binary_hash(&params.wasm_binary);
        let id = Uuid::new_v4();
        let now = Utc::now();

        // Wrap delete + insert in a transaction for atomicity
        let tx = client
            .transaction()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        // Delete any existing version for this (user_id, name) — upgrade-in-place
        tx.execute(
            "DELETE FROM wasm_channels WHERE user_id = $1 AND name = $2",
            &[&params.user_id, &params.name],
        )
        .await
        .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        let row = tx
            .query_one(
                format!(
                    concat!(
                        "INSERT INTO wasm_channels (",
                        "  id, user_id, name, version, wit_version, description,",
                        "  wasm_binary, binary_hash,",
                        "  capabilities_json, status, created_at, updated_at",
                        ") VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'active', $10, $10) ",
                        "RETURNING {}"
                    ),
                    CHANNEL_COLUMNS
                )
                .as_str(),
                &[
                    &id,
                    &params.user_id,
                    &params.name,
                    &params.version,
                    &params.wit_version,
                    &params.description,
                    &params.wasm_binary,
                    &binary_hash,
                    &params.capabilities_json,
                    &now,
                ],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        let channel = pg_row_to_channel(&row)?;

        tx.commit()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        Ok(channel)
    }

    async fn get(&self, key: ChannelKey<'_>) -> Result<StoredWasmChannel, WasmChannelStoreError> {
        let ChannelKey { user_id, name } = key;
        let client = self.client().await?;

        let row = client
            .query_opt(
                format!(
                    "SELECT {CHANNEL_COLUMNS} FROM wasm_channels WHERE user_id = $1 AND name = $2"
                )
                .as_str(),
                &[&user_id, &name],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        match row {
            Some(r) => pg_row_to_channel(&r),
            None => Err(WasmChannelStoreError::NotFound(name.to_string())),
        }
    }

    async fn get_with_binary(
        &self,
        key: ChannelKey<'_>,
    ) -> Result<StoredWasmChannelWithBinary, WasmChannelStoreError> {
        let ChannelKey { user_id, name } = key;
        let client = self.client().await?;

        let row = client
            .query_opt(
                format!(
                    "SELECT {CHANNEL_COLUMNS_WITH_BINARY} FROM wasm_channels WHERE user_id = $1 AND name = $2"
                )
                .as_str(),
                &[&user_id, &name],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let wasm_binary: Vec<u8> = r.get("wasm_binary");
                let binary_hash: Vec<u8> = r.get("binary_hash");

                check_binary_integrity(key, &wasm_binary, &binary_hash)?;

                let channel = pg_row_to_channel(&r)?;

                Ok(StoredWasmChannelWithBinary {
                    channel,
                    wasm_binary,
                    binary_hash,
                })
            }
            None => Err(WasmChannelStoreError::NotFound(name.to_string())),
        }
    }

    async fn list(&self, user_id: &str) -> Result<Vec<StoredWasmChannel>, WasmChannelStoreError> {
        let client = self.client().await?;

        let rows = client
            .query(
                format!(
                    "SELECT {CHANNEL_COLUMNS} FROM wasm_channels WHERE user_id = $1 ORDER BY name"
                )
                .as_str(),
                &[&user_id],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        rows.into_iter().map(|r| pg_row_to_channel(&r)).collect()
    }

    async fn delete(&self, key: ChannelKey<'_>) -> Result<bool, WasmChannelStoreError> {
        let ChannelKey { user_id, name } = key;
        let client = self.client().await?;

        let result = client
            .execute(
                "DELETE FROM wasm_channels WHERE user_id = $1 AND name = $2",
                &[&user_id, &name],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        Ok(result > 0)
    }
}

fn pg_row_to_channel(
    row: &tokio_postgres::Row,
) -> Result<StoredWasmChannel, WasmChannelStoreError> {
    Ok(StoredWasmChannel {
        id: row.get("id"),
        user_id: row.get("user_id"),
        name: row.get("name"),
        version: row.get("version"),
        wit_version: row.get("wit_version"),
        description: row.get("description"),
        capabilities_json: row.get("capabilities_json"),
        status: row.get("status"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}
