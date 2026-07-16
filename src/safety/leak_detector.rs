//! Secret leak detection for WASM sandbox.
//!
//! Scans data at the sandbox boundary to prevent secret exfiltration.
//! Uses Aho-Corasick for fast multi-pattern matching plus regex for
//! complex patterns.
//!
//! # Security Model
//!
//! Leak detection happens at TWO points:
//!
//! 1. **Before outbound requests** - Prevents WASM from exfiltrating secrets
//!    by encoding them in URLs, headers, or request bodies
//! 2. **After responses/outputs** - Prevents accidental exposure in logs,
//!    tool outputs, or data returned to WASM
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                         WASM HTTP Request Flow                              │
//! │                                                                              │
//! │   WASM ──► Allowlist ──► Leak Scan ──► Credential ──► Execute ──► Response │
//! │            Validator     (request)     Injector       Request      │        │
//! │                                                                    ▼        │
//! │                                      WASM ◀── Leak Scan ◀── Response       │
//! │                                               (response)                    │
//! └─────────────────────────────────────────────────────────────────────────────┘
//!
//! ┌─────────────────────────────────────────────────────────────────────────────┐
//! │                           Scan Result Actions                               │
//! │                                                                              │
//! │   LeakDetector.scan() ──► LeakScanResult                                   │
//! │                               │                                             │
//! │                               ├─► clean: pass through                       │
//! │                               ├─► warn: log, pass                           │
//! │                               ├─► redact: mask secret                       │
//! │                               └─► block: reject entirely                    │
//! └─────────────────────────────────────────────────────────────────────────────┘
//! ```

use std::ops::Range;

use aho_corasick::AhoCorasick;
use regex::Regex;

mod patterns;
mod redaction;
#[cfg(test)]
mod tests;

use patterns::{default_patterns, extract_literal_prefix, prefixes_overlap};
use redaction::{apply_redactions, mask_secret};

/// Action to take when a leak is detected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeakAction {
    /// Block the output entirely (for critical secrets).
    Block,
    /// Redact the secret, replacing it with [REDACTED].
    Redact,
    /// Log a warning but allow the output.
    Warn,
}

impl std::fmt::Display for LeakAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeakAction::Block => write!(f, "block"),
            LeakAction::Redact => write!(f, "redact"),
            LeakAction::Warn => write!(f, "warn"),
        }
    }
}

/// Severity of a detected leak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LeakSeverity {
    Low,
    Medium,
    High,
    Critical,
}

impl std::fmt::Display for LeakSeverity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeakSeverity::Low => write!(f, "low"),
            LeakSeverity::Medium => write!(f, "medium"),
            LeakSeverity::High => write!(f, "high"),
            LeakSeverity::Critical => write!(f, "critical"),
        }
    }
}

/// A pattern for detecting secret leaks.
#[derive(Debug, Clone)]
pub struct LeakPattern {
    pub name: String,
    pub regex: Regex,
    pub severity: LeakSeverity,
    pub action: LeakAction,
}

/// A detected potential secret leak.
#[derive(Debug, Clone)]
pub struct LeakMatch {
    pub pattern_name: String,
    pub severity: LeakSeverity,
    pub action: LeakAction,
    /// Location in the scanned content.
    pub location: Range<usize>,
    /// A preview of the match with the secret partially masked.
    pub masked_preview: String,
}

/// Result of scanning content for leaks.
#[derive(Debug)]
pub struct LeakScanResult {
    /// All detected potential leaks.
    pub matches: Vec<LeakMatch>,
    /// Whether any match requires blocking.
    pub should_block: bool,
    /// Content with secrets redacted (if redaction was applied).
    pub redacted_content: Option<String>,
}

impl LeakScanResult {
    /// Check if content is clean (no leaks detected).
    pub fn is_clean(&self) -> bool {
        self.matches.is_empty()
    }

    /// Get the highest severity found.
    pub fn max_severity(&self) -> Option<LeakSeverity> {
        self.matches.iter().map(|m| m.severity).max()
    }
}

/// Detector for secret leaks in output data.
pub struct LeakDetector {
    patterns: Vec<LeakPattern>,
    /// For fast prefix matching of known patterns
    prefix_matcher: Option<AhoCorasick>,
    known_prefixes: Vec<(String, usize)>, // (prefix, pattern_index)
}

impl LeakDetector {
    /// Create a new detector with default patterns.
    pub fn new() -> Self {
        Self::with_patterns(default_patterns())
    }

    /// Create a detector with custom patterns.
    pub fn with_patterns(patterns: Vec<LeakPattern>) -> Self {
        // Build prefix matcher for patterns that start with a known prefix
        let mut prefixes = Vec::new();
        for (idx, pattern) in patterns.iter().enumerate() {
            if let Some(prefix) = extract_literal_prefix(pattern.regex.as_str())
                && prefix.len() >= 3
            {
                prefixes.push((prefix, idx));
            }
        }

        let prefix_matcher = if !prefixes.is_empty() {
            let prefix_strings: Vec<&str> = prefixes.iter().map(|(s, _)| s.as_str()).collect();
            AhoCorasick::builder()
                .ascii_case_insensitive(false)
                .build(&prefix_strings)
                .ok()
        } else {
            None
        };

        Self {
            patterns,
            prefix_matcher,
            known_prefixes: prefixes,
        }
    }

    /// Scan content for potential secret leaks.
    pub fn scan(&self, content: &str) -> LeakScanResult {
        let mut matches = Vec::new();
        let mut should_block = false;
        let mut redact_ranges = Vec::new();

        // Use prefix matcher for quick elimination
        let candidate_indices: Vec<usize> = if let Some(ref matcher) = self.prefix_matcher {
            let mut indices = Vec::new();
            for mat in matcher.find_iter(content) {
                let found_prefix = &self.known_prefixes[mat.pattern().as_usize()].0;
                // Add all patterns whose prefix overlaps with the found prefix.
                // This handles two cases:
                // 1. A short prefix shadows a longer one (e.g. "sk-" shadows "sk-ant-api")
                // 2. Duplicate prefixes mapping to different patterns (e.g. "-----BEGIN" for PEM and SSH)
                for (other_prefix, other_idx) in &self.known_prefixes {
                    if prefixes_overlap(other_prefix, found_prefix) && !indices.contains(other_idx)
                    {
                        indices.push(*other_idx);
                    }
                }
            }
            // Also include patterns without prefixes
            for (idx, _) in self.patterns.iter().enumerate() {
                if !self.known_prefixes.iter().any(|(_, i)| *i == idx) && !indices.contains(&idx) {
                    indices.push(idx);
                }
            }
            indices
        } else {
            (0..self.patterns.len()).collect()
        };

        // Check candidate patterns
        for idx in candidate_indices {
            let pattern = &self.patterns[idx];
            for mat in pattern.regex.find_iter(content) {
                let matched_text = mat.as_str();
                let location = mat.start()..mat.end();

                let leak_match = LeakMatch {
                    pattern_name: pattern.name.clone(),
                    severity: pattern.severity,
                    action: pattern.action,
                    location: location.clone(),
                    masked_preview: mask_secret(matched_text),
                };

                if pattern.action == LeakAction::Block {
                    should_block = true;
                }

                if pattern.action == LeakAction::Redact {
                    redact_ranges.push(location.clone());
                }

                matches.push(leak_match);
            }
        }

        // Sort by location for proper redaction
        matches.sort_by_key(|m| m.location.start);
        redact_ranges.sort_by_key(|r| r.start);

        // Build redacted content if needed
        let redacted_content = if !redact_ranges.is_empty() {
            Some(apply_redactions(content, &redact_ranges))
        } else {
            None
        };

        LeakScanResult {
            matches,
            should_block,
            redacted_content,
        }
    }

    /// Scan content and return cleaned version based on action.
    ///
    /// Returns `Err` if content should be blocked, `Ok(content)` otherwise.
    pub fn scan_and_clean(&self, content: &str) -> Result<String, LeakDetectionError> {
        let result = self.scan(content);

        if result.should_block {
            // Find the blocking match for error message
            let blocking_match = result
                .matches
                .iter()
                .find(|m| m.action == LeakAction::Block);
            return Err(LeakDetectionError::SecretLeakBlocked {
                pattern: blocking_match
                    .map(|m| m.pattern_name.clone())
                    .unwrap_or_default(),
                preview: blocking_match
                    .map(|m| m.masked_preview.clone())
                    .unwrap_or_default(),
            });
        }

        // Log warnings
        for m in &result.matches {
            if m.action == LeakAction::Warn {
                tracing::warn!(
                    pattern = %m.pattern_name,
                    severity = %m.severity,
                    preview = %m.masked_preview,
                    "Potential secret leak detected (warning only)"
                );
            }
        }

        // Return redacted content if any, otherwise original
        Ok(result
            .redacted_content
            .unwrap_or_else(|| content.to_string()))
    }

    /// Scan an outbound HTTP request for potential secret leakage.
    ///
    /// This MUST be called before executing any HTTP request from WASM
    /// to prevent exfiltration of secrets via URL, headers, or body.
    ///
    /// Returns `Err` if any part contains a blocked secret pattern.
    pub fn scan_http_request(
        &self,
        url: &str,
        headers: &[(String, String)],
        body: Option<&[u8]>,
    ) -> Result<(), LeakDetectionError> {
        // Scan URL (most common exfiltration vector)
        self.scan_and_clean(url)?;

        // Scan each header value
        for (name, value) in headers {
            self.scan_and_clean(value)
                .map_err(|e| LeakDetectionError::SecretLeakBlocked {
                    pattern: format!("header:{}", name),
                    preview: e.to_string(),
                })?;
        }

        // Scan body if present. Use lossy UTF-8 conversion so a leading
        // non-UTF8 byte can't be used to skip scanning entirely.
        if let Some(body_bytes) = body {
            let body_str = String::from_utf8_lossy(body_bytes);
            self.scan_and_clean(&body_str)?;
        }

        Ok(())
    }

    /// Add a custom pattern at runtime.
    pub fn add_pattern(&mut self, pattern: LeakPattern) {
        self.patterns.push(pattern);
        // Note: prefix_matcher won't be updated; rebuild if needed
    }

    /// Get the number of patterns.
    pub fn pattern_count(&self) -> usize {
        self.patterns.len()
    }
}

impl Default for LeakDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Error from leak detection.
#[derive(Debug, Clone, thiserror::Error)]
pub enum LeakDetectionError {
    #[error("Secret leak blocked: pattern '{pattern}' matched '{preview}'")]
    SecretLeakBlocked { pattern: String, preview: String },
}
