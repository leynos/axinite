use rstest::rstest;

use crate::exports::near::agent::channel::{StatusType, StatusUpdate};
use crate::status::{
    classify_status_update, status_message_for_user, truncate_status_message,
    TelegramStatusAction, TELEGRAM_STATUS_MAX_CHARS,
};

fn status_update(status: StatusType, message: &str) -> StatusUpdate {
    StatusUpdate {
        status,
        message: message.to_string(),
        metadata_json: "{}".to_string(),
    }
}

#[test]
fn test_classify_status_update_thinking() {
    let update = status_update(StatusType::Thinking, "Thinking...");

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Typing)
    );
}

#[test]
fn test_classify_status_update_done_ignored() {
    let update = status_update(StatusType::Done, "Done");

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_tool_started_ignored() {
    let update = status_update(StatusType::ToolStarted, "Tool started: http_request");

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_tool_completed_ignored() {
    let update = status_update(
        StatusType::ToolCompleted,
        "Tool completed: http_request (ok)",
    );

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_tool_result_ignored() {
    let update = status_update(StatusType::ToolResult, "Tool result: http_request ...");

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_awaiting_approval_ignored() {
    let update = status_update(StatusType::Status, "Awaiting approval");

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_interrupted_ignored() {
    let update = status_update(StatusType::Interrupted, "Interrupted");

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_done_ignored_case_insensitive() {
    let update = status_update(StatusType::Status, "done");

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_interrupted_ignored() {
    let update = status_update(StatusType::Status, "interrupted");

    assert_eq!(classify_status_update(&update), None);
}

#[test]
fn test_classify_status_update_status_rejected_ignored() {
    let update = status_update(StatusType::Status, "Rejected");

    assert_eq!(classify_status_update(&update), None);
}

#[rstest]
#[case(
    StatusType::ApprovalNeeded,
    "Approval needed for tool 'http_request'",
    "Approval needed for tool 'http_request'"
)]
#[case(
    StatusType::AuthRequired,
    "Authentication required for weather.",
    "Authentication required for weather."
)]
#[case(
    StatusType::JobStarted,
    "Job started: Daily sync",
    "Job started: Daily sync"
)]
#[case(
    StatusType::AuthCompleted,
    "Authentication completed for weather.",
    "Authentication completed for weather."
)]
#[case(
    StatusType::Status,
    "Context compaction started",
    "Context compaction started"
)]
fn test_classify_status_update_notify_variants(
    #[case] status: StatusType,
    #[case] message: &str,
    #[case] expected_message: &str,
) {
    let update = status_update(status, message);

    assert_eq!(
        classify_status_update(&update),
        Some(TelegramStatusAction::Notify(expected_message.to_string()))
    );
}

#[test]
fn test_status_message_for_user_ignores_blank() {
    let update = status_update(StatusType::AuthRequired, "   ");

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
