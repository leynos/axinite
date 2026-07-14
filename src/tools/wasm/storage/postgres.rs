//! PostgreSQL implementation of the WASM tool store.

use std::collections::HashMap;

use chrono::Utc;
use deadpool_postgres::Pool;
use uuid::Uuid;

use crate::tools::wasm::capabilities::EndpointPattern;

use super::{
    NativeWasmToolStore, StoreToolParams, StoredCapabilities, StoredWasmTool,
    StoredWasmToolWithBinary, ToolKey, ToolStatus, WasmStorageError, compute_binary_hash,
    verify_binary_integrity,
};

/// PostgreSQL implementation of WasmToolStore.
pub struct PostgresWasmToolStore {
    pool: Pool,
}

impl PostgresWasmToolStore {
    pub fn new(pool: Pool) -> Self {
        Self { pool }
    }
}

impl NativeWasmToolStore for PostgresWasmToolStore {
    async fn store(&self, params: StoreToolParams) -> Result<StoredWasmTool, WasmStorageError> {
        let mut client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let binary_hash = compute_binary_hash(&params.wasm_binary);
        let id = Uuid::new_v4();
        let now = Utc::now();

        // Wrap delete + insert in a transaction for atomicity
        let tx = client
            .transaction()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        // Delete any existing version for this (user_id, name) — upgrade-in-place
        tx.execute(
            "DELETE FROM wasm_tools WHERE user_id = $1 AND name = $2",
            &[&params.user_id, &params.name],
        )
        .await
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let row = tx
            .query_one(
                r#"
                INSERT INTO wasm_tools (
                    id, user_id, name, version, wit_version, description, wasm_binary, binary_hash,
                    parameters_schema, source_url, trust_level, status, created_at, updated_at
                )
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, 'active', $12, $12)
                RETURNING id, user_id, name, version, wit_version, description, parameters_schema,
                          source_url, trust_level, status, created_at, updated_at
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
                    &params.parameters_schema,
                    &params.source_url,
                    &params.trust_level.to_string(),
                    &now,
                ],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let tool = row_to_tool(&row)?;

        tx.commit()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        Ok(tool)
    }

    async fn get(&self, key: ToolKey<'_>) -> Result<StoredWasmTool, WasmStorageError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let row = client
            .query_opt(
                r#"
                SELECT id, user_id, name, version, wit_version, description, parameters_schema,
                       source_url, trust_level, status, created_at, updated_at
                FROM wasm_tools
                WHERE user_id = $1 AND name = $2 AND status = 'active'
                "#,
                &[&key.user_id, &key.name],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let tool = row_to_tool(&r)?;
                match tool.status {
                    ToolStatus::Active => Ok(tool),
                    ToolStatus::Disabled => Err(WasmStorageError::Disabled),
                    ToolStatus::Quarantined => Err(WasmStorageError::Quarantined),
                }
            }
            None => Err(WasmStorageError::NotFound(key.name.to_string())),
        }
    }

    async fn get_with_binary(
        &self,
        key: ToolKey<'_>,
    ) -> Result<StoredWasmToolWithBinary, WasmStorageError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let row = client
            .query_opt(
                r#"
                SELECT id, user_id, name, version, wit_version, description, wasm_binary, binary_hash,
                       parameters_schema, source_url, trust_level, status, created_at, updated_at
                FROM wasm_tools
                WHERE user_id = $1 AND name = $2 AND status = 'active'
                "#,
                &[&key.user_id, &key.name],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let wasm_binary: Vec<u8> = r.get("wasm_binary");
                let binary_hash: Vec<u8> = r.get("binary_hash");

                // Verify integrity
                if !verify_binary_integrity(&wasm_binary, &binary_hash) {
                    tracing::error!(
                        user_id = key.user_id,
                        name = key.name,
                        "WASM binary integrity check failed"
                    );
                    return Err(WasmStorageError::IntegrityCheckFailed);
                }

                let tool = row_to_tool(&r)?;

                match tool.status {
                    ToolStatus::Active => Ok(StoredWasmToolWithBinary {
                        tool,
                        wasm_binary,
                        binary_hash,
                    }),
                    ToolStatus::Disabled => Err(WasmStorageError::Disabled),
                    ToolStatus::Quarantined => Err(WasmStorageError::Quarantined),
                }
            }
            None => Err(WasmStorageError::NotFound(key.name.to_string())),
        }
    }

    async fn get_capabilities(
        &self,
        tool_id: Uuid,
    ) -> Result<Option<StoredCapabilities>, WasmStorageError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let row = client
            .query_opt(
                r#"
                SELECT id, wasm_tool_id, http_allowlist, allowed_secrets, tool_aliases,
                       requests_per_minute, requests_per_hour, max_request_body_bytes,
                       max_response_body_bytes, workspace_read_prefixes, http_timeout_secs
                FROM tool_capabilities
                WHERE wasm_tool_id = $1
                "#,
                &[&tool_id],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        match row {
            Some(r) => {
                let http_allowlist_json: serde_json::Value = r.get("http_allowlist");
                let tool_aliases_json: serde_json::Value = r.get("tool_aliases");

                let http_allowlist: Vec<EndpointPattern> =
                    serde_json::from_value(http_allowlist_json).unwrap_or_default();
                let tool_aliases: HashMap<String, String> =
                    serde_json::from_value(tool_aliases_json).unwrap_or_default();

                Ok(Some(StoredCapabilities {
                    id: r.get("id"),
                    wasm_tool_id: r.get("wasm_tool_id"),
                    http_allowlist,
                    allowed_secrets: r.get("allowed_secrets"),
                    tool_aliases,
                    requests_per_minute: r.get::<_, i32>("requests_per_minute") as u32,
                    requests_per_hour: r.get::<_, i32>("requests_per_hour") as u32,
                    max_request_body_bytes: r.get("max_request_body_bytes"),
                    max_response_body_bytes: r.get("max_response_body_bytes"),
                    workspace_read_prefixes: r.get("workspace_read_prefixes"),
                    http_timeout_secs: r.get("http_timeout_secs"),
                }))
            }
            None => Ok(None),
        }
    }

    async fn list(&self, user_id: &str) -> Result<Vec<StoredWasmTool>, WasmStorageError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let rows = client
            .query(
                r#"
                SELECT id, user_id, name, version, wit_version, description,
                       parameters_schema, source_url, trust_level, status, created_at, updated_at
                FROM wasm_tools
                WHERE user_id = $1
                ORDER BY name
                "#,
                &[&user_id],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        rows.into_iter().map(|r| row_to_tool(&r)).collect()
    }

    async fn update_status(
        &self,
        key: ToolKey<'_>,
        status: ToolStatus,
    ) -> Result<(), WasmStorageError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let result = client
            .execute(
                "UPDATE wasm_tools SET status = $1, updated_at = NOW() WHERE user_id = $2 AND name = $3",
                &[&status.to_string(), &key.user_id, &key.name],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        if result == 0 {
            return Err(WasmStorageError::NotFound(key.name.to_string()));
        }

        Ok(())
    }

    async fn delete(&self, key: ToolKey<'_>) -> Result<bool, WasmStorageError> {
        let client = self
            .pool
            .get()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let result = client
            .execute(
                "DELETE FROM wasm_tools WHERE user_id = $1 AND name = $2",
                &[&key.user_id, &key.name],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        Ok(result > 0)
    }
}

fn row_to_tool(row: &tokio_postgres::Row) -> Result<StoredWasmTool, WasmStorageError> {
    let trust_level_str: String = row.get("trust_level");
    let status_str: String = row.get("status");

    Ok(StoredWasmTool {
        id: row.get("id"),
        user_id: row.get("user_id"),
        name: row.get("name"),
        version: row.get("version"),
        wit_version: row.get("wit_version"),
        description: row.get("description"),
        parameters_schema: row.get("parameters_schema"),
        source_url: row.get("source_url"),
        trust_level: trust_level_str
            .parse()
            .map_err(WasmStorageError::InvalidData)?,
        status: status_str.parse().map_err(WasmStorageError::InvalidData)?,
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    })
}
