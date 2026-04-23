//! Support modules compiled only for the `e2e_traces` harness.

#[path = "assertions.rs"]
pub mod assertions;
#[path = "cleanup.rs"]
pub mod cleanup;
#[path = "fixtures.rs"]
pub mod fixtures;
#[path = "instrumented_llm.rs"]
pub mod instrumented_llm;
#[path = "metrics.rs"]
pub mod metrics;
#[cfg(feature = "libsql")]
#[path = "routines.rs"]
pub mod routines;
#[path = "test_channel.rs"]
pub mod test_channel;
#[path = "test_rig/mod.rs"]
pub mod test_rig;
#[path = "trace_json_patch.rs"]
mod trace_json_patch;
#[path = "trace_llm.rs"]
pub mod trace_llm;
#[path = "trace_provider.rs"]
mod trace_provider;
#[path = "trace_types.rs"]
pub mod trace_types;

#[cfg(feature = "libsql")]
pub use test_rig::helpers::run_recorded_trace;

#[cfg(feature = "libsql")]
type AsyncUnit<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>>;
#[cfg(feature = "libsql")]
type AsyncStatusEvents<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Vec<ironclaw::channels::StatusUpdate>> + 'a>,
>;
type AsyncTraceLlmFromFile = std::pin::Pin<
    Box<
        dyn std::future::Future<Output = Result<trace_llm::TraceLlm, Box<dyn std::error::Error>>>
            + Send,
    >,
>;

#[cfg(feature = "libsql")]
fn _clear_sig<'a>(rig: &'a test_rig::TestRig) -> AsyncUnit<'a> {
    Box::pin(test_rig::TestRig::clear(rig))
}

#[cfg(feature = "libsql")]
fn _captured_status_events_async_sig<'a>(rig: &'a test_rig::TestRig) -> AsyncStatusEvents<'a> {
    Box::pin(test_rig::TestRig::captured_status_events_async(rig))
}

fn _trace_llm_from_file_async_sig(path: String) -> AsyncTraceLlmFromFile {
    Box::pin(trace_llm::TraceLlm::from_file_async(path))
}

#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig) -> AsyncStatusEvents<'a> =
    _captured_status_events_async_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig) -> AsyncUnit<'a> = _clear_sig;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> f64 = test_rig::TestRig::estimated_cost_usd;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> bool = test_rig::TestRig::has_safety_warnings;
const _: fn(String) -> AsyncTraceLlmFromFile = _trace_llm_from_file_async_sig;
const _: fn(&trace_llm::TraceLlm) -> usize = trace_llm::TraceLlm::calls;
const _: fn(&trace_llm::TraceLlm) -> usize = trace_llm::TraceLlm::hint_mismatches;
const _: fn(String, Vec<trace_types::TraceTurn>) -> trace_llm::LlmTrace = trace_llm::LlmTrace::new;
const _: for<'a> fn(&'a trace_llm::LlmTrace) -> Vec<&'a ironclaw::llm::recording::TraceStep> =
    trace_llm::LlmTrace::playable_steps;
