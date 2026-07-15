//! Characterisation tests for terminal job-state transitions and duplicates.

use crate::context::JobState;
use crate::testing::CapturingStore;
use crate::testing::worker_harness::*;
use crate::worker::job::Worker;

#[rstest::rstest]
#[case::completed(
    TerminalTestCase {
        method: TerminalMethod::Completed,
        expected_state: JobState::Completed,
        expected_status: "completed",
        expected_reason: Some("Job completed successfully"),
    }
)]
#[case::failed(
    TerminalTestCase {
        method: TerminalMethod::Failed("budget exceeded"),
        expected_state: JobState::Failed,
        expected_status: "failed",
        expected_reason: Some("budget exceeded"),
    }
)]
#[case::stuck(
    TerminalTestCase {
        method: TerminalMethod::Stuck("timeout"),
        expected_state: JobState::Stuck,
        expected_status: "stuck",
        expected_reason: Some("timeout"),
    }
)]
#[tokio::test]
async fn test_terminal_state_characterises_persistence(
    #[case] case: TerminalTestCase,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (worker, store) = make_worker_with_capturing_store(vec![]).await?;

    // Transition to InProgress first
    transition_to_in_progress(&worker).await?;

    // Execute the terminal state transition
    case.method.apply_transition(&worker).await?;

    // Verify state in ContextManager
    let ctx = worker
        .context_manager()
        .get_context(worker.job_id)
        .await
        .expect("failed to get context after terminal transition");
    assert_eq!(ctx.state, case.expected_state);

    assert_terminal_persistence_with_snapshot(
        &store,
        case.expected_state,
        case.expected_status,
        case.expected_reason,
    )
    .await?;
    Ok(())
}

/// Test case structure for parameterised terminal state tests.
struct TerminalTestCase {
    method: TerminalMethod,
    expected_state: JobState,
    expected_status: &'static str,
    expected_reason: Option<&'static str>,
}

async fn get_call_counts(store: &CapturingStore) -> (usize, usize) {
    let calls = store.calls();
    let status_count = calls.status_history.lock().await.len();
    let event_count = calls.event_history.lock().await.len();
    (status_count, event_count)
}

/// One rejected-transition scenario: the method to attempt, the terminal
/// state already reached, and the persistence call counts recorded before
/// the attempt.
struct RejectedTransitionCase {
    /// Terminal transition expected to be rejected.
    rejected: TerminalMethod,
    /// Terminal state the job is already in.
    expected_state: JobState,
    /// `(status_count, event_count)` captured before the attempt.
    before: (usize, usize),
}

async fn assert_rejected_does_not_persist(
    worker: &Worker,
    store: &CapturingStore,
    case: RejectedTransitionCase,
) {
    let RejectedTransitionCase {
        rejected,
        expected_state,
        before,
    } = case;
    let result = match rejected {
        TerminalMethod::Completed => worker.mark_completed().await,
        TerminalMethod::Failed(reason) => worker.mark_failed(reason).await,
        TerminalMethod::Stuck(reason) => worker.mark_stuck(reason).await,
    };
    assert!(
        result.is_err(),
        "Terminal transition {:?} after {:?} should be rejected",
        rejected,
        expected_state
    );

    let after = get_call_counts(store).await;
    assert_eq!(
        after.0, before.0,
        "Rejected transition {:?} after {:?} should not persist status",
        rejected, expected_state
    );
    assert_eq!(
        after.1, before.1,
        "Rejected transition {:?} after {:?} should not persist event",
        rejected, expected_state
    );
}

async fn run_single_terminal_case(
    method: TerminalMethod,
    expected_state: JobState,
    expected_status: &str,
    expected_reason: Option<&str>,
) -> anyhow::Result<()> {
    let (worker, store) = make_worker_with_capturing_store(vec![]).await?;
    transition_to_in_progress(&worker).await?;

    method.apply_transition(&worker).await?;

    let ctx = worker.context_manager().get_context(worker.job_id).await?;
    assert_eq!(
        ctx.state, expected_state,
        "State should match expected terminal state"
    );

    assert_terminal_persistence(&store, expected_state, expected_status, expected_reason).await?;
    let before = get_call_counts(&store).await;

    for rejected in [
        TerminalMethod::Completed,
        TerminalMethod::Failed("cross-terminal failure"),
        TerminalMethod::Stuck("cross-terminal stuck"),
    ] {
        assert_rejected_does_not_persist(
            &worker,
            &store,
            RejectedTransitionCase {
                rejected,
                expected_state,
                before,
            },
        )
        .await;
    }

    Ok(())
}

#[tokio::test]
async fn test_double_completed_transition_rejected()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (worker, store) = make_worker_with_capturing_store(vec![]).await?;

    // Transition to InProgress first
    transition_to_in_progress(&worker).await?;

    // First call succeeds
    worker
        .mark_completed()
        .await
        .expect("first mark_completed should succeed");

    // Record call counts before attempting duplicate transition
    let status_count_before = store.calls().status_history.lock().await.len();
    let event_count_before = store.calls().event_history.lock().await.len();

    // Second call should fail
    let result = worker.mark_completed().await;
    assert!(
        result.is_err(),
        "Double transition to Completed should be rejected"
    );

    // Verify no new persistence calls were made on rejected transition
    let status_count_after = store.calls().status_history.lock().await.len();
    let event_count_after = store.calls().event_history.lock().await.len();
    assert_eq!(
        status_count_after, status_count_before,
        "Rejected transition should not persist status"
    );
    assert_eq!(
        event_count_after, event_count_before,
        "Rejected transition should not persist event"
    );

    assert_terminal_persistence_with_snapshot(
        &store,
        JobState::Completed,
        "completed",
        Some("Job completed successfully"),
    )
    .await?;
    Ok(())
}

/// Terminal transition rejection test for duplicate state changes.
///
/// Verifies that after transitioning to a terminal state (Completed,
/// Failed, or Stuck), subsequent attempts to transition to any terminal
/// state are rejected and persistence calls remain unchanged.
///
/// This is a curated test covering the three terminal states; it does
/// not generate arbitrary sequences or property-based inputs.
#[tokio::test]
async fn test_terminal_transition_rejects_duplicates()
-> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let test_cases = [
        (
            TerminalMethod::Completed,
            JobState::Completed,
            "completed",
            Some("Job completed successfully"),
        ),
        (
            TerminalMethod::Failed("test failure"),
            JobState::Failed,
            "failed",
            Some("test failure"),
        ),
        (
            TerminalMethod::Stuck("test stuck"),
            JobState::Stuck,
            "stuck",
            Some("test stuck"),
        ),
    ];

    for (method, expected_state, expected_status, expected_reason) in test_cases {
        run_single_terminal_case(method, expected_state, expected_status, expected_reason).await?;
    }
    Ok(())
}
