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

/// Default leak detection patterns.
pub(super) fn default_patterns() -> Vec<LeakPattern> {
    vec![
        // OpenAI API keys
        LeakPattern {
            name: "openai_api_key".to_string(),
            regex: Regex::new(r"sk-(?:proj-)?[a-zA-Z0-9]{20,}(?:T3BlbkFJ[a-zA-Z0-9_-]*)?").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // Anthropic API keys
        LeakPattern {
            name: "anthropic_api_key".to_string(),
            regex: Regex::new(r"sk-ant-api[a-zA-Z0-9_-]{90,}").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // AWS Access Key ID
        LeakPattern {
            name: "aws_access_key".to_string(),
            regex: Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // GitHub tokens
        LeakPattern {
            name: "github_token".to_string(),
            regex: Regex::new(r"gh[pousr]_[A-Za-z0-9_]{36,}").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // GitHub fine-grained PAT
        LeakPattern {
            name: "github_fine_grained_pat".to_string(),
            regex: Regex::new(r"github_pat_[a-zA-Z0-9]{22}_[a-zA-Z0-9]{59}").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // Stripe keys
        LeakPattern {
            name: "stripe_api_key".to_string(),
            regex: Regex::new(r"sk_(?:live|test)_[a-zA-Z0-9]{24,}").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // NEAR AI session tokens
        LeakPattern {
            name: "nearai_session".to_string(),
            regex: Regex::new(r"sess_[a-zA-Z0-9]{32,}").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // PEM private keys
        LeakPattern {
            name: "pem_private_key".to_string(),
            regex: Regex::new(r"-----BEGIN\s+(?:RSA\s+)?PRIVATE\s+KEY-----").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // SSH private keys
        LeakPattern {
            name: "ssh_private_key".to_string(),
            regex: Regex::new(r"-----BEGIN\s+(?:OPENSSH|EC|DSA)\s+PRIVATE\s+KEY-----").unwrap(),
            severity: LeakSeverity::Critical,
            action: LeakAction::Block,
        },
        // Google API keys
        LeakPattern {
            name: "google_api_key".to_string(),
            regex: Regex::new(r"AIza[0-9A-Za-z_-]{35}").unwrap(),
            severity: LeakSeverity::High,
            action: LeakAction::Block,
        },
        // Slack tokens
        LeakPattern {
            name: "slack_token".to_string(),
            regex: Regex::new(r"xox[baprs]-[0-9a-zA-Z-]{10,}").unwrap(),
            severity: LeakSeverity::High,
            action: LeakAction::Block,
        },
        // Twilio API keys
        LeakPattern {
            name: "twilio_api_key".to_string(),
            regex: Regex::new(r"SK[a-fA-F0-9]{32}").unwrap(),
            severity: LeakSeverity::High,
            action: LeakAction::Block,
        },
        // SendGrid API keys
        LeakPattern {
            name: "sendgrid_api_key".to_string(),
            regex: Regex::new(r"SG\.[a-zA-Z0-9_-]{22}\.[a-zA-Z0-9_-]{43}").unwrap(),
            severity: LeakSeverity::High,
            action: LeakAction::Block,
        },
        // Bearer tokens (redact instead of block, might be intentional)
        LeakPattern {
            name: "bearer_token".to_string(),
            regex: Regex::new(r"Bearer\s+[a-zA-Z0-9_-]{20,}").unwrap(),
            severity: LeakSeverity::High,
            action: LeakAction::Redact,
        },
        // Authorization header with key
        LeakPattern {
            name: "auth_header".to_string(),
            regex: Regex::new(r"(?i)authorization:\s*[a-zA-Z]+\s+[a-zA-Z0-9_-]{20,}").unwrap(),
            severity: LeakSeverity::High,
            action: LeakAction::Redact,
        },
        // High entropy hex (potential secrets, warn only)
        // Uses word boundary since look-around isn't supported in the regex crate.
        // This catches standalone 64-char hex strings (like SHA256 hashes used as secrets).
        LeakPattern {
            name: "high_entropy_hex".to_string(),
            regex: Regex::new(r"\b[a-fA-F0-9]{64}\b").unwrap(),
            severity: LeakSeverity::Medium,
            action: LeakAction::Warn,
        },
    ]
}
