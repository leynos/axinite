//! WASM tool activation port.
//!
//! Isolates WASM tool loading, capability parsing, and tool-registry
//! registration behind a trait boundary.

use super::ActivationFuture;

/// Object-safe port for activating WASM tool extensions.
pub trait WasmToolActivationPort: Send + Sync {
    /// Load the named WASM tool from disk, register it, and return
    /// the activation result.
    fn activate_wasm_tool<'a>(&'a self, name: &'a str) -> ActivationFuture<'a>;
}
