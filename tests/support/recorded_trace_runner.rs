//! Helpers for replaying recorded traces through a full `TestRig`.

use std::time::Duration;

use anyhow::Context;

use crate::support::test_rig::TestRigBuilder;
use crate::support::trace_types::LlmTrace;

/// Load a recorded trace fixture, build a rig, run and verify expects, then shut down.
///
/// `filename` is relative to `tests/fixtures/llm_traces/recorded/`.
#[cfg(feature = "libsql")]
pub async fn run_recorded_trace(filename: &str) -> anyhow::Result<()> {
    let path = format!(
        "{}/tests/fixtures/llm_traces/recorded/{filename}",
        env!("CARGO_MANIFEST_DIR")
    );
    let trace = LlmTrace::from_file_async(&path)
        .await
        .with_context(|| format!("loading recorded trace fixture: {filename}"))?;
    let rig = TestRigBuilder::new()
        .with_trace(trace.clone())
        .build()
        .await
        .with_context(|| format!("building test rig for recorded trace: {filename}"))?;
    rig.run_and_verify_trace(&trace, Duration::from_secs(30))
        .await;
    rig.shutdown();
    Ok(())
}
