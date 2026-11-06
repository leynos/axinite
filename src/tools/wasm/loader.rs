//! Generic WASM tool loader for loading tools from files or directories.
//!
//! This module provides a way to load WASM tools dynamically at runtime from:
//! - A directory containing `<name>.wasm` and `<name>.capabilities.json`
//! - Build artefacts in `tools-src/` (dev mode, auto-detected)
//! - Database storage (via [`WasmToolStore`](crate::tools::wasm::WasmToolStore))
//!
//! # Example: Loading from Directory
//!
//! ```text
//! ~/.axinite/tools/
//! ├── slack.wasm
//! ├── slack.capabilities.json
//! ├── github.wasm
//! └── github.capabilities.json
//! ```
//!
//! ```ignore
//! let loader = WasmToolLoader::new(runtime, registry);
//! loader.load_from_dir(Path::new("~/.axinite/tools/")).await?;
//! ```
//!
//! # Dev Mode
//!
//! When `load_dev_tools()` is called, the loader scans `tools-src/*/` for build
//! artefacts. Tools found there are loaded directly from the build output,
//! skipping the install directory. This means during development you just
//! rebuild the WASM and restart the host, no manual copy step needed.
//!
//! # Security
//!
//! Tools loaded from files are assigned `TrustLevel::User` by default, meaning
//! they run with the most restrictive permissions. Only tools explicitly marked
//! as `verified` or `system` in the database get elevated trust.

mod dev_tools;
mod discovery;
#[cfg(test)]
mod tests;
mod tool_loader;
mod wit_compat;

use std::path::PathBuf;

use crate::tools::registry::WasmRegistrationError;
use crate::tools::wasm::{WasmError, WasmStorageError};

pub use dev_tools::{
    discover_dev_tools, load_dev_tools, resolve_wasm_target_dir, wasm_artifact_path,
};
pub use discovery::discover_tools;
pub use tool_loader::WasmToolLoader;
pub use wit_compat::check_wit_version_compat;

/// Error during WASM tool loading.
#[derive(Debug, thiserror::Error)]
pub enum WasmLoadError {
    /// Reading a tool file failed; converted from `std::io::Error`.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The `.wasm` path passed to a loader does not exist. Carries that path.
    #[error("WASM file not found: {0}")]
    WasmNotFound(PathBuf),

    /// A capabilities file path was expected but is missing. The current loader
    /// does not raise this: a missing capabilities file means default deny.
    #[error("Capabilities file not found: {0}")]
    CapabilitiesNotFound(PathBuf),

    /// The `.capabilities.json` file failed to parse. Carries the parser message.
    #[error("Invalid capabilities JSON: {0}")]
    InvalidCapabilities(String),

    /// The WASM module failed to compile; wraps the underlying `WasmError`.
    #[error("WASM compilation error: {0}")]
    Compilation(#[from] WasmError),

    /// Reading the tool from the WASM tool store failed; wraps `WasmStorageError`.
    #[error("Storage error: {0}")]
    Storage(#[from] WasmStorageError),

    /// The tool could not be registered with the tool registry.
    #[error("Registration error: {0}")]
    Registration(#[from] WasmRegistrationError),

    /// The tool name is empty or contains a path separator. Carries the rejected name.
    #[error("Invalid tool name: {0}")]
    InvalidName(String),

    /// The declared WIT version cannot be used with the host. Carries a rebuild or
    /// update hint for the user.
    #[error("WIT version mismatch: {0}")]
    WitVersionMismatch(String),
}

/// Whether a name contains a path separator ('/' or '\\').
///
/// Names with separators could escape the plugin directory, so loaders
/// reject them outright.
pub(crate) fn contains_path_separator(name: &str) -> bool {
    name.contains('/') || name.contains('\\')
}

/// Results from loading multiple tools.
#[derive(Debug, Default)]
pub struct LoadResults {
    /// Names of successfully loaded tools.
    pub loaded: Vec<String>,

    /// Errors encountered (path/name, error).
    pub errors: Vec<(PathBuf, WasmLoadError)>,
}

impl LoadResults {
    /// Check if all tools loaded successfully.
    pub fn all_succeeded(&self) -> bool {
        self.errors.is_empty()
    }

    /// Get the count of successfully loaded tools.
    pub fn success_count(&self) -> usize {
        self.loaded.len()
    }

    /// Get the count of failed tools.
    pub fn error_count(&self) -> usize {
        self.errors.len()
    }
}

/// A discovered WASM tool (not yet loaded).
#[derive(Debug)]
pub struct DiscoveredTool {
    /// Path to the WASM file.
    pub wasm_path: PathBuf,

    /// Path to the capabilities file (if present).
    pub capabilities_path: Option<PathBuf>,
}
