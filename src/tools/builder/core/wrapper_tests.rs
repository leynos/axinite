//! Tests for the build software native-tool wrapper.

use std::sync::{Arc, Mutex};

use super::super::domain::SoftwareBuilderFuture;
use super::*;
use insta::assert_snapshot;
use rstest::rstest;

type AnalyzeResult = dyn Fn() -> Result<BuildRequirement, AgentToolError> + Send + Sync;
type BuildResultFn = dyn Fn(&BuildRequirement) -> Result<BuildResult, AgentToolError> + Send + Sync;

struct FixedClock {
    elapsed: std::time::Duration,
}

impl Clock for FixedClock {
    fn now(&self) -> std::time::Instant {
        std::time::Instant::now()
    }

    fn elapsed_since(&self, _start: std::time::Instant) -> std::time::Duration {
        self.elapsed
    }
}

fn assert_invalid_parameters<T: std::fmt::Debug>(result: Result<T, ToolError>, expected_msg: &str) {
    match result.expect_err("expected invalid parameters error") {
        ToolError::InvalidParameters(msg) => assert_eq!(msg, expected_msg),
        other => panic!("unexpected error: {:?}", other),
    }
}

fn assert_execution_failed<T: std::fmt::Debug>(result: Result<T, ToolError>, expected_msg: &str) {
    match result.expect_err("expected execution failure") {
        ToolError::ExecutionFailed(msg) => assert_eq!(msg, expected_msg),
        other => panic!("unexpected error: {:?}", other),
    }
}

async fn execute_override_error(override_key: &str, override_value: &str, expected_msg: &str) {
    let builder = FakeSoftwareBuilder::always_analyze(test_requirement());
    let tool = BuildSoftwareTool::new(Arc::new(builder));
    let mut params = serde_json::json!({"description": "x"});
    params[override_key] = override_value.into();
    let result = tool.execute(params, &JobContext::default()).await;
    assert_invalid_parameters(result, expected_msg);
}

async fn execute_capturing_requirement(params: serde_json::Value) -> BuildRequirement {
    let analyzed = test_requirement();
    let build_result = test_build_result(analyzed.clone());
    let (builder, captured_requirement) =
        FakeSoftwareBuilder::success_with_capture(analyzed, build_result);
    let tool = BuildSoftwareTool::new(Arc::new(builder));

    tool.execute(params, &JobContext::default())
        .await
        .expect("expected execute to return successful output");

    captured_requirement
        .lock()
        .expect("captured requirement mutex should not be poisoned")
        .clone()
        .expect("expected build to capture requirement")
}

struct FakeSoftwareBuilder {
    analyze_result: Arc<AnalyzeResult>,
    build_result: Arc<BuildResultFn>,
}

impl FakeSoftwareBuilder {
    fn analyze_error(message: &'static str) -> Self {
        Self {
            analyze_result: Arc::new(move || Err(AgentToolError::BuilderFailed(message.into()))),
            build_result: Arc::new(|_| panic!("build not expected")),
        }
    }

    fn always_analyze(req: BuildRequirement) -> Self {
        Self {
            analyze_result: Arc::new(move || Ok(req.clone())),
            build_result: Arc::new(|_| panic!("build not expected")),
        }
    }

    fn build_error(req: BuildRequirement, message: &'static str) -> Self {
        Self {
            analyze_result: Arc::new(move || Ok(req.clone())),
            build_result: Arc::new(move |_| Err(AgentToolError::BuilderFailed(message.into()))),
        }
    }

    fn success(req: BuildRequirement, result: BuildResult) -> Self {
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

    fn success_with_capture(
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
                    .expect("captured requirement mutex should not be poisoned") =
                    Some(requirement.clone());
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

fn test_requirement() -> BuildRequirement {
    BuildRequirement {
        name: ProjectName::new("test_tool").expect("test project name should be valid"),
        description: "build a test tool".to_string(),
        software_type: SoftwareType::Library,
        language: Language::Rust,
        input_spec: None,
        output_spec: None,
        dependencies: vec![],
        capabilities: vec![],
    }
}

fn expected_requirement(software_type: SoftwareType, language: Language) -> BuildRequirement {
    BuildRequirement {
        software_type,
        language,
        ..test_requirement()
    }
}

fn test_build_result(requirement: BuildRequirement) -> BuildResult {
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

#[rstest]
#[case("wasm_tool", SoftwareType::WasmTool)]
#[case("cli_binary", SoftwareType::CliBinary)]
#[case("library", SoftwareType::Library)]
#[case("script", SoftwareType::Script)]
#[tokio::test]
async fn execute_valid_type_overrides_are_applied(
    #[case] value: &str,
    #[case] expected: SoftwareType,
) {
    let captured = execute_capturing_requirement(serde_json::json!({
        "description": "build a test tool",
        "type": value,
    }))
    .await;

    assert_eq!(captured.software_type, expected);
    assert_eq!(captured.language, Language::Rust);
}

#[rstest]
#[case("rust", Language::Rust)]
#[case("python", Language::Python)]
#[case("typescript", Language::TypeScript)]
#[case("bash", Language::Bash)]
#[tokio::test]
async fn execute_valid_language_overrides_are_applied(
    #[case] value: &str,
    #[case] expected: Language,
) {
    let captured = execute_capturing_requirement(serde_json::json!({
        "description": "build a test tool",
        "language": value,
    }))
    .await;

    assert_eq!(captured.software_type, SoftwareType::Library);
    assert_eq!(captured.language, expected);
}

#[tokio::test]
async fn execute_missing_description_returns_error() {
    let builder = FakeSoftwareBuilder::always_analyze(test_requirement());
    let tool = BuildSoftwareTool::new(Arc::new(builder));

    let result = tool
        .execute(serde_json::json!({}), &JobContext::default())
        .await;

    assert_invalid_parameters(result, "missing 'description'");
}

#[tokio::test]
async fn execute_analyze_failure_returns_execution_failed() {
    let builder = FakeSoftwareBuilder::analyze_error("analysis exploded");
    let tool = BuildSoftwareTool::new(Arc::new(builder));

    let result = tool
        .execute(
            serde_json::json!({
                "description": "build a test tool",
            }),
            &JobContext::default(),
        )
        .await;

    assert_execution_failed(
        result,
        "Analysis failed: Tool builder failed: analysis exploded",
    );
}

#[tokio::test]
async fn execute_build_failure_returns_execution_failed() {
    let builder = FakeSoftwareBuilder::build_error(test_requirement(), "build exploded");
    let tool = BuildSoftwareTool::new(Arc::new(builder));

    let result = tool
        .execute(
            serde_json::json!({
                "description": "build a test tool",
            }),
            &JobContext::default(),
        )
        .await;

    assert_execution_failed(result, "Build failed: Tool builder failed: build exploded");
}

#[rstest]
#[case("garbage", "unknown type: garbage")]
#[case("web_service", "unknown type: web_service")]
#[case("WasmTool", "unknown type: WasmTool")]
#[tokio::test]
async fn execute_invalid_type_override_returns_error(
    #[case] value: &str,
    #[case] expected_msg: &str,
) {
    execute_override_error("type", value, expected_msg).await;
}

#[rstest]
#[case("cobol", "unknown language: cobol")]
#[case("go", "unknown language: go")]
#[case("Rust", "unknown language: Rust")]
#[tokio::test]
async fn execute_invalid_language_override_returns_error(
    #[case] value: &str,
    #[case] expected_msg: &str,
) {
    execute_override_error("language", value, expected_msg).await;
}

#[tokio::test]
async fn execute_valid_params_returns_success_output() {
    let requirement = test_requirement();
    let build_result = test_build_result(requirement.clone());
    let builder = FakeSoftwareBuilder::success(requirement, build_result);
    let tool = BuildSoftwareTool::new(Arc::new(builder));

    let output = tool
        .execute(
            serde_json::json!({
                "description": "build a test tool",
            }),
            &JobContext::default(),
        )
        .await
        .expect("expected execute to return successful output");

    assert_eq!(output.result["success"], true);

    let captured = execute_capturing_requirement(serde_json::json!({
        "description": "build a test tool",
    }))
    .await;
    assert_eq!(captured.software_type, SoftwareType::Library);
    assert_eq!(captured.language, Language::Rust);
}

#[tokio::test]
async fn execute_success_output_matches_snapshot() {
    let requirement = test_requirement();
    let build_result = test_build_result(requirement.clone());
    let builder = FakeSoftwareBuilder::success(requirement, build_result);
    let clock = Arc::new(FixedClock {
        elapsed: std::time::Duration::from_millis(42),
    });
    let tool = BuildSoftwareTool::new_with_clock(Arc::new(builder), clock);

    let output = tool
        .execute(
            serde_json::json!({
                "description": "build a test tool",
            }),
            &JobContext::default(),
        )
        .await
        .expect("expected execute to return successful output");

    assert_eq!(output.duration, std::time::Duration::from_millis(42));
    assert_eq!(output.cost, None);
    assert_eq!(output.raw, None);
    assert_snapshot!(
        serde_json::to_string_pretty(&output.result).expect("output result should serialize")
    );
}

#[tokio::test]
async fn execute_valid_overrides_are_applied_before_build() {
    let analyzed = test_requirement();
    let expected = expected_requirement(SoftwareType::WasmTool, Language::TypeScript);
    let (builder, captured_requirement) =
        FakeSoftwareBuilder::success_with_capture(analyzed, test_build_result(expected));
    let tool = BuildSoftwareTool::new(Arc::new(builder));

    let output = tool
        .execute(
            serde_json::json!({
                "description": "build a test tool",
                "type": "wasm_tool",
                "language": "typescript",
            }),
            &JobContext::default(),
        )
        .await
        .expect("expected execute to apply valid overrides");

    assert_eq!(output.result["success"], true);
    assert_eq!(output.result["name"], "test_tool");
    let captured = captured_requirement
        .lock()
        .expect("captured requirement mutex should not be poisoned")
        .clone()
        .expect("expected build to capture requirement");
    assert_eq!(captured.software_type, SoftwareType::WasmTool);
    assert_eq!(captured.language, Language::TypeScript);
}
