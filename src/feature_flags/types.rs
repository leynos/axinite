//! Validated identifiers shared by feature-flag commands and store ports.

/// Longest accepted deployment identifier.
pub const MAX_DEPLOYMENT_ID_LEN: usize = 64;

/// A validated deployment identifier, independent of its transport.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DeploymentId(String);

/// An identifier is empty, overlong, or outside the deployment alphabet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidDeploymentId;

impl DeploymentId {
    /// Validate an already normalized deployment identifier.
    pub fn parse(value: &str) -> Result<Self, InvalidDeploymentId> {
        if value.len() > MAX_DEPLOYMENT_ID_LEN || !has_identifier_alphabet(value) {
            return Err(InvalidDeploymentId);
        }
        Ok(Self(value.to_owned()))
    }

    /// The identifier used by the persistence adapter.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A validated flag name; unknown but syntactically valid names remain accepted.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FlagName(String);

/// A flag name is empty or outside the RFC 0009 identifier alphabet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidFlagName;

impl FlagName {
    /// Validate a flag name without coupling commands to the compiled catalogue.
    pub fn parse(value: &str) -> Result<Self, InvalidFlagName> {
        if !has_identifier_alphabet(value) {
            return Err(InvalidFlagName);
        }
        Ok(Self(value.to_owned()))
    }

    /// The flag name used by the persistence adapter.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn has_identifier_alphabet(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_'))
}

impl std::fmt::Display for InvalidDeploymentId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "deployment ID must be 1-64 characters of [a-z0-9_]"
        )
    }
}
impl std::error::Error for InvalidDeploymentId {}

impl std::fmt::Display for InvalidFlagName {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "flag name must contain only [a-z0-9_]+")
    }
}
impl std::error::Error for InvalidFlagName {}

impl std::borrow::Borrow<str> for DeploymentId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl std::borrow::Borrow<str> for FlagName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}
