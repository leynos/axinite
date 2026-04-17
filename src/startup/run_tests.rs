//! Regression tests for startup run and shutdown sequencing.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use ironclaw::tunnel::{NativeTunnel, Tunnel};

use super::run_shutdown_sequence;

struct TestTunnel {
    stopped: Arc<AtomicBool>,
}

impl NativeTunnel for TestTunnel {
    fn name(&self) -> &str {
        "test-tunnel"
    }

    fn start<'a>(
        &'a self,
        _local_host: &'a str,
        _local_port: u16,
    ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send + 'a {
        async { Ok("https://example.test".to_string()) }
    }

    fn stop(&self) -> impl std::future::Future<Output = anyhow::Result<()>> + Send + '_ {
        let stopped = Arc::clone(&self.stopped);
        async move {
            stopped.store(true, Ordering::SeqCst);
            Ok(())
        }
    }

    fn health_check(&self) -> impl std::future::Future<Output = bool> + Send + '_ {
        async { true }
    }

    fn public_url(&self) -> Option<String> {
        Some("https://example.test".to_string())
    }
}

struct FailingSideEffects;

impl FailingSideEffects {
    fn start(self) -> anyhow::Result<()> {
        Err(anyhow::anyhow!("injected failure"))
    }
}

fn test_tunnel(stopped: Arc<AtomicBool>) -> Option<Box<dyn Tunnel>> {
    Some(Box::new(TestTunnel { stopped }))
}

#[tokio::test]
async fn run_shutdown_sequence_broadcasts_and_tears_down_tunnel() {
    let shutdown_tx = tokio::sync::broadcast::channel(1).0;
    let mut receiver = shutdown_tx.subscribe();
    let mcp_process_manager = ironclaw::tools::mcp::McpProcessManager::new();
    let stopped = Arc::new(AtomicBool::new(false));
    let active_tunnel = test_tunnel(Arc::clone(&stopped));

    run_shutdown_sequence(
        &shutdown_tx,
        &mcp_process_manager,
        &None,
        &None,
        &active_tunnel,
    )
    .await;

    assert!(matches!(receiver.try_recv(), Ok(())));
    assert!(stopped.load(Ordering::SeqCst));
}

#[tokio::test]
async fn run_shutdown_sequence_still_runs_when_side_effect_start_fails() {
    let shutdown_tx = tokio::sync::broadcast::channel(1).0;
    let mut receiver = shutdown_tx.subscribe();
    let mcp_process_manager = ironclaw::tools::mcp::McpProcessManager::new();
    let stopped = Arc::new(AtomicBool::new(false));
    let active_tunnel = test_tunnel(Arc::clone(&stopped));
    let side_effects = FailingSideEffects;

    let run_result: anyhow::Result<()> = async {
        side_effects.start()?;
        Ok(())
    }
    .await;

    run_shutdown_sequence(
        &shutdown_tx,
        &mcp_process_manager,
        &None,
        &None,
        &active_tunnel,
    )
    .await;

    assert!(matches!(
        run_result,
        Err(ref error) if error.to_string().contains("injected failure")
    ));
    assert!(matches!(receiver.try_recv(), Ok(())));
    assert!(stopped.load(Ordering::SeqCst));
}
