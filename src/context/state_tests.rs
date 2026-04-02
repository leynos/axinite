//! Comprehensive tests for job-state transitions, lifecycle helpers, token
//! budgeting, and `stuck_since()` timestamp tracking.

use super::*;

#[test]
fn test_valid_state_transitions() {
    assert!(JobState::Pending.can_transition_to(JobState::InProgress));
    assert!(JobState::InProgress.can_transition_to(JobState::Completed));
}

#[test]
fn test_invalid_state_transitions() {
    assert!(!JobState::Completed.can_transition_to(JobState::Pending));
    assert!(!JobState::Accepted.can_transition_to(JobState::InProgress));
}

#[test]
fn test_terminal_states_are_terminal() {
    assert!(JobState::Accepted.is_terminal());
    assert!(JobState::Failed.is_terminal());
    assert!(JobState::Cancelled.is_terminal());
}

#[test]
fn test_non_terminal_states_are_not_terminal() {
    assert!(!JobState::Pending.is_terminal());
    assert!(!JobState::InProgress.is_terminal());
    assert!(!JobState::Completed.is_terminal());
    assert!(!JobState::Submitted.is_terminal());
    assert!(!JobState::Stuck.is_terminal());
}

#[test]
fn test_job_state_from_str_parses_known_values() {
    let cases = [
        ("pending", JobState::Pending),
        ("in_progress", JobState::InProgress),
        ("completed", JobState::Completed),
        ("submitted", JobState::Submitted),
        ("accepted", JobState::Accepted),
        ("failed", JobState::Failed),
        ("stuck", JobState::Stuck),
        ("cancelled", JobState::Cancelled),
    ];
    for (input, expected) in cases {
        let failure_message = format!("failed to parse '{input}' into JobState");
        assert_eq!(
            input.parse::<JobState>().expect(&failure_message),
            expected,
            "failed to parse '{input}'",
        );
    }
}

#[test]
fn test_job_state_from_str_rejects_unknown_values() {
    assert!("unknown".parse::<JobState>().is_err());
}

#[test]
fn test_job_context_transitions() {
    let mut ctx = JobContext::new("Test", "Test job");
    assert_eq!(ctx.state, JobState::Pending);

    ctx.transition_to(JobState::InProgress, None)
        .expect("failed to transition to InProgress");
    assert_eq!(ctx.state, JobState::InProgress);
    assert!(ctx.started_at.is_some());

    ctx.transition_to(JobState::Completed, Some("Done".to_string()))
        .expect("failed to transition to Completed");
    assert_eq!(ctx.state, JobState::Completed);
}

#[test]
fn test_transition_history_capped() {
    let mut ctx = JobContext::new("Test", "Transition cap test");
    // Cycle through Pending -> InProgress -> Stuck -> InProgress -> Stuck ...
    ctx.transition_to(JobState::InProgress, None)
        .expect("failed to transition to InProgress");
    for i in 0..250 {
        ctx.mark_stuck(format!("stuck {}", i))
            .expect("failed to mark context as stuck");
        ctx.attempt_recovery().expect("failed to attempt recovery");
    }
    // 1 initial + 250*2 = 501 transitions, should be capped at 200
    assert_eq!(
        ctx.transitions.len(),
        200,
        "transitions should be capped at exactly 200"
    );
}

#[test]
fn test_add_tokens_within_budget_is_accepted() {
    let mut ctx = JobContext::new("Test", "Budget test");
    ctx.max_tokens = 1000;
    assert!(ctx.add_tokens(500).is_ok());
    assert_eq!(ctx.total_tokens_used, 500);
}

#[test]
fn test_add_tokens_exceeding_budget_errors_but_still_records() {
    let mut ctx = JobContext::new("Test", "Budget test");
    ctx.max_tokens = 1000;
    ctx.add_tokens(500).unwrap();
    assert!(ctx.add_tokens(600).is_err());
    assert_eq!(ctx.total_tokens_used, 1100); // tokens still recorded
}

#[test]
fn test_add_tokens_unlimited() {
    let mut ctx = JobContext::new("Test", "No budget");
    // max_tokens = 0 means unlimited
    assert!(ctx.add_tokens(1_000_000).is_ok());
}

#[test]
fn test_budget_exceeded() {
    let mut ctx = JobContext::new("Test", "Money test");
    ctx.budget = Some(Decimal::new(100, 0)); // $100
    assert!(!ctx.budget_exceeded());
    ctx.add_cost(Decimal::new(50, 0));
    assert!(!ctx.budget_exceeded());
    ctx.add_cost(Decimal::new(60, 0));
    assert!(ctx.budget_exceeded());
}

#[test]
fn test_budget_exceeded_none() {
    let ctx = JobContext::new("Test", "No budget");
    assert!(!ctx.budget_exceeded()); // No budget = never exceeded
}

#[test]
fn test_stuck_recovery() {
    let mut ctx = JobContext::new("Test", "Test job");
    ctx.transition_to(JobState::InProgress, None)
        .expect("transition_to failed");
    ctx.mark_stuck("Timed out").expect("mark_stuck failed");
    assert_eq!(ctx.state, JobState::Stuck);

    ctx.attempt_recovery().expect("attempt_recovery failed");
    assert_eq!(ctx.state, JobState::InProgress);
    assert_eq!(ctx.repair_attempts, 1);
}

#[test]
fn test_stuck_since_returns_none_when_job_was_never_stuck() {
    let mut ctx = JobContext::new("Test", "Test job");
    ctx.transition_to(JobState::InProgress, None)
        .expect("failed to transition JobContext to InProgress");

    assert_eq!(ctx.stuck_since(), None);
}

#[test]
fn test_stuck_since_returns_latest_stuck_transition() {
    let mut ctx = JobContext::new("Test", "Test job");
    ctx.transition_to(JobState::InProgress, None)
        .expect("transition_to failed");
    let first_stuck_at = ctx.created_at + chrono::Duration::seconds(1);
    let second_stuck_at = first_stuck_at + chrono::Duration::seconds(1);

    ctx.mark_stuck("First stall").expect("mark_stuck failed");
    ctx.transitions
        .last_mut()
        .expect("first stuck transition should exist")
        .timestamp = first_stuck_at;
    ctx.attempt_recovery().expect("attempt_recovery failed");
    ctx.mark_stuck("Second stall").expect("mark_stuck failed");
    ctx.transitions
        .last_mut()
        .expect("second stuck transition should exist")
        .timestamp = second_stuck_at;

    assert_eq!(ctx.stuck_since(), Some(second_stuck_at));
}
