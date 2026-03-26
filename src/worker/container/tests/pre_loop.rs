//! Tests for pre-loop failure handling and startup error reporting.

use std::sync::Arc;

use axum::http::StatusCode;
use rstest::rstest;
use uuid::Uuid;

use super::test_support::{RuntimeTestState, setup_runtime_test};
use crate::error::{Error, ToolError};
use crate::worker::api::WorkerState;
use crate::worker::container::{WorkerError, WorkerExecutionResult};

#[derive(Clone, Copy, Debug)]
enum PreLoopFailureCase {
    GetJob,
    HydrateCredentials,
}

async fn assert_startup_failure_completions(state: &RuntimeTestState) {
    let completions = state.completions.lock().await;
    assert_eq!(
        completions.len(),
        1,
        "expected a terminal completion report"
    );
    assert_eq!(
        completions[0].message.as_deref(),
        Some("Worker failed during startup")
    );
    drop(completions);

    let result_events = state.result_events.lock().await;
    assert_eq!(result_events.len(), 1, "expected a terminal result event");
    assert_eq!(result_events[0]["message"], "Worker failed during startup");
    assert_eq!(result_events[0]["success"], false);
}

async fn assert_startup_failure(state: &RuntimeTestState) {
    let statuses = state.statuses.lock().await;
    assert_eq!(
        statuses.len(),
        1,
        "expected exactly one terminal status update, got {statuses:?}"
    );
    let failed_status = statuses
        .first()
        .filter(|status| status.state == WorkerState::Failed)
        .expect("expected a terminal failed status update");
    assert_eq!(failed_status.iteration, 100);
    assert_eq!(
        failed_status.message.as_deref(),
        Some("pre-loop failure"),
        "expected a sanitised pre-loop failure message, got {failed_status:?}"
    );
    drop(statuses);

    assert_startup_failure_completions(state).await;
}

#[rstest]
#[case(PreLoopFailureCase::GetJob)]
#[case(PreLoopFailureCase::HydrateCredentials)]
#[tokio::test]
async fn worker_runtime_reports_failed_status_for_pre_loop_errors(
    #[case] case: PreLoopFailureCase,
) -> anyhow::Result<()> {
    let state = Arc::new(RuntimeTestState::default());
    match case {
        PreLoopFailureCase::GetJob => {
            state
                .job_statuses
                .lock()
                .await
                .push_back(StatusCode::INTERNAL_SERVER_ERROR);
            state.status_statuses.lock().await.push_back(StatusCode::OK);
        }
        PreLoopFailureCase::HydrateCredentials => {
            state.job_statuses.lock().await.push_back(StatusCode::OK);
            state
                .credential_statuses
                .lock()
                .await
                .push_back(StatusCode::INTERNAL_SERVER_ERROR);
            state.status_statuses.lock().await.push_back(StatusCode::OK);
        }
    }

    let job_id = Uuid::new_v4();
    let mut harness = setup_runtime_test(Arc::clone(&state), job_id).await?;

    let error = harness
        .take_runtime()
        .run()
        .await
        .expect_err("expected runtime to fail before the execution loop");
    assert!(
        !error.to_string().is_empty(),
        "pre-loop failure should preserve the original error"
    );

    assert_startup_failure(&state).await;

    Ok(())
}

#[tokio::test]
async fn worker_runtime_emits_failed_status_for_initial_status_rejections() -> anyhow::Result<()> {
    let state = Arc::new(RuntimeTestState::default());
    state.job_statuses.lock().await.push_back(StatusCode::OK);
    state
        .status_statuses
        .lock()
        .await
        .push_back(StatusCode::INTERNAL_SERVER_ERROR);
    state.status_statuses.lock().await.push_back(StatusCode::OK);

    let job_id = Uuid::new_v4();
    let mut harness = setup_runtime_test(Arc::clone(&state), job_id).await?;

    let error = harness.take_runtime().run().await;
    let error = error.expect_err("expected runtime to fail when the initial status is rejected");

    assert!(
        matches!(error, WorkerError::OrchestratorRejected { .. }),
        "expected initial status rejection error, got {error}"
    );

    let statuses = state.statuses.lock().await;
    assert_eq!(
        statuses.len(),
        2,
        "expected rejected status + terminal failed status"
    );
    assert_eq!(statuses[0].state, WorkerState::InProgress);
    assert_eq!(statuses[1].state, WorkerState::Failed);
    assert_eq!(statuses[1].iteration, 100);
    assert_eq!(
        statuses[1].message.as_deref(),
        Some("pre-loop failure"),
        "expected a sanitised pre-loop failure status payload, got {:?}",
        statuses[1]
    );
    drop(statuses);

    assert_startup_failure_completions(&state).await;

    Ok(())
}

#[rstest]
#[case(
    WorkerExecutionResult::Failed(Error::Tool(ToolError::ExecutionFailed {
        name: "shell".to_string(),
        reason: "token secret-123 leaked".to_string(),
    })),
    "Execution failed"
)]
#[case(WorkerExecutionResult::TimedOut, "Execution timed out")]
#[tokio::test]
async fn worker_runtime_sanitizes_failure_messages(
    #[case] execution: WorkerExecutionResult,
    #[case] expected_message: &str,
) -> anyhow::Result<()> {
    let state = Arc::new(RuntimeTestState::default());
    let job_id = Uuid::new_v4();
    let harness = setup_runtime_test(Arc::clone(&state), job_id).await?;

    harness
        .runtime
        .as_ref()
        .expect("runtime test harness should contain a runtime")
        .report_completion(execution, 7)
        .await
        .expect("report_completion should succeed in test harness");

    let completions = state.completions.lock().await;
    assert_eq!(completions.len(), 1);
    assert_eq!(completions[0].message.as_deref(), Some(expected_message));
    assert_eq!(completions[0].iterations, 7);
    drop(completions);

    let result_events = state.result_events.lock().await;
    assert_eq!(result_events.len(), 1);
    assert_eq!(result_events[0]["message"], expected_message);
    assert_eq!(result_events[0]["success"], false);
    assert!(
        result_events[0].to_string().contains(expected_message),
        "expected result payload to contain the sanitised message"
    );
    assert!(
        !result_events[0].to_string().contains("secret-123"),
        "result payload should not leak the detailed error text"
    );

    Ok(())
}
