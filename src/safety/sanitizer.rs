//! Sanitizer for detecting and neutralizing prompt injection attempts.

use std::ops::Range;

use aho_corasick::AhoCorasick;
use regex::Regex;

use crate::safety::Severity;

/// Result of sanitizing external content.
#[derive(Debug, Clone)]
pub struct SanitizedOutput {
    /// The sanitized content.
    pub content: String,
    /// Warnings about potential injection attempts.
    pub warnings: Vec<InjectionWarning>,
    /// Whether the content was modified during sanitization.
    pub was_modified: bool,
}

/// Warning about a potential injection attempt.
#[derive(Debug, Clone)]
pub struct InjectionWarning {
    /// The pattern that was detected.
    pub pattern: String,
    /// Severity of the potential injection.
    pub severity: Severity,
    /// Location in the original content.
    pub location: Range<usize>,
    /// Human-readable description.
    pub description: String,
}

/// Sanitizer for external data.
pub struct Sanitizer {
    /// Fast pattern matcher for known injection patterns.
    ///
    /// `None` only if the matcher failed to build, which cannot happen for
    /// the compile-time constant pattern list; regex detection still runs.
    pattern_matcher: Option<AhoCorasick>,
    /// Patterns with their metadata.
    patterns: Vec<PatternInfo>,
    /// Regex patterns for more complex detection.
    regex_patterns: Vec<RegexPattern>,
}

struct PatternInfo {
    pattern: String,
    severity: Severity,
    description: String,
}

impl PatternInfo {
    /// Build a literal injection pattern with its severity and description.
    fn new(pattern: &str, severity: Severity, description: &str) -> Self {
        Self {
            pattern: pattern.to_string(),
            severity,
            description: description.to_string(),
        }
    }
}

struct RegexPattern {
    regex: Regex,
    name: String,
    severity: Severity,
    description: String,
}

impl RegexPattern {
    /// Build a named regex injection pattern with its severity and
    /// description. Panics on an invalid pattern; all patterns are
    /// compile-time constants exercised by unit tests.
    fn new(pattern: &str, name: &str, severity: Severity, description: &str) -> Self {
        Self {
            regex: Regex::new(pattern).unwrap(),
            name: name.to_string(),
            severity,
            description: description.to_string(),
        }
    }
}

/// Default literal injection patterns matched case-insensitively.
fn default_literal_patterns() -> Vec<PatternInfo> {
    use Severity::{Critical, High, Medium};
    vec![
        // Direct instruction injection
        PatternInfo::new(
            "ignore previous",
            High,
            "Attempt to override previous instructions",
        ),
        PatternInfo::new(
            "ignore all previous",
            Critical,
            "Attempt to override all previous instructions",
        ),
        PatternInfo::new("disregard", Medium, "Potential instruction override"),
        PatternInfo::new("forget everything", High, "Attempt to reset context"),
        // Role manipulation
        PatternInfo::new("you are now", High, "Attempt to change assistant role"),
        PatternInfo::new("act as", Medium, "Potential role manipulation"),
        PatternInfo::new("pretend to be", Medium, "Potential role manipulation"),
        // System message injection
        PatternInfo::new("system:", Critical, "Attempt to inject system message"),
        PatternInfo::new("assistant:", High, "Attempt to inject assistant response"),
        PatternInfo::new("user:", High, "Attempt to inject user message"),
        // Special tokens
        PatternInfo::new("<|", Critical, "Potential special token injection"),
        PatternInfo::new("|>", Critical, "Potential special token injection"),
        PatternInfo::new("[INST]", Critical, "Potential instruction token injection"),
        PatternInfo::new("[/INST]", Critical, "Potential instruction token injection"),
        // New instructions
        PatternInfo::new(
            "new instructions",
            High,
            "Attempt to provide new instructions",
        ),
        PatternInfo::new(
            "updated instructions",
            High,
            "Attempt to update instructions",
        ),
        // Code/command injection markers
        PatternInfo::new(
            "```system",
            High,
            "Potential code block instruction injection",
        ),
        PatternInfo::new(
            "```bash\nsudo",
            Medium,
            "Potential dangerous command injection",
        ),
    ]
}

/// Default regex patterns for more complex injection detection.
fn default_regex_patterns() -> Vec<RegexPattern> {
    use Severity::{Critical, High, Medium};
    vec![
        RegexPattern::new(
            r"(?i)base64[:\s]+[A-Za-z0-9+/=]{50,}",
            "base64_payload",
            Medium,
            "Potential encoded payload",
        ),
        RegexPattern::new(
            r"(?i)eval\s*\(",
            "eval_call",
            High,
            "Potential code evaluation attempt",
        ),
        RegexPattern::new(
            r"(?i)exec\s*\(",
            "exec_call",
            High,
            "Potential code execution attempt",
        ),
        RegexPattern::new(
            r"\x00",
            "null_byte",
            Critical,
            "Null byte injection attempt",
        ),
    ]
}

/// Build the case-insensitive Aho-Corasick matcher over the literal patterns.
///
/// Building from the small compile-time constant pattern list cannot exceed
/// aho-corasick's limits; log loudly rather than panic if that invariant is
/// ever broken so degraded matching is never silent.
fn build_pattern_matcher(patterns: &[PatternInfo]) -> Option<AhoCorasick> {
    let pattern_strings: Vec<&str> = patterns.iter().map(|p| p.pattern.as_str()).collect();
    AhoCorasick::builder()
        .ascii_case_insensitive(true)
        .build(&pattern_strings)
        .map_err(|error| {
            tracing::error!(
                %error,
                "Failed to build pattern matcher; literal injection-pattern \
                 detection is disabled"
            );
        })
        .ok()
}

impl Sanitizer {
    /// Create a new sanitizer with default patterns.
    pub fn new() -> Self {
        let patterns = default_literal_patterns();
        let pattern_matcher = build_pattern_matcher(&patterns);
        let regex_patterns = default_regex_patterns();

        Self {
            pattern_matcher,
            patterns,
            regex_patterns,
        }
    }

    /// Sanitize content by detecting and escaping potential injection attempts.
    pub fn sanitize(&self, content: &str) -> SanitizedOutput {
        let mut warnings = Vec::new();

        // Detect patterns using Aho-Corasick
        for mat in self
            .pattern_matcher
            .iter()
            .flat_map(|m| m.find_iter(content))
        {
            let pattern_info = &self.patterns[mat.pattern().as_usize()];
            warnings.push(InjectionWarning {
                pattern: pattern_info.pattern.clone(),
                severity: pattern_info.severity,
                location: mat.start()..mat.end(),
                description: pattern_info.description.clone(),
            });
        }

        // Detect regex patterns
        for pattern in &self.regex_patterns {
            for mat in pattern.regex.find_iter(content) {
                warnings.push(InjectionWarning {
                    pattern: pattern.name.clone(),
                    severity: pattern.severity,
                    location: mat.start()..mat.end(),
                    description: pattern.description.clone(),
                });
            }
        }

        // Sort warnings by severity (critical first)
        warnings.sort_by_key(|b| std::cmp::Reverse(b.severity));

        // Determine if we need to modify content
        let has_critical = warnings.iter().any(|w| w.severity == Severity::Critical);

        let (content, was_modified) = if has_critical {
            // For critical issues, escape the entire content
            (self.escape_content(content), true)
        } else {
            (content.to_string(), false)
        };

        SanitizedOutput {
            content,
            warnings,
            was_modified,
        }
    }

    /// Detect injection attempts without modifying content.
    pub fn detect(&self, content: &str) -> Vec<InjectionWarning> {
        self.sanitize(content).warnings
    }

    /// Whether the line opens with a chat role marker (`system:`, `user:`,
    /// or `assistant:`) once leading whitespace and case are ignored.
    fn starts_with_role_marker(line: &str) -> bool {
        let trimmed = line.trim_start().to_lowercase();
        ["system:", "user:", "assistant:"]
            .iter()
            .any(|marker| trimmed.starts_with(marker))
    }

    /// Escape content to neutralize potential injections.
    fn escape_content(&self, content: &str) -> String {
        // Replace special patterns with escaped versions
        let mut escaped = content.to_string();

        // Escape special tokens
        escaped = escaped.replace("<|", "\\<|");
        escaped = escaped.replace("|>", "|\\>");
        escaped = escaped.replace("[INST]", "\\[INST]");
        escaped = escaped.replace("[/INST]", "\\[/INST]");

        // Remove null bytes
        escaped = escaped.replace('\x00', "");

        // Escape role markers at the start of lines
        let lines: Vec<&str> = escaped.lines().collect();
        let escaped_lines: Vec<String> = lines
            .into_iter()
            .map(|line| {
                if Self::starts_with_role_marker(line) {
                    format!("[ESCAPED] {}", line)
                } else {
                    line.to_string()
                }
            })
            .collect();

        escaped_lines.join("\n")
    }
}

impl Default for Sanitizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests;
