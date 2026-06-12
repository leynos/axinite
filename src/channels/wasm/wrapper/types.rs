//! Domain newtypes for WASM wrapper values that carry security semantics.

/// A non-empty, lowercased host pattern used for credential matching
/// (e.g. `"api.slack.com"`, `"*.example.org"`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(super) struct HostPattern(String);

impl HostPattern {
    /// Constructs a `HostPattern`, normalising to lowercase.
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
/// Memory is zeroed on `Drop`.
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

impl Drop for SecretValue {
    fn drop(&mut self) {
        // Zero memory to limit the window in which secret material lives.
        // SAFETY: overwriting valid UTF-8 bytes with zeros is safe here because
        // the String is being dropped immediately after.
        unsafe {
            let bytes = self.0.as_bytes_mut();
            std::ptr::write_bytes(bytes.as_mut_ptr(), 0, bytes.len());
        }
    }
}

impl Clone for SecretValue {
    fn clone(&self) -> Self {
        Self(self.0.clone())
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
