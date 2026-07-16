//! WASM channel binary storage with integrity verification.
//!
//! Stores compiled WASM channels in the database with BLAKE3 hash verification.
//! Mirrors the pattern in `crate::tools::wasm::storage` but without capabilities table.
//!
//! # Storage Flow
//!
//! ```text
//! WASM bytes ──► BLAKE3 hash ──► Store in database
//!                    │               (binary + hash)
//!                    │
//!                    └──► Later: Load ──► Verify hash ──► Return bytes
//! ```

use chrono::{DateTime, Utc};
use std::future::Future;
use uuid::Uuid;

#[cfg(feature = "libsql")]
mod libsql;
#[cfg(feature = "postgres")]
mod postgres;

/// PostgreSQL implementation of WasmChannelStore.
#[cfg(feature = "postgres")]
pub struct PostgresWasmChannelStore {
    pool: deadpool_postgres::Pool,
}

/// libSQL/Turso implementation of WasmChannelStore.
///
/// Holds an `Arc<Database>` handle and creates a fresh connection per operation,
/// matching the connection-per-request pattern used by the main `LibSqlBackend`.
#[cfg(feature = "libsql")]
pub struct LibSqlWasmChannelStore {
    db: std::sync::Arc<crate::db::libsql::LibSqlDatabase>,
}

/// A stored WASM channel (metadata only, no binary).
#[derive(Debug, Clone)]
pub struct StoredWasmChannel {
    pub id: Uuid,
    pub user_id: String,
    pub name: String,
    pub version: String,
    pub wit_version: String,
    pub description: String,
    pub capabilities_json: String,
    pub status: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Full channel data including binary.
#[derive(Debug)]
pub struct StoredWasmChannelWithBinary {
    pub channel: StoredWasmChannel,
    pub wasm_binary: Vec<u8>,
    pub binary_hash: Vec<u8>,
}

/// Identifies a stored channel by owner and name.
///
/// Mirrors `crate::tools::wasm::storage::ToolKey` so both WASM stores share
/// the same lookup-key shape.
#[derive(Clone, Copy)]
pub struct ChannelKey<'a> {
    pub user_id: &'a str,
    pub name: &'a str,
}

/// Parameters for storing a new WASM channel.
pub struct StoreChannelParams {
    pub user_id: String,
    pub name: String,
    pub version: String,
    pub wit_version: String,
    pub description: String,
    pub wasm_binary: Vec<u8>,
    pub capabilities_json: String,
}

/// Error from WASM channel storage operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum WasmChannelStoreError {
    #[error("Channel not found: {0}")]
    NotFound(String),

    #[error("Binary integrity check failed: hash mismatch")]
    IntegrityCheckFailed,

    #[error("Database error: {0}")]
    Database(String),

    #[error("Invalid data: {0}")]
    InvalidData(String),
}

/// Metadata column list shared by both backends' SELECT statements.
#[cfg(any(feature = "libsql", feature = "postgres"))]
const CHANNEL_COLUMNS: &str = concat!(
    "id, user_id, name, version, wit_version, description, ",
    "capabilities_json, status, created_at, updated_at"
);

/// Column list including the binary payload, shared by both backends'
/// `get_with_binary` queries.
#[cfg(any(feature = "libsql", feature = "postgres"))]
const CHANNEL_COLUMNS_WITH_BINARY: &str = concat!(
    "id, user_id, name, version, wit_version, description, ",
    "wasm_binary, binary_hash, ",
    "capabilities_json, status, created_at, updated_at"
);

/// Verify a loaded binary against its stored hash, logging and returning
/// `IntegrityCheckFailed` on mismatch.
#[cfg(any(feature = "libsql", feature = "postgres"))]
fn check_binary_integrity(
    key: ChannelKey<'_>,
    wasm_binary: &[u8],
    binary_hash: &[u8],
) -> Result<(), WasmChannelStoreError> {
    if crate::tools::wasm::storage::verify_binary_integrity(wasm_binary, binary_hash) {
        return Ok(());
    }
    tracing::error!(
        user_id = key.user_id,
        name = key.name,
        "WASM channel binary integrity check failed"
    );
    Err(WasmChannelStoreError::IntegrityCheckFailed)
}

/// Trait for WASM channel storage.
pub trait WasmChannelStore: Send + Sync {
    /// Use return-position `impl Future + Send` to preserve the implicit
    /// `Send` guarantee from `async-trait` without keeping the proc macro.
    /// Store a new WASM channel.
    fn store(
        &self,
        params: StoreChannelParams,
    ) -> impl Future<Output = Result<StoredWasmChannel, WasmChannelStoreError>> + Send;

    /// Get channel metadata (without binary).
    fn get(
        &self,
        key: ChannelKey<'_>,
    ) -> impl Future<Output = Result<StoredWasmChannel, WasmChannelStoreError>> + Send;

    /// Get channel with binary (verifies integrity).
    fn get_with_binary(
        &self,
        key: ChannelKey<'_>,
    ) -> impl Future<Output = Result<StoredWasmChannelWithBinary, WasmChannelStoreError>> + Send;

    /// List all channels for a user.
    fn list(
        &self,
        user_id: &str,
    ) -> impl Future<Output = Result<Vec<StoredWasmChannel>, WasmChannelStoreError>> + Send;

    /// Delete a channel.
    fn delete(
        &self,
        key: ChannelKey<'_>,
    ) -> impl Future<Output = Result<bool, WasmChannelStoreError>> + Send;
}
