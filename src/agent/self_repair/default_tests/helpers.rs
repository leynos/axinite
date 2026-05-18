//! Shared helpers for default self-repair unit tests.

use std::path::PathBuf;

use chrono::Utc;
use uuid::Uuid;

use crate::error::ToolError;
use crate::tools::builder::ProjectName;
use crate::tools::{BuildRequirement, BuildResult, Language, NativeSoftwareBuilder, SoftwareType};

/// Constructs a minimal [`BuildRequirement`] for use in helper unit tests.
pub(super) fn stub_build_requirement() -> Result<BuildRequirement, ToolError> {
    Ok(BuildRequirement {
        name: ProjectName::new("test-tool").map_err(ToolError::BuilderFailed)?,
        description: "test".to_string(),
        software_type: SoftwareType::WasmTool,
        language: Language::Rust,
        input_spec: None,
        output_spec: None,
        dependencies: vec![],
        capabilities: vec![],
    })
}

/// Constructs a [`BuildResult`] with the given outcome fields.
pub(super) fn stub_build_result(
    is_success: bool,
    error: Option<&str>,
    iterations: u32,
    is_registered: bool,
) -> Result<BuildResult, ToolError> {
    Ok(BuildResult {
        build_id: Uuid::nil(),
        requirement: stub_build_requirement()?,
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
    })
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
/// `claim_tool`.
pub(super) struct StubSoftwareBuilder {
    outcome: StubBuilderOutcome,
}

impl StubSoftwareBuilder {
    pub(super) fn new(outcome: StubBuilderOutcome) -> Self {
        Self { outcome }
    }
}

impl NativeSoftwareBuilder for StubSoftwareBuilder {
    async fn analyze(&self, _description: &str) -> Result<BuildRequirement, ToolError> {
        Err(ToolError::BuilderFailed(
            "unexpected StubSoftwareBuilder::analyze call".to_string(),
        ))
    }

    async fn build(&self, _requirement: &BuildRequirement) -> Result<BuildResult, ToolError> {
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
            )?),
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
