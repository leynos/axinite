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

pub(crate) use ironclaw::testing_wasm::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};

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
const _: fn(String, String, Vec<trace_llm::TraceStep>) -> trace_llm::LlmTrace =
    trace_llm::LlmTrace::single_turn;
const _: () = {
    let _ = test_rig::TestChannelHandle::new;
    let _ = test_rig::TestRigBuilder::new;
    let _ = test_rig::TestRigBuilder::with_trace;
    let _ = test_rig::TestRigBuilder::with_llm;
    let _ = test_rig::TestRigBuilder::with_max_tool_iterations;
    let _ = test_rig::TestRigBuilder::with_extra_tools;
    let _ = test_rig::TestRigBuilder::with_injection_check;
    let _ = test_rig::TestRigBuilder::with_auto_approve_tools;
    let _ = test_rig::TestRigBuilder::with_skills;
    let _ = test_rig::TestRigBuilder::with_routines;
    let _ = test_rig::TestRigBuilder::with_http_exchanges;
    let _ = test_rig::TestRig::send_message;
    let _ = test_rig::TestRig::send_incoming;
    let _ = test_rig::TestRig::captured_llm_requests;
    let _ = test_rig::TestRig::wait_for_responses;
    let _ = test_rig::TestRig::tool_calls_started;
    let _ = test_rig::TestRig::tool_calls_completed;
    let _ = test_rig::TestRig::tool_results;
    let _ = test_rig::TestRig::tool_timings;
    let _ = test_rig::TestRig::captured_status_events;
    let _ = test_rig::TestRig::clear;
    let _ = test_rig::TestRig::llm_call_count;
    let _ = test_rig::TestRig::total_input_tokens;
    let _ = test_rig::TestRig::total_output_tokens;
    let _ = test_rig::TestRig::estimated_cost_usd;
    let _ = test_rig::TestRig::elapsed_ms;
    let _ = test_rig::TestRig::collect_metrics;
    let _ = test_rig::TestRig::run_trace;
    let _ = test_rig::TestRig::run_and_verify_trace;
    let _ = test_rig::TestRig::verify_trace_expects;
    let _ = test_rig::TestRig::shutdown;
    let _ = test_rig::TestRig::has_safety_warnings;
    #[cfg(feature = "libsql")]
    {
        let _ = test_rig::TestRigBuilder::build;
        let _ = test_rig::run_recorded_trace;
        let _ = test_rig::TestRig::database;
        let _ = test_rig::TestRig::workspace;
        let _ = test_rig::TestRig::trace_llm;
    }
};
