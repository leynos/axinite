//! Shared assertion helpers for build result tests.

use pretty_assertions::assert_eq;

use super::super::{BuildLog, BuildPhase, BuildRequirement, BuildResult, BuilderConfig};

#[track_caller]
pub(super) fn assert_build_requirement_roundtrip(req: &BuildRequirement) {
    let json = serde_json::to_string(req).expect("serialize BuildRequirement");
    let deserialized: BuildRequirement =
        serde_json::from_str(&json).expect("deserialize BuildRequirement");
    assert_eq!(
        (
            &deserialized.name,
            &deserialized.description,
            &deserialized.software_type,
            &deserialized.language,
            &deserialized.input_spec,
            &deserialized.output_spec,
            &deserialized.dependencies,
            &deserialized.capabilities,
        ),
        (
            &req.name,
            &req.description,
            &req.software_type,
            &req.language,
            &req.input_spec,
            &req.output_spec,
            &req.dependencies,
            &req.capabilities,
        )
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

pub(super) struct FailureCounts {
    pub(super) warnings: usize,
    pub(super) tests_passed: u32,
    pub(super) tests_failed: u32,
}

#[track_caller]
pub(super) fn assert_build_result_failure(
    res: &BuildResult,
    expected_error: &str,
    expected: FailureCounts,
) {
    assert_build_failure_contains(res, expected_error);
    assert_eq!(
        (
            res.validation_warnings.len(),
            res.tests_passed,
            res.tests_failed,
            res.registered,
        ),
        (
            expected.warnings,
            expected.tests_passed,
            expected.tests_failed,
            false
        )
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
pub(super) fn assert_builder_config_defaults(config: &BuilderConfig) {
    assert!(
        config.max_iterations > 0 && !config.timeout.is_zero() && config.timeout.as_secs() >= 60,
        "defaults should provide a positive iteration cap and non-trivial timeout"
    );
    assert!(
        config.validate_wasm && config.run_tests && config.auto_register,
        "validation, tests, and registration should default to enabled"
    );
    assert!(
        !config.cleanup_on_failure
            && config.wasm_output_dir.is_none()
            && config
                .build_dir
                .to_string_lossy()
                .contains("ironclaw-builds"),
        "cleanup, wasm output, and build directory defaults should be sensible"
    );
}

#[track_caller]
pub(super) fn assert_optional_fields_none(req: &BuildRequirement) {
    assert!(req.input_spec.is_none() && req.output_spec.is_none());
    assert!(req.dependencies.is_empty() && req.capabilities.is_empty());
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
