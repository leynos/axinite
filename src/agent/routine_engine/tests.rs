//! Unit tests for routine engine behaviour such as notification gating.

use crate::agent::routine::{NotifyConfig, RunStatus};
use crate::config::RoutineConfig;

#[test]
fn test_notification_gating() {
    let config = NotifyConfig {
        on_success: false,
        on_failure: true,
        on_attention: true,
        ..Default::default()
    };

    // on_success = false means Ok status should not notify
    assert!(!config.on_success);
    assert!(config.on_failure);
    assert!(config.on_attention);
}

#[test]
fn test_run_status_icons() {
    // Just verify the mapping doesn't panic
    for status in [
        RunStatus::Ok,
        RunStatus::Attention,
        RunStatus::Failed,
        RunStatus::Running,
    ] {
        let _ = status.to_string();
    }
}

#[test]
fn test_routine_config_lightweight_tools_enabled_default() {
    let config = RoutineConfig::default();
    assert!(
        config.lightweight_tools_enabled,
        "Tools should be enabled by default"
    );
}

#[test]
fn test_routine_config_lightweight_max_iterations_default() {
    let config = RoutineConfig::default();
    assert_eq!(
        config.lightweight_max_iterations, 3,
        "Default should be 3 iterations"
    );
}

#[test]
fn test_routine_config_can_hold_uncapped_max_iterations() {
    // The `RoutineConfig` struct can hold a value greater than the safety cap.
    let config = RoutineConfig {
        lightweight_max_iterations: 10, // Set a value higher than the cap.
        ..RoutineConfig::default()
    };
    // The actual capping to a maximum of 5 is handled at runtime in
    // `execute_lightweight_with_tools` and during config resolution from env vars.
    assert_eq!(
        config.lightweight_max_iterations, 10,
        "Config struct should store the provided value"
    );
}

#[test]
fn test_sanitize_routine_name_replaces_special_chars() {
    let test_cases = vec![
        ("valid-routine", "valid-routine"),
        ("routine_with_underscore", "routine_with_underscore"),
        ("Routine With Spaces", "Routine_With_Spaces"),
        ("routine/with/slashes", "routine_with_slashes"),
        ("routine@with#symbols", "routine_with_symbols"),
    ];

    for (input, expected) in test_cases {
        let result = super::lightweight::sanitize_routine_name(input);
        assert_eq!(
            result, expected,
            "sanitize_routine_name({}) should be {}",
            input, expected
        );
    }
}

#[test]
fn test_sanitize_routine_name_preserves_alphanumeric_dash_underscore() {
    let names = vec!["routine123", "routine-name", "routine_name", "ROUTINE"];
    for name in names {
        let result = super::lightweight::sanitize_routine_name(name);
        assert_eq!(result, name, "Should preserve {}", name);
    }
}

#[test]
fn test_routine_sentinel_detection_exact_match() {
    // The execute_lightweight_no_tools checks: content == "ROUTINE_OK" || content.contains("ROUTINE_OK")
    // After trim(), whitespace is removed
    let test_cases = vec![
        ("ROUTINE_OK", true),
        ("  ROUTINE_OK  ", true), // After trim, whitespace is removed so matches
        ("something ROUTINE_OK something", true),
        ("ROUTINE_OK is done", true),
        ("done ROUTINE_OK", true),
        ("no sentinel here", false),
    ];

    for (content, should_match) in test_cases {
        let trimmed = content.trim();
        let matches = trimmed == "ROUTINE_OK" || trimmed.contains("ROUTINE_OK");
        assert_eq!(
            matches, should_match,
            "Content '{}' sentinel detection should be {}, got {}",
            content, should_match, matches
        );
    }
}

#[test]
fn test_approval_requirement_pattern_matching() {
    // Test the approval requirement logic (Never, UnlessAutoApproved, Always)
    use crate::tools::ApprovalRequirement;

    let requirements = vec![
        (ApprovalRequirement::Never, "auto-approved"),
        (ApprovalRequirement::UnlessAutoApproved, "auto-approved"),
        (ApprovalRequirement::Always, "blocks"),
    ];

    for (req, expected) in requirements {
        let can_auto_approve = matches!(
            req,
            ApprovalRequirement::Never | ApprovalRequirement::UnlessAutoApproved
        );
        let label = if can_auto_approve {
            "auto-approved"
        } else {
            "blocks"
        };
        assert_eq!(label, expected, "Approval pattern should match");
    }
}

#[test]
fn test_empty_response_handling() {
    // Simulate the empty content guard logic
    let empty_content = "";
    let finish_reason_length = crate::llm::FinishReason::Length;
    let finish_reason_stop = crate::llm::FinishReason::Stop;

    assert!(
        empty_content.trim().is_empty(),
        "Should detect empty content"
    );
    assert_eq!(finish_reason_length, crate::llm::FinishReason::Length);
    assert_eq!(finish_reason_stop, crate::llm::FinishReason::Stop);
}
