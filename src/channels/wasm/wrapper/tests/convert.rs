use super::super::convert::status_to_wit;

/// Calls `status_to_wit` with the given status and a `null` JSON metadata
/// value (sufficient for the majority of tests that do not assert on
/// `metadata_json` content), and returns the resulting WIT struct.
fn wit_for(status: &crate::channels::StatusUpdate) -> super::super::wit_channel::StatusUpdate {
    status_to_wit(status, &serde_json::json!(null))
}

/// As `wit_for`, but uses the provided `metadata` value.
fn wit_for_meta(
    status: &crate::channels::StatusUpdate,
    metadata: &serde_json::Value,
) -> super::super::wit_channel::StatusUpdate {
    status_to_wit(status, metadata)
}

#[test]
fn test_status_to_wit_thinking() {
    let wit = wit_for_meta(
        &crate::channels::StatusUpdate::Thinking("Processing...".into()),
        &serde_json::json!({"chat_id": 42}),
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Thinking
    ));
    assert_eq!(wit.message, "Processing...");
    assert!(wit.metadata_json.contains("42"));
}

#[test]
fn test_status_to_wit_done() {
    let wit = wit_for(&crate::channels::StatusUpdate::Status("Done".into()));
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Done
    ));
}

#[test]
fn test_status_to_wit_done_case_insensitive() {
    let wit = wit_for(&crate::channels::StatusUpdate::Status("done".into()));
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Done
    ));

    let wit = wit_for(&crate::channels::StatusUpdate::Status(" Done ".into()));
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Done
    ));
}

#[test]
fn test_status_to_wit_interrupted() {
    let wit = wit_for(&crate::channels::StatusUpdate::Status("Interrupted".into()));
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Interrupted
    ));
}

#[test]
fn test_status_to_wit_interrupted_case_insensitive() {
    let wit = wit_for(&crate::channels::StatusUpdate::Status("interrupted".into()));
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Interrupted
    ));

    let wit = wit_for(&crate::channels::StatusUpdate::Status(
        " Interrupted ".into(),
    ));
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Interrupted
    ));
}

#[test]
fn test_status_to_wit_generic_status() {
    let wit = wit_for(&crate::channels::StatusUpdate::Status(
        "Awaiting approval".into(),
    ));
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Status
    ));
    assert_eq!(wit.message, "Awaiting approval");
}

#[test]
fn test_status_to_wit_auth_required() {
    let wit = wit_for_meta(
        &crate::channels::StatusUpdate::AuthRequired {
            extension_name: "weather".to_string(),
            instructions: Some("Paste your token".to_string()),
            auth_url: Some("https://example.com/auth".to_string()),
            setup_url: None,
        },
        &serde_json::json!({"chat_id": 42}),
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::AuthRequired
    ));
    assert!(wit.message.contains("Authentication required for weather"));
    assert!(wit.message.contains("Paste your token"));
}

#[test]
fn test_status_to_wit_tool_started() {
    let wit = wit_for_meta(
        &crate::channels::StatusUpdate::ToolStarted {
            name: "http_request".to_string(),
        },
        &serde_json::json!({"chat_id": 7}),
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolStarted
    ));
    assert_eq!(wit.message, "Tool started: http_request");
}

#[test]
fn test_status_to_wit_tool_completed_success() {
    let wit = wit_for(&crate::channels::StatusUpdate::ToolCompleted {
        name: "http_request".to_string(),
        success: true,
        error: None,
        parameters: None,
    });
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolCompleted
    ));
    assert_eq!(wit.message, "Tool completed: http_request (ok)");
}

#[test]
fn test_status_to_wit_tool_completed_failure() {
    let wit = wit_for(&crate::channels::StatusUpdate::ToolCompleted {
        name: "http_request".to_string(),
        success: false,
        error: Some("connection refused".to_string()),
        parameters: None,
    });
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolCompleted
    ));
    assert_eq!(wit.message, "Tool completed: http_request (failed)");
}

#[test]
fn test_status_to_wit_tool_result() {
    let wit = wit_for(&crate::channels::StatusUpdate::ToolResult {
        name: "http_request".to_string(),
        preview: "{\"temperature\": 22}".to_string(),
    });
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolResult
    ));
    assert!(wit.message.starts_with("Tool result: http_request\n"));
}

#[test]
fn test_status_to_wit_tool_result_truncates_preview() {
    let wit = wit_for(&crate::channels::StatusUpdate::ToolResult {
        name: "big_tool".to_string(),
        preview: "x".repeat(400),
    });
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolResult
    ));
    assert!(wit.message.ends_with("..."));
}

#[test]
fn test_status_to_wit_job_started() {
    let wit = wit_for_meta(
        &crate::channels::StatusUpdate::JobStarted {
            job_id: "job-1".to_string(),
            title: "Daily sync".to_string(),
            browse_url: "https://example.com/jobs/job-1".to_string(),
        },
        &serde_json::json!({"chat_id": 1}),
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::JobStarted
    ));
    assert!(wit.message.contains("Daily sync"));
    assert!(wit.message.contains("https://example.com/jobs/job-1"));
}

#[test]
fn test_status_to_wit_auth_completed_success() {
    let wit = wit_for(&crate::channels::StatusUpdate::AuthCompleted {
        extension_name: "weather".to_string(),
        success: true,
        message: "Token saved".to_string(),
    });
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::AuthCompleted
    ));
    assert!(wit.message.contains("Authentication completed"));
    assert!(wit.message.contains("Token saved"));
}

#[test]
fn test_status_to_wit_auth_completed_failure() {
    let wit = wit_for(&crate::channels::StatusUpdate::AuthCompleted {
        extension_name: "weather".to_string(),
        success: false,
        message: "Invalid token".to_string(),
    });
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::AuthCompleted
    ));
    assert!(wit.message.contains("Authentication failed"));
    assert!(wit.message.contains("Invalid token"));
}

#[test]
fn test_status_to_wit_approval_needed() {
    let wit = wit_for_meta(
        &crate::channels::StatusUpdate::ApprovalNeeded {
            request_id: "req-123".to_string(),
            tool_name: "http_request".to_string(),
            description: "Fetch weather data".to_string(),
            parameters: serde_json::json!({"url": "https://api.weather.test"}),
        },
        &serde_json::json!({"chat_id": 42}),
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ApprovalNeeded
    ));
    assert!(wit.message.contains("http_request"));
    assert!(wit.message.contains("/approve"));
}
