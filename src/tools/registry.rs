//! Tool registry for managing available tools.

mod builtins;
mod loader;
mod names;
#[cfg(test)]
mod tests;

pub use builtins::{ImageToolsRegistration, VisionToolsRegistration};
pub use loader::{
    ToolRegistry, WasmFromStorageRegistration, WasmRegistrationError, WasmToolRegistration,
};
pub use names::PROTECTED_TOOL_NAMES;

/// Return `true` when `name` belongs to the protected built-in namespace.
pub fn is_protected_tool_name(name: &str) -> bool {
    names::is_protected_tool_name(name)
}
