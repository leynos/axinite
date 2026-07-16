//! Unit tests for converting channel status updates to WIT types.

use rstest::rstest;

use crate::channels::StatusUpdate;

use super::super::convert::status_to_wit;
use super::super::wit_channel::StatusType;

/// Calls `status_to_wit` with the given status and a `null` JSON metadata
/// value (sufficient for the majority of tests that do not assert on
/// `metadata_json` content), and returns the resulting WIT struct.
fn wit_for(status: &StatusUpdate) -> super::super::wit_channel::StatusUpdate {
    status_to_wit(status, &serde_json::json!(null))
}

/// As `wit_for`, but uses the provided `metadata` value.
fn wit_for_meta(
    status: &StatusUpdate,
    metadata: &serde_json::Value,
) -> super::super::wit_channel::StatusUpdate {
    status_to_wit(status, metadata)
}

fn assert_status_type(actual: &StatusType, expected: &StatusType) {
    assert_eq!(
        std::mem::discriminant(actual),
        std::mem::discriminant(expected)
    );
}

fn assert_message_contains_all(message: &str, expected_parts: &[&str]) {
    for expected in expected_parts {
        assert!(
            message.contains(expected),
            "expected message {:?} to contain {:?}",
            message,
            expected
        );
    }
}

#[test]
fn test_status_to_wit_thinking() {
    let wit = wit_for_meta(
        &StatusUpdate::Thinking("Processing...".into()),
        &serde_json::json!({"chat_id": 42}),
    );
    assert_status_type(&wit.status, &StatusType::Thinking);
    assert_eq!(wit.message, "Processing...");
    assert!(wit.metadata_json.contains("42"));
}

#[rstest]
#[case("Done", StatusType::Done, "Done")]
#[case("done", StatusType::Done, "done")]
#[case(" Done ", StatusType::Done, " Done ")]
#[case("Interrupted", StatusType::Interrupted, "Interrupted")]
#[case("interrupted", StatusType::Interrupted, "interrupted")]
#[case(" Interrupted ", StatusType::Interrupted, " Interrupted ")]
#[case("Awaiting approval", StatusType::Status, "Awaiting approval")]
fn test_status_to_wit_status_text_classification(
    #[case] input: &str,
    #[case] expected_status: StatusType,
    #[case] expected_message: &str,
) {
    let wit = wit_for(&StatusUpdate::Status(input.into()));

    assert_status_type(&wit.status, &expected_status);
    assert_eq!(wit.message, expected_message);
}

#[test]
fn test_status_to_wit_auth_required() {
    let wit = wit_for_meta(
        &StatusUpdate::AuthRequired {
            extension_name: "weather".to_string(),
            instructions: Some("Paste your token".to_string()),
            auth_url: Some("https://example.com/auth".to_string()),
            setup_url: None,
        },
        &serde_json::json!({"chat_id": 42}),
    );
    assert_status_type(&wit.status, &StatusType::AuthRequired);
    assert!(wit.message.contains("Authentication required for weather"));
    assert!(wit.message.contains("Paste your token"));
}

#[test]
fn test_status_to_wit_tool_started() {
    let wit = wit_for_meta(
        &StatusUpdate::ToolStarted {
            name: "http_request".to_string(),
        },
        &serde_json::json!({"chat_id": 7}),
    );
    assert_status_type(&wit.status, &StatusType::ToolStarted);
    assert_eq!(wit.message, "Tool started: http_request");
}

#[rstest]
#[case(true, None, "Tool completed: http_request (ok)")]
#[case(false, Some("connection refused".to_string()), "Tool completed: http_request (failed)")]
fn test_status_to_wit_tool_completed(
    #[case] success: bool,
    #[case] error: Option<String>,
    #[case] expected_message: &str,
) {
    let wit = wit_for(&StatusUpdate::ToolCompleted {
        name: "http_request".to_string(),
        success,
        error,
        parameters: None,
    });

    assert_status_type(&wit.status, &StatusType::ToolCompleted);
    assert_eq!(wit.message, expected_message);
}

#[test]
fn test_status_to_wit_tool_result() {
    let wit = wit_for(&StatusUpdate::ToolResult {
        name: "http_request".to_string(),
        preview: "{\"temperature\": 22}".to_string(),
    });
    assert_status_type(&wit.status, &StatusType::ToolResult);
    assert!(wit.message.starts_with("Tool result: http_request\n"));
}

#[test]
fn test_status_to_wit_tool_result_truncates_preview() {
    let wit = wit_for(&StatusUpdate::ToolResult {
        name: "big_tool".to_string(),
        preview: "x".repeat(400),
    });
    assert_status_type(&wit.status, &StatusType::ToolResult);
    assert!(wit.message.ends_with("..."));
}

#[test]
fn test_status_to_wit_job_started() {
    let wit = wit_for_meta(
        &StatusUpdate::JobStarted {
            job_id: "job-1".to_string(),
            title: "Daily sync".to_string(),
            browse_url: "https://example.com/jobs/job-1".to_string(),
        },
        &serde_json::json!({"chat_id": 1}),
    );
    assert_status_type(&wit.status, &StatusType::JobStarted);
    assert!(wit.message.contains("Daily sync"));
    assert!(wit.message.contains("https://example.com/jobs/job-1"));
}

#[rstest]
#[case(true,  "Token saved",   ["Authentication completed", "Token saved"])]
#[case(false, "Invalid token", ["Authentication failed",    "Invalid token"])]
fn test_status_to_wit_auth_completed(
    #[case] success: bool,
    #[case] message: &str,
    #[case] expected_parts: [&str; 2],
) {
    let wit = wit_for(&StatusUpdate::AuthCompleted {
        extension_name: "weather".to_string(),
        success,
        message: message.to_string(),
    });

    assert_status_type(&wit.status, &StatusType::AuthCompleted);
    assert_message_contains_all(&wit.message, &expected_parts);
}

#[test]
fn test_status_to_wit_approval_needed() {
    let wit = wit_for_meta(
        &StatusUpdate::ApprovalNeeded {
            request_id: "req-123".to_string(),
            tool_name: "http_request".to_string(),
            description: "Fetch weather data".to_string(),
            parameters: serde_json::json!({"url": "https://api.weather.test"}),
        },
        &serde_json::json!({"chat_id": 42}),
    );
    assert_status_type(&wit.status, &StatusType::ApprovalNeeded);
    assert!(wit.message.contains("http_request"));
    assert!(wit.message.contains("/approve"));
}
