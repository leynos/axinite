//! The pure setup planner.
//!
//! Planning is a pure transformation from declarations plus submitted
//! operations into an ordered list of effects, which the service layer
//! executes. It performs no I/O and never contacts the provider.

use std::fmt;

use super::endpoint::validate_endpoint_url;
use super::error::SetupError;
use super::model::{
    CredentialName, EndpointField, EndpointKind, EndpointName, EndpointUrl, FieldKey,
    MAX_SETUP_FIELDS, SetupField, SetupOperation,
};

/// One effect the planner has resolved and the service must execute.
pub enum PlannedEffect {
    /// Replace a credential value.
    WriteSecret {
        /// Credential destination.
        name: CredentialName,
        /// Submitted value, never printed by the domain layer.
        value: String,
    },
    /// A blank secret submission is a no-op that keeps the stored value.
    KeepSecret {
        /// Credential destination left unchanged.
        name: CredentialName,
    },
    /// Validate and persist an endpoint binding.
    ReplaceEndpoint {
        /// Endpoint destination.
        name: EndpointName,
        /// Declared kind, recorded alongside the binding.
        kind: EndpointKind,
        /// Validated canonical URL.
        url: EndpointUrl,
    },
    /// Remove an endpoint binding. Idempotent.
    ClearEndpoint {
        /// Endpoint destination.
        name: EndpointName,
    },
}

impl PlannedEffect {
    /// The declaration this effect targets.
    pub fn field_key(&self) -> FieldKey {
        match self {
            Self::WriteSecret { name, .. } | Self::KeepSecret { name } => {
                FieldKey::Secret(name.clone())
            }
            Self::ReplaceEndpoint { name, .. } | Self::ClearEndpoint { name } => {
                FieldKey::Endpoint(name.clone())
            }
        }
    }

    /// Whether this effect mutates stored state.
    pub fn mutates(&self) -> bool {
        !matches!(self, Self::KeepSecret { .. })
    }
}

impl fmt::Debug for PlannedEffect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WriteSecret { name, .. } => f
                .debug_struct("WriteSecret")
                .field("name", name)
                .field("value", &"[REDACTED]")
                .finish(),
            Self::KeepSecret { name } => f.debug_struct("KeepSecret").field("name", name).finish(),
            Self::ReplaceEndpoint { name, kind, .. } => f
                .debug_struct("ReplaceEndpoint")
                .field("name", name)
                .field("kind", kind)
                .field("url", &"[REDACTED]")
                .finish(),
            Self::ClearEndpoint { name } => {
                f.debug_struct("ClearEndpoint").field("name", name).finish()
            }
        }
    }
}

/// A validated execution plan for one setup submission.
#[derive(Debug)]
pub struct SetupPlan {
    effects: Vec<PlannedEffect>,
    declarations: Vec<SetupField>,
}

impl SetupPlan {
    /// Ordered effects to execute.
    pub fn effects(&self) -> &[PlannedEffect] {
        &self.effects
    }

    /// Declarations the plan was built from.
    pub fn declarations(&self) -> &[SetupField] {
        &self.declarations
    }

    /// Consume the plan, yielding ordered effects.
    pub fn into_effects(self) -> Vec<PlannedEffect> {
        self.effects
    }

    /// Endpoint declarations that require a stored binding.
    pub fn required_endpoints(&self) -> impl Iterator<Item = &EndpointField> {
        self.declarations.iter().filter_map(|field| match field {
            SetupField::Endpoint(endpoint) if !endpoint.optional => Some(endpoint),
            _ => None,
        })
    }
}

/// Build an execution plan from declarations and submitted operations.
///
/// Omitted fields are untouched. A blank secret value keeps the stored
/// credential. An endpoint set must carry a value; clearing is explicit. Two
/// operations for the same field are rejected before any effect is produced,
/// so planning never partially applies a submission.
pub fn plan_setup(
    declarations: &[SetupField],
    operations: Vec<SetupOperation>,
) -> Result<SetupPlan, SetupError> {
    validate_declarations(declarations)?;
    validate_operations(declarations, &operations)?;
    let effects = resolve_operations(declarations, operations)?;
    Ok(SetupPlan {
        effects,
        declarations: declarations.to_vec(),
    })
}

fn validate_operations(
    declarations: &[SetupField],
    operations: &[SetupOperation],
) -> Result<(), SetupError> {
    if operations.len() > MAX_SETUP_FIELDS {
        return Err(SetupError::TooManyOperations {
            max: MAX_SETUP_FIELDS,
        });
    }

    let mut seen = Vec::with_capacity(operations.len());
    for operation in operations {
        let field = operation.field_key();
        if seen.contains(&field) {
            return Err(SetupError::DuplicateOperation { field });
        }
        seen.push(field.clone());
        check_declared(declarations, &field)?;
    }
    Ok(())
}

fn check_declared(declarations: &[SetupField], field: &FieldKey) -> Result<(), SetupError> {
    let declared = declarations
        .iter()
        .find(|declared| declared.key() == *field)
        .or_else(|| {
            declarations
                .iter()
                .find(|declared| declared.key().as_str() == field.as_str())
        });
    match declared {
        None => Err(SetupError::UndeclaredField {
            field: field.clone(),
        }),
        Some(declared) if declared.key() != *field => Err(SetupError::WrongFieldKind {
            field: field.clone(),
        }),
        Some(_) => Ok(()),
    }
}

fn resolve_operations(
    declarations: &[SetupField],
    operations: Vec<SetupOperation>,
) -> Result<Vec<PlannedEffect>, SetupError> {
    operations
        .into_iter()
        .map(|operation| resolve_operation(declarations, operation))
        .collect()
}

/// Validate a declaration set: bounded, uniquely keyed, and free of aliases.
///
/// Secret keys are compared through the case-folded identity the secrets
/// store uses, so `Token` and `token` cannot address two different values.
/// Endpoint names are compared exactly, matching their store key.
pub fn validate_declarations(declarations: &[SetupField]) -> Result<(), SetupError> {
    if declarations.len() > MAX_SETUP_FIELDS {
        return Err(SetupError::TooManyFields {
            max: MAX_SETUP_FIELDS,
        });
    }
    let mut secrets: Vec<String> = Vec::new();
    let mut endpoints: Vec<EndpointName> = Vec::new();
    for field in declarations {
        match field {
            SetupField::Secret(secret) => {
                let destination = secret.name.destination();
                if secrets.contains(&destination) {
                    return Err(SetupError::DuplicateField { field: field.key() });
                }
                secrets.push(destination);
            }
            SetupField::Endpoint(endpoint) => {
                if endpoints.contains(&endpoint.name) {
                    return Err(SetupError::DuplicateField { field: field.key() });
                }
                endpoints.push(endpoint.name.clone());
            }
        }
    }
    Ok(())
}

fn resolve_operation(
    declarations: &[SetupField],
    operation: SetupOperation,
) -> Result<PlannedEffect, SetupError> {
    match operation {
        SetupOperation::Secret { name, value } => Ok(secret_effect(name, value)),
        SetupOperation::EndpointClear { name } => Ok(PlannedEffect::ClearEndpoint { name }),
        SetupOperation::EndpointSet { name, url } => endpoint_effect(declarations, name, url),
    }
}

fn secret_effect(name: CredentialName, value: String) -> PlannedEffect {
    if value.trim().is_empty() {
        PlannedEffect::KeepSecret { name }
    } else {
        PlannedEffect::WriteSecret { name, value }
    }
}

fn endpoint_effect(
    declarations: &[SetupField],
    name: EndpointName,
    raw: String,
) -> Result<PlannedEffect, SetupError> {
    if raw.trim().is_empty() {
        return Err(SetupError::EmptyEndpoint {
            field: FieldKey::Endpoint(name),
        });
    }
    let kind = declared_endpoint_kind(declarations, &name)?;
    let url = validate_endpoint_url(&raw).map_err(|rejection| SetupError::InvalidEndpoint {
        field: FieldKey::Endpoint(name.clone()),
        category: rejection.category(),
    })?;
    Ok(PlannedEffect::ReplaceEndpoint { name, kind, url })
}

fn declared_endpoint_kind(
    declarations: &[SetupField],
    name: &EndpointName,
) -> Result<EndpointKind, SetupError> {
    declarations
        .iter()
        .find_map(|field| match field {
            SetupField::Endpoint(endpoint) if endpoint.name == *name => Some(endpoint.kind.clone()),
            _ => None,
        })
        .ok_or_else(|| SetupError::UndeclaredField {
            field: FieldKey::Endpoint(name.clone()),
        })
}
