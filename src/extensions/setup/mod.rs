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
//!
//! The service and its storage adapters are added in a later milestone; this
//! module currently contains no database or HTTP dependency.

pub mod endpoint;
pub mod error;
pub mod manifest;
pub mod model;
pub mod policy;

#[cfg(test)]
mod endpoint_tests;
#[cfg(test)]
mod planning_tests;
#[cfg(test)]
mod tests;

pub use endpoint::{validate_endpoint_url, validation_policy_version};
pub use error::{EndpointRejection, SetupError};
pub use model::{
    ENDPOINT_ENVELOPE_VERSION, ENDPOINT_VALIDATION_POLICY_VERSION, EndpointField, EndpointKind,
    EndpointName, EndpointUrl, FieldKey, FieldKeyKind, MAX_ENDPOINT_KIND_BYTES,
    MAX_ENDPOINT_URL_BYTES, MAX_FIELD_KEY_BYTES, MAX_SETUP_FIELDS, SecretField, SetupField,
    SetupOperation,
};
pub use policy::{PlannedEffect, SetupPlan, plan_setup, validate_declarations};
