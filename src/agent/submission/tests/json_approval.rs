//! Tests for structured JSON exec-approval parsing.

use uuid::Uuid;

use crate::agent::submission::{Submission, SubmissionParser};

/// Check that a submission is an `ExecApproval` carrying exactly the
/// expected request id and flags.
fn is_exec_approval(
    submission: &Submission,
    request_id: Uuid,
    approved: bool,
    always: bool,
) -> bool {
    matches!(
        submission,
        Submission::ExecApproval { request_id: rid, approved: a, always: al }
            if (*rid, *a, *al) == (request_id, approved, always)
    )
}

#[test]
fn test_parser_json_exec_approval() {
    let req_id = Uuid::new_v4();
    let json = serde_json::to_string(&Submission::ExecApproval {
        request_id: req_id,
        approved: true,
        always: false,
    })
    .expect("serialize");

    let submission = SubmissionParser::parse(&json);
    assert!(is_exec_approval(&submission, req_id, true, false));
}

#[test]
fn test_parser_json_exec_approval_always() {
    let req_id = Uuid::new_v4();
    let json = serde_json::to_string(&Submission::ExecApproval {
        request_id: req_id,
        approved: true,
        always: true,
    })
    .expect("serialize");

    let submission = SubmissionParser::parse(&json);
    assert!(is_exec_approval(&submission, req_id, true, true));
}

#[test]
fn test_parser_json_exec_approval_deny() {
    let req_id = Uuid::new_v4();
    let json = serde_json::to_string(&Submission::ExecApproval {
        request_id: req_id,
        approved: false,
        always: false,
    })
    .expect("serialize");

    let submission = SubmissionParser::parse(&json);
    assert!(is_exec_approval(&submission, req_id, false, false));
}

#[test]
fn test_parser_json_non_approval_stays_user_input() {
    // A JSON UserInput should NOT be intercepted, it should be treated as text
    let json = r#"{"UserInput":{"content":"hello"}}"#;
    let submission = SubmissionParser::parse(json);
    assert!(matches!(submission, Submission::UserInput { .. }));
}

#[test]
fn test_parser_json_roundtrip_matches_approval_handler() {
    // Simulate exactly what chat_approval_handler does: serialize a Submission::ExecApproval
    // and verify the parser picks it up correctly.
    let request_id = Uuid::new_v4();
    let approval = Submission::ExecApproval {
        request_id,
        approved: true,
        always: false,
    };
    let json = serde_json::to_string(&approval).expect("serialize");
    eprintln!("Serialized approval JSON: {}", json);

    let parsed = SubmissionParser::parse(&json);
    assert!(
        is_exec_approval(&parsed, request_id, true, false),
        "Expected ExecApproval, got {:?}",
        parsed
    );
}
