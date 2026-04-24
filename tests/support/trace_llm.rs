//! Thin re-export surface for trace-based test LLM helpers.

pub(crate) use super::trace_json_patch::patch_json_value;
pub(crate) use super::trace_provider::TraceLlm;
pub(crate) use super::trace_types::{LlmTrace, TraceExpects, load_trace_with_mutation};
pub(crate) use ironclaw::llm::recording::{TraceResponse, TraceStep, TraceToolCall};

type AsyncLoadTraceWithMutation =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<LlmTrace>>>>;

fn _load_trace_with_mutation_sig(path: String) -> AsyncLoadTraceWithMutation {
    Box::pin(load_trace_with_mutation(path, |_| {}))
}

const _: fn(String) -> AsyncLoadTraceWithMutation = _load_trace_with_mutation_sig;
