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

struct RegexPattern {
    regex: Regex,
    name: String,
    severity: Severity,
    description: String,
}

impl Sanitizer {
    /// Create a new sanitizer with default patterns.
    pub fn new() -> Self {
        let patterns = vec![
            // Direct instruction injection
            PatternInfo {
                pattern: "ignore previous".to_string(),
                severity: Severity::High,
                description: "Attempt to override previous instructions".to_string(),
            },
            PatternInfo {
                pattern: "ignore all previous".to_string(),
                severity: Severity::Critical,
                description: "Attempt to override all previous instructions".to_string(),
            },
            PatternInfo {
                pattern: "disregard".to_string(),
                severity: Severity::Medium,
                description: "Potential instruction override".to_string(),
            },
            PatternInfo {
                pattern: "forget everything".to_string(),
                severity: Severity::High,
                description: "Attempt to reset context".to_string(),
            },
            // Role manipulation
            PatternInfo {
                pattern: "you are now".to_string(),
                severity: Severity::High,
                description: "Attempt to change assistant role".to_string(),
            },
            PatternInfo {
                pattern: "act as".to_string(),
                severity: Severity::Medium,
                description: "Potential role manipulation".to_string(),
            },
            PatternInfo {
                pattern: "pretend to be".to_string(),
                severity: Severity::Medium,
                description: "Potential role manipulation".to_string(),
            },
            // System message injection
            PatternInfo {
                pattern: "system:".to_string(),
                severity: Severity::Critical,
                description: "Attempt to inject system message".to_string(),
            },
            PatternInfo {
                pattern: "assistant:".to_string(),
                severity: Severity::High,
                description: "Attempt to inject assistant response".to_string(),
            },
            PatternInfo {
                pattern: "user:".to_string(),
                severity: Severity::High,
                description: "Attempt to inject user message".to_string(),
            },
            // Special tokens
            PatternInfo {
                pattern: "<|".to_string(),
                severity: Severity::Critical,
                description: "Potential special token injection".to_string(),
            },
            PatternInfo {
                pattern: "|>".to_string(),
                severity: Severity::Critical,
                description: "Potential special token injection".to_string(),
            },
            PatternInfo {
                pattern: "[INST]".to_string(),
                severity: Severity::Critical,
                description: "Potential instruction token injection".to_string(),
            },
            PatternInfo {
                pattern: "[/INST]".to_string(),
                severity: Severity::Critical,
                description: "Potential instruction token injection".to_string(),
            },
            // New instructions
            PatternInfo {
                pattern: "new instructions".to_string(),
                severity: Severity::High,
                description: "Attempt to provide new instructions".to_string(),
            },
            PatternInfo {
                pattern: "updated instructions".to_string(),
                severity: Severity::High,
                description: "Attempt to update instructions".to_string(),
            },
            // Code/command injection markers
            PatternInfo {
                pattern: "```system".to_string(),
                severity: Severity::High,
                description: "Potential code block instruction injection".to_string(),
            },
            PatternInfo {
                pattern: "```bash\nsudo".to_string(),
                severity: Severity::Medium,
                description: "Potential dangerous command injection".to_string(),
            },
        ];

        let pattern_strings: Vec<&str> = patterns.iter().map(|p| p.pattern.as_str()).collect();
        // Building from the small compile-time constant pattern list cannot
        // exceed aho-corasick's limits; log loudly rather than panic if that
        // invariant is ever broken so degraded matching is never silent.
        let pattern_matcher = AhoCorasick::builder()
            .ascii_case_insensitive(true)
            .build(&pattern_strings)
            .map_err(|error| {
                tracing::error!(
                    %error,
                    "Failed to build pattern matcher; literal injection-pattern \
                     detection is disabled"
                );
            })
            .ok();

        // Regex patterns for more complex detection
        let regex_patterns = vec![
            RegexPattern {
                regex: Regex::new(r"(?i)base64[:\s]+[A-Za-z0-9+/=]{50,}").unwrap(),
                name: "base64_payload".to_string(),
                severity: Severity::Medium,
                description: "Potential encoded payload".to_string(),
            },
            RegexPattern {
                regex: Regex::new(r"(?i)eval\s*\(").unwrap(),
                name: "eval_call".to_string(),
                severity: Severity::High,
                description: "Potential code evaluation attempt".to_string(),
            },
            RegexPattern {
                regex: Regex::new(r"(?i)exec\s*\(").unwrap(),
                name: "exec_call".to_string(),
                severity: Severity::High,
                description: "Potential code execution attempt".to_string(),
            },
            RegexPattern {
                regex: Regex::new(r"\x00").unwrap(),
                name: "null_byte".to_string(),
                severity: Severity::Critical,
                description: "Null byte injection attempt".to_string(),
            },
        ];

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
