//! Secret storage with PostgreSQL persistence.
//!
//! Provides CRUD operations for encrypted secrets. The store handles:
//! - Encryption/decryption via SecretsCrypto
//! - Expiration checking
//! - Usage tracking
//! - Access control (which secrets a tool can use)

use core::future::Future;
use core::pin::Pin;

use uuid::Uuid;

use crate::secrets::types::{CreateSecretParams, DecryptedSecret, Secret, SecretError, SecretRef};

/// Boxed future used at the dyn secrets-store boundary.
pub type SecretsStoreFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Trait for secret storage operations.
///
/// Allows for different implementations (PostgreSQL, in-memory for testing).
pub trait SecretsStore: Send + Sync {
    /// Store a new secret.
    fn create<'a>(
        &'a self,
        user_id: &'a str,
        params: CreateSecretParams,
    ) -> SecretsStoreFuture<'a, Result<Secret, SecretError>>;

    /// Get a secret by name (encrypted form).
    fn get<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> SecretsStoreFuture<'a, Result<Secret, SecretError>>;

    /// Get and decrypt a secret.
    fn get_decrypted<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> SecretsStoreFuture<'a, Result<DecryptedSecret, SecretError>>;

    /// Check if a secret exists.
    fn exists<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> SecretsStoreFuture<'a, Result<bool, SecretError>>;

    /// List all secret references for a user (no values).
    fn list<'a>(
        &'a self,
        user_id: &'a str,
    ) -> SecretsStoreFuture<'a, Result<Vec<SecretRef>, SecretError>>;

    /// Delete a secret.
    fn delete<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> SecretsStoreFuture<'a, Result<bool, SecretError>>;

    /// Update secret usage tracking.
    fn record_usage<'a>(
        &'a self,
        secret_id: Uuid,
    ) -> SecretsStoreFuture<'a, Result<(), SecretError>>;

    /// Check if a secret is accessible by a tool (based on allowed_secrets).
    fn is_accessible<'a>(
        &'a self,
        user_id: &'a str,
        secret_name: &'a str,
        allowed_secrets: &'a [String],
    ) -> SecretsStoreFuture<'a, Result<bool, SecretError>>;
}

/// Native async sibling trait for concrete secrets-store implementations.
pub trait NativeSecretsStore: Send + Sync {
    /// See [`SecretsStore::create`].
    fn create<'a>(
        &'a self,
        user_id: &'a str,
        params: CreateSecretParams,
    ) -> impl Future<Output = Result<Secret, SecretError>> + Send + 'a;

    /// See [`SecretsStore::get`].
    fn get<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> impl Future<Output = Result<Secret, SecretError>> + Send + 'a;

    /// See [`SecretsStore::get_decrypted`].
    fn get_decrypted<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> impl Future<Output = Result<DecryptedSecret, SecretError>> + Send + 'a;

    /// See [`SecretsStore::exists`].
    fn exists<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> impl Future<Output = Result<bool, SecretError>> + Send + 'a;

    /// See [`SecretsStore::list`].
    fn list<'a>(
        &'a self,
        user_id: &'a str,
    ) -> impl Future<Output = Result<Vec<SecretRef>, SecretError>> + Send + 'a;

    /// See [`SecretsStore::delete`].
    fn delete<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> impl Future<Output = Result<bool, SecretError>> + Send + 'a;

    /// See [`SecretsStore::record_usage`].
    fn record_usage<'a>(
        &'a self,
        secret_id: Uuid,
    ) -> impl Future<Output = Result<(), SecretError>> + Send + 'a;

    /// See [`SecretsStore::is_accessible`].
    fn is_accessible<'a>(
        &'a self,
        user_id: &'a str,
        secret_name: &'a str,
        allowed_secrets: &'a [String],
    ) -> impl Future<Output = Result<bool, SecretError>> + Send + 'a;
}

impl<T> SecretsStore for T
where
    T: NativeSecretsStore + Send + Sync,
{
    fn create<'a>(
        &'a self,
        user_id: &'a str,
        params: CreateSecretParams,
    ) -> SecretsStoreFuture<'a, Result<Secret, SecretError>> {
        Box::pin(NativeSecretsStore::create(self, user_id, params))
    }

    fn get<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> SecretsStoreFuture<'a, Result<Secret, SecretError>> {
        Box::pin(NativeSecretsStore::get(self, user_id, name))
    }

    fn get_decrypted<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> SecretsStoreFuture<'a, Result<DecryptedSecret, SecretError>> {
        Box::pin(NativeSecretsStore::get_decrypted(self, user_id, name))
    }

    fn exists<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> SecretsStoreFuture<'a, Result<bool, SecretError>> {
        Box::pin(NativeSecretsStore::exists(self, user_id, name))
    }

    fn list<'a>(
        &'a self,
        user_id: &'a str,
    ) -> SecretsStoreFuture<'a, Result<Vec<SecretRef>, SecretError>> {
        Box::pin(NativeSecretsStore::list(self, user_id))
    }

    fn delete<'a>(
        &'a self,
        user_id: &'a str,
        name: &'a str,
    ) -> SecretsStoreFuture<'a, Result<bool, SecretError>> {
        Box::pin(NativeSecretsStore::delete(self, user_id, name))
    }

    fn record_usage<'a>(
        &'a self,
        secret_id: Uuid,
    ) -> SecretsStoreFuture<'a, Result<(), SecretError>> {
        Box::pin(NativeSecretsStore::record_usage(self, secret_id))
    }

    fn is_accessible<'a>(
        &'a self,
        user_id: &'a str,
        secret_name: &'a str,
        allowed_secrets: &'a [String],
    ) -> SecretsStoreFuture<'a, Result<bool, SecretError>> {
        Box::pin(NativeSecretsStore::is_accessible(
            self,
            user_id,
            secret_name,
            allowed_secrets,
        ))
    }
}

#[cfg(any(feature = "postgres", feature = "libsql"))]
mod access;

#[cfg(any(feature = "postgres", feature = "libsql"))]
mod common;

#[cfg(feature = "postgres")]
mod postgres;

#[cfg(feature = "postgres")]
pub use self::postgres::PostgresSecretsStore;

#[cfg(feature = "libsql")]
mod libsql;

#[cfg(feature = "libsql")]
pub use self::libsql::LibSqlSecretsStore;

/// In-memory secrets store. Used for testing and as a fallback when no
/// persistent secrets backend is configured (extension listing/install still
/// works, but stored secrets won't survive a restart).
pub mod in_memory;

#[cfg(test)]
mod tests;
