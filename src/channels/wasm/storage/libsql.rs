//! libSQL/Turso backend for WASM channel storage.

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::tools::wasm::storage::compute_binary_hash;

use super::{
    CHANNEL_COLUMNS, CHANNEL_COLUMNS_WITH_BINARY, ChannelKey, StoreChannelParams,
    StoredWasmChannel, StoredWasmChannelWithBinary, WasmChannelStore, WasmChannelStoreError,
    check_binary_integrity,
};

use super::LibSqlWasmChannelStore;

impl LibSqlWasmChannelStore {
    pub fn new(db: std::sync::Arc<crate::db::libsql::LibSqlDatabase>) -> Self {
        Self { db }
    }

    async fn connect(&self) -> Result<libsql::Connection, WasmChannelStoreError> {
        let conn =
            self.db.connect().await.map_err(|e| {
                WasmChannelStoreError::Database(format!("Connection failed: {}", e))
            })?;
        Ok(conn)
    }
}

impl WasmChannelStore for LibSqlWasmChannelStore {
    async fn store(
        &self,
        params: StoreChannelParams,
    ) -> Result<StoredWasmChannel, WasmChannelStoreError> {
        let binary_hash = compute_binary_hash(&params.wasm_binary);
        let id = Uuid::new_v4();
        let now = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);

        let conn = self.connect().await?;
        let tx = conn
            .transaction()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        // Delete any existing version for this (user_id, name) — upgrade-in-place
        tx.execute(
            "DELETE FROM wasm_channels WHERE user_id = ?1 AND name = ?2",
            libsql::params![params.user_id.as_str(), params.name.as_str()],
        )
        .await
        .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        tx.execute(
            r#"
                INSERT INTO wasm_channels (
                    id, user_id, name, version, wit_version, description, wasm_binary, binary_hash,
                    capabilities_json, status, created_at, updated_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'active', ?10, ?10)
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
                params.capabilities_json.as_str(),
                now.as_str(),
            ],
        )
        .await
        .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        // Read back the row within the same transaction
        let mut rows = tx
            .query(
                &format!(
                    "SELECT {CHANNEL_COLUMNS} FROM wasm_channels WHERE user_id = ?1 AND name = ?2"
                ),
                libsql::params![params.user_id.as_str(), params.name.as_str()],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        let row = rows
            .next()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?
            .ok_or_else(|| {
                WasmChannelStoreError::Database("Insert succeeded but row not found".into())
            })?;

        let channel = libsql_row_to_channel(&row)?;

        tx.commit()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        Ok(channel)
    }

    async fn get(&self, key: ChannelKey<'_>) -> Result<StoredWasmChannel, WasmChannelStoreError> {
        let ChannelKey { user_id, name } = key;
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {CHANNEL_COLUMNS} FROM wasm_channels WHERE user_id = ?1 AND name = ?2"
                ),
                libsql::params![user_id, name],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?
        {
            Some(row) => libsql_row_to_channel(&row),
            None => Err(WasmChannelStoreError::NotFound(name.to_string())),
        }
    }

    async fn get_with_binary(
        &self,
        key: ChannelKey<'_>,
    ) -> Result<StoredWasmChannelWithBinary, WasmChannelStoreError> {
        let ChannelKey { user_id, name } = key;
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {CHANNEL_COLUMNS_WITH_BINARY} FROM wasm_channels WHERE user_id = ?1 AND name = ?2"
                ),
                libsql::params![user_id, name],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        match rows
            .next()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?
        {
            Some(row) => {
                let wasm_binary: Vec<u8> = row
                    .get(6)
                    .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;
                let binary_hash: Vec<u8> = row
                    .get(7)
                    .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

                check_binary_integrity(key, &wasm_binary, &binary_hash)?;

                let channel = libsql_row_to_channel_at(&row, 8)?;

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
        let conn = self.connect().await?;
        let mut rows = conn
            .query(
                &format!(
                    "SELECT {CHANNEL_COLUMNS} FROM wasm_channels WHERE user_id = ?1 ORDER BY name"
                ),
                libsql::params![user_id],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        let mut channels = Vec::new();
        while let Some(row) = rows
            .next()
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?
        {
            channels.push(libsql_row_to_channel(&row)?);
        }
        Ok(channels)
    }

    async fn delete(&self, key: ChannelKey<'_>) -> Result<bool, WasmChannelStoreError> {
        let ChannelKey { user_id, name } = key;
        let conn = self.connect().await?;
        let result = conn
            .execute(
                "DELETE FROM wasm_channels WHERE user_id = ?1 AND name = ?2",
                libsql::params![user_id, name],
            )
            .await
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

        Ok(result > 0)
    }
}

#[allow(dead_code)]
fn libsql_channel_opt_text(s: Option<&str>) -> libsql::Value {
    match s {
        Some(s) => libsql::Value::Text(s.to_string()),
        None => libsql::Value::Null,
    }
}

fn libsql_channel_parse_ts(s: &str) -> Result<DateTime<Utc>, WasmChannelStoreError> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(ndt.and_utc());
    }
    if let Ok(ndt) = chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(ndt.and_utc());
    }
    Err(WasmChannelStoreError::InvalidData(format!(
        "unparsable timestamp: {:?}",
        s
    )))
}

/// Parse a channel row.
///
/// The first six columns are always `id(0)..description(5)`;
/// `capabilities_idx` gives the index of `capabilities_json`, with
/// `status`, `created_at`, and `updated_at` following consecutively.
/// Metadata-only queries use index 6; queries that also select
/// `wasm_binary`/`binary_hash` use index 8.
fn libsql_row_to_channel_at(
    row: &libsql::Row,
    capabilities_idx: i32,
) -> Result<StoredWasmChannel, WasmChannelStoreError> {
    let id_str: String = row
        .get(0)
        .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;
    let created_at_str: String = row
        .get(capabilities_idx + 2)
        .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;
    let updated_at_str: String = row
        .get(capabilities_idx + 3)
        .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?;

    Ok(StoredWasmChannel {
        id: id_str
            .parse()
            .map_err(|e: uuid::Error| WasmChannelStoreError::InvalidData(e.to_string()))?,
        user_id: row
            .get(1)
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?,
        name: row
            .get(2)
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?,
        version: row
            .get(3)
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?,
        wit_version: row
            .get(4)
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?,
        description: row
            .get(5)
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?,
        capabilities_json: row
            .get(capabilities_idx)
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?,
        status: row
            .get(capabilities_idx + 1)
            .map_err(|e| WasmChannelStoreError::Database(e.to_string()))?,
        created_at: libsql_channel_parse_ts(&created_at_str)?,
        updated_at: libsql_channel_parse_ts(&updated_at_str)?,
    })
}

/// Parse a channel row from a metadata-only query.
fn libsql_row_to_channel(row: &libsql::Row) -> Result<StoredWasmChannel, WasmChannelStoreError> {
    libsql_row_to_channel_at(row, 6)
}
