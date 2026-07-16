//! Declarative hook bundle configuration types, parse errors, and shared
//! timeout validation for bundled hooks.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::hooks::{HookFailureMode, HookPoint};

pub(super) const MAX_HOOK_TIMEOUT_MS: u64 = 30_000;

/// Errors while parsing or compiling declarative hook bundles.
#[derive(Debug, thiserror::Error)]
pub enum HookBundleError {
    #[error("Invalid hook bundle format: {0}")]
    InvalidFormat(String),

    #[error("Hook '{hook}' must declare at least one hook point")]
    MissingHookPoints { hook: String },

    #[error("Hook '{hook}' has invalid regex '{pattern}': {reason}")]
    InvalidRegex {
        hook: String,
        pattern: String,
        reason: String,
    },

    #[error("Hook '{hook}' timeout must be between 1 and {max_ms} ms")]
    InvalidTimeout { hook: String, max_ms: u64 },

    #[error("Outbound webhook hook '{hook}' has invalid url: {url}")]
    InvalidWebhookUrl { hook: String, url: String },

    #[error("Outbound webhook hook '{hook}' must use https, got '{scheme}'")]
    InvalidWebhookScheme { hook: String, scheme: String },

    #[error("Outbound webhook hook '{hook}' cannot target host '{host}'")]
    ForbiddenWebhookHost { hook: String, host: String },

    #[error("Outbound webhook hook '{hook}' has invalid header '{header}': {reason}")]
    InvalidWebhookHeader {
        hook: String,
        header: String,
        reason: String,
    },

    #[error("Outbound webhook hook '{hook}' cannot set restricted header '{header}'")]
    ForbiddenWebhookHeader { hook: String, header: String },

    #[error("Outbound webhook hook '{hook}' max_in_flight must be at least 1")]
    InvalidWebhookMaxInFlight { hook: String },
}

/// A declarative hook bundle loaded from workspace files or extension capabilities.
///
/// Supports two bundled hook types:
/// - Rule hooks (`rules`) for reject/regex transform/prepend/append logic
/// - Outbound webhook hooks (`outbound_webhooks`) for fire-and-forget event delivery
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HookBundleConfig {
    /// Declarative content/tool/session rules.
    #[serde(default)]
    pub rules: Vec<HookRuleConfig>,
    /// Fire-and-forget webhook notifications on selected hook points.
    #[serde(default)]
    pub outbound_webhooks: Vec<OutboundWebhookConfig>,
}

impl HookBundleConfig {
    /// Parse a hook bundle from JSON value.
    ///
    /// Accepts either:
    /// - object form: `{ "rules": [...], "outbound_webhooks": [...] }`
    /// - array form:  `[ {rule}, {rule} ]` (shorthand for rules only)
    pub fn from_value(value: &serde_json::Value) -> Result<Self, HookBundleError> {
        if value.is_array() {
            let rules: Vec<HookRuleConfig> = serde_json::from_value(value.clone())
                .map_err(|e| HookBundleError::InvalidFormat(e.to_string()))?;
            return Ok(Self {
                rules,
                outbound_webhooks: Vec::new(),
            });
        }

        serde_json::from_value(value.clone())
            .map_err(|e| HookBundleError::InvalidFormat(e.to_string()))
    }
}

/// Summary of hook registrations performed from a bundle.
#[derive(Debug, Default, Clone, Copy)]
pub struct HookRegistrationSummary {
    /// Number of non-webhook hook registrations (audit/rule hooks).
    pub hooks: usize,
    /// Number of outbound webhook hook registrations.
    pub outbound_webhooks: usize,
    /// Number of invalid/failed registrations skipped.
    pub errors: usize,
}

impl HookRegistrationSummary {
    /// Total number of hooks successfully registered.
    pub fn total_registered(&self) -> usize {
        self.hooks + self.outbound_webhooks
    }

    pub fn merge(&mut self, other: HookRegistrationSummary) {
        self.hooks += other.hooks;
        self.outbound_webhooks += other.outbound_webhooks;
        self.errors += other.errors;
    }
}

/// Declarative regex/string rule hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookRuleConfig {
    /// Stable hook name (scoped with source during registration).
    pub name: String,
    /// Lifecycle points where this rule applies.
    pub points: Vec<HookPoint>,
    /// Optional priority override (lower runs first).
    #[serde(default)]
    pub priority: Option<u32>,
    /// Failure handling mode (default fail_open).
    #[serde(default)]
    pub failure_mode: Option<HookFailureMode>,
    /// Optional timeout override for this hook in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Optional regex guard. If provided and no match, rule is a no-op.
    #[serde(default)]
    pub when_regex: Option<String>,
    /// Optional immediate reject reason if guard matches.
    #[serde(default)]
    pub reject_reason: Option<String>,
    /// Regex replacements applied in order.
    #[serde(default)]
    pub replacements: Vec<RegexReplacementConfig>,
    /// Text prepended to the event's primary content.
    #[serde(default)]
    pub prepend: Option<String>,
    /// Text appended to the event's primary content.
    #[serde(default)]
    pub append: Option<String>,
}

/// A single regex replacement step in a rule hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegexReplacementConfig {
    pub pattern: String,
    pub replacement: String,
}

/// Declarative fire-and-forget outbound webhook hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutboundWebhookConfig {
    /// Stable webhook hook name (scoped with source during registration).
    pub name: String,
    /// Lifecycle points that trigger this webhook.
    pub points: Vec<HookPoint>,
    /// Target URL.
    pub url: String,
    /// Optional static headers.
    #[serde(default)]
    pub headers: HashMap<String, String>,
    /// Optional timeout override in milliseconds.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Optional priority override (lower runs first).
    #[serde(default)]
    pub priority: Option<u32>,
    /// Optional max number of concurrent in-flight deliveries.
    #[serde(default)]
    pub max_in_flight: Option<usize>,
}

pub(super) fn timeout_from_ms(
    timeout_ms: Option<u64>,
    hook_name: &str,
) -> Result<Duration, HookBundleError> {
    if let Some(ms) = timeout_ms {
        if ms == 0 || ms > MAX_HOOK_TIMEOUT_MS {
            return Err(HookBundleError::InvalidTimeout {
                hook: hook_name.to_string(),
                max_ms: MAX_HOOK_TIMEOUT_MS,
            });
        }
        Ok(Duration::from_millis(ms))
    } else {
        Ok(Duration::from_secs(5))
    }
}
