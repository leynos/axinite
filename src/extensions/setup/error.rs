//! Semantic setup failures and endpoint rejection categories.
//!
//! Every error carries a stable category and, where available, a declared
//! field identity. Submitted values, parser text, and backend error chains are
//! never included.

use super::model::{FieldKey, FieldKeyKind, MAX_FIELD_KEY_BYTES};

/// Why an endpoint URL failed validation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointRejection {
    /// The submitted value was blank.
    Empty,
    /// The submitted value exceeded the byte limit.
    TooLong,
    /// The value contains a control character, whitespace, or backslash.
    InvalidCharacter,
    /// The value is not an absolute URL.
    NotAbsolute,
    /// The URL parser rejected the value.
    Malformed,
    /// The scheme is not `https`.
    InsecureScheme,
    /// The URL has no host.
    MissingHost,
    /// The URL carries a username or password.
    CredentialsNotAllowed,
    /// The URL carries a query string.
    QueryNotAllowed,
    /// The URL carries a fragment.
    FragmentNotAllowed,
    /// The host names a local-only destination.
    LocalHostname,
    /// The host is an IP literal that is not publicly routable.
    NonPublicAddress,
}

impl EndpointRejection {
    /// Stable, value-free category for responses and logs.
    pub fn category(self) -> &'static str {
        match self {
            Self::Empty => "endpoint_empty",
            Self::TooLong => "endpoint_too_long",
            Self::InvalidCharacter => "endpoint_invalid_character",
            Self::NotAbsolute => "endpoint_not_absolute",
            Self::Malformed => "endpoint_malformed",
            Self::InsecureScheme => "endpoint_insecure_scheme",
            Self::MissingHost => "endpoint_missing_host",
            Self::CredentialsNotAllowed => "endpoint_credentials_not_allowed",
            Self::QueryNotAllowed => "endpoint_query_not_allowed",
            Self::FragmentNotAllowed => "endpoint_fragment_not_allowed",
            Self::LocalHostname => "endpoint_local_hostname",
            Self::NonPublicAddress => "endpoint_non_public_address",
        }
    }
}

/// Semantic setup failure. Carries no submitted value, parser text, or
/// backend error chain.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum SetupError {
    /// The declaration list exceeded the supported size.
    #[error("setup declares more than {max} fields")]
    TooManyFields {
        /// Maximum supported field count.
        max: usize,
    },
    /// The operation list exceeded the supported size.
    #[error("setup submits more than {max} operations")]
    TooManyOperations {
        /// Maximum supported operation count.
        max: usize,
    },
    /// A field key was empty.
    #[error("{kind} key must not be empty")]
    EmptyFieldKey {
        /// Which kind of key was empty.
        kind: FieldKeyKind,
    },
    /// A field key exceeded its byte limit.
    #[error("{kind} key exceeds {max} bytes")]
    FieldKeyTooLong {
        /// Which kind of key was too long.
        kind: FieldKeyKind,
        /// Maximum supported byte length.
        max: usize,
    },
    /// A field key contained a control character.
    #[error("{kind} key contains a control character")]
    InvalidFieldKey {
        /// Which kind of key was invalid.
        kind: FieldKeyKind,
    },
    /// Two declarations resolve to the same stored value.
    #[error("duplicate setup field {field}")]
    DuplicateField {
        /// The repeated declaration key.
        field: FieldKey,
    },
    /// Legacy and typed declarations were supplied together.
    #[error("setup must not mix required_secrets with typed fields")]
    MixedLegacyAndTypedDeclarations,
    /// An operation targets a field that is not declared.
    #[error("setup operation targets undeclared field {field}")]
    UndeclaredField {
        /// The undeclared key.
        field: FieldKey,
    },
    /// An operation targets a declaration of the other kind.
    #[error("setup operation uses the wrong kind for field {field}")]
    WrongFieldKind {
        /// The mismatched key.
        field: FieldKey,
    },
    /// Two operations target the same field.
    #[error("setup submits more than one operation for field {field}")]
    DuplicateOperation {
        /// The repeated key.
        field: FieldKey,
    },
    /// An endpoint set used a blank value instead of an explicit clear.
    #[error("endpoint field {field} has an empty value; clear it explicitly")]
    EmptyEndpoint {
        /// The endpoint key.
        field: FieldKey,
    },
    /// An endpoint URL failed validation.
    #[error("endpoint field {field} failed validation ({category})")]
    InvalidEndpoint {
        /// The endpoint key.
        field: FieldKey,
        /// Value-free rejection category.
        category: &'static str,
    },
}

impl SetupError {
    /// Stable, value-free category for responses and logs.
    pub fn category(&self) -> &'static str {
        match self {
            Self::TooManyFields { .. } => "too_many_fields",
            Self::TooManyOperations { .. } => "too_many_operations",
            Self::EmptyFieldKey { .. } => "empty_field_key",
            Self::FieldKeyTooLong { .. } => "field_key_too_long",
            Self::InvalidFieldKey { .. } => "invalid_field_key",
            Self::DuplicateField { .. } => "duplicate_field",
            Self::MixedLegacyAndTypedDeclarations => "mixed_declarations",
            Self::UndeclaredField { .. } => "undeclared_field",
            Self::WrongFieldKind { .. } => "wrong_field_kind",
            Self::DuplicateOperation { .. } => "duplicate_operation",
            Self::EmptyEndpoint { .. } => "endpoint_empty",
            Self::InvalidEndpoint { category, .. } => category,
        }
    }

    /// The declared field this failure concerns, when one is available.
    pub fn field(&self) -> Option<&FieldKey> {
        match self {
            Self::DuplicateField { field }
            | Self::UndeclaredField { field }
            | Self::WrongFieldKind { field }
            | Self::DuplicateOperation { field }
            | Self::EmptyEndpoint { field }
            | Self::InvalidEndpoint { field, .. } => Some(field),
            _ => None,
        }
    }
}

pub(super) fn validate_key(raw: &str, kind: FieldKeyKind) -> Result<(), SetupError> {
    if raw.is_empty() {
        return Err(SetupError::EmptyFieldKey { kind });
    }
    if raw.len() > MAX_FIELD_KEY_BYTES {
        return Err(SetupError::FieldKeyTooLong {
            kind,
            max: MAX_FIELD_KEY_BYTES,
        });
    }
    if raw.chars().any(char::is_control) {
        return Err(SetupError::InvalidFieldKey { kind });
    }
    Ok(())
}
