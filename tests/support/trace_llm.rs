//! Thin re-export surface for trace-based test LLM helpers.

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

/// Recursively patch string values in a JSON value, replacing `from` with `to`.
pub(super) fn patch_json_value(value: &mut serde_json::Value, from: &str, to: &str) {
    match value {
        serde_json::Value::String(s) if s.contains(from) => {
            *s = s.replace(from, to);
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                patch_json_value(item, from, to);
            }
        }
        serde_json::Value::Object(obj) => {
            for (_, v) in obj {
                patch_json_value(v, from, to);
            }
        }
        _ => {}
    }
}
