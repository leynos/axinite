//! Tests for the builder core domain types and result structures.
//!
//! These tests cover builder-specific serialization, command planning, and
//! result-shape invariants without invoking the full LLM-driven build loop.

use super::*;
use rstest::rstest;
use std::path::Path;

mod assertions {
    use super::*;
    use pretty_assertions::assert_eq;

    #[track_caller]
    pub(super) fn assert_build_requirement_roundtrip(req: &BuildRequirement) {
        let json = serde_json::to_string(req).expect("serialize BuildRequirement");
        let deserialized: BuildRequirement =
            serde_json::from_str(&json).expect("deserialize BuildRequirement");
        assert_eq!(
            (
                deserialized.name,
                deserialized.description,
                deserialized.software_type,
                deserialized.language,
                deserialized.input_spec,
                deserialized.output_spec,
                deserialized.dependencies,
                deserialized.capabilities,
            ),
            (
                req.name.clone(),
                req.description.clone(),
                req.software_type.clone(),
                req.language.clone(),
                req.input_spec.clone(),
                req.output_spec.clone(),
                req.dependencies.clone(),
                req.capabilities.clone(),
            )
        );
    }

    #[track_caller]
    pub(super) fn assert_optional_fields_none(req: &BuildRequirement) {
        assert_eq!(
            (
                req.input_spec.as_ref(),
                req.output_spec.as_ref(),
                req.dependencies.as_slice(),
                req.capabilities.as_slice(),
            ),
            (None, None, [].as_slice(), [].as_slice())
        );
    }

    #[track_caller]
    pub(super) fn assert_builder_config_defaults(config: &BuilderConfig) {
        assert_eq!(
            (
                config.max_iterations > 0,
                !config.timeout.is_zero() && config.timeout.as_secs() >= 60,
                config.validate_wasm,
                config.run_tests,
                config.auto_register,
                config.cleanup_on_failure,
                config.wasm_output_dir.as_ref(),
                config
                    .build_dir
                    .to_string_lossy()
                    .contains("ironclaw-builds"),
            ),
            (true, true, true, true, true, false, None, true)
        );
    }

    #[track_caller]
    pub(super) fn assert_build_success(res: &BuildResult) {
        assert!(res.success, "expected build to succeed");
        assert!(
            res.error.is_none(),
            "expected successful build to have no error, got {:?}",
            res.error
        );
    }

    #[track_caller]
    pub(super) fn assert_build_failure_contains(res: &BuildResult, needle: &str) {
        assert!(!res.success, "expected build to fail");
        assert!(
            res.error
                .as_deref()
                .is_some_and(|error| error.contains(needle)),
            "expected build error to contain {:?}, got {:?}",
            needle,
            res.error
        );
    }

    #[track_caller]
    pub(super) fn assert_build_result_success(res: &BuildResult) {
        assert_build_success(res);
        assert_eq!(
            (
                res.iterations,
                res.tests_passed,
                res.tests_failed,
                res.registered
            ),
            (3, 5, 0, true)
        );
    }

    #[track_caller]
    pub(super) fn assert_build_result_failure(
        res: &BuildResult,
        expected_error: &str,
        expected_warnings: usize,
        tests_passed: u32,
        tests_failed: u32,
    ) {
        assert_build_failure_contains(res, expected_error);
        assert_eq!(
            (
                res.iterations,
                res.validation_warnings.len(),
                res.tests_passed,
                res.tests_failed,
                res.registered,
            ),
            (10, expected_warnings, tests_passed, tests_failed, false)
        );
    }

    #[track_caller]
    pub(super) fn assert_build_result_defaults(result: &BuildResult) {
        assert_eq!(
            (
                result.validation_warnings.as_slice(),
                result.tests_passed,
                result.tests_failed,
                result.registered,
            ),
            ([].as_slice(), 0, 0, false)
        );
    }

    #[track_caller]
    pub(super) fn assert_logs_contain_phase(logs: &[BuildLog], phase: BuildPhase) {
        assert!(
            logs.iter().any(|log| log.phase == phase),
            "expected logs to contain phase {:?}, got {:?}",
            phase,
            logs.iter().map(|log| log.phase).collect::<Vec<_>>()
        );
    }

    #[track_caller]
    pub(super) fn assert_logs_message_contains(logs: &[BuildLog], needle: &str) {
        assert!(
            logs.iter().any(|log| log.message.contains(needle)
                || log
                    .details
                    .as_deref()
                    .is_some_and(|details| details.contains(needle))),
            "expected logs to contain {:?}, got {:?}",
            needle,
            logs.iter()
                .map(|log| (&log.message, log.details.as_deref()))
                .collect::<Vec<_>>()
        );
    }
}

enum CommandExpectation<'a> {
    Program(&'a str),
    Args(&'a [&'a str]),
}

#[track_caller]
fn assert_command_program_and_args(
    command: ExecutionCommand,
    expected_program: &str,
    expected_args: &[&str],
) {
    let actual_args = command.args.iter().map(String::as_str).collect::<Vec<_>>();
    assert_eq!(
        (command.program.as_str(), actual_args.as_slice()),
        (expected_program, expected_args)
    );
}

#[track_caller]
fn assert_command_matches(command: ExecutionCommand, expectation: CommandExpectation<'_>) {
    match expectation {
        CommandExpectation::Program(expected) => {
            assert_eq!(command.program, expected);
        }
        CommandExpectation::Args(expected) => {
            let actual_args = command.args.iter().map(String::as_str).collect::<Vec<_>>();
            assert_eq!(actual_args.as_slice(), expected);
        }
    }
}

#[rstest]
#[case(Language::Rust, "rs")]
#[case(Language::Python, "py")]
#[case(Language::TypeScript, "ts")]
#[case(Language::JavaScript, "js")]
#[case(Language::Go, "go")]
#[case(Language::Bash, "sh")]
fn test_language_extension_all_variants(#[case] lang: Language, #[case] expected_ext: &str) {
    assert_eq!(lang.extension(), expected_ext);
}

#[rstest]
#[case(Language::Rust, "cargo", &["build", "--release"])]
#[case(Language::TypeScript, "npm", &["run", "build"])]
#[case(Language::Go, "go", &["build", "./..."])]
fn test_language_build_command_compiled_returns_some(
    #[case] lang: Language,
    #[case] expected_program: &str,
    #[case] expected_args: &[&str],
) {
    let dir = Path::new("/tmp/project");
    let command = lang.build_command(dir);
    assert!(command.is_some());
    let command = command.expect("compiled language build command");
    assert_command_program_and_args(command, expected_program, expected_args);
}

#[test]
fn test_language_build_command_interpreted_returns_none() {
    let dir = Path::new("/tmp/project");
    assert!(Language::Python.build_command(dir).is_none());
    assert!(Language::JavaScript.build_command(dir).is_none());
    assert!(Language::Bash.build_command(dir).is_none());
}

#[test]
fn test_language_build_command_includes_project_dir() {
    let dir = Path::new("/home/user/my_project");
    for lang in [Language::Rust, Language::TypeScript, Language::Go] {
        let cmd = lang.build_command(dir);
        assert!(
            cmd.as_ref()
                .expect("compiled language build command")
                .cwd
                .as_path()
                == dir,
            "{:?} build command should contain project dir",
            lang
        );
    }
}

#[test]
fn test_language_test_command_all_variants_non_empty() {
    let dir = Path::new("/tmp/project");
    let all_languages = [
        Language::Rust,
        Language::Python,
        Language::TypeScript,
        Language::JavaScript,
        Language::Go,
        Language::Bash,
    ];
    for lang in all_languages {
        let cmd = lang.test_command(dir);
        assert!(
            !cmd.program.is_empty(),
            "{:?} test command should not be empty",
            lang
        );
        assert!(
            cmd.cwd.as_path() == dir,
            "{:?} test command should contain project dir",
            lang
        );
    }
}

#[rstest]
#[case(Language::Rust, CommandExpectation::Program("cargo"))]
#[case(Language::Python, CommandExpectation::Args(&["-m", "pytest"]))]
#[case(Language::TypeScript, CommandExpectation::Program("npm"))]
#[case(Language::JavaScript, CommandExpectation::Program("npm"))]
#[case(Language::Go, CommandExpectation::Args(&["test", "./..."]))]
#[case(Language::Bash, CommandExpectation::Program("sh"))]
fn test_language_test_command_specific_tools(
    #[case] lang: Language,
    #[case] expectation: CommandExpectation<'_>,
) {
    let dir = Path::new("/tmp/p");
    assert_command_matches(lang.test_command(dir), expectation);
}

#[test]
fn test_software_type_serde_roundtrip() {
    let variants = [
        SoftwareType::WasmTool,
        SoftwareType::CliBinary,
        SoftwareType::Library,
        SoftwareType::Script,
        SoftwareType::WebService,
    ];
    let expected_strings = [
        "\"wasm_tool\"",
        "\"cli_binary\"",
        "\"library\"",
        "\"script\"",
        "\"web_service\"",
    ];
    for (variant, expected) in variants.iter().zip(expected_strings.iter()) {
        let json = serde_json::to_string(variant).expect("serialize SoftwareType variant");
        assert_eq!(&json, expected, "serialization mismatch for {:?}", variant);
        let deserialized: SoftwareType =
            serde_json::from_str(&json).expect("deserialize SoftwareType");
        assert_eq!(
            &deserialized, variant,
            "roundtrip mismatch for {:?}",
            variant
        );
    }
}

#[test]
fn test_language_serde_roundtrip() {
    let variants = [
        Language::Rust,
        Language::Python,
        Language::TypeScript,
        Language::JavaScript,
        Language::Go,
        Language::Bash,
    ];
    let expected_strings = [
        "\"rust\"",
        "\"python\"",
        "\"type_script\"",
        "\"java_script\"",
        "\"go\"",
        "\"bash\"",
    ];
    for (variant, expected) in variants.iter().zip(expected_strings.iter()) {
        let json = serde_json::to_string(variant).expect("serialize Language variant");
        assert_eq!(&json, expected, "serialization mismatch for {:?}", variant);
        let deserialized: Language = serde_json::from_str(&json).expect("deserialize Language");
        assert_eq!(
            &deserialized, variant,
            "roundtrip mismatch for {:?}",
            variant
        );
    }
}

#[test]
fn test_build_requirement_serde_roundtrip() {
    use assertions::*;

    let req = BuildRequirement {
        name: ProjectName::new("my_tool").expect("valid project name"),
        description: "A tool that does stuff".into(),
        software_type: SoftwareType::WasmTool,
        language: Language::Rust,
        input_spec: Some("JSON object with 'query' field".into()),
        output_spec: Some("JSON object with 'result' field".into()),
        dependencies: vec!["serde".into(), "reqwest".into()],
        capabilities: vec!["http".into(), "workspace".into()],
    };
    assert_build_requirement_roundtrip(&req);
}

#[test]
fn test_build_requirement_serde_optional_fields_none() {
    use assertions::*;

    let req = BuildRequirement {
        name: ProjectName::new("minimal").expect("valid project name"),
        description: "Bare minimum".into(),
        software_type: SoftwareType::Script,
        language: Language::Bash,
        input_spec: None,
        output_spec: None,
        dependencies: vec![],
        capabilities: vec![],
    };
    let json = serde_json::to_string(&req).expect("serialize BuildRequirement");
    let deserialized: BuildRequirement =
        serde_json::from_str(&json).expect("deserialize BuildRequirement");
    assert_optional_fields_none(&deserialized);
}

#[test]
fn test_builder_config_default_sensible_values() {
    use assertions::*;

    let config = BuilderConfig::default();
    assert_builder_config_defaults(&config);
}

#[test]
fn test_build_phase_serde_roundtrip() {
    let variants = [
        BuildPhase::Analyzing,
        BuildPhase::Scaffolding,
        BuildPhase::Implementing,
        BuildPhase::Building,
        BuildPhase::Testing,
        BuildPhase::Fixing,
        BuildPhase::Validating,
        BuildPhase::Registering,
        BuildPhase::Packaging,
        BuildPhase::Complete,
        BuildPhase::Failed,
    ];
    for variant in &variants {
        let json = serde_json::to_string(variant).expect("serialize BuildPhase variant");
        let deserialized: BuildPhase = serde_json::from_str(&json).expect("deserialize BuildPhase");
        assert_eq!(
            &deserialized, variant,
            "roundtrip mismatch for {:?}",
            variant
        );
    }
}

#[test]
fn test_build_result_serde_success() {
    use assertions::*;

    let result = BuildResult {
        build_id: Uuid::nil(),
        requirement: BuildRequirement {
            name: ProjectName::new("test_tool").expect("valid project name"),
            description: "test".into(),
            software_type: SoftwareType::WasmTool,
            language: Language::Rust,
            input_spec: None,
            output_spec: None,
            dependencies: vec![],
            capabilities: vec![],
        },
        artifact_path: PathBuf::from("/tmp/test.wasm"),
        logs: vec![],
        success: true,
        error: None,
        started_at: Utc::now(),
        completed_at: Utc::now(),
        iterations: 3,
        validation_warnings: vec![],
        tests_passed: 5,
        tests_failed: 0,
        registered: true,
    };
    let json = serde_json::to_string(&result).expect("serialize BuildResult");
    let deserialized: BuildResult = serde_json::from_str(&json).expect("deserialize BuildResult");
    assert_build_result_success(&deserialized);
}

#[test]
fn test_build_result_serde_failure() {
    use assertions::*;

    let result = BuildResult {
        build_id: Uuid::nil(),
        requirement: BuildRequirement {
            name: ProjectName::new("broken").expect("valid project name"),
            description: "fails".into(),
            software_type: SoftwareType::CliBinary,
            language: Language::Go,
            input_spec: None,
            output_spec: None,
            dependencies: vec![],
            capabilities: vec![],
        },
        artifact_path: PathBuf::from("/tmp/broken"),
        logs: vec![],
        success: false,
        error: Some("compilation error: undefined reference".into()),
        started_at: Utc::now(),
        completed_at: Utc::now(),
        iterations: 10,
        validation_warnings: vec!["missing export".into()],
        tests_passed: 2,
        tests_failed: 3,
        registered: false,
    };
    let json = serde_json::to_string(&result).expect("serialize BuildResult");
    let deserialized: BuildResult = serde_json::from_str(&json).expect("deserialize BuildResult");
    assert_build_result_failure(
        &deserialized,
        "compilation error: undefined reference",
        1,
        2,
        3,
    );
}

#[test]
fn test_build_result_default_fields_from_json() {
    use assertions::*;

    // Verify #[serde(default)] fields can be omitted in JSON
    let json = serde_json::json!({
        "build_id": "00000000-0000-0000-0000-000000000000",
        "requirement": {
            "name": "x",
            "description": "y",
            "software_type": "script",
            "language": "bash",
            "input_spec": null,
            "output_spec": null,
            "dependencies": [],
            "capabilities": []
        },
        "artifact_path": "/tmp/x.sh",
        "logs": [],
        "success": true,
        "error": null,
        "started_at": "2025-01-01T00:00:00Z",
        "completed_at": "2025-01-01T00:01:00Z",
        "iterations": 1
    });
    let result: BuildResult =
        serde_json::from_value(json).expect("deserialize BuildResult from value");
    assert_build_result_defaults(&result);
}

#[test]
fn test_build_log_serde_roundtrip() {
    use assertions::*;

    let log = BuildLog {
        timestamp: Utc::now(),
        phase: BuildPhase::Building,
        message: "Running cargo build".into(),
        details: Some("cargo build --release 2>&1".into()),
    };
    let json = serde_json::to_string(&log).expect("serialize BuildLog");
    let deserialized: BuildLog = serde_json::from_str(&json).expect("deserialize BuildLog");
    let logs = vec![deserialized.clone()];
    assert_logs_contain_phase(&logs, BuildPhase::Building);
    assert_logs_message_contains(&logs, "Running cargo build");
    assert_eq!(
        deserialized.details.as_deref(),
        Some("cargo build --release 2>&1")
    );
}

#[test]
fn test_build_log_serde_details_none() {
    use assertions::*;

    let log = BuildLog {
        timestamp: Utc::now(),
        phase: BuildPhase::Complete,
        message: "Done".into(),
        details: None,
    };
    let json = serde_json::to_string(&log).expect("serialize BuildLog");
    let deserialized: BuildLog = serde_json::from_str(&json).expect("deserialize BuildLog");
    assert_logs_contain_phase(std::slice::from_ref(&deserialized), BuildPhase::Complete);
    assert_logs_message_contains(std::slice::from_ref(&deserialized), "Done");
    assert!(deserialized.details.is_none());
}
