//! Test harnesses, doubles, and helpers for crate-level tests.
//!
//! The public surface here supports both full integration-style tests and
//! targeted unit tests. Use [`TestHarnessBuilder`] and [`TestHarness`] when a
//! test needs fully wired `AgentDeps` with sensible defaults, [`null_db`] when
//! the test needs null persistence or captured persistence calls, and
//! [`worker_harness`] when the focus is `Worker` setup and terminal-state
//! behaviour.
//!
//! The [`null_db`] exports cover both null persistence and call verification:
//! [`NullDatabase`] is the baseline no-op database, [`CapturingStore`] records
//! persistence interactions, and [`Calls`], [`EventCall`],
//! [`EventCallWithId`], [`StatusCall`], and [`StatusCallWithId`] expose the
//! captured status and event payloads for assertions.
//!
//! # Usage
//!
//! ```rust,no_run
//! use axinite::testing::TestHarnessBuilder;
//!
//! #[tokio::test]
//! async fn test_something() {
//!     let harness = TestHarnessBuilder::new()
//!         .build()
//!         .await
//!         .expect("test harness should build");
//!     // use harness.deps, harness.db, etc.
//! }
//! ```

pub mod credentials;
pub mod github;
mod harness;
#[cfg(feature = "postgres")]
pub mod postgres;
#[cfg(test)]
mod settings_tests;
mod stub_channel;
mod stub_llm;
pub mod test_utils;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub mod null_db;
#[cfg(test)]
pub use null_db::{
    Calls, CapturingStore, EventCall, EventCallWithId, NullDatabase, StatusCall, StatusCallWithId,
};
#[cfg(test)]
pub mod worker_harness;

use anyhow::Result;

#[cfg(all(feature = "libsql", feature = "test-helpers"))]
pub use harness::test_db;
pub use harness::{TestHarness, TestHarnessBuilder};
pub use stub_channel::StubChannel;
pub use stub_llm::{StubErrorKind, StubLlm};

pub use crate::testing_wasm::{
    github_tool_source_dir, github_wasm_artifact, metadata_test_runtime,
};
use crate::tools::wasm::{Capabilities, WasmToolWrapper};

/// Build a `WasmToolWrapper` for the shared GitHub WASM fixture.
///
/// Tests use this helper to avoid duplicating the fixture runtime wiring in
/// each module that needs a real WASM component instance.
pub async fn github_wasm_wrapper() -> Result<WasmToolWrapper> {
    let wasm_path = github_wasm_artifact()?;
    let runtime = metadata_test_runtime()?;
    let wasm_bytes = ambient_fs::read(&wasm_path)?;
    let prepared = runtime.prepare("github", &wasm_bytes, None).await?;
    let wrapper = WasmToolWrapper::new(runtime, prepared, Capabilities::default());
    let (description, schema) = wrapper.exported_metadata()?;

    Ok(wrapper.with_description(description).with_schema(schema))
}
