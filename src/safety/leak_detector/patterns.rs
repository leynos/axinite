//! Built-in leak detection patterns and prefix helpers.
//!
//! Provides the default set of secret patterns (API keys, tokens, private
//! keys) plus helpers for extracting literal prefixes used by the fast
//! Aho-Corasick pre-filter.

use regex::Regex;

use super::{LeakAction, LeakPattern, LeakSeverity};

/// Return `true` when either prefix is a leading substring of the other.
pub(super) fn prefixes_overlap(a: &str, b: &str) -> bool {
    a.starts_with(b) || b.starts_with(a)
}

/// Extract a literal prefix from a regex pattern (if one exists).
pub(super) fn extract_literal_prefix(pattern: &str) -> Option<String> {
    let mut prefix = String::new();

    for ch in pattern.chars() {
        match ch {
            // These start special regex constructs
            '[' | '(' | '.' | '*' | '+' | '?' | '{' | '|' | '^' | '$' => break,
            // Escape sequence
            '\\' => break,
            // Regular character
            _ => prefix.push(ch),
        }
    }

    if prefix.len() >= 3 {
        Some(prefix)
    } else {
        None
    }
}

/// Declarative specification for one built-in leak pattern.
type PatternSpec = (&'static str, &'static str, LeakSeverity, LeakAction);

/// Built-in leak pattern table: (name, regex, severity, action).
const DEFAULT_PATTERN_SPECS: &[PatternSpec] = &[
    // OpenAI API keys
    (
        "openai_api_key",
        r"sk-(?:proj-)?[a-zA-Z0-9]{20,}(?:T3BlbkFJ[a-zA-Z0-9_-]*)?",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // Anthropic API keys
    (
        "anthropic_api_key",
        r"sk-ant-api[a-zA-Z0-9_-]{90,}",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // AWS Access Key ID
    (
        "aws_access_key",
        r"AKIA[0-9A-Z]{16}",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // GitHub tokens
    (
        "github_token",
        r"gh[pousr]_[A-Za-z0-9_]{36,}",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // GitHub fine-grained PAT
    (
        "github_fine_grained_pat",
        r"github_pat_[a-zA-Z0-9]{22}_[a-zA-Z0-9]{59}",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // Stripe keys
    (
        "stripe_api_key",
        r"sk_(?:live|test)_[a-zA-Z0-9]{24,}",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // NEAR AI session tokens
    (
        "nearai_session",
        r"sess_[a-zA-Z0-9]{32,}",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // PEM private keys
    (
        "pem_private_key",
        r"-----BEGIN\s+(?:RSA\s+)?PRIVATE\s+KEY-----",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // SSH private keys
    (
        "ssh_private_key",
        r"-----BEGIN\s+(?:OPENSSH|EC|DSA)\s+PRIVATE\s+KEY-----",
        LeakSeverity::Critical,
        LeakAction::Block,
    ),
    // Google API keys
    (
        "google_api_key",
        r"AIza[0-9A-Za-z_-]{35}",
        LeakSeverity::High,
        LeakAction::Block,
    ),
    // Slack tokens
    (
        "slack_token",
        r"xox[baprs]-[0-9a-zA-Z-]{10,}",
        LeakSeverity::High,
        LeakAction::Block,
    ),
    // Twilio API keys
    (
        "twilio_api_key",
        r"SK[a-fA-F0-9]{32}",
        LeakSeverity::High,
        LeakAction::Block,
    ),
    // SendGrid API keys
    (
        "sendgrid_api_key",
        r"SG\.[a-zA-Z0-9_-]{22}\.[a-zA-Z0-9_-]{43}",
        LeakSeverity::High,
        LeakAction::Block,
    ),
    // Bearer tokens (redact instead of block, might be intentional)
    (
        "bearer_token",
        r"Bearer\s+[a-zA-Z0-9_-]{20,}",
        LeakSeverity::High,
        LeakAction::Redact,
    ),
    // Authorization header with key
    (
        "auth_header",
        r"(?i)authorization:\s*[a-zA-Z]+\s+[a-zA-Z0-9_-]{20,}",
        LeakSeverity::High,
        LeakAction::Redact,
    ),
    // High entropy hex (potential secrets, warn only)
    // Uses word boundary since look-around isn't supported in the regex crate.
    // This catches standalone 64-char hex strings (like SHA256 hashes used as secrets).
    (
        "high_entropy_hex",
        r"\b[a-fA-F0-9]{64}\b",
        LeakSeverity::Medium,
        LeakAction::Warn,
    ),
];

/// Build a [`LeakPattern`] from one table entry.
///
/// Returns `None` when the regex does not compile; every entry is a
/// compile-time constant exercised by the unit tests, so a failure is
/// logged and the pattern skipped rather than panicking.
fn build_pattern(spec: &PatternSpec) -> Option<LeakPattern> {
    let (name, regex, severity, action) = spec;
    match Regex::new(regex) {
        Ok(regex) => Some(LeakPattern {
            name: (*name).to_string(),
            regex,
            severity: *severity,
            action: *action,
        }),
        Err(e) => {
            tracing::error!("built-in leak pattern '{}' failed to compile: {}", name, e);
            None
        }
    }
}

/// Default leak detection patterns.
pub(super) fn default_patterns() -> Vec<LeakPattern> {
    DEFAULT_PATTERN_SPECS
        .iter()
        .filter_map(build_pattern)
        .collect()
}
