//! Regression tests for startup run and shutdown sequencing.

use rstest::{fixture, rstest};

use crate::startup::run_flow::coordinate_start_run_shutdown;

use super::run_shutdown_sequence;

#[tokio::test]
async fn shutdown_broadcast_is_always_sent() {
    let shutdown_tx = tokio::sync::broadcast::channel::<()>(1).0;
    let mut rx = shutdown_tx.subscribe();
    let mcp_process_manager = ironclaw::tools::mcp::McpProcessManager::new();

    run_shutdown_sequence(&shutdown_tx, &mcp_process_manager, &None, &None, &None).await;

    assert!(matches!(rx.try_recv(), Ok(())));
}

struct FailingSideEffects;

impl FailingSideEffects {
    fn start(self) -> anyhow::Result<()> {
        anyhow::bail!("injected failure");
    }
}

struct ShutdownHarness {
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    rx: tokio::sync::broadcast::Receiver<()>,
    mcp_process_manager: ironclaw::tools::mcp::McpProcessManager,
}

enum FailureCase {
    SideEffects,
    Agent,
}

#[fixture]
fn shutdown_harness() -> ShutdownHarness {
    let shutdown_tx = tokio::sync::broadcast::channel::<()>(1).0;
    let rx = shutdown_tx.subscribe();
    ShutdownHarness {
        shutdown_tx,
        rx,
        mcp_process_manager: ironclaw::tools::mcp::McpProcessManager::new(),
    }
}

#[rstest]
#[case::side_effects(FailureCase::SideEffects)]
#[case::agent(FailureCase::Agent)]
#[tokio::test]
async fn shutdown_runs_after_failures(
    #[case] failure_case: FailureCase,
    mut shutdown_harness: ShutdownHarness,
) {
    let run_result = match failure_case {
        FailureCase::SideEffects => {
            let side_effects = FailingSideEffects;
            coordinate_start_run_shutdown(
                || side_effects.start(),
                || async { Ok(()) },
                || async {
                    run_shutdown_sequence(
                        &shutdown_harness.shutdown_tx,
                        &shutdown_harness.mcp_process_manager,
                        &None,
                        &None,
                        &None,
                    )
                    .await
                },
            )
            .await
        }
        FailureCase::Agent => {
            coordinate_start_run_shutdown(
                || Ok(()),
                || async { anyhow::bail!("injected agent failure") },
                || async {
                    run_shutdown_sequence(
                        &shutdown_harness.shutdown_tx,
                        &shutdown_harness.mcp_process_manager,
                        &None,
                        &None,
                        &None,
                    )
                    .await
                },
            )
            .await
        }
    };

    assert!(
        shutdown_harness.rx.try_recv().is_ok(),
        "shutdown must be broadcast even after failure"
    );
    assert!(run_result.is_err());
}
