//! Helpers shared by the SQL-backed secrets store implementations.
//!
//! The libSQL and PostgreSQL back-ends differ in SQL dialect and driver
//! types, but share the same error mapping, expiry handling, decryption
//! flow, and access-control checks. Those genuinely common parts live here.

use crate::secrets::crypto::SecretsCrypto;
use crate::secrets::types::{DecryptedSecret, Secret, SecretError};

use super::NativeSecretsStore;
use super::access::{ensure_not_expired, is_secret_name_allowed};

/// The full `secrets` column list, in the positional order the backend row
/// mappers consume. Shared by every backend's `SELECT`/`RETURNING` clause so the
/// column set is written in exactly one place.
pub(super) const SECRET_COLUMNS: &str = "id, user_id, name, encrypted_value, key_salt, provider, \
     expires_at, last_used_at, usage_count, created_at, updated_at";

/// Map a database driver error into [`SecretError::Database`].
pub(super) fn db_err(error: impl std::fmt::Display) -> SecretError {
    SecretError::Database(error.to_string())
}

/// Enforce expiry on an optionally fetched secret, reporting a missing row
/// as [`SecretError::NotFound`].
pub(super) fn require_live_secret(
    secret: Option<Secret>,
    name: &str,
) -> Result<Secret, SecretError> {
    match secret {
        Some(secret) => ensure_not_expired(secret),
        None => Err(SecretError::NotFound(name.to_string())),
    }
}

/// Fetch a secret via the store's `get` and decrypt it with `crypto`.
pub(super) async fn get_decrypted_via<S: NativeSecretsStore>(
    store: &S,
    crypto: &SecretsCrypto,
    user_id: &str,
    name: &str,
) -> Result<DecryptedSecret, SecretError> {
    let secret = NativeSecretsStore::get(store, user_id, name).await?;
    crypto.decrypt(&secret.encrypted_value, &secret.key_salt)
}

/// Whether `secret_name` both exists for the user and appears in the tool's
/// allow-list (which supports "openai_*"-style glob patterns).
pub(super) async fn is_accessible_via<S: NativeSecretsStore>(
    store: &S,
    user_id: &str,
    secret_name: &str,
    allowed_secrets: &[String],
) -> Result<bool, SecretError> {
    let secret_name_lower = secret_name.to_lowercase();
    if !NativeSecretsStore::exists(store, user_id, &secret_name_lower).await? {
        return Ok(false);
    }
    Ok(is_secret_name_allowed(&secret_name_lower, allowed_secrets))
}
