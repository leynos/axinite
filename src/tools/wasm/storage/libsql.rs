//! libSQL/Turso implementation of the WASM tool store.

use std::collections::HashMap;

use chrono::Utc;
use uuid::Uuid;

use crate::tools::wasm::capabilities::EndpointPattern;

use super::{
    NativeWasmToolStore, StoreToolParams, StoredCapabilities, StoredWasmTool,
    StoredWasmToolWithBinary, ToolKey, ToolStatus, WasmStorageError, compute_binary_hash,
    verify_binary_integrity,
};

mod rows;

use rows::{libsql_row_to_tool, libsql_row_to_tool_with_offset, libsql_wasm_opt_text};

// ==================== libSQL implementation ====================

/// libSQL/Turso implementation of WasmToolStore.
///
/// Holds an `Arc<Database>` handle and creates a fresh connection per operation,
/// matching the connection-per-request pattern used by the main `LibSqlBackend`.
pub struct LibSqlWasmToolStore {
    db: std::sync::Arc<crate::db::libsql::LibSqlDatabase>,
}

impl LibSqlWasmToolStore {
    pub fn new(db: std::sync::Arc<crate::db::libsql::LibSqlDatabase>) -> Self {
        Self { db }
    }

    async fn connect(&self) -> Result<libsql::Connection, WasmStorageError> {
        let conn = self
            .db
            .connect()
            .await
            .map_err(|e| WasmStorageError::Database(format!("Connection failed: {}", e)))?;
        Ok(conn)
    }
}

impl NativeWasmToolStore for LibSqlWasmToolStore {
    async fn store(&self, params: StoreToolParams) -> Result<StoredWasmTool, WasmStorageError> {
        let binary_hash = compute_binary_hash(&params.wasm_binary);
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let schema_str = serde_json::to_string(&params.parameters_schema)
            .map_err(|e| WasmStorageError::InvalidData(e.to_string()))?;

        // Wrap delete + INSERT + read-back in a transaction
        let conn = self.connect().await?;
        let tx = conn
            .transaction()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        // Delete any existing version for this (user_id, name) — upgrade-in-place
        tx.execute(
            "DELETE FROM wasm_tools WHERE user_id = ?1 AND name = ?2",
            libsql::params![params.user_id.as_str(), params.name.as_str()],
        )
        .await
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        tx.execute(
            r#"
                INSERT INTO wasm_tools (
                    id, user_id, name, version, wit_version, description, wasm_binary, binary_hash,
                    parameters_schema, source_url, trust_level, status, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, 'active', ?12, ?12)
                "#,
            libsql::params![
                id.to_string(),
                params.user_id.as_str(),
                params.name.as_str(),
                params.version.as_str(),
                params.wit_version.as_str(),
                params.description.as_str(),
                libsql::Value::Blob(params.wasm_binary),
                libsql::Value::Blob(binary_hash),
                schema_str.as_str(),
                libsql_wasm_opt_text(params.source_url.as_deref()),
                params.trust_level.to_string(),
                now.as_str(),
            ],
        )
        .await
        .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        // Read back the row within the same transaction
        let mut rows = tx
            .query(
                r#"
                SELECT id, user_id, name, version, wit_version, description, parameters_schema,
                       source_url, trust_level, status, created_at, updated_at
                FROM wasm_tools
                WHERE user_id = ?1 AND name = ?2
                "#,
                libsql::params![params.user_id.as_str(), params.name.as_str()],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let row = rows
            .next()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?
            .ok_or_else(|| {
                WasmStorageError::Database("Insert succeeded but row not found".into())
            })?;

        let tool = libsql_row_to_tool(&row)?;

        tx.commit()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        Ok(tool)
    }

    async fn get(&self, key: ToolKey<'_>) -> Result<StoredWasmTool, WasmStorageError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, user_id, name, version, wit_version, description, parameters_schema,
                       source_url, trust_level, status, created_at, updated_at
                FROM wasm_tools
                WHERE user_id = ?1 AND name = ?2 AND status = 'active'
                "#,
                libsql::params![key.user_id, key.name],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?
        {
            Some(row) => {
                let tool = libsql_row_to_tool(&row)?;
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
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, user_id, name, version, wit_version, description, wasm_binary, binary_hash,
                       parameters_schema, source_url, trust_level, status, created_at, updated_at
                FROM wasm_tools
                WHERE user_id = ?1 AND name = ?2 AND status = 'active'
                "#,
                libsql::params![key.user_id, key.name],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?
        {
            Some(row) => {
                let wasm_binary: Vec<u8> = row
                    .get(6)
                    .map_err(|e| WasmStorageError::Database(e.to_string()))?;
                let binary_hash: Vec<u8> = row
                    .get(7)
                    .map_err(|e| WasmStorageError::Database(e.to_string()))?;

                if !verify_binary_integrity(&wasm_binary, &binary_hash) {
                    tracing::error!(
                        user_id = key.user_id,
                        name = key.name,
                        "WASM binary integrity check failed"
                    );
                    return Err(WasmStorageError::IntegrityCheckFailed);
                }

                // Parse metadata from the row (different column offsets due to binary/hash)
                let tool = libsql_row_to_tool_with_offset(&row)?;

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
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, wasm_tool_id, http_allowlist, allowed_secrets, tool_aliases,
                       requests_per_minute, requests_per_hour, max_request_body_bytes,
                       max_response_body_bytes, workspace_read_prefixes, http_timeout_secs
                FROM tool_capabilities
                WHERE wasm_tool_id = ?1
                "#,
                libsql::params![tool_id.to_string()],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?
        {
            Some(row) => {
                let id_str: String = row
                    .get(0)
                    .map_err(|e| WasmStorageError::Database(e.to_string()))?;
                let tool_id_str: String = row
                    .get(1)
                    .map_err(|e| WasmStorageError::Database(e.to_string()))?;
                let http_allowlist_str: String = row.get::<String>(2).unwrap_or_default();
                let allowed_secrets_str: String = row.get::<String>(3).unwrap_or_default();
                let tool_aliases_str: String = row.get::<String>(4).unwrap_or_default();
                let rpm: i64 = row.get::<i64>(5).unwrap_or(60);
                let rph: i64 = row.get::<i64>(6).unwrap_or(1000);
                let max_req: i64 = row.get::<i64>(7).unwrap_or(1048576);
                let max_resp: i64 = row.get::<i64>(8).unwrap_or(10485760);
                let ws_prefixes_str: String = row.get::<String>(9).unwrap_or_default();
                let timeout: i64 = row.get::<i64>(10).unwrap_or(30);

                let http_allowlist: Vec<EndpointPattern> =
                    serde_json::from_str(&http_allowlist_str).unwrap_or_default();
                let allowed_secrets: Vec<String> =
                    serde_json::from_str(&allowed_secrets_str).unwrap_or_default();
                let tool_aliases: HashMap<String, String> =
                    serde_json::from_str(&tool_aliases_str).unwrap_or_default();
                let workspace_read_prefixes: Vec<String> =
                    serde_json::from_str(&ws_prefixes_str).unwrap_or_default();

                Ok(Some(StoredCapabilities {
                    id: id_str
                        .parse()
                        .map_err(|e: uuid::Error| WasmStorageError::InvalidData(e.to_string()))?,
                    wasm_tool_id: tool_id_str
                        .parse()
                        .map_err(|e: uuid::Error| WasmStorageError::InvalidData(e.to_string()))?,
                    http_allowlist,
                    allowed_secrets,
                    tool_aliases,
                    requests_per_minute: rpm as u32,
                    requests_per_hour: rph as u32,
                    max_request_body_bytes: max_req,
                    max_response_body_bytes: max_resp,
                    workspace_read_prefixes,
                    http_timeout_secs: timeout as i32,
                }))
            }
            None => Ok(None),
        }
    }

    async fn list(&self, user_id: &str) -> Result<Vec<StoredWasmTool>, WasmStorageError> {
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                r#"
                SELECT id, user_id, name, version, wit_version, description, parameters_schema,
                       source_url, trust_level, status, created_at, updated_at
                FROM wasm_tools
                WHERE user_id = ?1
                ORDER BY name
                "#,
                libsql::params![user_id],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        let mut tools = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?
        {
            tools.push(libsql_row_to_tool(&row)?);
        }
        Ok(tools)
    }

    async fn update_status(
        &self,
        key: ToolKey<'_>,
        status: ToolStatus,
    ) -> Result<(), WasmStorageError> {
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        let conn = self.connect().await?;

        let result = conn
            .execute(
                "UPDATE wasm_tools SET status = ?1, updated_at = ?2 WHERE user_id = ?3 AND name = ?4",
                libsql::params![status.to_string(), now.as_str(), key.user_id, key.name],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        if result == 0 {
            return Err(WasmStorageError::NotFound(key.name.to_string()));
        }

        Ok(())
    }

    async fn delete(&self, key: ToolKey<'_>) -> Result<bool, WasmStorageError> {
        let conn = self.connect().await?;
        let result = conn
            .execute(
                "DELETE FROM wasm_tools WHERE user_id = ?1 AND name = ?2",
                libsql::params![key.user_id, key.name],
            )
            .await
            .map_err(|e| WasmStorageError::Database(e.to_string()))?;

        Ok(result > 0)
    }
}
