//! Tool registry for managing available tools.

mod builtins;
mod hosted;
mod loader;
mod names;
mod schema;
#[cfg(test)]
mod tests;
/// Helpers that compile and configure a WASM component into a
/// [`wasm_registration::PreparedWasmTool`] ready for registry insertion,
/// including credential-mapping extraction and guest-metadata recovery.
mod wasm_preparation;
#[cfg(test)]
mod wasm_preparation_tests;
/// Types and helpers for the WASM tool registration path, including
/// [`WasmToolRegistration`], [`WasmRegistrationError`], and
/// the `normalized_description` utility.
mod wasm_registration;
#[cfg(test)]
mod wasm_registration_tests;

pub use builtins::{ImageToolsRegistration, RegisterJobToolsOptions, VisionToolsRegistration};
pub use hosted::HostedToolLookupError;
pub use loader::{ToolRegistry, WasmFromStorageRegistration};
pub use names::PROTECTED_TOOL_NAMES;
pub use wasm_registration::{WasmRegistrationError, WasmToolRegistration};

/// Return `true` when `name` belongs to the protected built-in namespace.
pub fn is_protected_tool_name(name: &str) -> bool {
    names::is_protected_tool_name(name)
}
