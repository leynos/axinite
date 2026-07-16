//! Backend-agnostic secrets-store policy helpers shared by the SQL-backed
//! stores (allowed-name glob matching and expiry checks).

use chrono::Utc;

use crate::secrets::types::{Secret, SecretError};

/// Return `true` when a (lowercased) secret name matches the allowed list.
///
/// Supports simple glob patterns where a trailing `*` matches any suffix,
/// e.g. `openai_*` matches `openai_api_key`.
pub(super) fn is_secret_name_allowed(secret_name_lower: &str, allowed_secrets: &[String]) -> bool {
    for pattern in allowed_secrets {
        let pattern_lower = pattern.to_lowercase();
        if pattern_lower == secret_name_lower {
            return true;
        }

        if let Some(prefix) = pattern_lower.strip_suffix('*')
            && secret_name_lower.starts_with(prefix)
        {
            return true;
        }
    }

    false
}

/// Reject a secret whose expiry timestamp has passed.
pub(super) fn ensure_not_expired(secret: Secret) -> Result<Secret, SecretError> {
    if let Some(expires_at) = secret.expires_at
        && expires_at < Utc::now()
    {
        return Err(SecretError::Expired);
    }
    Ok(secret)
}
