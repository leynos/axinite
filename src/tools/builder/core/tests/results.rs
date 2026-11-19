//! Serde tests for `BuildResult` and `BuildLog` shapes.

use std::path::PathBuf;

use chrono::Utc;
use uuid::Uuid;

use super::super::{
    BuildLog, BuildPhase, BuildRequirement, BuildResult, Language, ProjectName, SoftwareType,
};

#[test]
fn test_build_result_serde_success() {
    use super::assertions::*;

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
    use super::assertions::*;

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
    assert_eq!(deserialized.iterations, 10);
    assert_build_result_failure(
        &deserialized,
        "compilation error: undefined reference",
        FailureCounts {
            warnings: 1,
            tests_passed: 2,
            tests_failed: 3,
        },
    );
}

#[test]
fn test_build_result_default_fields_from_json() {
    use super::assertions::*;

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
    use super::assertions::*;

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
    use super::assertions::*;

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
