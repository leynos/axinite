//! Tool registry for managing available tools.

mod builtins;
mod extension;
mod job;
mod loader;
mod names;
#[cfg(test)]
mod tests;
mod wasm;

pub use extension::{ImageToolsArgs, VisionToolsArgs};
pub use job::RegisterJobToolsConfig;
pub use loader::{ToolRegistry, WasmRegistrationError, WasmToolRegistration};
pub use names::PROTECTED_TOOL_NAMES;
pub use wasm::WasmFromStorageArgs;

pub fn is_protected_tool_name(name: &str) -> bool {
    names::is_protected_tool_name(name)
}
