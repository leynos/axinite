//! Shared helpers for default self-repair unit tests.

use std::path::PathBuf;
use std::sync::Arc;

use chrono::Utc;
use tokio::sync::Barrier;
use uuid::Uuid;

use crate::agent::self_repair::BrokenTool;
use crate::error::ToolError;
use crate::testing::null_db::CapturingStore;
use crate::tools::builder::ProjectName;
use crate::tools::{BuildRequirement, BuildResult, Language, NativeSoftwareBuilder, SoftwareType};

/// Constructs a minimal [`BrokenTool`] for use in helper unit tests.
pub(super) fn stub_broken_tool(
    name: &str,
    last_error: Option<&str>,
    repair_attempts: u32,
) -> BrokenTool {
    BrokenTool {
        name: name.to_string(),
        failure_count: 3,
        last_error: last_error.map(str::to_string),
        first_failure: Utc::now(),
        last_failure: Utc::now(),
        last_build_result: None,
        repair_attempts,
    }
}

/// Constructs a minimal [`BuildRequirement`] for use in helper unit tests.
pub(super) fn stub_build_requirement() -> BuildRequirement {
    BuildRequirement {
        name: ProjectName::new("test-tool").expect("valid project name"),
        description: "test".to_string(),
        software_type: SoftwareType::WasmTool,
        language: Language::Rust,
        input_spec: None,
        output_spec: None,
        dependencies: vec![],
        capabilities: vec![],
    }
}

/// Constructs a [`BuildResult`] with the given outcome fields.
pub(super) fn stub_build_result(
    is_success: bool,
    error: Option<&str>,
    iterations: u32,
    is_registered: bool,
) -> BuildResult {
    BuildResult {
        build_id: Uuid::nil(),
        requirement: stub_build_requirement(),
        artifact_path: PathBuf::from("/tmp/test"),
        logs: vec![],
        success: is_success,
        error: error.map(str::to_string),
        started_at: Utc::now(),
        completed_at: Utc::now(),
        iterations,
        validation_warnings: vec![],
        tests_passed: 0,
        tests_failed: 0,
        registered: is_registered,
    }
}

/// Configures the outcome of a single [`NativeSoftwareBuilder::build`] call.
pub(super) enum StubBuilderOutcome {
    /// The builder returns a [`BuildResult`] with the given fields.
    BuildSucceeded {
        is_success: bool,
        error: Option<&'static str>,
        iterations: u32,
        is_registered: bool,
    },
    /// The builder returns [`ToolError::BuilderFailed`] with the given message.
    BuilderErrored(&'static str),
}

/// Hand-rolled stub implementing [`NativeSoftwareBuilder`].
///
/// `analyze` and `repair` always return [`ToolError::BuilderFailed`].
/// `build` returns a result or error as configured by [`StubBuilderOutcome`].
///
/// Repair claims the tool before calling `build`; use
/// `DefaultSelfRepair::with_claim_overlap_barrier` in tests that must overlap
/// `claim_tool`. When `build_barrier` is set, `build` awaits it before
/// yielding and resolving the stub outcome.
pub(super) struct StubSoftwareBuilder {
    outcome: StubBuilderOutcome,
    pub(super) build_barrier: Option<Arc<Barrier>>,
}

impl StubSoftwareBuilder {
    pub(super) fn new(outcome: StubBuilderOutcome) -> Self {
        Self {
            outcome,
            build_barrier: None,
        }
    }

    pub(super) fn with_build_barrier(mut self, barrier: Arc<Barrier>) -> Self {
        self.build_barrier = Some(barrier);
        self
    }
}

impl NativeSoftwareBuilder for StubSoftwareBuilder {
    async fn analyze(&self, _description: &str) -> Result<BuildRequirement, ToolError> {
        Err(ToolError::BuilderFailed(
            "unexpected StubSoftwareBuilder::analyze call".to_string(),
        ))
    }

    async fn build(&self, _requirement: &BuildRequirement) -> Result<BuildResult, ToolError> {
        if let Some(barrier) = self.build_barrier.as_ref() {
            barrier.wait().await;
        }
        tokio::task::yield_now().await;
        match &self.outcome {
            StubBuilderOutcome::BuildSucceeded {
                is_success,
                error,
                iterations,
                is_registered,
            } => Ok(stub_build_result(
                *is_success,
                *error,
                *iterations,
                *is_registered,
            )),
            StubBuilderOutcome::BuilderErrored(msg) => {
                Err(ToolError::BuilderFailed(msg.to_string()))
            }
        }
    }

    async fn repair(&self, _result: &BuildResult, _error: &str) -> Result<BuildResult, ToolError> {
        Err(ToolError::BuilderFailed(
            "unexpected StubSoftwareBuilder::repair call".to_string(),
        ))
    }
}

/// A [`CapturingStore`] configured to fail on `mark_tool_repaired`.
pub(super) type FailingRepairStore = CapturingStore;

/// Constructs a [`FailingRepairStore`] that fails its first
/// `mark_tool_repaired` call with a `DatabaseError::NotFound`.
pub(super) fn failing_repair_store() -> FailingRepairStore {
    CapturingStore::failing_mark_tool_repaired_once(crate::error::DatabaseError::NotFound {
        entity: "tool_failure".to_string(),
        id: "simulated mark_tool_repaired failure".to_string(),
    })
}
