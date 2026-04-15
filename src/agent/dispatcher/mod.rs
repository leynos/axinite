//! Dispatcher entry module. Wires core loop orchestration, types, and the
//! delegate layer.
//! Responsibilities: run-loop configuration, tool execution dispatch, and
//! approval/auth handling.

mod core;
pub(crate) mod delegate;
#[cfg(test)]
mod tests;
mod types;

/// Re-export commonly used dispatcher entry types and helpers so sibling crate
/// modules can keep stable imports while the implementation remains split
/// across `core`, `types`, and `delegate`.
pub(crate) use core::RunLoopCtx;
pub(crate) use types::{
    AgenticLoopResult, ChatToolRequest, PREVIEW_MAX_CHARS, check_auth_required,
    execute_chat_tool_standalone, parse_auth_result, truncate_for_preview,
};
