//! Thin re-export surface for trace-based test LLM helpers.

pub(crate) use super::trace_json_patch::patch_json_value;
pub use super::trace_provider::TraceLlm;
pub use super::trace_types::{LlmTrace, TraceExpects};

#[expect(
    unused_imports,
    reason = "re-exported for downstream test modules that may not use every item"
)]
pub use ironclaw::llm::recording::{
    ExpectedToolResult, HttpExchange, HttpExchangeRequest, HttpExchangeResponse,
    MemorySnapshotEntry, RequestHint, TraceResponse, TraceStep, TraceToolCall,
};
