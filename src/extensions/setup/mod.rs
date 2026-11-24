//! Typed extension setup fields and confidential endpoint bindings.
//!
//! The feature separates three concerns:
//!
//! - [`model`] holds pure domain values.
//! - [`error`] holds semantic failures and rejection categories.
//! - [`endpoint`] validates endpoint URLs offline.
//! - [`policy`] plans setup operations without performing I/O.
//! - [`manifest`] normalizes shipped capabilities manifests, including the
//!   legacy `required_secrets` form, into typed declarations.
//! - [`binding`] defines scoped ciphertext records and the driven repository
//!   port.
//! - [`service`] validates, seals, and reports status for endpoint bindings.
//!
//! Database adapters live under `src/db/`; this module has no SQL or HTTP
//! dependency, and no transport exposes an endpoint value.

pub mod binding;
pub mod endpoint;
pub mod error;
pub mod manifest;
pub mod model;
pub mod policy;
pub mod service;

#[cfg(test)]
pub(crate) mod binding_contract;
#[cfg(test)]
pub(crate) mod binding_test_support;
#[cfg(test)]
mod endpoint_tests;
#[cfg(test)]
mod planning_tests;
#[cfg(test)]
mod service_tests;
#[cfg(test)]
mod tests;

pub use binding::{
    BindingFuture, BindingRecord, BindingScope, BindingStoreError, EndpointBindingRepository,
    MAX_BINDING_BATCH, NativeEndpointBindingRepository, StoredBinding,
};
pub use endpoint::{validate_endpoint_url, validation_policy_version};
pub use error::{EndpointRejection, SetupError};
pub use manifest::{normalize_channel_setup, normalize_tool_setup};
pub use model::{
    CredentialName, ENDPOINT_ENVELOPE_VERSION, ENDPOINT_VALIDATION_POLICY_VERSION, EndpointField,
    EndpointKind, EndpointName, EndpointUrl, FieldKey, FieldKeyKind, MAX_ENDPOINT_KIND_BYTES,
    MAX_ENDPOINT_URL_BYTES, MAX_FIELD_KEY_BYTES, MAX_SETUP_FIELDS, SecretField, SetupField,
    SetupOperation,
};
pub use policy::{PlannedEffect, SetupPlan, plan_setup, validate_declarations};
pub use service::{BindingServiceError, EndpointBindingService, EndpointState, EndpointStatus};
