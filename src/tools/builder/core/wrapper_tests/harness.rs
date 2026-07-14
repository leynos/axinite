//! Test harness for the build-software tool wrapper: fake builders,
//! fixtures, and shared assertion helpers.

use std::sync::{Arc, Mutex};

use super::super::super::domain::SoftwareBuilderFuture;
use super::super::*;

type AnalyzeResult = dyn Fn() -> Result<BuildRequirement, AgentToolError> + Send + Sync;
type BuildResultFn = dyn Fn(&BuildRequirement) -> Result<BuildResult, AgentToolError> + Send + Sync;

pub(super) fn assert_invalid_parameters<T: std::fmt::Debug>(
    result: Result<T, ToolError>,
    expected_msg: &str,
) {
    match result.expect_err("expected invalid parameters error") {
        ToolError::InvalidParameters(msg) => assert_eq!(msg, expected_msg),
        other => panic!("unexpected error: {:?}", other),
    }
}

pub(super) fn assert_execution_failed<T: std::fmt::Debug>(
    result: Result<T, ToolError>,
    expected_msg: &str,
) {
    match result.expect_err("expected execution failure") {
        ToolError::ExecutionFailed(msg) => assert_eq!(msg, expected_msg),
        other => panic!("unexpected error: {:?}", other),
    }
}

pub(super) async fn execute_override_error(
    override_key: &str,
    override_value: &str,
    expected_msg: &str,
) -> anyhow::Result<()> {
    let builder = FakeSoftwareBuilder::always_analyze(test_requirement()?);
    let tool = BuildSoftwareTool::new(Arc::new(builder));
    let mut params = serde_json::json!({"description": "x"});
    params[override_key] = override_value.into();
    let result = tool.execute(params, &JobContext::default()).await;
    assert_invalid_parameters(result, expected_msg);
    Ok(())
}

pub(super) async fn execute_capturing_requirement(
    params: serde_json::Value,
) -> anyhow::Result<BuildRequirement> {
    use anyhow::Context as _;

    let analyzed = test_requirement()?;
    let build_result = test_build_result(analyzed.clone());
    let (builder, captured_requirement) =
        FakeSoftwareBuilder::success_with_capture(analyzed, build_result);
    let tool = BuildSoftwareTool::new(Arc::new(builder));

    tool.execute(params, &JobContext::default())
        .await
        .map_err(|e| anyhow::anyhow!("expected execute to return successful output: {e:?}"))?;

    captured_requirement
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
        .context("expected build to capture requirement")
}

pub(super) struct FakeSoftwareBuilder {
    analyze_result: Arc<AnalyzeResult>,
    build_result: Arc<BuildResultFn>,
}

impl FakeSoftwareBuilder {
    pub(super) fn analyze_error(message: &'static str) -> Self {
        Self {
            analyze_result: Arc::new(move || Err(AgentToolError::BuilderFailed(message.into()))),
            build_result: Arc::new(|_| panic!("build not expected")),
        }
    }

    pub(super) fn always_analyze(req: BuildRequirement) -> Self {
        Self {
            analyze_result: Arc::new(move || Ok(req.clone())),
            build_result: Arc::new(|_| panic!("build not expected")),
        }
    }

    pub(super) fn build_error(req: BuildRequirement, message: &'static str) -> Self {
        Self {
            analyze_result: Arc::new(move || Ok(req.clone())),
            build_result: Arc::new(move |_| Err(AgentToolError::BuilderFailed(message.into()))),
        }
    }

    pub(super) fn success(req: BuildRequirement, result: BuildResult) -> Self {
        Self {
            analyze_result: Arc::new(move || Ok(req.clone())),
            build_result: Arc::new(move |requirement| {
                assert_eq!(requirement.name, result.requirement.name);
                assert_eq!(requirement.description, result.requirement.description);
                assert_eq!(requirement.software_type, result.requirement.software_type);
                assert_eq!(requirement.language, result.requirement.language);
                Ok(result.clone())
            }),
        }
    }

    pub(super) fn success_with_capture(
        req: BuildRequirement,
        result: BuildResult,
    ) -> (Self, Arc<Mutex<Option<BuildRequirement>>>) {
        let captured = Arc::new(Mutex::new(None));
        let build_capture = Arc::clone(&captured);
        let builder = Self {
            analyze_result: Arc::new(move || Ok(req.clone())),
            build_result: Arc::new(move |requirement| {
                *build_capture
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(requirement.clone());
                Ok(result.clone())
            }),
        };
        (builder, captured)
    }
}

impl SoftwareBuilder for FakeSoftwareBuilder {
    fn analyze<'a>(
        &'a self,
        _description: &'a str,
    ) -> SoftwareBuilderFuture<'a, Result<BuildRequirement, AgentToolError>> {
        let res = (self.analyze_result)();
        Box::pin(async move { res })
    }

    fn build<'a>(
        &'a self,
        requirement: &'a BuildRequirement,
    ) -> SoftwareBuilderFuture<'a, Result<BuildResult, AgentToolError>> {
        let res = (self.build_result)(requirement);
        Box::pin(async move { res })
    }

    fn repair<'a>(
        &'a self,
        _result: &'a BuildResult,
        _error: &'a str,
    ) -> SoftwareBuilderFuture<'a, Result<BuildResult, AgentToolError>> {
        Box::pin(async { panic!("repair not expected") })
    }
}

pub(super) fn test_requirement() -> anyhow::Result<BuildRequirement> {
    Ok(BuildRequirement {
        name: ProjectName::new("test_tool")
            .map_err(|e| anyhow::anyhow!("test project name should be valid: {e}"))?,
        description: "build a test tool".to_string(),
        software_type: SoftwareType::Library,
        language: Language::Rust,
        input_spec: None,
        output_spec: None,
        dependencies: vec![],
        capabilities: vec![],
    })
}

pub(super) fn expected_requirement(
    software_type: SoftwareType,
    language: Language,
) -> anyhow::Result<BuildRequirement> {
    Ok(BuildRequirement {
        software_type,
        language,
        ..test_requirement()?
    })
}

pub(super) fn test_build_result(requirement: BuildRequirement) -> BuildResult {
    let now = Utc::now();
    BuildResult {
        build_id: Uuid::nil(),
        requirement,
        artifact_path: PathBuf::from("/tmp/test-tool"),
        logs: vec![BuildLog {
            timestamp: now,
            phase: BuildPhase::Complete,
            message: "built".to_string(),
            details: None,
        }],
        success: true,
        error: None,
        started_at: now,
        completed_at: now,
        iterations: 1,
        validation_warnings: vec![],
        tests_passed: 1,
        tests_failed: 0,
        registered: false,
    }
}
