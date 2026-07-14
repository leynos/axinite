//! Host functions for WASM sandbox.
//!
//! Implements a minimal, security-focused host API following VMLogic patterns
//! from NEAR blockchain. The principle is: deny by default, grant minimal capabilities.
//!
//! # Extended API (V2)
//!
//! In addition to the basic log/time/workspace functions, the host now provides:
//!
//! - **http_request**: Make HTTP requests to allowlisted endpoints with credential injection
//! - **tool_invoke**: Call other tools via aliases
//! - **secret_exists**: Check if a secret exists (never read values)
//!
//! # Security Architecture
//!
//! ```text
//! WASM Tool ──▶ Host Function ──▶ Allowlist ──▶ Credential ──▶ Execute
//! (untrusted)   (boundary)        Validator     Injector       Request
//!                                                    │
//!                                                    ▼
//!                              ◀────── Leak Detector ◀────── Response
//!                          (sanitized, no secrets)
//! ```

use std::time::{SystemTime, UNIX_EPOCH};

use crate::tools::wasm::capabilities::Capabilities;
use crate::tools::wasm::error::WasmError;

/// Maximum log entries per execution (prevents log spam attacks).
const MAX_LOG_ENTRIES: usize = 1000;

/// Maximum bytes per log message.
const MAX_LOG_MESSAGE_BYTES: usize = 4096;

/// Log levels matching the WIT interface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LogLevel::Trace => write!(f, "TRACE"),
            LogLevel::Debug => write!(f, "DEBUG"),
            LogLevel::Info => write!(f, "INFO"),
            LogLevel::Warn => write!(f, "WARN"),
            LogLevel::Error => write!(f, "ERROR"),
        }
    }
}

/// A single log entry from WASM execution.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    pub timestamp_millis: u64,
}

/// Host state maintained during WASM execution.
///
/// This is the "VMLogic" equivalent, it tracks all side effects and enforces limits.
/// Extended in V2 to support HTTP requests, tool invocation, and secret checks.
pub struct HostState {
    /// Collected log entries.
    logs: Vec<LogEntry>,
    /// Whether logging is still allowed (false after MAX_LOG_ENTRIES).
    logging_enabled: bool,
    /// Granted capabilities.
    capabilities: Capabilities,
    /// Count of log entries dropped due to rate limiting.
    logs_dropped: usize,
    /// User ID for secret/credential lookups.
    user_id: Option<String>,
    /// HTTP request count for rate limiting within this execution.
    http_request_count: u32,
    /// Tool invoke count for rate limiting within this execution.
    tool_invoke_count: u32,
}

impl std::fmt::Debug for HostState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostState")
            .field("logs_count", &self.logs.len())
            .field("logging_enabled", &self.logging_enabled)
            .field("logs_dropped", &self.logs_dropped)
            .field("user_id", &self.user_id)
            .field("http_request_count", &self.http_request_count)
            .field("tool_invoke_count", &self.tool_invoke_count)
            .finish()
    }
}

impl HostState {
    /// Create a new host state with the given capabilities.
    pub fn new(capabilities: Capabilities) -> Self {
        Self {
            logs: Vec::new(),
            logging_enabled: true,
            capabilities,
            logs_dropped: 0,
            user_id: None,
            http_request_count: 0,
            tool_invoke_count: 0,
        }
    }

    /// Create a new host state with user context.
    pub fn new_with_user(capabilities: Capabilities, user_id: impl Into<String>) -> Self {
        Self {
            logs: Vec::new(),
            logging_enabled: true,
            capabilities,
            logs_dropped: 0,
            user_id: Some(user_id.into()),
            http_request_count: 0,
            tool_invoke_count: 0,
        }
    }

    /// Create a minimal host state with no capabilities.
    pub fn minimal() -> Self {
        Self::new(Capabilities::default())
    }

    /// Get the user ID if set.
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Get the capabilities.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Log a message from WASM.
    ///
    /// Returns Ok(()) if logged, Err if rate limited or too long.
    pub fn log(&mut self, level: LogLevel, message: String) -> Result<(), WasmError> {
        if !self.logging_enabled {
            self.logs_dropped += 1;
            return Ok(()); // Silently drop, don't fail execution
        }

        if self.logs.len() >= MAX_LOG_ENTRIES {
            self.logging_enabled = false;
            self.logs_dropped += 1;
            tracing::warn!(
                "WASM log limit reached ({} entries), further logs dropped",
                MAX_LOG_ENTRIES
            );
            return Ok(());
        }

        // Truncate overly long messages
        let message = if message.len() > MAX_LOG_MESSAGE_BYTES {
            let mut truncated = message[..MAX_LOG_MESSAGE_BYTES].to_string();
            truncated.push_str("... (truncated)");
            truncated
        } else {
            message
        };

        let timestamp_millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        self.logs.push(LogEntry {
            level,
            message,
            timestamp_millis,
        });

        Ok(())
    }

    /// Get current timestamp in milliseconds.
    pub fn now_millis(&self) -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }

    /// Read from workspace if capability granted.
    pub fn workspace_read(&self, path: &str) -> Result<Option<String>, WasmError> {
        // Check if workspace capability is granted
        let capability = match &self.capabilities.workspace_read {
            Some(cap) => cap,
            None => return Ok(None), // No capability, return None
        };

        // Validate path (security critical)
        validate_workspace_path(path)?;

        // Check allowed prefixes if any are specified
        if !capability.allowed_prefixes.is_empty() {
            let allowed = capability
                .allowed_prefixes
                .iter()
                .any(|prefix| path.starts_with(prefix));
            if !allowed {
                tracing::debug!(
                    path = path,
                    allowed = ?capability.allowed_prefixes,
                    "WASM workspace read denied: path not in allowed prefixes"
                );
                return Ok(None);
            }
        }

        // Actually read from workspace
        match &capability.reader {
            Some(reader) => Ok(reader.read(path)),
            None => Ok(None), // No reader configured
        }
    }

    /// Get collected logs after execution.
    pub fn take_logs(&mut self) -> Vec<LogEntry> {
        std::mem::take(&mut self.logs)
    }

    /// Get number of logs dropped due to rate limiting.
    pub fn logs_dropped(&self) -> usize {
        self.logs_dropped
    }

    /// Check if a secret exists (does not expose value).
    ///
    /// Returns false if:
    /// - Secrets capability not granted
    /// - Secret name not in allowed list
    /// - User ID not set
    pub fn secret_exists(&self, name: &str) -> bool {
        let capability = match &self.capabilities.secrets {
            Some(cap) => cap,
            None => return false,
        };

        // Check if name is allowed
        capability.is_allowed(name)
    }

    /// Check if HTTP capability is available for a given URL and method.
    ///
    /// Returns an error message if not allowed.
    pub fn check_http_allowed(&self, url: &str, method: &str) -> Result<(), String> {
        let capability = self
            .capabilities
            .http
            .as_ref()
            .ok_or_else(|| "HTTP capability not granted".to_string())?;

        // Use the allowlist validator
        use crate::tools::wasm::allowlist::AllowlistValidator;

        let validator = AllowlistValidator::new(capability.allowlist.clone());
        let result = validator.validate(url, method);

        if result.is_allowed() {
            Ok(())
        } else {
            Err(format!("HTTP request not allowed: {:?}", result))
        }
    }

    /// Check if tool invocation is allowed for an alias.
    ///
    /// Returns the real tool name if allowed, error otherwise.
    pub fn check_tool_invoke_allowed(&self, alias: &str) -> Result<String, String> {
        let capability = self
            .capabilities
            .tool_invoke
            .as_ref()
            .ok_or_else(|| "Tool invocation capability not granted".to_string())?;

        capability
            .resolve_alias(alias)
            .map(|s| s.to_string())
            .ok_or_else(|| format!("Unknown tool alias: {}", alias))
    }

    /// Increment HTTP request counter and check rate limit.
    ///
    /// Returns error if rate limit exceeded.
    pub fn record_http_request(&mut self) -> Result<(), String> {
        // Verify HTTP capability exists
        let _capability = self
            .capabilities
            .http
            .as_ref()
            .ok_or_else(|| "HTTP capability not granted".to_string())?;

        self.http_request_count += 1;

        // Simple per-execution rate limit (additional to global rate limiter)
        // This prevents a single execution from making too many requests
        const MAX_REQUESTS_PER_EXECUTION: u32 = 50;
        if self.http_request_count > MAX_REQUESTS_PER_EXECUTION {
            return Err(format!(
                "Too many HTTP requests in single execution (max {})",
                MAX_REQUESTS_PER_EXECUTION
            ));
        }

        Ok(())
    }

    /// Increment tool invoke counter and check rate limit.
    ///
    /// Returns error if rate limit exceeded.
    pub fn record_tool_invoke(&mut self) -> Result<(), String> {
        self.tool_invoke_count += 1;

        const MAX_INVOKES_PER_EXECUTION: u32 = 20;
        if self.tool_invoke_count > MAX_INVOKES_PER_EXECUTION {
            return Err(format!(
                "Too many tool invocations in single execution (max {})",
                MAX_INVOKES_PER_EXECUTION
            ));
        }

        Ok(())
    }

    /// Get HTTP request count for this execution.
    pub fn http_request_count(&self) -> u32 {
        self.http_request_count
    }

    /// Get tool invoke count for this execution.
    pub fn tool_invoke_count(&self) -> u32 {
        self.tool_invoke_count
    }
}

mod paths;

#[cfg(test)]
mod tests;

use paths::validate_workspace_path;
