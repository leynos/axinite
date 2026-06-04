use crate::exports::near::agent::channel::{StatusType, StatusUpdate};
use crate::status::{
    classify_status_update, status_message_for_user, truncate_status_message,
    TelegramStatusAction, TELEGRAM_STATUS_MAX_CHARS,
};

#[test]
fn test_classify_status_update_thinking() {
    let update = StatusUpdate {
        status: StatusType::Thinking,
        message: "Thinking...".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Typing)
    );
}

#[test]
fn test_classify_status_update_approval_needed() {
    let update = StatusUpdate {
        status: StatusType::ApprovalNeeded,
        message: "Approval needed for tool 'http_request'".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Approval needed for tool 'http_request'".to_string()
        ))
    );
}

#[test]
fn test_classify_status_update_done_ignored() {
    let update = StatusUpdate {
        status: StatusType::Done,
        message: "Done".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_auth_required() {
    let update = StatusUpdate {
        status: StatusType::AuthRequired,
        message: "Authentication required for weather.".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Authentication required for weather.".to_string()
        ))
    );
}

#[test]
fn test_classify_status_update_tool_started_ignored() {
    let update = StatusUpdate {
        status: StatusType::ToolStarted,
        message: "Tool started: http_request".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_tool_completed_ignored() {
    let update = StatusUpdate {
        status: StatusType::ToolCompleted,
        message: "Tool completed: http_request (ok)".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_job_started_notify() {
    let update = StatusUpdate {
        status: StatusType::JobStarted,
        message: "Job started: Daily sync".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Job started: Daily sync".to_string()
        ))
    );
}

#[test]
fn test_classify_status_update_auth_completed_notify() {
    let update = StatusUpdate {
        status: StatusType::AuthCompleted,
        message: "Authentication completed for weather.".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Authentication completed for weather.".to_string()
        ))
    );
}

#[test]
fn test_classify_status_update_tool_result_ignored() {
    let update = StatusUpdate {
        status: StatusType::ToolResult,
        message: "Tool result: http_request ...".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_awaiting_approval_ignored() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "Awaiting approval".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_interrupted_ignored() {
    let update = StatusUpdate {
        status: StatusType::Interrupted,
        message: "Interrupted".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_done_ignored_case_insensitive() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "done".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_interrupted_ignored() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "interrupted".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_rejected_ignored() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "Rejected".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_notify() {
    let update = StatusUpdate {
        status: StatusType::Status,
        message: "Context compaction started".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(
            "Context compaction started".to_string()
        ))
    );
}

#[test]
fn test_status_message_for_user_ignores_blank() {
    let update = StatusUpdate {
        status: StatusType::AuthRequired,
        message: "   ".to_string(),
        metadata_json: "{}".to_string(),
    };

    assert_eq!(status_message_for_user(&update), None);
}

#[test]
fn test_truncate_status_message_appends_ellipsis() {
    let input = "abcdefghijklmnopqrstuvwxyz";
    let output = truncate_status_message(input, 10);
    assert_eq!(output, "abcdefghij...");
}

#[test]
fn test_status_message_for_user_truncates_long_input() {
    let update = StatusUpdate {
        status: StatusType::AuthRequired,
        message: "x".repeat(700),
        metadata_json: "{}".to_string(),
    };

    let msg = status_message_for_user(&update).expect("expected message");
    assert!(msg.len() <= TELEGRAM_STATUS_MAX_CHARS + 3);
    assert!(msg.ends_with("..."));
}
