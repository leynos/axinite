use crate::agent::submission::{Submission, SubmissionParser};

use super::super::status_to_wit;

fn assert_submission(input: &str, expected_approved: bool, expected_always: bool) {
    match SubmissionParser::parse(input) {
        Submission::ApprovalResponse { approved, always } => {
            assert_eq!(
                approved, expected_approved,
                "wrong `approved` for input {:?}",
                input
            );
            assert_eq!(
                always, expected_always,
                "wrong `always` for input {:?}",
                input
            );
        }
        other => panic!("expected ApprovalResponse for {:?}, got {:?}", input, other),
    }
}

#[test]
fn test_approval_prompt_roundtrip_submission_aliases() {
    let metadata = serde_json::json!({"chat_id": 42});
    let wit = status_to_wit(
        &crate::channels::StatusUpdate::ApprovalNeeded {
            request_id: "req-321".to_string(),
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
    assert!(wit.message.contains("/approve"));
    assert!(wit.message.contains("/deny"));
    assert!(wit.message.contains("/always"));

    assert_submission("/approve", true, false);
    assert_submission("/deny", false, false);
    assert_submission("/always", true, true);
}
