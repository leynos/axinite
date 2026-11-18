//! Normalization of capabilities manifests into typed setup declarations.
//!
//! This is the only place where the shipped `setup.required_secrets` form is
//! understood. Every runtime consumer works with [`SetupField`] values, so a
//! legacy secret declaration and a typed secret declaration are
//! indistinguishable after normalization.

use crate::channels::wasm::SecretSetupSchema;
use crate::channels::wasm::SetupSchema as ChannelSetupSchema;
use crate::tools::wasm::{ToolSecretSetupSchema, ToolSetupFieldSchema};

use super::error::SetupError;
use super::model::{
    CredentialName, EndpointField, EndpointKind, EndpointName, SecretField, SetupField,
};
use super::policy::validate_declarations;

/// Normalize a tool's setup schema into typed declarations.
///
/// A tool may use either the legacy `required_secrets` list or the typed
/// `fields` list. Supplying both is rejected rather than merged, because the
/// two forms could disagree about the same destination.
pub fn normalize_tool_setup(
    legacy: &[ToolSecretSetupSchema],
    typed: &[ToolSetupFieldSchema],
) -> Result<Vec<SetupField>, SetupError> {
    reject_mixed_declarations(legacy, typed)?;
    let declarations = if typed.is_empty() {
        normalize_legacy_secrets(legacy)?
    } else {
        normalize_typed_fields(typed)?
    };
    validate_declarations(&declarations)?;
    Ok(declarations)
}

/// Normalize a channel's setup schema into typed declarations.
///
/// Channels remain secret-only; a channel manifest cannot declare an
/// endpoint. Auto-generation is preserved from the manifest.
pub fn normalize_channel_setup(setup: &ChannelSetupSchema) -> Result<Vec<SetupField>, SetupError> {
    let declarations = setup
        .required_secrets
        .iter()
        .map(channel_secret)
        .collect::<Result<Vec<_>, _>>()?;
    validate_declarations(&declarations)?;
    Ok(declarations)
}

fn reject_mixed_declarations(
    legacy: &[ToolSecretSetupSchema],
    typed: &[ToolSetupFieldSchema],
) -> Result<(), SetupError> {
    if legacy.is_empty() || typed.is_empty() {
        return Ok(());
    }
    Err(SetupError::MixedLegacyAndTypedDeclarations)
}

fn normalize_legacy_secrets(
    legacy: &[ToolSecretSetupSchema],
) -> Result<Vec<SetupField>, SetupError> {
    legacy.iter().map(legacy_secret).collect()
}

fn legacy_secret(secret: &ToolSecretSetupSchema) -> Result<SetupField, SetupError> {
    Ok(SetupField::Secret(SecretField {
        name: CredentialName::parse(secret.name.clone())?,
        prompt: secret.prompt.clone(),
        optional: secret.optional,
        auto_generate: false,
    }))
}

fn channel_secret(secret: &SecretSetupSchema) -> Result<SetupField, SetupError> {
    Ok(SetupField::Secret(SecretField {
        name: CredentialName::parse(secret.name.clone())?,
        prompt: secret.prompt.clone(),
        optional: secret.optional,
        auto_generate: secret.auto_generate.is_some(),
    }))
}

fn normalize_typed_fields(typed: &[ToolSetupFieldSchema]) -> Result<Vec<SetupField>, SetupError> {
    typed.iter().map(normalize_tool_field).collect()
}

fn normalize_tool_field(field: &ToolSetupFieldSchema) -> Result<SetupField, SetupError> {
    match field {
        ToolSetupFieldSchema::Secret {
            name,
            prompt,
            optional,
        } => typed_secret(name, prompt, *optional),
        ToolSetupFieldSchema::Endpoint {
            name,
            kind,
            prompt,
            optional,
        } => typed_endpoint(name, kind, prompt, *optional),
    }
}

fn typed_secret(name: &str, prompt: &str, optional: bool) -> Result<SetupField, SetupError> {
    Ok(SetupField::Secret(SecretField {
        name: CredentialName::parse(name)?,
        prompt: prompt.to_string(),
        optional,
        auto_generate: false,
    }))
}

fn typed_endpoint(
    name: &str,
    kind: &str,
    prompt: &str,
    optional: bool,
) -> Result<SetupField, SetupError> {
    Ok(SetupField::Endpoint(EndpointField {
        name: EndpointName::parse(name)?,
        kind: EndpointKind::parse(kind)?,
        prompt: prompt.to_string(),
        optional,
    }))
}
