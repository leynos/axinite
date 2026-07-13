//! Shell execution tool for running commands in a sandboxed environment.
//!
//! Provides controlled command execution with:
//! - Docker sandbox isolation (when enabled)
//! - Working directory isolation
//! - Timeout enforcement
//! - Output capture and truncation
//! - Blocked command patterns for safety
//! - Command injection/obfuscation detection
//! - Environment scrubbing (only safe vars forwarded to child processes)
//!
//! # Security Layers
//!
//! Commands pass through multiple validation stages before execution:
//!
//! ```text
//!   command string
//!       |
//!       v
//!   [blocked command check]  -- exact pattern match (rm -rf /, fork bomb, etc.)
//!       |
//!       v
//!   [dangerous pattern check] -- substring match (sudo, eval, $(curl, etc.)
//!       |
//!       v
//!   [injection detection]    -- obfuscation (base64|sh, DNS exfil, netcat, etc.)
//!       |
//!       v
//!   [sandbox or direct exec]
//!       |                  \
//!   (Docker container)   (host process with env scrubbing)
//! ```
//!
//! # Execution Modes
//!
//! When sandbox is available and enabled:
//! - Commands run inside ephemeral Docker containers
//! - Network traffic goes through a validating proxy
//! - Credentials are injected by the proxy, never exposed to commands
//!
//! When sandbox is unavailable:
//! - Commands run directly on host with scrubbed environment
//! - Only safe env vars (PATH, HOME, LANG, etc.) forwarded to child processes
//! - API keys, session tokens, and credentials are NOT inherited
//!
//! The module is split by concern: [`policy`] holds blocked-command patterns
//! and injection detection, [`tool`] the `ShellTool` type and its `NativeTool`
//! implementation, and [`exec`] the sandboxed and direct execution paths.

mod exec;
mod policy;
mod tool;

#[cfg(test)]
mod tests;

pub use policy::{detect_command_injection, requires_explicit_approval};
pub use tool::ShellTool;
