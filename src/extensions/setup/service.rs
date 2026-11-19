//! Host-side service for confidential endpoint bindings.
//!
//! The service owns validation-time metadata, encryption, scope construction,
//! and status derivation. Repository adapters only persist and load
//! ciphertext records, so policy mistakes cannot be fixed in one backend and
//! missed in the other.
//!
//! Status reads authenticate every declared record. A row that exists but
//! cannot be opened, or whose kind or validation policy no longer matches its
//! declaration, is reported as needing attention rather than as configured.
//! Decrypted URLs are dropped immediately after authentication; this type has
//! no getter for them until the host execution consumer exists.

use std::fmt;
use std::sync::Arc;

use crate::secrets::SecretsCrypto;

use super::binding::{
    BindingRecord, BindingScope, EndpointBindingRepository, MAX_BINDING_BATCH, StoredBinding,
};
use super::endpoint::validation_policy_version;
use super::model::{
    ENDPOINT_ENVELOPE_VERSION, EndpointField, EndpointKind, EndpointName, EndpointUrl, FieldKey,
    SetupField,
};

/// Domain tag that prefixes the binding associated data.
const BINDING_AAD_DOMAIN: &[u8] = b"axinite/endpoint-binding/aad/v1";

/// Failure of one endpoint-binding operation, safe to surface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BindingServiceError {
    /// Storage could not be read or written. No mutation is claimed.
    StoreUnavailable {
        /// The field involved, when the operation targeted exactly one.
        field: Option<FieldKey>,
    },
    /// Sealing failed before any record was written.
    EncryptionFailed {
        /// The field that could not be sealed.
        field: FieldKey,
    },
}

impl fmt::Display for BindingServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreUnavailable { field: Some(field) } => {
                write!(f, "endpoint binding storage unavailable for {field}")
            }
            Self::StoreUnavailable { field: None } => {
                f.write_str("endpoint binding storage unavailable")
            }
            Self::EncryptionFailed { field } => {
                write!(f, "endpoint binding encryption failed for {field}")
            }
        }
    }
}

impl std::error::Error for BindingServiceError {}

/// Computed state of one declared endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointState {
    /// No record is stored for this declared endpoint.
    Missing,
    /// A record exists, authenticates, and matches the declaration.
    Configured,
    /// The record's kind, envelope version, or validation policy is stale.
    Stale,
    /// The record exists but cannot be authenticated or decoded.
    Corrupt,
}

impl EndpointState {
    /// Whether the endpoint satisfies its declaration.
    pub fn is_configured(self) -> bool {
        matches!(self, Self::Configured)
    }

    /// Whether the endpoint needs an explicit replace or clear.
    pub fn needs_attention(self) -> bool {
        matches!(self, Self::Stale | Self::Corrupt)
    }
}

/// State of one declared endpoint, identified without its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointStatus {
    field: EndpointName,
    state: EndpointState,
}

impl EndpointStatus {
    /// The declaration this status describes.
    pub fn field(&self) -> &EndpointName {
        &self.field
    }

    /// The computed state.
    pub fn state(&self) -> EndpointState {
        self.state
    }
}

/// Validates, seals, and persists confidential endpoint bindings.
pub struct EndpointBindingService {
    repository: Arc<dyn EndpointBindingRepository>,
    crypto: Arc<SecretsCrypto>,
}

impl EndpointBindingService {
    /// Build a service over a repository and the shared master-key holder.
    pub fn new(repository: Arc<dyn EndpointBindingRepository>, crypto: Arc<SecretsCrypto>) -> Self {
        Self { repository, crypto }
    }

    /// Atomically replace one scoped binding with a newly sealed record.
    ///
    /// The URL must already have been validated by the planner; no I/O to the
    /// endpoint's host occurs here.
    pub async fn replace(
        &self,
        scope: &BindingScope,
        name: &EndpointName,
        kind: &EndpointKind,
        url: &EndpointUrl,
    ) -> Result<(), BindingServiceError> {
        let field = FieldKey::Endpoint(name.clone());
        let associated_data = associated_data(
            scope,
            name.as_str(),
            kind.as_str(),
            ENDPOINT_ENVELOPE_VERSION,
            validation_policy_version(),
        );
        let (encrypted_value, key_salt) = self
            .crypto
            .encrypt_endpoint_binding(url.expose().as_bytes(), &associated_data)
            .map_err(|_| BindingServiceError::EncryptionFailed {
                field: field.clone(),
            })?;

        let record = BindingRecord {
            scope: scope.clone(),
            endpoint_name: name.clone(),
            endpoint_kind: kind.clone(),
            envelope_version: ENDPOINT_ENVELOPE_VERSION,
            validation_policy_version: validation_policy_version(),
            encrypted_value,
            key_salt,
        };

        self.repository
            .upsert(&record)
            .await
            .map_err(|_| BindingServiceError::StoreUnavailable { field: Some(field) })
    }

    /// Delete one scoped binding. Clearing an absent binding is a success and
    /// reports `false`.
    pub async fn clear(
        &self,
        scope: &BindingScope,
        name: &EndpointName,
    ) -> Result<bool, BindingServiceError> {
        self.repository
            .delete(scope, name.as_str())
            .await
            .map_err(|_| BindingServiceError::StoreUnavailable {
                field: Some(FieldKey::Endpoint(name.clone())),
            })
    }

    /// Remove every binding in a scope, for extension uninstall cleanup.
    pub async fn clear_scope(&self, scope: &BindingScope) -> Result<u64, BindingServiceError> {
        self.repository
            .delete_scope(scope)
            .await
            .map_err(|_| BindingServiceError::StoreUnavailable { field: None })
    }

    /// Compute the state of every declared endpoint in one bounded read.
    ///
    /// The result preserves declaration order and includes only endpoint
    /// declarations. A repository failure is distinct from every field being
    /// absent and is returned as an error.
    pub async fn statuses(
        &self,
        scope: &BindingScope,
        declarations: &[SetupField],
    ) -> Result<Vec<EndpointStatus>, BindingServiceError> {
        let fields: Vec<&EndpointField> = declarations
            .iter()
            .filter_map(|field| match field {
                SetupField::Endpoint(endpoint) => Some(endpoint),
                SetupField::Secret(_) => None,
            })
            .collect();

        if fields.is_empty() {
            return Ok(Vec::new());
        }

        let stored = self
            .repository
            .list(scope, MAX_BINDING_BATCH)
            .await
            .map_err(|_| BindingServiceError::StoreUnavailable { field: None })?;

        Ok(fields
            .into_iter()
            .map(|field| EndpointStatus {
                field: field.name.clone(),
                state: state_for(scope, field, &stored, &self.crypto),
            })
            .collect())
    }
}

/// Derive the state of one declaration from the scoped stored rows.
fn state_for(
    scope: &BindingScope,
    field: &EndpointField,
    stored: &[StoredBinding],
    crypto: &SecretsCrypto,
) -> EndpointState {
    let Some(row) = stored
        .iter()
        .find(|row| row.endpoint_name == field.name.as_str())
    else {
        return EndpointState::Missing;
    };

    if row.envelope_version != ENDPOINT_ENVELOPE_VERSION {
        return EndpointState::Stale;
    }
    if row.validation_policy_version != validation_policy_version() {
        return EndpointState::Stale;
    }
    if row.endpoint_kind != field.kind.as_str() {
        return EndpointState::Stale;
    }

    let associated_data = associated_data(
        scope,
        &row.endpoint_name,
        &row.endpoint_kind,
        row.envelope_version,
        row.validation_policy_version,
    );

    match crypto.decrypt_endpoint_binding(&row.encrypted_value, &row.key_salt, &associated_data) {
        // The plaintext is dropped here. Only its successful authentication is
        // used, so a status read never retains or returns the URL.
        Ok(_) => EndpointState::Configured,
        Err(_) => EndpointState::Corrupt,
    }
}

/// Encode unambiguous associated data for one binding.
///
/// Every component is length-prefixed with its byte length, so concatenating
/// `("ab", "c")` can never equal `("a", "bc")`. The domain tag separates this
/// encoding from any other use of the same master key.
fn associated_data(
    scope: &BindingScope,
    endpoint_name: &str,
    endpoint_kind: &str,
    envelope_version: u16,
    policy_version: u16,
) -> Vec<u8> {
    let mut data = Vec::new();
    push_component(&mut data, BINDING_AAD_DOMAIN);
    push_component(&mut data, scope.user_id().as_bytes());
    push_component(&mut data, scope.extension_name().as_bytes());
    push_component(&mut data, endpoint_name.as_bytes());
    push_component(&mut data, endpoint_kind.as_bytes());
    push_component(&mut data, &envelope_version.to_be_bytes());
    push_component(&mut data, &policy_version.to_be_bytes());
    data
}

fn push_component(data: &mut Vec<u8>, component: &[u8]) {
    data.extend_from_slice(&(component.len() as u64).to_be_bytes());
    data.extend_from_slice(component);
}

#[cfg(test)]
mod tests {
    //! Associated-data encoding is the anti-transplant boundary.

    use super::associated_data;
    use crate::extensions::setup::BindingScope;

    #[test]
    fn scope_components_cannot_be_shifted_between_fields() {
        let left = associated_data(&BindingScope::new("ab", "c"), "endpoint", "jmap", 1, 1);
        let right = associated_data(&BindingScope::new("a", "bc"), "endpoint", "jmap", 1, 1);

        assert_ne!(left, right);
    }

    #[test]
    fn every_component_changes_the_associated_data() {
        let base = associated_data(&BindingScope::new("user", "ext"), "calendar", "jmap", 1, 1);
        let variants = [
            associated_data(&BindingScope::new("other", "ext"), "calendar", "jmap", 1, 1),
            associated_data(
                &BindingScope::new("user", "other"),
                "calendar",
                "jmap",
                1,
                1,
            ),
            associated_data(&BindingScope::new("user", "ext"), "other", "jmap", 1, 1),
            associated_data(
                &BindingScope::new("user", "ext"),
                "calendar",
                "caldav",
                1,
                1,
            ),
            associated_data(&BindingScope::new("user", "ext"), "calendar", "jmap", 2, 1),
            associated_data(&BindingScope::new("user", "ext"), "calendar", "jmap", 1, 2),
        ];

        for variant in variants {
            assert_ne!(base, variant);
        }
    }
}
