//! Tests for parsing control commands and approval-response aliases.

use uuid::Uuid;

use crate::agent::submission::{Submission, SubmissionParser};

#[test]
fn test_submission_types() {
    let input = Submission::user_input("Hello");
    assert!(input.starts_turn());
    assert!(!input.is_control());

    let undo = Submission::undo();
    assert!(!undo.starts_turn());
    assert!(undo.is_control());
}

#[test]
fn test_parser_user_input() {
    let submission = SubmissionParser::parse("Hello, how are you?");
    assert!(
        matches!(submission, Submission::UserInput { content } if content == "Hello, how are you?")
    );
}

#[test]
fn test_parser_undo() {
    let submission = SubmissionParser::parse("/undo");
    assert!(matches!(submission, Submission::Undo));

    let submission = SubmissionParser::parse("/UNDO");
    assert!(matches!(submission, Submission::Undo));
}

#[test]
fn test_parser_redo() {
    let submission = SubmissionParser::parse("/redo");
    assert!(matches!(submission, Submission::Redo));
}

#[test]
fn test_parser_interrupt() {
    let submission = SubmissionParser::parse("/interrupt");
    assert!(matches!(submission, Submission::Interrupt));

    let submission = SubmissionParser::parse("/stop");
    assert!(matches!(submission, Submission::Interrupt));
}

#[test]
fn test_parser_compact() {
    let submission = SubmissionParser::parse("/compact");
    assert!(matches!(submission, Submission::Compact));
}

#[test]
fn test_parser_clear() {
    let submission = SubmissionParser::parse("/clear");
    assert!(matches!(submission, Submission::Clear));
}

#[test]
fn test_parser_new_thread() {
    let submission = SubmissionParser::parse("/thread new");
    assert!(matches!(submission, Submission::NewThread));

    let submission = SubmissionParser::parse("/new");
    assert!(matches!(submission, Submission::NewThread));
}

#[test]
fn test_parser_switch_thread() {
    let uuid = Uuid::new_v4();
    let submission = SubmissionParser::parse(&format!("/thread {}", uuid));
    assert!(matches!(submission, Submission::SwitchThread { thread_id } if thread_id == uuid));
}

#[test]
fn test_parser_resume() {
    let uuid = Uuid::new_v4();
    let submission = SubmissionParser::parse(&format!("/resume {}", uuid));
    assert!(matches!(submission, Submission::Resume { checkpoint_id } if checkpoint_id == uuid));
}

#[test]
fn test_parser_heartbeat() {
    let submission = SubmissionParser::parse("/heartbeat");
    assert!(matches!(submission, Submission::Heartbeat));
}

#[test]
fn test_parser_summarize() {
    let submission = SubmissionParser::parse("/summarize");
    assert!(matches!(submission, Submission::Summarize));

    let submission = SubmissionParser::parse("/summary");
    assert!(matches!(submission, Submission::Summarize));
}

#[test]
fn test_parser_suggest() {
    let submission = SubmissionParser::parse("/suggest");
    assert!(matches!(submission, Submission::Suggest));
}

#[test]
fn test_parser_invalid_commands_become_user_input() {
    // Invalid UUID should become user input
    let submission = SubmissionParser::parse("/thread not-a-uuid");
    assert!(matches!(submission, Submission::UserInput { .. }));

    // Unknown command should become user input
    let submission = SubmissionParser::parse("/unknown");
    assert!(matches!(submission, Submission::UserInput { content } if content == "/unknown"));
}

#[test]
fn test_parser_approval_response_aliases() {
    // approve once
    assert!(matches!(
        SubmissionParser::parse("y"),
        Submission::ApprovalResponse {
            approved: true,
            always: false
        }
    ));
    assert!(matches!(
        SubmissionParser::parse("/approve"),
        Submission::ApprovalResponse {
            approved: true,
            always: false
        }
    ));

    // approve always
    assert!(matches!(
        SubmissionParser::parse("a"),
        Submission::ApprovalResponse {
            approved: true,
            always: true
        }
    ));
    assert!(matches!(
        SubmissionParser::parse("/always"),
        Submission::ApprovalResponse {
            approved: true,
            always: true
        }
    ));

    // deny
    assert!(matches!(
        SubmissionParser::parse("n"),
        Submission::ApprovalResponse {
            approved: false,
            always: false
        }
    ));
    assert!(matches!(
        SubmissionParser::parse("/deny"),
        Submission::ApprovalResponse {
            approved: false,
            always: false
        }
    ));
}

#[test]
fn test_parser_quit() {
    assert!(matches!(SubmissionParser::parse("/quit"), Submission::Quit));
    assert!(matches!(SubmissionParser::parse("/exit"), Submission::Quit));
    assert!(matches!(
        SubmissionParser::parse("/shutdown"),
        Submission::Quit
    ));
    assert!(matches!(SubmissionParser::parse("/QUIT"), Submission::Quit));
    assert!(matches!(SubmissionParser::parse("/Exit"), Submission::Quit));
}
