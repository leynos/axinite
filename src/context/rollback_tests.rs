//! Rollback-specific tests for `JobContext::set_state_rollback`.

use super::*;

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
