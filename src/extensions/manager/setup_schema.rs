//! Setup declarations and current state for one installed extension.
//!
//! A snapshot pairs the manifest's typed declarations with the state read
//! from the secrets store and the endpoint-binding store. It is the single
//! source of truth for readiness: a field is satisfied only when the stores
//! say so, and a store failure is an error rather than an empty answer.

use crate::extensions::setup::{
    BindingScope, CredentialName, EndpointName, EndpointState, EndpointStatus, FieldKey,
    SetupField, SetupOperation, normalize_channel_setup, normalize_tool_setup,
};
use crate::extensions::{ExtensionError, ExtensionKind};
use crate::tools::wasm::CapabilitiesFile;

use super::ExtensionManager;

/// Why one declared field does not satisfy its declaration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlockerReason {
    /// No secret value or endpoint binding is stored.
    Missing,
    /// A stored binding no longer matches its declaration.
    Stale,
    /// A stored binding cannot be authenticated.
    Corrupt,
}

impl BlockerReason {
    /// Stable, value-free category for responses and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Stale => "stale",
            Self::Corrupt => "corrupt",
        }
    }
}

impl From<EndpointState> for BlockerReason {
    fn from(state: EndpointState) -> Self {
        match state {
            EndpointState::Stale => Self::Stale,
            EndpointState::Corrupt => Self::Corrupt,
            EndpointState::Missing | EndpointState::Configured => Self::Missing,
        }
    }
}

/// One declaration that currently blocks readiness.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupBlocker {
    field: FieldKey,
    reason: BlockerReason,
}

impl SetupBlocker {
    /// The blocking declaration.
    pub fn field(&self) -> &FieldKey {
        &self.field
    }

    /// Why the declaration is unsatisfied.
    pub fn reason(&self) -> BlockerReason {
        self.reason
    }
}

/// State of one declared credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretStatus {
    name: CredentialName,
    provided: bool,
}

impl SecretStatus {
    /// The declared credential name.
    pub fn name(&self) -> &CredentialName {
        &self.name
    }

    /// Whether a value is stored for this credential.
    pub fn provided(&self) -> bool {
        self.provided
    }
}

/// Declarations plus the state read for every declared field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupSnapshot {
    declarations: Vec<SetupField>,
    secrets: Vec<SecretStatus>,
    endpoints: Vec<EndpointStatus>,
}

impl SetupSnapshot {
    /// The manifest's typed declarations, in declaration order.
    pub fn declarations(&self) -> &[SetupField] {
        &self.declarations
    }

    /// Stored state of every declared credential.
    pub fn secret_statuses(&self) -> &[SecretStatus] {
        &self.secrets
    }

    /// Stored state of every declared endpoint, in declaration order.
    pub fn endpoint_statuses(&self) -> &[EndpointStatus] {
        &self.endpoints
    }

    /// Whether a credential is stored, or `None` when it is not declared.
    pub fn secret_provided(&self, name: &CredentialName) -> Option<bool> {
        self.secrets
            .iter()
            .find(|status| status.name == *name)
            .map(SecretStatus::provided)
    }

    /// The state of a declared endpoint, or `None` when it is not declared.
    pub fn endpoint_state(&self, name: &EndpointName) -> Option<EndpointState> {
        self.endpoints
            .iter()
            .find(|status| status.field() == name)
            .map(EndpointStatus::state)
    }

    /// Whether every declaration is satisfied.
    ///
    /// An optional endpoint may be absent, but a stored binding that is stale
    /// or corrupt always blocks readiness, because its state is ambiguous.
    pub fn ready(&self) -> bool {
        self.blockers().is_empty()
    }

    /// Every declaration that currently blocks readiness.
    pub fn blockers(&self) -> Vec<SetupBlocker> {
        let mut blockers = Vec::new();
        for field in &self.declarations {
            if let Some(reason) = self.blocker_for(field) {
                blockers.push(SetupBlocker {
                    field: field.key(),
                    reason,
                });
            }
        }
        blockers
    }

    fn blocker_for(&self, field: &SetupField) -> Option<BlockerReason> {
        match field {
            SetupField::Secret(secret) => self.secret_blocker(secret),
            SetupField::Endpoint(endpoint) => self.endpoint_blocker(endpoint),
        }
    }

    fn secret_blocker(
        &self,
        secret: &crate::extensions::setup::SecretField,
    ) -> Option<BlockerReason> {
        if secret.optional || self.secret_provided(&secret.name) == Some(true) {
            return None;
        }
        Some(BlockerReason::Missing)
    }

    fn endpoint_blocker(
        &self,
        endpoint: &crate::extensions::setup::EndpointField,
    ) -> Option<BlockerReason> {
        match self.endpoint_state(&endpoint.name) {
            Some(EndpointState::Configured) => None,
            Some(EndpointState::Missing) if endpoint.optional => None,
            Some(state) => Some(BlockerReason::from(state)),
            None if endpoint.optional => None,
            None => Some(BlockerReason::Missing),
        }
    }
}

impl ExtensionManager {
    /// Read the declarations and stored state for one installed extension.
    ///
    /// A repository or manifest failure is returned as an error: readiness is
    /// never inferred from an unreadable store.
    pub async fn setup_snapshot(&self, name: &str) -> Result<SetupSnapshot, ExtensionError> {
        let kind = self.determine_installed_kind(name).await?;
        let declarations = self.setup_declarations(name, kind).await?;
        let secrets = self.secret_statuses(&declarations).await;
        let endpoints = self.endpoint_state_list(name, &declarations).await?;
        Ok(SetupSnapshot {
            declarations,
            secrets,
            endpoints,
        })
    }

    /// Build the setup snapshot and then plan the operations against it.
    pub async fn plan_submission(
        &self,
        name: &str,
        operations: Vec<SetupOperation>,
    ) -> Result<crate::extensions::setup::SetupPlan, ExtensionError> {
        let snapshot = self.setup_snapshot(name).await?;
        crate::extensions::setup::plan_setup(snapshot.declarations(), operations)
            .map_err(setup_rejection)
    }

    async fn setup_declarations(
        &self,
        name: &str,
        kind: ExtensionKind,
    ) -> Result<Vec<SetupField>, ExtensionError> {
        match kind {
            ExtensionKind::WasmChannel => self.channel_declarations(name).await,
            ExtensionKind::WasmTool => self.tool_declarations(name).await,
            _ => Ok(Vec::new()),
        }
    }

    async fn channel_declarations(&self, name: &str) -> Result<Vec<SetupField>, ExtensionError> {
        let Some(cap_file) = self.load_channel_capabilities(name).await? else {
            return Ok(Vec::new());
        };
        normalize_channel_setup(&cap_file.setup).map_err(setup_rejection)
    }

    /// Declarations for a tool, excluding OAuth client fields the host
    /// resolves automatically.
    async fn tool_declarations(&self, name: &str) -> Result<Vec<SetupField>, ExtensionError> {
        let Some(cap_file) = self.load_tool_capabilities(name).await else {
            return Ok(Vec::new());
        };
        let Some(setup) = &cap_file.setup else {
            return Ok(Vec::new());
        };
        let declarations = normalize_tool_setup(&setup.required_secrets, &setup.fields)
            .map_err(setup_rejection)?;
        Ok(filter_auto_resolved(declarations, &cap_file))
    }

    async fn secret_statuses(&self, declarations: &[SetupField]) -> Vec<SecretStatus> {
        let mut statuses = Vec::new();
        for field in declarations {
            let SetupField::Secret(secret) = field else {
                continue;
            };
            let provided = self
                .secrets
                .exists(&self.user_id, &secret.name.destination())
                .await
                .unwrap_or(false);
            statuses.push(SecretStatus {
                name: secret.name.clone(),
                provided,
            });
        }
        statuses
    }

    async fn endpoint_state_list(
        &self,
        name: &str,
        declarations: &[SetupField],
    ) -> Result<Vec<EndpointStatus>, ExtensionError> {
        let declares_endpoint = declarations
            .iter()
            .any(|field| matches!(field, SetupField::Endpoint(_)));
        if !declares_endpoint {
            return Ok(Vec::new());
        }
        let Some(service) = &self.endpoint_bindings else {
            return Err(ExtensionError::Other(
                "Endpoint binding storage is unavailable".to_string(),
            ));
        };
        let scope = BindingScope::new(&self.user_id, name);
        service
            .statuses(&scope, declarations)
            .await
            .map_err(|e| ExtensionError::Other(e.to_string()))
    }
}

/// Drop declarations for OAuth client fields the host supplies itself.
fn filter_auto_resolved(
    declarations: Vec<SetupField>,
    cap_file: &CapabilitiesFile,
) -> Vec<SetupField> {
    declarations
        .into_iter()
        .filter(|field| match field {
            SetupField::Secret(secret) => {
                !ExtensionManager::is_auto_resolved_oauth_field(secret.name.as_str(), cap_file)
            }
            SetupField::Endpoint(_) => true,
        })
        .collect()
}

/// Turn a planning failure into the manager's typed rejection.
pub(super) fn setup_rejection(error: crate::extensions::setup::SetupError) -> ExtensionError {
    ExtensionError::SetupRejected {
        category: error.category(),
        field: error.field().map(ToString::to_string),
    }
}
