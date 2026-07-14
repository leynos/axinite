//! Tests for system-command and job-command parsing.

use crate::agent::submission::{Submission, SubmissionParser};

#[test]
fn test_parser_system_command_help() {
    let submission = SubmissionParser::parse("/help");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "help" && args.is_empty())
    );

    let submission = SubmissionParser::parse("/?");
    assert!(matches!(submission, Submission::SystemCommand { command, .. } if command == "help"));

    let submission = SubmissionParser::parse("/HELP");
    assert!(matches!(submission, Submission::SystemCommand { command, .. } if command == "help"));
}

#[test]
fn test_parser_system_command_model() {
    // No args: show current model
    let submission = SubmissionParser::parse("/model");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "model" && args.is_empty())
    );

    // With args: switch model
    let submission = SubmissionParser::parse("/model gpt-4o");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "model" && args == vec!["gpt-4o"])
    );

    // Case insensitive command, preserves arg case
    let submission = SubmissionParser::parse("/MODEL Claude-3.5");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "model" && args == vec!["Claude-3.5"])
    );
}

#[test]
fn test_parser_system_command_version() {
    let submission = SubmissionParser::parse("/version");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "version" && args.is_empty())
    );
}

#[test]
fn test_parser_system_command_tools() {
    let submission = SubmissionParser::parse("/tools");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "tools" && args.is_empty())
    );
}

#[test]
fn test_parser_system_command_ping() {
    let submission = SubmissionParser::parse("/ping");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "ping" && args.is_empty())
    );
}

#[test]
fn test_parser_system_command_debug() {
    let submission = SubmissionParser::parse("/debug");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "debug" && args.is_empty())
    );
}

#[test]
fn test_parser_system_command_is_control() {
    let submission = SubmissionParser::parse("/help");
    assert!(submission.is_control());
    assert!(!submission.starts_turn());
}

#[test]
fn test_parser_system_command_skills() {
    let submission = SubmissionParser::parse("/skills");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args } if command == "skills" && args.is_empty())
    );

    // Case insensitive
    let submission = SubmissionParser::parse("/SKILLS");
    assert!(matches!(submission, Submission::SystemCommand { command, .. } if command == "skills"));
}

#[test]
fn test_parser_system_command_skills_search() {
    let submission = SubmissionParser::parse("/skills search markdown");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args }
            if command == "skills" && args == vec!["search", "markdown"])
    );

    // Multiple words in query
    let submission = SubmissionParser::parse("/skills search code review tools");
    assert!(
        matches!(submission, Submission::SystemCommand { command, args }
            if command == "skills" && args == vec!["search", "code", "review", "tools"])
    );
}

#[test]
fn test_parser_job_status() {
    // /status with no id → all jobs
    let s = SubmissionParser::parse("/status");
    assert!(matches!(s, Submission::JobStatus { job_id: None }));

    // /progress alias
    let s = SubmissionParser::parse("/progress");
    assert!(matches!(s, Submission::JobStatus { job_id: None }));

    // /status with id
    let s = SubmissionParser::parse("/status abc123");
    assert!(matches!(s, Submission::JobStatus { job_id: Some(id) } if id == "abc123"));

    // /progress with id
    let s = SubmissionParser::parse("/progress abc123");
    assert!(matches!(s, Submission::JobStatus { job_id: Some(id) } if id == "abc123"));

    // case insensitive
    let s = SubmissionParser::parse("/STATUS");
    assert!(matches!(s, Submission::JobStatus { job_id: None }));
}

#[test]
fn test_parser_job_list() {
    // /list is an alias for /status with no job_id
    let s = SubmissionParser::parse("/list");
    assert!(matches!(s, Submission::JobStatus { job_id: None }));

    let s = SubmissionParser::parse("/LIST");
    assert!(matches!(s, Submission::JobStatus { job_id: None }));
}

#[test]
fn test_parser_job_cancel() {
    let s = SubmissionParser::parse("/cancel abc123");
    assert!(matches!(s, Submission::JobCancel { job_id } if job_id == "abc123"));

    // /cancel with no id → falls through to UserInput
    let s = SubmissionParser::parse("/cancel");
    assert!(matches!(s, Submission::UserInput { .. }));
}

#[test]
fn test_job_commands_are_control() {
    assert!(SubmissionParser::parse("/status").is_control());
    assert!(SubmissionParser::parse("/list").is_control());
    assert!(SubmissionParser::parse("/cancel abc").is_control());
}
