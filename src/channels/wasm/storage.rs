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
        user_id: &str,
        name: &str,
    ) -> impl Future<Output = Result<StoredWasmChannel, WasmChannelStoreError>> + Send;

    /// Get channel with binary (verifies integrity).
    fn get_with_binary(
        &self,
        user_id: &str,
        name: &str,
    ) -> impl Future<Output = Result<StoredWasmChannelWithBinary, WasmChannelStoreError>> + Send;

    /// List all channels for a user.
    fn list(
        &self,
        user_id: &str,
    ) -> impl Future<Output = Result<Vec<StoredWasmChannel>, WasmChannelStoreError>> + Send;

    /// Delete a channel.
    fn delete(
        &self,
        user_id: &str,
        name: &str,
    ) -> impl Future<Output = Result<bool, WasmChannelStoreError>> + Send;
}
