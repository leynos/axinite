//! Scoped endpoint-binding records and their driven persistence port.
//!
//! A record is addressed by the host scope `(user_id, extension_name)` plus the
//! manifest-declared endpoint name. The repository stores ciphertext only and
//! performs no validation, decryption, or policy checks; those belong to
//! [`super::service::EndpointBindingService`].
//!
//! `Debug` for record types prints lengths and identifiers, never ciphertext,
//! salts, or plaintext URLs.

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use super::model::{EndpointKind, EndpointName, MAX_SETUP_FIELDS};

/// Boxed future returned by the dyn-safe repository port.
pub type BindingFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The host-owned identity a binding belongs to.
///
/// Endpoint names are only unique within this scope; two extensions, or two
/// users, may each declare an endpoint with the same name without aliasing.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct BindingScope {
    user_id: String,
    extension_name: String,
}

impl BindingScope {
    /// Construct a scope from the authenticated user and extension identity.
    pub fn new(user_id: impl Into<String>, extension_name: impl Into<String>) -> Self {
        Self {
            user_id: user_id.into(),
            extension_name: extension_name.into(),
        }
    }

    /// The user that owns the binding.
    pub fn user_id(&self) -> &str {
        &self.user_id
    }

    /// The extension that declared the binding.
    pub fn extension_name(&self) -> &str {
        &self.extension_name
    }
}

/// A new or replacement ciphertext record to persist atomically.
#[derive(Clone)]
pub struct BindingRecord {
    /// Owning scope.
    pub scope: BindingScope,
    /// Manifest-declared endpoint name.
    pub endpoint_name: EndpointName,
    /// Declared kind, authenticated as part of the associated data.
    pub endpoint_kind: EndpointKind,
    /// Envelope format version written by the sealer.
    pub envelope_version: u16,
    /// Validation policy under which the URL was accepted.
    pub validation_policy_version: u16,
    /// AES-256-GCM envelope: `nonce || ciphertext || tag`.
    pub encrypted_value: Vec<u8>,
    /// Per-record HKDF salt.
    pub key_salt: Vec<u8>,
}

impl fmt::Debug for BindingRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BindingRecord")
            .field("scope", &self.scope)
            .field("endpoint_name", &self.endpoint_name)
            .field("endpoint_kind", &self.endpoint_kind)
            .field("envelope_version", &self.envelope_version)
            .field("validation_policy_version", &self.validation_policy_version)
            .field("encrypted_value", &"[REDACTED]")
            .field("key_salt", &"[REDACTED]")
            .finish()
    }
}

/// A ciphertext record as read back from storage.
///
/// Versions and kind are returned as stored, without interpretation, so the
/// service can identify stale or transplanted rows.
pub struct StoredBinding {
    /// Endpoint name the row is keyed by.
    pub endpoint_name: String,
    /// Kind recorded when the row was written.
    pub endpoint_kind: String,
    /// Envelope format version recorded with the row.
    pub envelope_version: u16,
    /// Validation policy version recorded with the row.
    pub validation_policy_version: u16,
    /// AES-256-GCM envelope: `nonce || ciphertext || tag`.
    pub encrypted_value: Vec<u8>,
    /// Per-record HKDF salt.
    pub key_salt: Vec<u8>,
}

impl fmt::Debug for StoredBinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StoredBinding")
            .field("endpoint_name", &self.endpoint_name)
            .field("endpoint_kind", &self.endpoint_kind)
            .field("envelope_version", &self.envelope_version)
            .field("validation_policy_version", &self.validation_policy_version)
            .field("encrypted_value", &"[REDACTED]")
            .field("key_salt", &"[REDACTED]")
            .finish()
    }
}

/// A storage failure category that is safe to surface.
///
/// Backend error text, SQL, and driver chains stay behind this boundary so
/// they cannot reach logs or responses on the setup path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingStoreError {
    /// The database could not be reached or the query failed.
    Unavailable,
    /// A stored row could not be decoded into the repository record shape.
    Corrupt,
}

impl fmt::Display for BindingStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("endpoint binding storage unavailable"),
            Self::Corrupt => f.write_str("endpoint binding storage corrupt"),
        }
    }
}

impl std::error::Error for BindingStoreError {}

/// Maximum number of rows a single scoped read may return.
pub const MAX_BINDING_BATCH: usize = MAX_SETUP_FIELDS;

/// Driven persistence port for confidential endpoint bindings.
pub trait EndpointBindingRepository: Send + Sync {
    /// Atomically insert or replace one scoped record.
    fn upsert<'a>(
        &'a self,
        record: &'a BindingRecord,
    ) -> BindingFuture<'a, Result<(), BindingStoreError>>;

    /// Delete one scoped record, reporting whether a row existed.
    fn delete<'a>(
        &'a self,
        scope: &'a BindingScope,
        endpoint_name: &'a str,
    ) -> BindingFuture<'a, Result<bool, BindingStoreError>>;

    /// Read up to `limit` records for one scope, ordered by endpoint name.
    fn list<'a>(
        &'a self,
        scope: &'a BindingScope,
        limit: usize,
    ) -> BindingFuture<'a, Result<Vec<StoredBinding>, BindingStoreError>>;

    /// Delete every record in a scope, returning the number removed.
    fn delete_scope<'a>(
        &'a self,
        scope: &'a BindingScope,
    ) -> BindingFuture<'a, Result<u64, BindingStoreError>>;
}

/// Native (non-boxed) form of [`EndpointBindingRepository`].
///
/// Adapters implement this trait; the blanket implementation below provides
/// the dyn-compatible port without hand-writing a boxed future in every
/// method, following the secrets-store precedent.
pub trait NativeEndpointBindingRepository: Send + Sync {
    /// Atomically insert or replace one scoped record.
    fn upsert<'a>(
        &'a self,
        record: &'a BindingRecord,
    ) -> impl Future<Output = Result<(), BindingStoreError>> + Send + 'a;

    /// Delete one scoped record, reporting whether a row existed.
    fn delete<'a>(
        &'a self,
        scope: &'a BindingScope,
        endpoint_name: &'a str,
    ) -> impl Future<Output = Result<bool, BindingStoreError>> + Send + 'a;

    /// Read up to `limit` records for one scope, ordered by endpoint name.
    fn list<'a>(
        &'a self,
        scope: &'a BindingScope,
        limit: usize,
    ) -> impl Future<Output = Result<Vec<StoredBinding>, BindingStoreError>> + Send + 'a;

    /// Delete every record in a scope, returning the number removed.
    fn delete_scope<'a>(
        &'a self,
        scope: &'a BindingScope,
    ) -> impl Future<Output = Result<u64, BindingStoreError>> + Send + 'a;
}

impl<T: NativeEndpointBindingRepository + Send + Sync> EndpointBindingRepository for T {
    fn upsert<'a>(
        &'a self,
        record: &'a BindingRecord,
    ) -> BindingFuture<'a, Result<(), BindingStoreError>> {
        Box::pin(NativeEndpointBindingRepository::upsert(self, record))
    }

    fn delete<'a>(
        &'a self,
        scope: &'a BindingScope,
        endpoint_name: &'a str,
    ) -> BindingFuture<'a, Result<bool, BindingStoreError>> {
        Box::pin(NativeEndpointBindingRepository::delete(
            self,
            scope,
            endpoint_name,
        ))
    }

    fn list<'a>(
        &'a self,
        scope: &'a BindingScope,
        limit: usize,
    ) -> BindingFuture<'a, Result<Vec<StoredBinding>, BindingStoreError>> {
        Box::pin(NativeEndpointBindingRepository::list(self, scope, limit))
    }

    fn delete_scope<'a>(
        &'a self,
        scope: &'a BindingScope,
    ) -> BindingFuture<'a, Result<u64, BindingStoreError>> {
        Box::pin(NativeEndpointBindingRepository::delete_scope(self, scope))
    }
}
