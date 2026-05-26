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
