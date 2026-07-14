//! PostgreSQL backend for WASM channel storage.

use chrono::Utc;
use deadpool_postgres::Pool;
use uuid::Uuid;

use crate::tools::wasm::storage::{compute_binary_hash, verify_binary_integrity};

use super::{
    StoreChannelParams, StoredWasmChannel, StoredWasmChannelWithBinary, WasmChannelStore,
    WasmChannelStoreError,
};

use super::PostgresWasmChannelStore;

impl PostgresWasmChannelStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl WasmChannelStore for PostgresWasmChannelStore {
    async fn store(
        &self,
        params: StoreChannelParams,
    ) -> Result<StoredWasmChannel, WasmChannelStoreError> {
        let mut client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

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
                r#"
                INSERT INTO wasm_channels (
                    id, user_id, name, version, wit_version, description, wasm_binary, binary_hash,
                    capabilities_json, status, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'active', $10, $10)
                RETURNING id, user_id, name, version, wit_version, description,
                          capabilities_json, status, created_at, updated_at
                "#,
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

    async fn get(
        &self,
        user_id: &str,
        name: &str,
    ) -> Result<StoredWasmChannel, WasmChannelStoreError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        let row = client
            .query_opt(
                r#"
                SELECT id, user_id, name, version, wit_version, description,
                       capabilities_json, status, created_at, updated_at
                FROM wasm_channels
                WHERE user_id = $1 AND name = $2
                "#,
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
        user_id: &str,
        name: &str,
    ) -> Result<StoredWasmChannelWithBinary, WasmChannelStoreError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        let row = client
            .query_opt(
                r#"
                SELECT id, user_id, name, version, wit_version, description,
                       wasm_binary, binary_hash,
                       capabilities_json, status, created_at, updated_at
                FROM wasm_channels
                WHERE user_id = $1 AND name = $2
                "#,
                &[&user_id, &name],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let wasm_binary: Vec<u8> = r.get("wasm_binary");
                let binary_hash: Vec<u8> = r.get("binary_hash");

                if !verify_binary_integrity(&wasm_binary, &binary_hash) {
                    tracing::error!(
                        user_id = user_id,
                        name = name,
                        "WASM channel binary integrity check failed"
                    );
                    return Err(WasmChannelStoreError::IntegrityCheckFailed);
                }

                let channel = StoredWasmChannel {
                    id: r.get("id"),
                    user_id: r.get("user_id"),
                    name: r.get("name"),
                    version: r.get("version"),
                    wit_version: r.get("wit_version"),
                    description: r.get("description"),
                    capabilities_json: r.get("capabilities_json"),
                    status: r.get("status"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                };

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
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        let rows = client
            .query(
                r#"
                SELECT id, user_id, name, version, wit_version, description,
                       capabilities_json, status, created_at, updated_at
                FROM wasm_channels
                WHERE user_id = $1
                ORDER BY name
                "#,
                &[&user_id],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        rows.into_iter().map(|r| pg_row_to_channel(&r)).collect()
    }

    async fn delete(&self, user_id: &str, name: &str) -> Result<bool, WasmChannelStoreError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

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
