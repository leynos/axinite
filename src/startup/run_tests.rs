//! Regression tests for startup run and shutdown sequencing.

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

#[tokio::test]
async fn shutdown_runs_after_side_effects_failure() {
    let shutdown_tx = tokio::sync::broadcast::channel::<()>(1).0;
    let mut rx = shutdown_tx.subscribe();
    let mcp_process_manager = ironclaw::tools::mcp::McpProcessManager::new();
    let side_effects = FailingSideEffects;

    let run_result: anyhow::Result<()> = async {
        side_effects.start()?;
        Ok(())
    }
    .await;

    run_shutdown_sequence(&shutdown_tx, &mcp_process_manager, &None, &None, &None).await;

    assert!(
        rx.try_recv().is_ok(),
        "shutdown must be broadcast even after start failure"
    );
    assert!(run_result.is_err());
}
