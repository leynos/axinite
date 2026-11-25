//! Pure domain values for extension setup fields and operations.
//!
//! The types here deliberately separate declarations (manifest-defined) from
//! submitted values. Confidential inputs never implement `Debug`,
//! `Serialize`, or `Clone`-and-print paths, so accidental logging of a
//! submitted endpoint URL is a type error rather than a review catch.

use std::fmt;

use super::error::{SetupError, validate_key};

/// Maximum number of declared fields or submitted operations per extension.
pub const MAX_SETUP_FIELDS: usize = 128;

/// Maximum UTF-8 byte length of a field key (secret or endpoint name).
pub const MAX_FIELD_KEY_BYTES: usize = 128;

/// Maximum UTF-8 byte length of an endpoint kind.
pub const MAX_ENDPOINT_KIND_BYTES: usize = 64;

/// Maximum UTF-8 byte length of a submitted endpoint URL.
pub const MAX_ENDPOINT_URL_BYTES: usize = 8192;

/// Version of the endpoint validation policy implemented by this crate.
///
/// Records carry the policy version under which they were validated. A record
/// whose stored version differs is reported as needing attention rather than
/// being silently revalidated.
pub const ENDPOINT_VALIDATION_POLICY_VERSION: u16 = 1;

/// Version of the encrypted endpoint envelope written by this crate.
pub const ENDPOINT_ENVELOPE_VERSION: u16 = 1;

/// A manifest-declared secret destination (a credential name).
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct CredentialName(String);

impl CredentialName {
    /// Validate and construct a credential name.
    pub fn parse(raw: impl Into<String>) -> Result<Self, SetupError> {
        let raw = raw.into();
        validate_key(&raw, FieldKeyKind::Secret)?;
        Ok(Self(raw))
    }

    /// Borrow the declared name.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Destination identity used by the secrets store, which folds case.
    pub fn destination(&self) -> String {
        self.0.to_lowercase()
    }
}

impl fmt::Display for CredentialName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for CredentialName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("CredentialName").field(&self.0).finish()
    }
}

/// A manifest-declared endpoint destination. The name is opaque metadata: it
/// must not be copied from a hostname, but this type cannot enforce that.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EndpointName(String);

impl EndpointName {
    /// Validate and construct an endpoint name.
    pub fn parse(raw: impl Into<String>) -> Result<Self, SetupError> {
        let raw = raw.into();
        validate_key(&raw, FieldKeyKind::Endpoint)?;
        Ok(Self(raw))
    }

    /// Borrow the declared name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EndpointName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for EndpointName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("EndpointName").field(&self.0).finish()
    }
}

/// Declarative endpoint kind, for example `jmap`.
///
/// The kind is metadata only. It never triggers discovery or a provider
/// exchange during setup.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct EndpointKind(String);

impl EndpointKind {
    /// Validate and construct an endpoint kind.
    pub fn parse(raw: impl Into<String>) -> Result<Self, SetupError> {
        let raw = raw.into();
        if raw.is_empty() {
            return Err(SetupError::EmptyFieldKey {
                kind: FieldKeyKind::EndpointKind,
            });
        }
        if raw.len() > MAX_ENDPOINT_KIND_BYTES {
            return Err(SetupError::FieldKeyTooLong {
                kind: FieldKeyKind::EndpointKind,
                max: MAX_ENDPOINT_KIND_BYTES,
            });
        }
        if raw.chars().any(char::is_control) {
            return Err(SetupError::InvalidFieldKey {
                kind: FieldKeyKind::EndpointKind,
            });
        }
        Ok(Self(raw))
    }

    /// Borrow the declared kind.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for EndpointKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl fmt::Debug for EndpointKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("EndpointKind").field(&self.0).finish()
    }
}

/// Which declaration a field key belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldKeyKind {
    /// A credential name.
    Secret,
    /// An endpoint name.
    Endpoint,
    /// An endpoint kind (only used by validation errors).
    EndpointKind,
}

impl fmt::Display for FieldKeyKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Secret => f.write_str("secret"),
            Self::Endpoint => f.write_str("endpoint"),
            Self::EndpointKind => f.write_str("endpoint kind"),
        }
    }
}

/// Identity of one declared setup field.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum FieldKey {
    /// A credential field.
    Secret(CredentialName),
    /// An endpoint field.
    Endpoint(EndpointName),
}

impl FieldKey {
    /// The declaration kind this key addresses.
    pub fn kind(&self) -> FieldKeyKind {
        match self {
            Self::Secret(_) => FieldKeyKind::Secret,
            Self::Endpoint(_) => FieldKeyKind::Endpoint,
        }
    }

    /// Borrow the underlying key text.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Secret(name) => name.as_str(),
            Self::Endpoint(name) => name.as_str(),
        }
    }
}

impl fmt::Display for FieldKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind(), self.as_str())
    }
}

/// A secret declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretField {
    /// Credential destination name.
    pub name: CredentialName,
    /// User-facing prompt from the manifest.
    pub prompt: String,
    /// Whether the user may skip the field.
    pub optional: bool,
    /// Whether the channel auto-generates a value when none is supplied.
    pub auto_generate: bool,
}

/// An endpoint declaration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointField {
    /// Opaque endpoint destination name.
    pub name: EndpointName,
    /// Declarative endpoint kind.
    pub kind: EndpointKind,
    /// User-facing prompt from the manifest.
    pub prompt: String,
    /// Whether the user may skip the field.
    pub optional: bool,
}

/// One manifest-declared setup field.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetupField {
    /// A credential field.
    Secret(SecretField),
    /// A confidential endpoint field.
    Endpoint(EndpointField),
}

impl SetupField {
    /// Identity of this declaration.
    pub fn key(&self) -> FieldKey {
        match self {
            Self::Secret(field) => FieldKey::Secret(field.name.clone()),
            Self::Endpoint(field) => FieldKey::Endpoint(field.name.clone()),
        }
    }

    /// Whether the field may be omitted.
    pub fn is_optional(&self) -> bool {
        match self {
            Self::Secret(field) => field.optional,
            Self::Endpoint(field) => field.optional,
        }
    }

    /// Whether the field declares auto-generation.
    pub fn auto_generate(&self) -> bool {
        match self {
            Self::Secret(field) => field.auto_generate,
            Self::Endpoint(_) => false,
        }
    }

    /// User-facing prompt from the manifest.
    pub fn prompt(&self) -> &str {
        match self {
            Self::Secret(field) => &field.prompt,
            Self::Endpoint(field) => &field.prompt,
        }
    }
}

/// A submitted setup operation, preserving duplicates until the planner
/// rejects them.
pub enum SetupOperation {
    /// Set a secret value. A blank value means "keep the stored value".
    Secret {
        /// Declared credential name.
        name: CredentialName,
        /// Submitted value, never printed by the domain layer.
        value: String,
    },
    /// Validate and replace an endpoint binding.
    EndpointSet {
        /// Declared endpoint name.
        name: EndpointName,
        /// Submitted URL, never printed by the domain layer.
        url: String,
    },
    /// Remove an endpoint binding. Idempotent.
    EndpointClear {
        /// Declared endpoint name.
        name: EndpointName,
    },
}

impl SetupOperation {
    /// Identity of the declaration this operation targets.
    pub fn field_key(&self) -> FieldKey {
        match self {
            Self::Secret { name, .. } => FieldKey::Secret(name.clone()),
            Self::EndpointSet { name, .. } | Self::EndpointClear { name } => {
                FieldKey::Endpoint(name.clone())
            }
        }
    }
}

impl fmt::Debug for SetupOperation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Secret { name, .. } => f
                .debug_struct("Secret")
                .field("name", name)
                .field("value", &"[REDACTED]")
                .finish(),
            Self::EndpointSet { name, .. } => f
                .debug_struct("EndpointSet")
                .field("name", name)
                .field("url", &"[REDACTED]")
                .finish(),
            Self::EndpointClear { name } => {
                f.debug_struct("EndpointClear").field("name", name).finish()
            }
        }
    }
}

/// A validated, canonical absolute HTTPS endpoint URL.
///
/// Validation is offline and never resolves DNS or contacts the target. The
/// value is confidential: `Debug` redacts it and it is never serialized.
#[derive(Clone, PartialEq, Eq)]
pub struct EndpointUrl(String);

impl EndpointUrl {
    /// Wrap a URL that the validation policy has already canonicalized.
    pub(super) fn from_canonical(canonical: String) -> Self {
        Self(canonical)
    }

    /// Borrow the canonical URL. Callers must keep the value inside the host
    /// process and must not log it.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for EndpointUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EndpointUrl([REDACTED])")
    }
}
