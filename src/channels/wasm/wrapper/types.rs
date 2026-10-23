//! Domain newtypes for WASM wrapper values that carry security semantics.

use zeroize::{Zeroize, ZeroizeOnDrop};

/// A non-empty, lowercased host pattern used for credential matching
/// (e.g. `"api.slack.com"`, `"*.example.org"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct HostPattern(String);

impl HostPattern {
    /// Constructs a `HostPattern`, normalizing to lowercase.
    /// Returns `None` if `s` is empty.
    pub(super) fn new(s: impl Into<String>) -> Option<Self> {
        let s = s.into().to_lowercase();
        if s.is_empty() { None } else { Some(Self(s)) }
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for HostPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A raw secret value.
///
/// `Debug` is deliberately redacted to prevent accidental logging.
/// Memory is zeroed on `Drop` by the derived [`ZeroizeOnDrop`] implementation,
/// which overwrites the string's heap buffer with volatile writes and so needs
/// no handwritten unsafe code.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub(super) struct SecretValue(String);

impl SecretValue {
    pub(super) fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("[REDACTED]")
    }
}

/// A validated channel name (non-empty).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct ChannelName(String);

impl ChannelName {
    pub(super) fn new(s: impl Into<String>) -> Option<Self> {
        let s = s.into();
        if s.is_empty() { None } else { Some(Self(s)) }
    }

    pub(super) fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ChannelName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// Identifies where a credential-injection operation is being applied.
///
/// Used as a structured diagnostic label in place of a bare `&str` context
/// argument, preventing accidental substitution of arbitrary strings.
#[derive(Clone, Copy, Debug)]
pub(super) enum CredentialContext<'a> {
    /// The URL being resolved.
    Url,
    /// A named request header.
    Header(&'a str),
}

impl std::fmt::Display for CredentialContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Url => f.write_str("url"),
            Self::Header(name) => write!(f, "header:{}", name),
        }
    }
}

/// A validated HTTP method.
///
/// Replaces bare `&str` / `String` method arguments in internal helpers,
/// eliminating a source of stringly-typed interfaces.
#[derive(Clone, Copy, Debug)]
pub(super) enum HttpMethod {
    Get,
    Post,
    Put,
    Delete,
    Patch,
    Head,
}

impl HttpMethod {
    /// Parses a case-insensitive method string. Returns `None` for unknown methods.
    pub(super) fn from_str(s: &str) -> Option<Self> {
        match s.to_uppercase().as_str() {
            "GET" => Some(Self::Get),
            "POST" => Some(Self::Post),
            "PUT" => Some(Self::Put),
            "DELETE" => Some(Self::Delete),
            "PATCH" => Some(Self::Patch),
            "HEAD" => Some(Self::Head),
            _ => None,
        }
    }

    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Delete => "DELETE",
            Self::Patch => "PATCH",
            Self::Head => "HEAD",
        }
    }
}

impl std::fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The raw HTTP request parameters supplied by the WASM guest, before any
/// host-side credential injection or access-control checks.
///
/// Lifetime `'a` covers the borrowed method string, header JSON string, and
/// optional body slice; the URL is owned because it will be mutated during
/// injection.
pub(super) struct OutboundRequestSpec<'a> {
    /// HTTP method (e.g. `"GET"`, `"POST"`).
    pub(super) method: HttpMethod,
    /// Target URL; credential placeholders will be resolved by the host.
    pub(super) url: String,
    /// Request headers as a JSON object string.
    pub(super) headers_json: &'a str,
    /// Optional request body bytes.
    pub(super) body: Option<&'a [u8]>,
}

#[cfg(test)]
mod tests {
    //! `SecretValue` redaction, copy independence, and zeroing contract.

    use super::SecretValue;
    use zeroize::{Zeroize, ZeroizeOnDrop};

    /// Fails to compile unless `T` zeroes its memory when dropped, so removing
    /// the derive (and with it the guarantee) is a build error, not a silent
    /// change.
    const fn assert_zeroizes_on_drop<T: ZeroizeOnDrop>() {}

    const _: () = assert_zeroizes_on_drop::<SecretValue>();

    #[test]
    fn debug_output_is_redacted() {
        let secret = SecretValue::new("token-123");
        assert_eq!(format!("{secret:?}"), "[REDACTED]");
    }

    #[test]
    fn zeroizing_clears_the_value() {
        let mut secret = SecretValue::new("token-123");
        secret.zeroize();
        assert!(secret.is_empty());
        assert_eq!(secret.as_str(), "");
    }

    #[test]
    fn a_clone_is_zeroed_independently_of_the_original() {
        let original = SecretValue::new("token-123");
        let mut copy = original.clone();
        copy.zeroize();
        assert!(copy.is_empty());
        assert_eq!(original.as_str(), "token-123");
    }
}
