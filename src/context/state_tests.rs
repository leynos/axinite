//! Comprehensive tests for job-state transitions, lifecycle helpers, token
//! budgeting, and `stuck_since()` timestamp tracking.

use super::*;
use rand::{Rng, SeedableRng, rngs::StdRng};
use rstest::rstest;

fn all_job_states() -> [JobState; 8] {
    [
        JobState::Pending,
        JobState::InProgress,
        JobState::Completed,
        JobState::Submitted,
        JobState::Accepted,
        JobState::Failed,
        JobState::Stuck,
        JobState::Cancelled,
    ]
}

fn completion_timestamp_for(transitions: &[StateTransition]) -> Option<DateTime<Utc>> {
    transitions
        .iter()
        .rev()
        .find(|transition| {
            matches!(
                transition.to,
                JobState::Completed | JobState::Accepted | JobState::Failed | JobState::Cancelled
            )
        })
        .map(|transition| transition.timestamp)
}

fn rollback_tracked_as_completed(state: JobState) -> bool {
    matches!(
        state,
        JobState::Completed | JobState::Accepted | JobState::Failed | JobState::Cancelled
    )
}

fn transition_snapshot(
    transitions: &[StateTransition],
) -> Vec<(JobState, JobState, DateTime<Utc>, Option<String>)> {
    transitions
        .iter()
        .map(|transition| {
            (
                transition.from,
                transition.to,
                transition.timestamp,
                transition.reason.clone(),
            )
        })
        .collect()
}

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

#[rstest]
#[case(JobState::Accepted, true)]
#[case(JobState::Failed, true)]
#[case(JobState::Cancelled, true)]
#[case(JobState::InProgress, false)]
#[case(JobState::Pending, false)]
#[case(JobState::Completed, false)]
#[case(JobState::Submitted, false)]
#[case(JobState::Stuck, false)]
fn test_terminal_states(#[case] state: JobState, #[case] expected: bool) {
    assert_eq!(state.is_terminal(), expected);
}

#[rstest]
#[case("pending", JobState::Pending)]
#[case("in_progress", JobState::InProgress)]
#[case("completed", JobState::Completed)]
#[case("submitted", JobState::Submitted)]
#[case("accepted", JobState::Accepted)]
#[case("failed", JobState::Failed)]
#[case("stuck", JobState::Stuck)]
#[case("cancelled", JobState::Cancelled)]
fn test_job_state_from_str_parses_known_values(#[case] input: &str, #[case] expected: JobState) {
    let parsed = input
        .parse::<JobState>()
        .expect("failed to parse JobState from test input");
    assert_eq!(parsed, expected, "failed to parse '{input}'");
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
    ctx.add_tokens(500)
        .expect("failed to add tokens to ctx during token-budget test");
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

#[test]
fn test_set_state_rollback_ignores_mismatched_transition_history() {
    let mut ctx = JobContext::new("Test", "Rollback mismatch test");
    ctx.transition_to(JobState::InProgress, None)
        .expect("failed to transition to InProgress");
    ctx.transition_to(JobState::Completed, Some("Done".to_string()))
        .expect("failed to transition to Completed");

    let expected_state = ctx.state;
    let expected_completed_at = ctx.completed_at;
    let expected_transition_len = ctx.transitions.len();
    let expected_last_transition = ctx
        .transitions
        .last()
        .map(|transition| (transition.from, transition.to, transition.reason.clone()));

    ctx.set_state_rollback(JobState::Pending);

    assert_eq!(
        ctx.state, expected_state,
        "rollback should not change state when the latest transition does not match"
    );
    assert_eq!(
        ctx.completed_at, expected_completed_at,
        "rollback should not change completed_at when the latest transition does not match"
    );
    assert_eq!(
        ctx.transitions.len(),
        expected_transition_len,
        "rollback should not change transition count when the latest transition does not match"
    );
    assert_eq!(
        ctx.transitions.last().map(|transition| (
            transition.from,
            transition.to,
            transition.reason.clone()
        )),
        expected_last_transition,
        "rollback should not change the latest transition when the latest transition does not match"
    );
}

#[test]
fn test_set_state_rollback_applies_across_bounded_state_pairs() {
    let base = Utc::now();

    for (previous_idx, previous) in all_job_states().into_iter().enumerate() {
        for (current_idx, current) in all_job_states().into_iter().enumerate() {
            let mut ctx = JobContext::new("Test", "Rollback property test");
            let earlier_timestamp =
                base + chrono::Duration::seconds((previous_idx * 10 + current_idx) as i64);
            let rollback_timestamp = earlier_timestamp + chrono::Duration::seconds(1);

            ctx.transitions.push(StateTransition {
                from: JobState::Pending,
                to: JobState::Completed,
                timestamp: earlier_timestamp,
                reason: Some("earlier terminal".to_string()),
            });
            ctx.transitions.push(StateTransition {
                from: previous,
                to: current,
                timestamp: rollback_timestamp,
                reason: Some("rollback edge".to_string()),
            });
            ctx.state = current;
            ctx.completed_at = Some(rollback_timestamp);

            let before_len = ctx.transitions.len();
            assert!(
                ctx.last_transition_matches_rollback(previous),
                "expected rollback edge to match for previous={previous:?}, current={current:?}"
            );

            ctx.set_state_rollback(previous);

            assert_eq!(
                ctx.state, previous,
                "rollback should restore previous state for previous={previous:?}, current={current:?}"
            );
            assert_eq!(
                ctx.transitions.len(),
                before_len - 1,
                "rollback should remove the latest transition for previous={previous:?}, current={current:?}"
            );
            assert_eq!(
                ctx.completed_at,
                if rollback_tracked_as_completed(previous) {
                    completion_timestamp_for(&ctx.transitions)
                } else {
                    None
                },
                "rollback should recompute completed_at from remaining transitions for previous={previous:?}, current={current:?}"
            );
        }
    }
}

#[test]
fn test_set_state_rollback_skips_mismatched_edges_across_bounded_state_pairs() {
    let base = Utc::now();

    for (previous_idx, previous) in all_job_states().into_iter().enumerate() {
        for (current_idx, current) in all_job_states().into_iter().enumerate() {
            let mut ctx = JobContext::new("Test", "Rollback mismatch property test");
            let earlier_timestamp =
                base + chrono::Duration::seconds((previous_idx * 10 + current_idx) as i64);
            let latest_timestamp = earlier_timestamp + chrono::Duration::seconds(1);
            let mismatched_from = all_job_states()
                .into_iter()
                .find(|candidate| *candidate != previous)
                .expect("expected at least one distinct JobState");

            ctx.transitions.push(StateTransition {
                from: JobState::Pending,
                to: JobState::Accepted,
                timestamp: earlier_timestamp,
                reason: Some("earlier terminal".to_string()),
            });
            ctx.transitions.push(StateTransition {
                from: mismatched_from,
                to: current,
                timestamp: latest_timestamp,
                reason: Some("mismatched rollback edge".to_string()),
            });
            ctx.state = current;
            ctx.completed_at = Some(latest_timestamp);

            let expected_state = ctx.state;
            let expected_completed_at = ctx.completed_at;
            let expected_transitions = transition_snapshot(&ctx.transitions);

            assert!(
                !ctx.last_transition_matches_rollback(previous),
                "expected rollback edge mismatch for previous={previous:?}, current={current:?}"
            );

            ctx.set_state_rollback(previous);

            assert_eq!(
                ctx.state, expected_state,
                "rollback should not change state when the edge mismatches for previous={previous:?}, current={current:?}"
            );
            assert_eq!(
                ctx.completed_at, expected_completed_at,
                "rollback should not change completed_at when the edge mismatches for previous={previous:?}, current={current:?}"
            );
            assert_eq!(
                transition_snapshot(&ctx.transitions),
                expected_transitions,
                "rollback should not change transitions when the edge mismatches for previous={previous:?}, current={current:?}"
            );
        }
    }
}

/// Simulate random `JobContext` and `JobState` transitions with `StdRng`; the `_` branch intentionally ignores random choices that are invalid for the current `JobState`.
fn apply_random_step(ctx: &mut JobContext, rng: &mut StdRng, case_idx: usize, step: usize) {
    match rng.gen_range(0..4) {
        0 if matches!(ctx.state, JobState::Pending) => {
            ctx.transition_to(JobState::InProgress, None)
                .expect("failed to transition to InProgress");
        }
        1 if matches!(ctx.state, JobState::InProgress) => {
            ctx.mark_stuck(format!("stall-{case_idx}-{step}"))
                .expect("failed to mark context as stuck");
        }
        2 if matches!(ctx.state, JobState::Stuck) => {
            ctx.attempt_recovery().expect("failed to attempt recovery");
        }
        _ => {}
    }
}

#[test]
fn test_stuck_since_matches_latest_stuck_transition_across_bounded_sequences() {
    let mut rng = StdRng::seed_from_u64(0x5EED_5EED);

    for sequence_len in 0..=32 {
        for case_idx in 0..32 {
            let mut ctx = JobContext::new("Test", "Randomized stuck_since test");

            for step in 0..sequence_len {
                apply_random_step(&mut ctx, &mut rng, case_idx, step);
            }

            let expected = ctx
                .transitions
                .iter()
                .rev()
                .find(|t| t.to == JobState::Stuck)
                .map(|t| t.timestamp);

            assert_eq!(
                ctx.stuck_since(),
                expected,
                concat!(
                    "stuck_since invariant failed for ",
                    "sequence_len={}, case_idx={}"
                ),
                sequence_len,
                case_idx
            );
        }
    }
}
