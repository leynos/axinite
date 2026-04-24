//! Support modules compiled only for the `support_unit_tests` harness.

#[path = "assertions.rs"]
pub mod assertions;
#[path = "cleanup_guard.rs"]
pub mod cleanup;
#[path = "instrumented_llm.rs"]
pub mod instrumented_llm;
#[path = "metrics.rs"]
pub mod metrics;
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
#[path = "trace_test_files.rs"]
pub mod trace_test_files;
#[path = "trace_types.rs"]
pub mod trace_types;

#[cfg(feature = "libsql")]
type AsyncUnit<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>>;
#[cfg(feature = "libsql")]
type AsyncOutgoingResponses<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Vec<ironclaw::channels::OutgoingResponse>> + 'a>,
>;
#[cfg(feature = "libsql")]
type AsyncTraceMetrics<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = metrics::TraceMetrics> + 'a>>;
#[cfg(feature = "libsql")]
type AsyncCompletedToolCalls<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = Vec<(String, bool)>> + 'a>>;
#[cfg(feature = "libsql")]
type AsyncStatusEvents<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Vec<ironclaw::channels::StatusUpdate>> + 'a>,
>;
#[cfg(feature = "libsql")]
type AsyncTraceRun<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Vec<Vec<ironclaw::channels::OutgoingResponse>>> + 'a>,
>;
#[cfg(feature = "libsql")]
type AsyncBuildRig =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<test_rig::TestRig>>>>;

#[cfg(feature = "libsql")]
fn _send_message_sig<'a>(rig: &'a test_rig::TestRig, content: &'a str) -> AsyncUnit<'a> {
    Box::pin(test_rig::TestRig::send_message(rig, content))
}

#[cfg(feature = "libsql")]
fn _send_incoming_sig<'a>(
    rig: &'a test_rig::TestRig,
    message: ironclaw::channels::IncomingMessage,
) -> AsyncUnit<'a> {
    Box::pin(test_rig::TestRig::send_incoming(rig, message))
}

#[cfg(feature = "libsql")]
fn _wait_for_responses_sig<'a>(
    rig: &'a test_rig::TestRig,
    count: usize,
    timeout: std::time::Duration,
) -> AsyncOutgoingResponses<'a> {
    Box::pin(test_rig::TestRig::wait_for_responses(rig, count, timeout))
}

#[cfg(feature = "libsql")]
fn _clear_sig<'a>(rig: &'a test_rig::TestRig) -> AsyncUnit<'a> {
    Box::pin(test_rig::TestRig::clear(rig))
}

#[cfg(feature = "libsql")]
fn _collect_metrics_sig<'a>(rig: &'a test_rig::TestRig) -> AsyncTraceMetrics<'a> {
    Box::pin(test_rig::TestRig::collect_metrics(rig))
}

#[cfg(feature = "libsql")]
fn _tool_calls_completed_async_sig<'a>(rig: &'a test_rig::TestRig) -> AsyncCompletedToolCalls<'a> {
    Box::pin(test_rig::TestRig::tool_calls_completed_async(rig))
}

#[cfg(feature = "libsql")]
fn _captured_status_events_async_sig<'a>(rig: &'a test_rig::TestRig) -> AsyncStatusEvents<'a> {
    Box::pin(test_rig::TestRig::captured_status_events_async(rig))
}

#[cfg(feature = "libsql")]
fn _run_trace_sig<'a>(
    rig: &'a test_rig::TestRig,
    trace: &'a trace_llm::LlmTrace,
    timeout: std::time::Duration,
) -> AsyncTraceRun<'a> {
    Box::pin(test_rig::TestRig::run_trace(rig, trace, timeout))
}

#[cfg(feature = "libsql")]
fn _run_and_verify_trace_sig<'a>(
    rig: &'a test_rig::TestRig,
    trace: &'a trace_llm::LlmTrace,
    timeout: std::time::Duration,
) -> AsyncTraceRun<'a> {
    Box::pin(test_rig::TestRig::run_and_verify_trace(rig, trace, timeout))
}

#[cfg(feature = "libsql")]
fn _build_sig(builder: test_rig::TestRigBuilder) -> AsyncBuildRig {
    Box::pin(test_rig::TestRigBuilder::build(builder))
}

#[cfg(feature = "libsql")]
fn _run_recorded_trace_sig<'a>(filename: &'a str) -> AsyncUnit<'a> {
    Box::pin(test_rig::helpers::run_recorded_trace(filename))
}

#[cfg(feature = "libsql")]
const _: fn(std::sync::Arc<test_channel::TestChannel>) -> test_rig::TestChannelHandle =
    test_rig::TestChannelHandle::new;
#[cfg(feature = "libsql")]
const _: fn() -> test_rig::TestRigBuilder = test_rig::TestRigBuilder::new;
#[cfg(feature = "libsql")]
const _: fn(test_rig::TestRigBuilder, trace_llm::LlmTrace) -> test_rig::TestRigBuilder =
    test_rig::TestRigBuilder::with_trace;
#[cfg(feature = "libsql")]
const _: fn(
    test_rig::TestRigBuilder,
    std::sync::Arc<dyn ironclaw::llm::LlmProvider>,
) -> test_rig::TestRigBuilder = test_rig::TestRigBuilder::with_llm;
#[cfg(feature = "libsql")]
const _: fn(test_rig::TestRigBuilder, usize) -> test_rig::TestRigBuilder =
    test_rig::TestRigBuilder::with_max_tool_iterations;
#[cfg(feature = "libsql")]
const _: fn(
    test_rig::TestRigBuilder,
    Vec<std::sync::Arc<dyn ironclaw::tools::Tool>>,
) -> test_rig::TestRigBuilder = test_rig::TestRigBuilder::with_extra_tools;
#[cfg(feature = "libsql")]
const _: fn(test_rig::TestRigBuilder, bool) -> test_rig::TestRigBuilder =
    test_rig::TestRigBuilder::with_injection_check;
#[cfg(feature = "libsql")]
const _: fn(test_rig::TestRigBuilder, bool) -> test_rig::TestRigBuilder =
    test_rig::TestRigBuilder::with_auto_approve_tools;
#[cfg(feature = "libsql")]
const _: fn(test_rig::TestRigBuilder) -> test_rig::TestRigBuilder =
    test_rig::TestRigBuilder::with_skills;
#[cfg(feature = "libsql")]
const _: fn(test_rig::TestRigBuilder) -> test_rig::TestRigBuilder =
    test_rig::TestRigBuilder::with_routines;
#[cfg(feature = "libsql")]
const _: fn(
    test_rig::TestRigBuilder,
    Vec<ironclaw::llm::recording::HttpExchange>,
) -> test_rig::TestRigBuilder = test_rig::TestRigBuilder::with_http_exchanges;
#[cfg(feature = "libsql")]
const _: fn(
    &test_rig::TestRig,
) -> Result<Vec<Vec<ironclaw::llm::ChatMessage>>, ironclaw::error::LlmError> =
    test_rig::TestRig::captured_llm_requests;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> Vec<String> = test_rig::TestRig::tool_calls_started;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> Vec<(String, bool)> = test_rig::TestRig::tool_calls_completed;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> Vec<(String, String)> = test_rig::TestRig::tool_results;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> Vec<(String, u64)> = test_rig::TestRig::tool_timings;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> Vec<ironclaw::channels::StatusUpdate> =
    test_rig::TestRig::captured_status_events;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> u32 = test_rig::TestRig::llm_call_count;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> u32 = test_rig::TestRig::total_input_tokens;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> u32 = test_rig::TestRig::total_output_tokens;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> f64 = test_rig::TestRig::estimated_cost_usd;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> u64 = test_rig::TestRig::elapsed_ms;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig, &trace_llm::LlmTrace, &[ironclaw::channels::OutgoingResponse]) =
    test_rig::TestRig::verify_trace_expects;
#[cfg(feature = "libsql")]
const _: fn(test_rig::TestRig) = test_rig::TestRig::shutdown;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> bool = test_rig::TestRig::has_safety_warnings;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig, &'a str) -> AsyncUnit<'a> = _send_message_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig, ironclaw::channels::IncomingMessage) -> AsyncUnit<'a> =
    _send_incoming_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(
    &'a test_rig::TestRig,
    usize,
    std::time::Duration,
) -> AsyncOutgoingResponses<'a> = _wait_for_responses_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig) -> AsyncUnit<'a> = _clear_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig) -> AsyncTraceMetrics<'a> = _collect_metrics_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig) -> AsyncCompletedToolCalls<'a> =
    _tool_calls_completed_async_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig) -> AsyncStatusEvents<'a> =
    _captured_status_events_async_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(
    &'a test_rig::TestRig,
    &'a trace_llm::LlmTrace,
    std::time::Duration,
) -> AsyncTraceRun<'a> = _run_trace_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(
    &'a test_rig::TestRig,
    &'a trace_llm::LlmTrace,
    std::time::Duration,
) -> AsyncTraceRun<'a> = _run_and_verify_trace_sig;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> &std::sync::Arc<dyn ironclaw::db::Database> =
    test_rig::TestRig::database;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> Option<&std::sync::Arc<ironclaw::workspace::Workspace>> =
    test_rig::TestRig::workspace;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> Option<&std::sync::Arc<trace_llm::TraceLlm>> =
    test_rig::TestRig::trace_llm;
#[cfg(feature = "libsql")]
const _: fn(test_rig::TestRigBuilder) -> AsyncBuildRig = _build_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a str) -> AsyncUnit<'a> = _run_recorded_trace_sig;
const _: fn(&mut trace_types::LlmTrace, &str, &str) -> usize = trace_types::LlmTrace::patch_path;
const _: for<'a> fn(&'a trace_types::LlmTrace) -> Vec<&'a ironclaw::llm::recording::TraceStep> =
    trace_types::LlmTrace::playable_steps;
