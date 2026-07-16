//! WASM binary storage with integrity verification.
//!
//! Stores compiled WASM tools in PostgreSQL with BLAKE3 hash verification.
//! On load, the hash is verified to detect tampering.
//!
//! # Storage Flow
//!
//! ```text
//! WASM bytes ──► BLAKE3 hash ──► Store in PostgreSQL
//!                    │               (binary + hash)
//!                    │
//!                    └──► Later: Load ──► Verify hash ──► Return bytes
//! ```

use core::future::Future;
use core::pin::Pin;

use uuid::Uuid;

mod types;

pub use self::types::*;

#[cfg(feature = "postgres")]
mod postgres;

#[cfg(feature = "postgres")]
pub use self::postgres::PostgresWasmToolStore;

#[cfg(feature = "libsql")]
mod libsql;

#[cfg(feature = "libsql")]
pub use self::libsql::LibSqlWasmToolStore;

/// Boxed future used at the dyn WASM-tool-store boundary.
pub type WasmToolStoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for WASM tool storage.
pub trait WasmToolStore: Send + Sync {
    /// Store a new WASM tool.
    fn store<'a>(
        &'a self,
        params: StoreToolParams,
    ) -> WasmToolStoreFuture<'a, Result<StoredWasmTool, WasmStorageError>>;

    /// Get tool metadata (without binary).
    fn get<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> WasmToolStoreFuture<'a, Result<StoredWasmTool, WasmStorageError>>;

    /// Get tool with binary (verifies integrity).
    fn get_with_binary<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> WasmToolStoreFuture<'a, Result<StoredWasmToolWithBinary, WasmStorageError>>;

    /// Get tool capabilities.
    fn get_capabilities<'a>(
        &'a self,
        tool_id: Uuid,
    ) -> WasmToolStoreFuture<'a, Result<Option<StoredCapabilities>, WasmStorageError>>;

    /// List all tools for a user.
    fn list<'a>(
        &'a self,
        user_id: &'a str,
    ) -> WasmToolStoreFuture<'a, Result<Vec<StoredWasmTool>, WasmStorageError>>;

    /// Update tool status.
    fn update_status<'a>(
        &'a self,
        key: ToolKey<'a>,
        status: ToolStatus,
    ) -> WasmToolStoreFuture<'a, Result<(), WasmStorageError>>;

    /// Delete a tool.
    fn delete<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> WasmToolStoreFuture<'a, Result<bool, WasmStorageError>>;
}

/// Native async sibling trait for concrete WASM-tool-store implementations.
pub trait NativeWasmToolStore: Send + Sync {
    /// See [`WasmToolStore::store`].
    fn store(
        &self,
        params: StoreToolParams,
    ) -> impl Future<Output = Result<StoredWasmTool, WasmStorageError>> + Send + '_;

    /// See [`WasmToolStore::get`].
    fn get<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> impl Future<Output = Result<StoredWasmTool, WasmStorageError>> + Send + 'a;

    /// See [`WasmToolStore::get_with_binary`].
    fn get_with_binary<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> impl Future<Output = Result<StoredWasmToolWithBinary, WasmStorageError>> + Send + 'a;

    /// See [`WasmToolStore::get_capabilities`].
    fn get_capabilities(
        &self,
        tool_id: Uuid,
    ) -> impl Future<Output = Result<Option<StoredCapabilities>, WasmStorageError>> + Send + '_;

    /// See [`WasmToolStore::list`].
    fn list<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Vec<StoredWasmTool>, WasmStorageError>> + Send + 'a;

    /// See [`WasmToolStore::update_status`].
    fn update_status<'a>(
        &'a self,
        key: ToolKey<'a>,
        status: ToolStatus,
    ) -> impl Future<Output = Result<(), WasmStorageError>> + Send + 'a;

    /// See [`WasmToolStore::delete`].
    fn delete<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> impl Future<Output = Result<bool, WasmStorageError>> + Send + 'a;
}

impl<T> WasmToolStore for T
where
    T: NativeWasmToolStore + Send + Sync,
{
    fn store<'a>(
        &'a self,
        params: StoreToolParams,
    ) -> WasmToolStoreFuture<'a, Result<StoredWasmTool, WasmStorageError>> {
        Box::pin(NativeWasmToolStore::store(self, params))
    }

    fn get<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> WasmToolStoreFuture<'a, Result<StoredWasmTool, WasmStorageError>> {
        Box::pin(NativeWasmToolStore::get(self, key))
    }

    fn get_with_binary<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> WasmToolStoreFuture<'a, Result<StoredWasmToolWithBinary, WasmStorageError>> {
        Box::pin(NativeWasmToolStore::get_with_binary(self, key))
    }

    fn get_capabilities<'a>(
        &'a self,
        tool_id: Uuid,
    ) -> WasmToolStoreFuture<'a, Result<Option<StoredCapabilities>, WasmStorageError>> {
        Box::pin(NativeWasmToolStore::get_capabilities(self, tool_id))
    }

    fn list<'a>(
        &'a self,
        user_id: &'a str,
    ) -> WasmToolStoreFuture<'a, Result<Vec<StoredWasmTool>, WasmStorageError>> {
        Box::pin(NativeWasmToolStore::list(self, user_id))
    }

    fn update_status<'a>(
        &'a self,
        key: ToolKey<'a>,
        status: ToolStatus,
    ) -> WasmToolStoreFuture<'a, Result<(), WasmStorageError>> {
        Box::pin(NativeWasmToolStore::update_status(self, key, status))
    }

    fn delete<'a>(
        &'a self,
        key: ToolKey<'a>,
    ) -> WasmToolStoreFuture<'a, Result<bool, WasmStorageError>> {
        Box::pin(NativeWasmToolStore::delete(self, key))
    }
}

/// Compute BLAKE3 hash of WASM binary.
pub fn compute_binary_hash(binary: &[u8]) -> Vec<u8> {
    let hash = blake3::hash(binary);
    hash.as_bytes().to_vec()
}

/// Verify binary integrity against stored hash.
pub fn verify_binary_integrity(binary: &[u8], expected_hash: &[u8]) -> bool {
    let actual_hash = compute_binary_hash(binary);
    actual_hash == expected_hash
}

#[cfg(test)]
mod tests;
