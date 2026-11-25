//! Execution of validated setup submissions.
//!
//! Planning happens before any write. Every effect then executes in order and
//! records its own outcome; a failure stops later effects and marks them
//! skipped, so a response never claims success for work that did not run.
//! Mutations whose acknowledgement was lost are reported as indeterminate
//! rather than as failures, because the store may already have committed.

use crate::extensions::ExtensionError;
use crate::extensions::setup::{
    BindingScope, CredentialName, EndpointKind, EndpointName, EndpointUrl, FieldKey, PlannedEffect,
    SetupOperation, SetupPlan, plan_setup,
};
use crate::secrets::CreateSecretParams;

use super::ExtensionManager;
use super::setup_schema::{SetupSnapshot, setup_rejection};

/// What happened to one declared field during a submission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldOutcome {
    /// The value was written and acknowledged.
    Stored,
    /// A blank secret submission left the stored value unchanged.
    Kept,
    /// The binding was removed and acknowledged.
    Cleared,
    /// The write was refused before it could commit.
    Failed,
    /// The write may or may not have committed.
    Indeterminate,
    /// The effect did not run because an earlier effect failed.
    Skipped,
}

impl FieldOutcome {
    /// Stable, value-free label for responses and logs.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stored => "stored",
            Self::Kept => "kept",
            Self::Cleared => "cleared",
            Self::Failed => "failed",
            Self::Indeterminate => "indeterminate",
            Self::Skipped => "skipped",
        }
    }

    /// Whether the outcome leaves the field's stored state ambiguous.
    pub fn is_ambiguous(self) -> bool {
        matches!(self, Self::Failed | Self::Indeterminate)
    }
}

/// Outcome recorded for one declared field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FieldOutcomeReport {
    field: FieldKey,
    outcome: FieldOutcome,
}

impl FieldOutcomeReport {
    /// The declaration this outcome concerns.
    pub fn field(&self) -> &FieldKey {
        &self.field
    }

    /// What happened to it.
    pub fn outcome(&self) -> FieldOutcome {
        self.outcome
    }
}

/// Result of executing one setup submission.
#[derive(Clone, Debug)]
pub struct SetupApplication {
    outcomes: Vec<FieldOutcomeReport>,
    snapshot: SetupSnapshot,
    activated: bool,
    auth_url: Option<String>,
    message: String,
    credentials_changed: bool,
}

impl SetupApplication {
    /// Per-field outcomes in execution order.
    pub fn outcomes(&self) -> &[FieldOutcomeReport] {
        &self.outcomes
    }

    /// Status recomputed from the stores after execution.
    pub fn snapshot(&self) -> &SetupSnapshot {
        &self.snapshot
    }

    /// Whether the extension was activated.
    pub fn activated(&self) -> bool {
        self.activated
    }

    /// OAuth URL to complete, when activation started one.
    pub fn auth_url(&self) -> Option<&str> {
        self.auth_url.as_deref()
    }

    /// Human-readable, value-free result message.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Whether the submission wrote at least one credential.
    ///
    /// Activation purges a tool's OAuth grant only when this is true, so an
    /// endpoint-only edit keeps the existing token.
    pub fn credentials_changed(&self) -> bool {
        self.credentials_changed
    }
}

/// Mutable accumulator while effects execute.
pub(super) struct Execution {
    pub(super) outcomes: Vec<FieldOutcomeReport>,
    pub(super) wrote_secret: bool,
    pub(super) stopped: bool,
}

impl Execution {
    fn new() -> Self {
        Self {
            outcomes: Vec::new(),
            wrote_secret: false,
            stopped: false,
        }
    }

    pub(super) fn record(&mut self, field: FieldKey, outcome: FieldOutcome) {
        if outcome.is_ambiguous() {
            self.stopped = true;
        }
        self.outcomes.push(FieldOutcomeReport { field, outcome });
    }
}

impl ExtensionManager {
    /// Validate, execute, and report one setup submission.
    ///
    /// The declaration set is read first, then every operation is validated by
    /// the pure planner. A rejected batch therefore produces no effects. The
    /// returned snapshot is always re-read from the stores.
    pub async fn apply_setup(
        &self,
        name: &str,
        operations: Vec<SetupOperation>,
    ) -> Result<SetupApplication, ExtensionError> {
        let snapshot = self.setup_snapshot(name).await?;
        let plan = plan_setup(snapshot.declarations(), operations).map_err(setup_rejection)?;
        self.validate_provider_credentials(name, plan.effects())
            .await?;

        let mut execution = Execution::new();
        self.run_effects(name, &plan, &mut execution).await;
        self.auto_generate_missing(name, &mut execution).await;
        let snapshot = self.setup_snapshot(name).await?;
        let (message, activated, auth_url) = self
            .finish_setup(name, &snapshot, execution.wrote_secret)
            .await;

        Ok(SetupApplication {
            outcomes: execution.outcomes,
            snapshot,
            activated,
            auth_url,
            message,
            credentials_changed: execution.wrote_secret,
        })
    }

    async fn run_effects(&self, name: &str, plan: &SetupPlan, execution: &mut Execution) {
        for (index, effect) in plan.effects().iter().enumerate() {
            if execution.stopped {
                skip_effects(&plan.effects()[index..], execution);
                return;
            }
            self.apply_effect(name, effect, execution).await;
        }
    }

    async fn apply_effect(&self, name: &str, effect: &PlannedEffect, execution: &mut Execution) {
        match effect {
            PlannedEffect::WriteSecret { name: field, value } => {
                // `name` is the extension under setup; `field` is the
                // credential destination.
                self.write_secret(name, field, value, execution).await;
            }
            PlannedEffect::KeepSecret { name: field } => {
                execution.record(FieldKey::Secret(field.clone()), FieldOutcome::Kept);
            }
            PlannedEffect::ReplaceEndpoint {
                name: field,
                kind,
                url,
            } => {
                let target = EndpointEffect {
                    name: field,
                    kind,
                    url,
                };
                self.replace_endpoint(name, &target, execution).await;
            }
            PlannedEffect::ClearEndpoint { name: field } => {
                self.clear_endpoint(name, field, execution).await;
            }
        }
    }

    async fn write_secret(
        &self,
        provider: &str,
        field: &CredentialName,
        value: &str,
        execution: &mut Execution,
    ) {
        let params =
            CreateSecretParams::new(field.as_str(), value).with_provider(provider.to_string());
        let outcome = match self.secrets.create(&self.user_id, params).await {
            Ok(_) => {
                execution.wrote_secret = true;
                FieldOutcome::Stored
            }
            // The store may have committed before the acknowledgement was
            // lost, so the outcome is not reported as a definite failure.
            Err(_) => FieldOutcome::Indeterminate,
        };
        execution.record(FieldKey::Secret(field.clone()), outcome);
    }

    async fn replace_endpoint(
        &self,
        extension: &str,
        target: &EndpointEffect<'_>,
        execution: &mut Execution,
    ) {
        let field = target.name;
        let Some(service) = &self.endpoint_bindings else {
            execution.record(FieldKey::Endpoint(field.clone()), FieldOutcome::Failed);
            return;
        };
        let scope = BindingScope::new(&self.user_id, extension);
        let identity = crate::extensions::setup::BindingIdentity {
            name: field.clone(),
            kind: target.kind.clone(),
            envelope_version: crate::extensions::setup::ENDPOINT_ENVELOPE_VERSION,
            policy_version: crate::extensions::setup::validation_policy_version(),
        };
        let outcome = match service.replace(&scope, &identity, target.url).await {
            Ok(_) => FieldOutcome::Stored,
            Err(crate::extensions::setup::BindingServiceError::EncryptionFailed { .. }) => {
                FieldOutcome::Failed
            }
            // Storage refused the write or its acknowledgement: treat both as
            // ambiguous rather than claiming the previous binding survives.
            Err(_) => FieldOutcome::Indeterminate,
        };
        execution.record(FieldKey::Endpoint(field.clone()), outcome);
    }

    async fn clear_endpoint(
        &self,
        extension: &str,
        field: &EndpointName,
        execution: &mut Execution,
    ) {
        let Some(service) = &self.endpoint_bindings else {
            execution.record(FieldKey::Endpoint(field.clone()), FieldOutcome::Failed);
            return;
        };
        let scope = BindingScope::new(&self.user_id, extension);
        let outcome = match service.clear(&scope, field).await {
            Ok(_) => FieldOutcome::Cleared,
            // Clearing an absent binding is a success; a store error leaves
            // the stored state unknown.
            Err(_) => FieldOutcome::Indeterminate,
        };
        execution.record(FieldKey::Endpoint(field.clone()), outcome);
    }
}

/// One endpoint replacement, borrowed from the planned effect.
struct EndpointEffect<'a> {
    name: &'a EndpointName,
    kind: &'a EndpointKind,
    url: &'a EndpointUrl,
}

/// Mark every effect that did not run.
fn skip_effects(effects: &[PlannedEffect], execution: &mut Execution) {
    for effect in effects {
        execution.outcomes.push(FieldOutcomeReport {
            field: effect.field_key(),
            outcome: FieldOutcome::Skipped,
        });
    }
}
