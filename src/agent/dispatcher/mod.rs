//! Dispatcher entry module. Wires turn-context assembly, agentic-loop
//! orchestration, tool dispatch, output previews, and the delegate layer.
//! Responsibilities: run-loop configuration, tool execution dispatch, and
//! approval/auth handling.

mod context;
pub(crate) mod delegate;
mod loop_core;
mod preview;
#[cfg(test)]
mod tests;
mod tool_dispatch;

/// Re-export commonly used dispatcher entry types and helpers so sibling crate
/// modules can keep stable imports while the implementation remains split
/// across `context`, `loop_core`, `tool_dispatch`, `preview`, and `delegate`.
pub(crate) use loop_core::{AgenticLoopResult, RunLoopCtx};
pub(crate) use preview::{PREVIEW_MAX_CHARS, truncate_for_preview};
pub(crate) use tool_dispatch::{
    ChatToolRequest, check_auth_required, execute_chat_tool_standalone, parse_auth_result,
};
