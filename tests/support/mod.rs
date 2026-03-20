//! Shared test-support utilities used across integration tests.
//!
//! Provides reusable assertions, cleanup helpers, instrumented LLMs, metrics,
//! channels, rigs, and trace helpers.

pub mod assertions;
pub mod cleanup;
pub mod fixtures;
pub mod instrumented_llm;
pub mod metrics;
pub mod telegram;
pub mod test_channel;
pub mod test_rig;
pub mod trace_llm;
mod trace_provider;
pub mod trace_types;

#[cfg(feature = "libsql")]
#[expect(
    unused_imports,
    reason = "re-exported recorded-trace helper is consumed selectively across test binaries"
)]
pub use test_rig::run_recorded_trace;
#[expect(
    unused_imports,
    reason = "re-exported shared test-rig helpers stay available to integration test modules"
)]
pub use test_rig::{TestChannelHandle, TestRig, TestRigBuilder};

macro_rules! touch {
    ($($e:expr),+ $(,)?) => { $(let _ = $e;)+ };
}

macro_rules! touch_const {
    ($($tt:tt)*) => {
        const _: $($tt)*;
    };
}

pub(crate) use ironclaw::testing_wasm::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};

type AsyncUnit<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>>;
type AsyncOutgoingResponses<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Vec<ironclaw::channels::OutgoingResponse>> + 'a>,
>;
type AsyncTraceMetrics<'a> =
    std::pin::Pin<Box<dyn std::future::Future<Output = metrics::TraceMetrics> + 'a>>;
type AsyncTraceRun<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Vec<Vec<ironclaw::channels::OutgoingResponse>>> + 'a>,
>;
#[cfg(feature = "libsql")]
type AsyncBuildRig =
    std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<test_rig::TestRig>>>>;

// These function-pointer constants intentionally perform compile-time type
// assertions. They catch signature mismatches for shared test helpers during
// compilation and have no runtime effect.
const _: fn() -> anyhow::Result<std::sync::Arc<ironclaw::tools::wasm::WasmToolRuntime>> =
    metadata_test_runtime;
const _: fn() -> std::path::PathBuf = github_tool_source_dir;
const _: fn() -> anyhow::Result<std::path::PathBuf> = github_wasm_artifact;
const _: fn() -> cleanup::CleanupGuard = cleanup::CleanupGuard::new;
const _: fn(cleanup::CleanupGuard, String) -> cleanup::CleanupGuard = cleanup::CleanupGuard::file;
const _: fn(cleanup::CleanupGuard, String) -> cleanup::CleanupGuard = cleanup::CleanupGuard::dir;
const _: fn(&str) -> std::io::Result<()> = cleanup::setup_test_dir;
const _: fn(&std::path::Path, &str) -> std::io::Result<String> =
    cleanup::setup_test_dir_with_suffix;
const _: &str = fixtures::FIXTURE_ROOT;
const _: std::time::Duration = fixtures::DEFAULT_TIMEOUT;
const _: std::time::Duration = fixtures::LONG_TIMEOUT;
const _: fn(&str, &str) -> String = fixtures::fixture_path;
const _: fn(String, String, Vec<trace_llm::TraceStep>) -> trace_llm::LlmTrace =
    trace_llm::LlmTrace::single_turn;

fn touch_cleanup_symbols() {
    use crate::support::cleanup;

    touch!(
        cleanup::CleanupGuard::new as fn() -> cleanup::CleanupGuard,
        cleanup::setup_test_dir as fn(&str) -> std::io::Result<()>,
        cleanup::setup_test_dir_with_suffix
            as fn(&std::path::Path, &str) -> std::io::Result<String>,
    );
}

fn touch_fixture_symbols() {
    use crate::support::fixtures;

    touch!(
        fixtures::FIXTURE_ROOT,
        fixtures::DEFAULT_TIMEOUT,
        fixtures::LONG_TIMEOUT,
        fixtures::fixture_path as fn(&str, &str) -> String,
    );
}

fn touch_trace_symbols() {
    use crate::support::{trace_llm, trace_types};

    touch!(
        trace_types::LlmTrace::single_turn
            as fn(String, String, Vec<trace_llm::TraceStep>) -> trace_llm::LlmTrace,
        trace_llm::patch_json_value as fn(&mut serde_json::Value, &str, &str),
    );
}

fn trace_support_symbol_refs() {
    const _: fn(&mut serde_json::Value, &str, &str) = trace_llm::patch_json_value;
    const _: fn(trace_llm::LlmTrace) -> trace_llm::TraceLlm = trace_llm::TraceLlm::from_trace;
    const _: fn(&trace_llm::TraceLlm) -> usize = trace_llm::TraceLlm::calls;
    const _: fn(&trace_llm::TraceLlm) -> usize = trace_llm::TraceLlm::hint_mismatches;
    const _: fn(
        &trace_llm::TraceLlm,
    ) -> Result<Vec<Vec<ironclaw::llm::ChatMessage>>, ironclaw::error::LlmError> =
        trace_llm::TraceLlm::captured_requests;
    const _: fn(String, Vec<trace_types::TraceTurn>) -> trace_llm::LlmTrace =
        trace_llm::LlmTrace::new;
    const _: fn(&mut trace_llm::LlmTrace, &str, &str) -> usize = trace_llm::LlmTrace::patch_path;
    const _: for<'a> fn(&'a trace_llm::LlmTrace) -> Vec<&'a trace_llm::TraceStep> =
        trace_llm::LlmTrace::playable_steps;

    fn assert_trace_llm_from_file_async<Fut>(f: fn(String) -> Fut)
    where
        Fut: std::future::Future<Output = Result<trace_llm::TraceLlm, Box<dyn std::error::Error>>>,
    {
        let _ = f;
    }

    fn assert_trace_from_file_async<Fut>(f: fn(String) -> Fut)
    where
        Fut: std::future::Future<Output = anyhow::Result<trace_llm::LlmTrace>>,
    {
        let _ = f;
    }

    let _: fn(String) -> _ = trace_llm::TraceLlm::from_file_async;
    assert_trace_llm_from_file_async(trace_llm::TraceLlm::from_file_async);
    let _: fn(String) -> _ = trace_llm::LlmTrace::from_file_async;
    assert_trace_from_file_async(trace_llm::LlmTrace::from_file_async);
}

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
    Box::pin(test_rig::run_recorded_trace(filename))
}

#[cfg(feature = "libsql")]
fn touch_test_rig_constructors() {
    touch_const!(
        fn(std::sync::Arc<test_channel::TestChannel>) -> test_rig::TestChannelHandle =
            test_rig::TestChannelHandle::new
    );
    touch_const!(fn() -> test_rig::TestRigBuilder = test_rig::TestRigBuilder::new);
    touch_const!(
        fn(test_rig::TestRigBuilder, trace_llm::LlmTrace) -> test_rig::TestRigBuilder =
            test_rig::TestRigBuilder::with_trace
    );
    touch_const!(
        fn(
            test_rig::TestRigBuilder,
            std::sync::Arc<dyn ironclaw::llm::LlmProvider>,
        ) -> test_rig::TestRigBuilder = test_rig::TestRigBuilder::with_llm
    );
    touch_const!(
        fn(test_rig::TestRigBuilder, usize) -> test_rig::TestRigBuilder =
            test_rig::TestRigBuilder::with_max_tool_iterations
    );
    touch_const!(
        fn(
            test_rig::TestRigBuilder,
            Vec<std::sync::Arc<dyn ironclaw::tools::Tool>>,
        ) -> test_rig::TestRigBuilder = test_rig::TestRigBuilder::with_extra_tools
    );
    touch_const!(
        fn(test_rig::TestRigBuilder, bool) -> test_rig::TestRigBuilder =
            test_rig::TestRigBuilder::with_injection_check
    );
    touch_const!(
        fn(test_rig::TestRigBuilder, bool) -> test_rig::TestRigBuilder =
            test_rig::TestRigBuilder::with_auto_approve_tools
    );
    touch_const!(
        fn(test_rig::TestRigBuilder) -> test_rig::TestRigBuilder =
            test_rig::TestRigBuilder::with_skills
    );
    touch_const!(
        fn(test_rig::TestRigBuilder) -> test_rig::TestRigBuilder =
            test_rig::TestRigBuilder::with_routines
    );
    touch_const!(
        fn(
            test_rig::TestRigBuilder,
            Vec<ironclaw::llm::recording::HttpExchange>,
        ) -> test_rig::TestRigBuilder = test_rig::TestRigBuilder::with_http_exchanges
    );
}

#[cfg(feature = "libsql")]
fn touch_test_rig_observers() {
    touch_const!(
        fn(&test_rig::TestRig) -> Result<Vec<Vec<ironclaw::llm::ChatMessage>>, ironclaw::error::LlmError> =
            test_rig::TestRig::captured_llm_requests
    );
    touch_const!(fn(&test_rig::TestRig) -> Vec<String> = test_rig::TestRig::tool_calls_started);
    touch_const!(
        fn(&test_rig::TestRig) -> Vec<(String, bool)> = test_rig::TestRig::tool_calls_completed
    );
    touch_const!(fn(&test_rig::TestRig) -> Vec<(String, String)> = test_rig::TestRig::tool_results);
    touch_const!(fn(&test_rig::TestRig) -> Vec<(String, u64)> = test_rig::TestRig::tool_timings);
    touch_const!(
        fn(&test_rig::TestRig) -> Vec<ironclaw::channels::StatusUpdate> =
            test_rig::TestRig::captured_status_events
    );
    touch_const!(fn(&test_rig::TestRig) -> u32 = test_rig::TestRig::llm_call_count);
    touch_const!(fn(&test_rig::TestRig) -> u32 = test_rig::TestRig::total_input_tokens);
    touch_const!(fn(&test_rig::TestRig) -> u32 = test_rig::TestRig::total_output_tokens);
    touch_const!(fn(&test_rig::TestRig) -> f64 = test_rig::TestRig::estimated_cost_usd);
    touch_const!(fn(&test_rig::TestRig) -> u64 = test_rig::TestRig::elapsed_ms);
    touch_const!(
        fn(&test_rig::TestRig, &trace_llm::LlmTrace, &[ironclaw::channels::OutgoingResponse]) =
            test_rig::TestRig::verify_trace_expects
    );
    touch_const!(fn(test_rig::TestRig) = test_rig::TestRig::shutdown);
    touch_const!(fn(&test_rig::TestRig) -> bool = test_rig::TestRig::has_safety_warnings);
}

#[cfg(feature = "libsql")]
fn touch_test_rig_async_sigs() {
    touch_const!(for<'a> fn(&'a test_rig::TestRig, &'a str) -> AsyncUnit<'a> = _send_message_sig);
    touch_const!(
        for<'a> fn(&'a test_rig::TestRig, ironclaw::channels::IncomingMessage) -> AsyncUnit<'a> =
            _send_incoming_sig
    );
    touch_const!(
        for<'a> fn(
            &'a test_rig::TestRig,
            usize,
            std::time::Duration,
        ) -> AsyncOutgoingResponses<'a> = _wait_for_responses_sig
    );
    touch_const!(for<'a> fn(&'a test_rig::TestRig) -> AsyncUnit<'a> = _clear_sig);
    touch_const!(for<'a> fn(&'a test_rig::TestRig) -> AsyncTraceMetrics<'a> = _collect_metrics_sig);
    touch_const!(
        for<'a> fn(
            &'a test_rig::TestRig,
            &'a trace_llm::LlmTrace,
            std::time::Duration,
        ) -> AsyncTraceRun<'a> = _run_trace_sig
    );
    touch_const!(
        for<'a> fn(
            &'a test_rig::TestRig,
            &'a trace_llm::LlmTrace,
            std::time::Duration,
        ) -> AsyncTraceRun<'a> = _run_and_verify_trace_sig
    );
}

#[cfg(feature = "libsql")]
fn touch_test_rig_db_sigs() {
    touch_const!(
        fn(&test_rig::TestRig) -> &std::sync::Arc<dyn ironclaw::db::Database> =
            test_rig::TestRig::database
    );
    touch_const!(
        fn(&test_rig::TestRig) -> Option<&std::sync::Arc<ironclaw::workspace::Workspace>> =
            test_rig::TestRig::workspace
    );
    touch_const!(
        fn(&test_rig::TestRig) -> Option<&std::sync::Arc<trace_llm::TraceLlm>> =
            test_rig::TestRig::trace_llm
    );
    touch_const!(fn(test_rig::TestRigBuilder) -> AsyncBuildRig = _build_sig);
    touch_const!(for<'a> fn(&'a str) -> AsyncUnit<'a> = _run_recorded_trace_sig);
}

#[cfg(feature = "libsql")]
fn touch_test_rig_symbols() {
    touch_test_rig_constructors();
    touch_test_rig_observers();
    touch_test_rig_async_sigs();
    touch_test_rig_db_sigs();
}

fn test_rig_symbol_refs() {
    touch_cleanup_symbols();
    touch_fixture_symbols();
    touch_trace_symbols();
    #[cfg(feature = "libsql")]
    touch_test_rig_symbols();
}

const _: fn() = trace_support_symbol_refs;
const _: fn() = test_rig_symbol_refs;
