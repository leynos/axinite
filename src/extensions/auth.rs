//! Typed authentication state and results for extensions, including the
//! flat JSON wire format expected by the JS frontend.

use serde::ser::SerializeMap;
use serde::{Deserialize, Serialize};

use super::descriptor::ExtensionKind;

/// Auth readiness state for the extensions list UI.
///
/// Used by `check_tool_auth_status` and `check_channel_auth_status` to
/// communicate a tool's credential state to the list handler without
/// ambiguous `(bool, bool)` tuples.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolAuthState {
    /// Token/credentials are present — ready to use.
    Ready,
    /// Auth section exists but the access token is missing (OAuth not completed).
    NeedsAuth,
    /// Setup credentials (client_id/secret) must be configured before OAuth can start.
    NeedsSetup,
    /// No auth configuration at all (no capabilities or auth section).
    NoAuth,
}

/// The typed auth status, carrying only the data relevant to each state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthStatus {
    /// Authentication is complete; no further action needed.
    Authenticated,
    /// No authentication is required for this extension.
    NoAuthRequired,
    /// OAuth flow started — user must open `auth_url` in their browser.
    AwaitingAuthorization {
        auth_url: String,
        callback_type: String,
    },
    /// Waiting for user to provide a token/key manually.
    AwaitingToken {
        instructions: String,
        setup_url: Option<String>,
    },
    /// OAuth client credentials need to be configured before auth can proceed.
    NeedsSetup {
        instructions: String,
        setup_url: Option<String>,
    },
}

mod wire {
    //! Wire-format status tokens: the single source of truth shared by
    //! [`AuthStatus::as_str`](super::AuthStatus::as_str) and the
    //! [`AuthResult`](super::AuthResult) deserializer.

    pub(super) const AUTHENTICATED: &str = "authenticated";
    pub(super) const NO_AUTH_REQUIRED: &str = "no_auth_required";
    pub(super) const AWAITING_AUTHORIZATION: &str = "awaiting_authorization";
    pub(super) const AWAITING_TOKEN: &str = "awaiting_token";
    pub(super) const NEEDS_SETUP: &str = "needs_setup";

    /// Every wire status, in declaration order, for error reporting.
    pub(super) const ALL: &[&str] = &[
        AUTHENTICATED,
        NO_AUTH_REQUIRED,
        AWAITING_AUTHORIZATION,
        AWAITING_TOKEN,
        NEEDS_SETUP,
    ];
}

impl AuthStatus {
    /// The wire-format status string (backward-compatible with JS consumers).
    pub fn as_str(&self) -> &'static str {
        match self {
            AuthStatus::Authenticated => wire::AUTHENTICATED,
            AuthStatus::NoAuthRequired => wire::NO_AUTH_REQUIRED,
            AuthStatus::AwaitingAuthorization { .. } => wire::AWAITING_AUTHORIZATION,
            AuthStatus::AwaitingToken { .. } => wire::AWAITING_TOKEN,
            AuthStatus::NeedsSetup { .. } => wire::NEEDS_SETUP,
        }
    }
}

/// Result of authenticating an extension.
#[derive(Debug, Clone)]
pub struct AuthResult {
    pub name: String,
    pub kind: ExtensionKind,
    pub status: AuthStatus,
}

impl AuthResult {
    // ── Constructors ──────────────────────────────────────────────────

    /// Shared constructor: pair a name and kind with a typed status.
    fn with_status(name: impl Into<String>, kind: ExtensionKind, status: AuthStatus) -> Self {
        Self {
            name: name.into(),
            kind,
            status,
        }
    }

    /// Shared constructor for the two statuses that carry `instructions` and an
    /// optional `setup_url` (`AwaitingToken` and `NeedsSetup`); `to_status`
    /// selects the variant.
    fn with_instructions(
        name: impl Into<String>,
        kind: ExtensionKind,
        instructions: String,
        setup_url: Option<String>,
        to_status: impl FnOnce(String, Option<String>) -> AuthStatus,
    ) -> Self {
        Self::with_status(name, kind, to_status(instructions, setup_url))
    }

    pub fn authenticated(name: impl Into<String>, kind: ExtensionKind) -> Self {
        Self::with_status(name, kind, AuthStatus::Authenticated)
    }

    pub fn no_auth_required(name: impl Into<String>, kind: ExtensionKind) -> Self {
        Self::with_status(name, kind, AuthStatus::NoAuthRequired)
    }

    pub fn awaiting_authorization(
        name: impl Into<String>,
        kind: ExtensionKind,
        auth_url: String,
        callback_type: String,
    ) -> Self {
        Self::with_status(
            name,
            kind,
            AuthStatus::AwaitingAuthorization {
                auth_url,
                callback_type,
            },
        )
    }

    pub fn awaiting_token(
        name: impl Into<String>,
        kind: ExtensionKind,
        instructions: String,
        setup_url: Option<String>,
    ) -> Self {
        Self::with_instructions(
            name,
            kind,
            instructions,
            setup_url,
            |instructions, setup_url| AuthStatus::AwaitingToken {
                instructions,
                setup_url,
            },
        )
    }

    pub fn needs_setup(
        name: impl Into<String>,
        kind: ExtensionKind,
        instructions: String,
        setup_url: Option<String>,
    ) -> Self {
        Self::with_instructions(
            name,
            kind,
            instructions,
            setup_url,
            |instructions, setup_url| AuthStatus::NeedsSetup {
                instructions,
                setup_url,
            },
        )
    }

    // ── Accessors ─────────────────────────────────────────────────────

    pub fn is_authenticated(&self) -> bool {
        matches!(self.status, AuthStatus::Authenticated)
    }

    pub fn auth_url(&self) -> Option<&str> {
        match &self.status {
            AuthStatus::AwaitingAuthorization { auth_url, .. } => Some(auth_url),
            _ => None,
        }
    }

    pub fn callback_type(&self) -> Option<&str> {
        match &self.status {
            AuthStatus::AwaitingAuthorization { callback_type, .. } => Some(callback_type),
            _ => None,
        }
    }

    pub fn instructions(&self) -> Option<&str> {
        match &self.status {
            AuthStatus::AwaitingToken { instructions, .. }
            | AuthStatus::NeedsSetup { instructions, .. } => Some(instructions),
            _ => None,
        }
    }

    pub fn setup_url(&self) -> Option<&str> {
        match &self.status {
            AuthStatus::AwaitingToken { setup_url, .. }
            | AuthStatus::NeedsSetup { setup_url, .. } => setup_url.as_deref(),
            _ => None,
        }
    }

    pub fn is_awaiting_token(&self) -> bool {
        matches!(self.status, AuthStatus::AwaitingToken { .. })
    }

    pub fn status_str(&self) -> &'static str {
        self.status.as_str()
    }
}

/// Serialize `AuthResult` to the same flat JSON shape the JS frontend expects.
impl Serialize for AuthResult {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        // Count fields: name + kind + status + optional fields
        let optional_count = self.auth_url().is_some() as usize
            + self.callback_type().is_some() as usize
            + self.instructions().is_some() as usize
            + self.setup_url().is_some() as usize;
        let mut map = serializer.serialize_map(Some(4 + optional_count))?;

        map.serialize_entry("name", &self.name)?;
        map.serialize_entry("kind", &self.kind)?;
        serialize_opt_entry(&mut map, "auth_url", self.auth_url())?;
        serialize_opt_entry(&mut map, "callback_type", self.callback_type())?;
        serialize_opt_entry(&mut map, "instructions", self.instructions())?;
        serialize_opt_entry(&mut map, "setup_url", self.setup_url())?;
        map.serialize_entry("awaiting_token", &self.is_awaiting_token())?;
        map.serialize_entry("status", self.status_str())?;
        map.end()
    }
}

/// Serialize an optional string field, emitting the entry only when present.
fn serialize_opt_entry<M: SerializeMap>(
    map: &mut M,
    key: &'static str,
    value: Option<&str>,
) -> Result<(), M::Error> {
    if let Some(v) = value {
        map.serialize_entry(key, v)?;
    }
    Ok(())
}

/// Deserialize from the flat JSON shape back into the typed enum.
impl<'de> Deserialize<'de> for AuthResult {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// Flat helper matching the old JSON shape.
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct Raw {
            name: String,
            kind: ExtensionKind,
            #[serde(default)]
            auth_url: Option<String>,
            #[serde(default)]
            callback_type: Option<String>,
            #[serde(default)]
            instructions: Option<String>,
            #[serde(default)]
            setup_url: Option<String>,
            #[serde(default)]
            awaiting_token: bool,
            status: String,
        }

        let raw = Raw::deserialize(deserializer)?;
        // The two token-bearing variants share instructions + setup_url; a
        // constructor closure keeps their construction from repeating.
        let with_instructions =
            |to_status: fn(String, Option<String>) -> AuthStatus| -> AuthStatus {
                to_status(
                    raw.instructions.clone().unwrap_or_default(),
                    raw.setup_url.clone(),
                )
            };
        let status = match raw.status.as_str() {
            wire::AUTHENTICATED => AuthStatus::Authenticated,
            wire::NO_AUTH_REQUIRED => AuthStatus::NoAuthRequired,
            wire::AWAITING_AUTHORIZATION => AuthStatus::AwaitingAuthorization {
                auth_url: raw.auth_url.unwrap_or_default(),
                callback_type: raw.callback_type.unwrap_or_default(),
            },
            wire::AWAITING_TOKEN => {
                with_instructions(|instructions, setup_url| AuthStatus::AwaitingToken {
                    instructions,
                    setup_url,
                })
            }
            wire::NEEDS_SETUP => {
                with_instructions(|instructions, setup_url| AuthStatus::NeedsSetup {
                    instructions,
                    setup_url,
                })
            }
            other => {
                return Err(serde::de::Error::unknown_variant(other, wire::ALL));
            }
        };
        Ok(AuthResult {
            name: raw.name,
            kind: raw.kind,
            status,
        })
    }
}
