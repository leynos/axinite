//! Extension setup DTOs: schema, tagged operations, and per-field outcomes.
//!
//! Endpoint values are write-only over the wire: a request may carry one, but
//! no response type here has a field for it.

use serde::{Deserialize, Serialize};

/// Setup schema for one extension: the fields it asks the user to provide.
#[derive(Debug, Serialize)]
pub struct ExtensionSetupResponse {
    /// Extension name the schema belongs to.
    pub name: String,
    /// Extension kind in snake_case, or empty when the extension is not installed.
    pub kind: String,
    /// Secret fields the extension requires, in declaration order.
    pub secrets: Vec<SecretFieldInfo>,
    /// Confidential endpoint fields, in declaration order.
    pub endpoints: Vec<EndpointFieldInfo>,
    /// Whether every required field is currently satisfied.
    pub ready: bool,
    /// Declarations that currently block readiness.
    pub blockers: Vec<SetupBlockerInfo>,
}

/// One secret an extension asks the user to provide during setup.
#[derive(Debug, Serialize)]
pub struct SecretFieldInfo {
    /// Secret name, the key expected in the setup request.
    pub name: String,
    /// Prompt text shown to the user when requesting the value.
    pub prompt: String,
    /// Whether the secret may be left blank.
    pub optional: bool,
    /// Whether this secret is already stored.
    pub provided: bool,
    /// Whether the secret will be auto-generated if left empty.
    pub auto_generate: bool,
}

/// One confidential endpoint an extension asks the user to configure.
///
/// The stored URL is never serialized; `state` reports only whether a binding
/// exists and authenticates.
#[derive(Debug, Serialize)]
pub struct EndpointFieldInfo {
    /// Endpoint name, the key expected in the setup request.
    pub name: String,
    /// Declarative endpoint kind from the manifest.
    pub kind: String,
    /// Prompt text shown to the user.
    pub prompt: String,
    /// Whether the endpoint may be left unset.
    pub optional: bool,
    /// `missing`, `configured`, `stale`, or `corrupt`.
    pub state: String,
}

/// One declaration that blocks setup readiness.
#[derive(Debug, Serialize)]
pub struct SetupBlockerInfo {
    /// Declared field name.
    pub field: String,
    /// `secret` or `endpoint`.
    pub kind: String,
    /// `missing`, `stale`, or `corrupt`.
    pub reason: String,
}

/// Endpoint operations submitted with a setup request.
#[derive(Debug, Default, Deserialize)]
pub struct EndpointOperations {
    /// Replacement URLs keyed by declared endpoint name.
    #[serde(default)]
    pub set: std::collections::HashMap<String, String>,
    /// Declared endpoint names to clear.
    #[serde(default)]
    pub clear: Vec<String>,
}

/// Request body submitting setup fields for an extension.
#[derive(Debug, Default, Deserialize)]
pub struct ExtensionSetupRequest {
    /// Secret values keyed by secret name. Names are checked against the
    /// extension's setup schema. A blank value keeps the stored one.
    #[serde(default)]
    pub secrets: std::collections::HashMap<String, String>,
    /// Endpoint replacements and clears.
    #[serde(default)]
    pub endpoints: EndpointOperations,
}

/// Result of one setup submission, with per-field outcomes.
#[derive(Debug, Serialize)]
pub struct ExtensionSetupSubmitResponse {
    /// Whether the submission completed without an ambiguous outcome.
    pub success: bool,
    /// Human-readable, value-free result.
    pub message: String,
    /// Whether the extension was activated.
    pub activated: bool,
    /// OAuth URL to open, when activation started a flow.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_url: Option<String>,
    /// Stable rejection category, when the submission was rejected.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Whether every required field is satisfied after the submission.
    pub ready: bool,
    /// Per-field outcomes in execution order.
    pub outcomes: Vec<SetupOutcomeInfo>,
    /// Endpoint state recomputed from the stores.
    pub endpoints: Vec<EndpointFieldInfo>,
    /// Declarations that still block readiness.
    pub blockers: Vec<SetupBlockerInfo>,
}

/// Outcome of one submitted field.
#[derive(Debug, Serialize)]
pub struct SetupOutcomeInfo {
    /// Declared field name.
    pub field: String,
    /// `secret` or `endpoint`.
    pub kind: String,
    /// `stored`, `kept`, `cleared`, `failed`, `indeterminate`, or `skipped`.
    pub outcome: String,
}
