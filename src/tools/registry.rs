//! Tool registry for managing available tools.

mod builtins;
mod hosted;
mod loader;
mod names;
mod schema;
#[cfg(test)]
mod tests;
mod wasm_preparation;
#[cfg(test)]
mod wasm_preparation_tests;
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
