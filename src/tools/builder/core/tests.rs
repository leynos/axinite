use super::*;

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
    let dir = "/tmp/project";
    let rust_cmd = Language::Rust.build_command(dir);
    assert!(rust_cmd.is_some());
    assert!(rust_cmd.unwrap().contains("cargo build"));

    let ts_cmd = Language::TypeScript.build_command(dir);
    assert!(ts_cmd.is_some());
    assert!(ts_cmd.unwrap().contains("npm run build"));

    let go_cmd = Language::Go.build_command(dir);
    assert!(go_cmd.is_some());
    assert!(go_cmd.unwrap().contains("go build"));
}

#[test]
fn test_language_build_command_interpreted_returns_none() {
    let dir = "/tmp/project";
    assert!(Language::Python.build_command(dir).is_none());
    assert!(Language::JavaScript.build_command(dir).is_none());
    assert!(Language::Bash.build_command(dir).is_none());
}

#[test]
fn test_language_build_command_includes_project_dir() {
    let dir = "/home/user/my_project";
    for lang in [Language::Rust, Language::TypeScript, Language::Go] {
        let cmd = lang.build_command(dir);
        assert!(
            cmd.as_ref().unwrap().contains(dir),
            "{:?} build command should contain project dir",
            lang
        );
    }
}

#[test]
fn test_language_test_command_all_variants_non_empty() {
    let dir = "/tmp/project";
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
            !cmd.is_empty(),
            "{:?} test command should not be empty",
            lang
        );
        assert!(
            cmd.contains(dir),
            "{:?} test command should contain project dir",
            lang
        );
    }
}

#[test]
fn test_language_test_command_specific_tools() {
    let dir = "/tmp/p";
    assert!(Language::Rust.test_command(dir).contains("cargo test"));
    assert!(Language::Python.test_command(dir).contains("pytest"));
    assert!(Language::TypeScript.test_command(dir).contains("npm test"));
    assert!(Language::JavaScript.test_command(dir).contains("npm test"));
    assert!(Language::Go.test_command(dir).contains("go test"));
    assert!(Language::Bash.test_command(dir).contains("shellcheck"));
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
        let json = serde_json::to_string(variant).unwrap();
        assert_eq!(&json, expected, "serialization mismatch for {:?}", variant);
        let deserialized: SoftwareType = serde_json::from_str(&json).unwrap();
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
        let json = serde_json::to_string(variant).unwrap();
        assert_eq!(&json, expected, "serialization mismatch for {:?}", variant);
        let deserialized: Language = serde_json::from_str(&json).unwrap();
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
        name: "my_tool".into(),
        description: "A tool that does stuff".into(),
        software_type: SoftwareType::WasmTool,
        language: Language::Rust,
        input_spec: Some("JSON object with 'query' field".into()),
        output_spec: Some("JSON object with 'result' field".into()),
        dependencies: vec!["serde".into(), "reqwest".into()],
        capabilities: vec!["http".into(), "workspace".into()],
    };
    let json = serde_json::to_string(&req).unwrap();
    let deserialized: BuildRequirement = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.name, req.name);
    assert_eq!(deserialized.description, req.description);
    assert_eq!(deserialized.software_type, req.software_type);
    assert_eq!(deserialized.language, req.language);
    assert_eq!(deserialized.input_spec, req.input_spec);
    assert_eq!(deserialized.output_spec, req.output_spec);
    assert_eq!(deserialized.dependencies, req.dependencies);
    assert_eq!(deserialized.capabilities, req.capabilities);
}

#[test]
fn test_build_requirement_serde_optional_fields_none() {
    let req = BuildRequirement {
        name: "minimal".into(),
        description: "Bare minimum".into(),
        software_type: SoftwareType::Script,
        language: Language::Bash,
        input_spec: None,
        output_spec: None,
        dependencies: vec![],
        capabilities: vec![],
    };
    let json = serde_json::to_string(&req).unwrap();
    let deserialized: BuildRequirement = serde_json::from_str(&json).unwrap();
    assert!(deserialized.input_spec.is_none());
    assert!(deserialized.output_spec.is_none());
    assert!(deserialized.dependencies.is_empty());
    assert!(deserialized.capabilities.is_empty());
}

#[test]
fn test_builder_config_default_sensible_values() {
    let config = BuilderConfig::default();
    assert!(config.max_iterations > 0, "max_iterations must be positive");
    assert!(!config.timeout.is_zero(), "timeout must be non-zero");
    assert!(
        config.timeout.as_secs() >= 60,
        "timeout should be at least 60 seconds"
    );
    assert!(config.validate_wasm, "validate_wasm should default to true");
    assert!(config.run_tests, "run_tests should default to true");
    assert!(config.auto_register, "auto_register should default to true");
    assert!(
        !config.cleanup_on_failure,
        "cleanup_on_failure should default to false for debugging"
    );
    assert!(
        config.wasm_output_dir.is_none(),
        "wasm_output_dir should default to None"
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
        let json = serde_json::to_string(variant).unwrap();
        let deserialized: BuildPhase = serde_json::from_str(&json).unwrap();
        assert_eq!(
            &deserialized, variant,
            "roundtrip mismatch for {:?}",
            variant
        );
    }
}

#[test]
fn test_build_result_serde_success() {
    let result = BuildResult {
        build_id: Uuid::nil(),
        requirement: BuildRequirement {
            name: "test_tool".into(),
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
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: BuildResult = serde_json::from_str(&json).unwrap();
    assert!(deserialized.success);
    assert!(deserialized.error.is_none());
    assert_eq!(deserialized.iterations, 3);
    assert_eq!(deserialized.tests_passed, 5);
    assert_eq!(deserialized.tests_failed, 0);
    assert!(deserialized.registered);
}

#[test]
fn test_build_result_serde_failure() {
    let result = BuildResult {
        build_id: Uuid::nil(),
        requirement: BuildRequirement {
            name: "broken".into(),
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
    let json = serde_json::to_string(&result).unwrap();
    let deserialized: BuildResult = serde_json::from_str(&json).unwrap();
    assert!(!deserialized.success);
    assert_eq!(
        deserialized.error.as_deref(),
        Some("compilation error: undefined reference")
    );
    assert_eq!(deserialized.iterations, 10);
    assert_eq!(deserialized.validation_warnings.len(), 1);
    assert_eq!(deserialized.tests_passed, 2);
    assert_eq!(deserialized.tests_failed, 3);
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
    let result: BuildResult = serde_json::from_value(json).unwrap();
    assert_eq!(result.validation_warnings, Vec::<String>::new());
    assert_eq!(result.tests_passed, 0);
    assert_eq!(result.tests_failed, 0);
    assert!(!result.registered);
}

#[test]
fn test_build_log_serde_roundtrip() {
    let log = BuildLog {
        timestamp: Utc::now(),
        phase: BuildPhase::Building,
        message: "Running cargo build".into(),
        details: Some("cargo build --release 2>&1".into()),
    };
    let json = serde_json::to_string(&log).unwrap();
    let deserialized: BuildLog = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.phase, BuildPhase::Building);
    assert_eq!(deserialized.message, "Running cargo build");
    assert_eq!(
        deserialized.details.as_deref(),
        Some("cargo build --release 2>&1")
    );
}

#[test]
fn test_build_log_serde_details_none() {
    let log = BuildLog {
        timestamp: Utc::now(),
        phase: BuildPhase::Complete,
        message: "Done".into(),
        details: None,
    };
    let json = serde_json::to_string(&log).unwrap();
    let deserialized: BuildLog = serde_json::from_str(&json).unwrap();
    assert!(deserialized.details.is_none());
    assert_eq!(deserialized.phase, BuildPhase::Complete);
}
