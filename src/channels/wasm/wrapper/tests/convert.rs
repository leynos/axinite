use super::super::convert::status_to_wit;

#[test]
fn test_status_to_wit_thinking() {
    let metadata = serde_json::json!({"chat_id": 42});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Thinking("Processing...".into()),
        &metadata,
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
    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("Done".into()),
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Done
    ));
}

#[test]
fn test_status_to_wit_done_case_insensitive() {
    let metadata = serde_json::json!(null);

    // lowercase
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("done".into()),
        &metadata,
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Done
    ));

    // with whitespace
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status(" Done ".into()),
        &metadata,
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Done
    ));
}

#[test]
fn test_status_to_wit_interrupted() {
    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("Interrupted".into()),
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Interrupted
    ));
}

#[test]
fn test_status_to_wit_interrupted_case_insensitive() {
    let metadata = serde_json::json!(null);

    // lowercase
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("interrupted".into()),
        &metadata,
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Interrupted
    ));

    // with whitespace
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status(" Interrupted ".into()),
        &metadata,
    );
    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Interrupted
    ));
}

#[test]
fn test_status_to_wit_generic_status() {
    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::Status("Awaiting approval".into()),
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::Status
    ));
    assert_eq!(wit.message, "Awaiting approval");
}

#[test]
fn test_status_to_wit_auth_required() {
    let metadata = serde_json::json!({"chat_id": 42});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::AuthRequired {
            extension_name: "weather".to_string(),
            instructions: Some("Paste your token".to_string()),
            auth_url: Some("https://example.com/auth".to_string()),
            setup_url: None,
        },
        &metadata,
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
    let metadata = serde_json::json!({"chat_id": 7});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolStarted {
            name: "http_request".to_string(),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolStarted
    ));
    assert_eq!(wit.message, "Tool started: http_request");
}

#[test]
fn test_status_to_wit_tool_completed_success() {
    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolCompleted {
            name: "http_request".to_string(),
            success: true,
            error: None,
            parameters: None,
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolCompleted
    ));
    assert_eq!(wit.message, "Tool completed: http_request (ok)");
}

#[test]
fn test_status_to_wit_tool_completed_failure() {
    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolCompleted {
            name: "http_request".to_string(),
            success: false,
            error: Some("connection refused".to_string()),
            parameters: None,
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolCompleted
    ));
    assert_eq!(wit.message, "Tool completed: http_request (failed)");
}

#[test]
fn test_status_to_wit_tool_result() {
    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolResult {
            name: "http_request".to_string(),
            preview: "{".to_string() + "\"temperature\": 22}",
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolResult
    ));
    assert!(wit.message.starts_with("Tool result: http_request\n"));
}

#[test]
fn test_status_to_wit_tool_result_truncates_preview() {
    let metadata = serde_json::json!(null);
    let long_preview = "x".repeat(400);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ToolResult {
            name: "big_tool".to_string(),
            preview: long_preview,
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ToolResult
    ));
    assert!(wit.message.ends_with("..."));
}

#[test]
fn test_status_to_wit_job_started() {
    let metadata = serde_json::json!({"chat_id": 1});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::JobStarted {
            job_id: "job-1".to_string(),
            title: "Daily sync".to_string(),
            browse_url: "https://example.com/jobs/job-1".to_string(),
        },
        &metadata,
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
    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::AuthCompleted {
            extension_name: "weather".to_string(),
            success: true,
            message: "Token saved".to_string(),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::AuthCompleted
    ));
    assert!(wit.message.contains("Authentication completed"));
    assert!(wit.message.contains("Token saved"));
}

#[test]
fn test_status_to_wit_auth_completed_failure() {
    let metadata = serde_json::json!(null);
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::AuthCompleted {
            extension_name: "weather".to_string(),
            success: false,
            message: "Invalid token".to_string(),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::AuthCompleted
    ));
    assert!(wit.message.contains("Authentication failed"));
    assert!(wit.message.contains("Invalid token"));
}

#[test]
fn test_status_to_wit_approval_needed() {
    let metadata = serde_json::json!({"chat_id": 42});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ApprovalNeeded {
            request_id: "req-123".to_string(),
            tool_name: "http_request".to_string(),
            description: "Fetch weather data".to_string(),
            parameters: serde_json::json!({"url": "https://api.weather.test"}),
        },
        &metadata,
    );

    assert!(matches!(
        wit.status,
        super::super::wit_channel::StatusType::ApprovalNeeded
    ));
    assert!(wit.message.contains("http_request"));
    assert!(wit.message.contains("/approve"));
}
