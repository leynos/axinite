//! Helpers for shared `TestRig`-based integration tests.
//!
//! Provides small utilities around `TestRigBuilder`, `LlmTrace`, and common
//! recorded-trace execution patterns used by multiple test binaries.

use std::time::Duration;

use crate::support::trace_llm::LlmTrace;

use super::TestRigBuilder;

/// Load a recorded trace fixture, build a rig, run and verify expects, then shut down.
///
/// `filename` is relative to `tests/fixtures/llm_traces/recorded/`.
#[cfg(feature = "libsql")]
pub async fn run_recorded_trace(filename: &str) {
    let path = format!(
        "{}/tests/fixtures/llm_traces/recorded/{filename}",
        env!("CARGO_MANIFEST_DIR")
    );
    let trace = LlmTrace::from_file_async(&path)
        .await
        .unwrap_or_else(|error| panic!("failed to load trace {filename}: {error}"));
    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .expect("failed to build test rig");
    rig.run_and_verify_trace(&trace, Duration::from_secs(30))
        .await;
    rig.shutdown();
}
