//! Behavioural tests for `BuildSoftwareTool::execute`: parameter
//! validation, overrides, failure mapping, and success output.

use std::sync::Arc;
use std::time::Duration;

use rstest::rstest;

use super::super::clock::FixedMonotonicClock;
use super::super::*;
use super::harness::{
    FakeSoftwareBuilder, assert_execution_failed, assert_invalid_parameters,
    execute_capturing_requirement, execute_override_error, expected_requirement, test_build_result,
    test_requirement,
};

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
    .await
    .expect("capturing execution should succeed");

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
    .await
    .expect("capturing execution should succeed");

    assert_eq!(captured.software_type, SoftwareType::Library);
    assert_eq!(captured.language, expected);
}

#[tokio::test]
async fn execute_missing_description_returns_error() {
    let requirement = test_requirement().expect("test requirement should build");
    let builder = FakeSoftwareBuilder::always_analyze(requirement);
    let tool = BuildSoftwareTool::new(Arc::new(builder));

    let result = tool
        .execute(serde_json::json!({}), &JobContext::default())
        .await;

    assert_invalid_parameters(result, "missing 'description'");
}

async fn execute_failure_returns_execution_failed(
    builder: Arc<dyn SoftwareBuilder>,
    expected_msg: &str,
) {
    let tool = BuildSoftwareTool::new(builder);
    let result = tool
        .execute(
            serde_json::json!({ "description": "build a test tool" }),
            &JobContext::default(),
        )
        .await;
    assert_execution_failed(result, expected_msg);
}

#[tokio::test]
async fn execute_analyze_failure_returns_execution_failed() {
    let builder = FakeSoftwareBuilder::analyze_error("analysis exploded");
    execute_failure_returns_execution_failed(
        Arc::new(builder),
        "Analysis failed: Tool builder failed: analysis exploded",
    )
    .await;
}

#[tokio::test]
async fn execute_build_failure_returns_execution_failed() {
    let builder = FakeSoftwareBuilder::build_error(
        test_requirement().expect("test requirement should build"),
        "build exploded",
    );
    execute_failure_returns_execution_failed(
        Arc::new(builder),
        "Build failed: Tool builder failed: build exploded",
    )
    .await;
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
    execute_override_error("type", value, expected_msg)
        .await
        .expect("override error scenario should run");
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
    execute_override_error("language", value, expected_msg)
        .await
        .expect("override error scenario should run");
}

#[tokio::test]
async fn execute_valid_params_returns_success_output() {
    let requirement = test_requirement().expect("test requirement should build");
    let build_result = test_build_result(requirement.clone());
    let builder = FakeSoftwareBuilder::success(requirement, build_result);
    let tool = BuildSoftwareTool::new_with_clock(
        Arc::new(builder),
        Arc::new(FixedMonotonicClock::with_elapsed(Duration::from_millis(1))),
    );

    let output = tool
        .execute(
            serde_json::json!({
                "description": "build a test tool",
            }),
            &JobContext::default(),
        )
        .await
        .expect("expected execute to return successful output");

    assert_eq!(
        output.duration,
        Duration::from_millis(1),
        "duration must reflect the injected clock seam exactly"
    );
    assert_eq!(output.result["success"], true);

    let captured = execute_capturing_requirement(serde_json::json!({
        "description": "build a test tool",
    }))
    .await
    .expect("capturing execution should succeed");
    assert_eq!(captured.software_type, SoftwareType::Library);
    assert_eq!(captured.language, Language::Rust);
}

#[tokio::test]
async fn execute_valid_overrides_are_applied_before_build() {
    let analyzed = test_requirement().expect("test requirement should build");
    let expected = expected_requirement(SoftwareType::WasmTool, Language::TypeScript)
        .expect("expected requirement should build");
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
