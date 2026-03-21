//! Tests for the builder core domain types and result structures.
//!
//! These tests cover builder-specific serialization, command planning, and
//! result-shape invariants without invoking the full LLM-driven build loop.

use super::*;
use std::path::Path;

mod assertions {
    use super::*;

    pub(super) fn assert_build_success(res: &BuildResult) {
        assert!(res.success, "expected build to succeed");
        assert!(
            res.error.is_none(),
            "expected successful build to have no error, got {:?}",
            res.error
        );
    }

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

    pub(super) fn assert_logs_contain_phase(logs: &[BuildLog], phase: BuildPhase) {
        assert!(
            logs.iter().any(|log| log.phase == phase),
            "expected logs to contain phase {:?}, got {:?}",
            phase,
            logs.iter().map(|log| log.phase).collect::<Vec<_>>()
        );
    }

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

#[test]
fn test_language_extension_all_variants() {
    assert_eq!(Language::Rust.extension(), "rs");
    assert_eq!(Language::Python.extension(), "py");
    assert_eq!(Language::TypeScript.extension(), "ts");
    assert_eq!(Language::JavaScript.extension(), "js");
    assert_eq!(Language::Go.extension(), "go");
    assert_eq!(Language::Bash.extension(), "sh");
}

#[test]
fn test_language_build_command_compiled_returns_some() {
    let dir = Path::new("/tmp/project");
    let rust_cmd = Language::Rust.build_command(dir);
    assert!(rust_cmd.is_some());
    let rust_cmd = rust_cmd.expect("rust build command");
    assert_eq!(rust_cmd.program, "cargo");
    assert_eq!(rust_cmd.args, vec!["build", "--release"]);

    let ts_cmd = Language::TypeScript.build_command(dir);
    assert!(ts_cmd.is_some());
    let ts_cmd = ts_cmd.expect("typescript build command");
    assert_eq!(ts_cmd.program, "npm");
    assert_eq!(ts_cmd.args, vec!["run", "build"]);

    let go_cmd = Language::Go.build_command(dir);
    assert!(go_cmd.is_some());
    let go_cmd = go_cmd.expect("go build command");
    assert_eq!(go_cmd.program, "go");
    assert_eq!(go_cmd.args, vec!["build", "./..."]);
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

#[test]
fn test_language_test_command_specific_tools() {
    let dir = Path::new("/tmp/p");
    assert_eq!(Language::Rust.test_command(dir).program, "cargo");
    assert_eq!(
        Language::Python.test_command(dir).args,
        vec!["-m", "pytest"]
    );
    assert_eq!(Language::TypeScript.test_command(dir).program, "npm");
    assert_eq!(Language::JavaScript.test_command(dir).program, "npm");
    assert_eq!(Language::Go.test_command(dir).args, vec!["test", "./..."]);
    assert_eq!(Language::Bash.test_command(dir).program, "sh");
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
    let json = serde_json::to_string(&req).expect("serialize BuildRequirement");
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
            req.name,
            req.description,
            req.software_type,
            req.language,
            req.input_spec,
            req.output_spec,
            req.dependencies,
            req.capabilities,
        )
    );
}

#[test]
fn test_build_requirement_serde_optional_fields_none() {
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
    assert!(deserialized.input_spec.is_none() && deserialized.output_spec.is_none());
    assert!(deserialized.dependencies.is_empty() && deserialized.capabilities.is_empty());
}

#[test]
fn test_builder_config_default_sensible_values() {
    let config = BuilderConfig::default();
    assert!(
        config.max_iterations > 0 && !config.timeout.is_zero() && config.timeout.as_secs() >= 60,
        "defaults should provide a positive iteration cap and non-trivial timeout"
    );
    assert!(
        config.validate_wasm && config.run_tests && config.auto_register,
        "validation, tests, and registration should default to enabled"
    );
    assert!(
        !config.cleanup_on_failure && config.wasm_output_dir.is_none(),
        "cleanup should stay disabled and wasm_output_dir should default to None"
    );
    assert!(
        config
            .build_dir
            .to_string_lossy()
            .contains("ironclaw-builds"),
        "build_dir should contain 'ironclaw-builds'"
    );
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
    assert_build_success(&deserialized);
    assert_eq!(deserialized.iterations, 3);
    assert_eq!(
        (deserialized.tests_passed, deserialized.tests_failed),
        (5, 0)
    );
    assert!(deserialized.registered);
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
    assert_build_failure_contains(&deserialized, "compilation error: undefined reference");
    assert_eq!(deserialized.iterations, 10);
    assert_eq!(
        (
            deserialized.validation_warnings.len(),
            deserialized.tests_passed,
            deserialized.tests_failed,
        ),
        (1, 2, 3)
    );
    assert!(!deserialized.registered);
}

#[test]
fn test_build_result_default_fields_from_json() {
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
    assert_eq!(result.validation_warnings, Vec::<String>::new());
    assert_eq!(result.tests_passed, 0);
    assert_eq!(result.tests_failed, 0);
    assert!(!result.registered);
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
