//! Shared start/run sequencing helpers for the startup run loop.

use ironclaw::agent::Agent;

/// Coordinates startup side effects, the agent run future, and failure-path
/// shutdown cleanup.
pub(crate) async fn coordinate_start_run_shutdown<Start, Run, RunFuture, Shutdown, ShutdownFuture>(
    start: Start,
    run: Run,
    shutdown: Shutdown,
) -> anyhow::Result<()>
where
    Start: FnOnce() -> anyhow::Result<()>,
    Run: FnOnce() -> RunFuture,
    RunFuture: std::future::Future<Output = anyhow::Result<()>>,
    Shutdown: FnOnce() -> ShutdownFuture,
    ShutdownFuture: std::future::Future<Output = ()>,
{
    start()?;
    let run_result = run().await;
    if run_result.is_err() {
        shutdown().await;
    }
    run_result
}

/// Starts runtime side effects and enters the agent run loop.
///
/// Propagates any `side_effects.start()` error before the loop begins,
/// then maps the agent's own error type into `anyhow::Error` on exit.
pub(crate) async fn run_with_side_effects<Shutdown, ShutdownFuture>(
    side_effects: ironclaw::app::RuntimeSideEffects,
    agent: Agent,
    shutdown: Shutdown,
) -> anyhow::Result<()>
where
    Shutdown: FnOnce() -> ShutdownFuture,
    ShutdownFuture: std::future::Future<Output = ()>,
{
    coordinate_start_run_shutdown(
        || side_effects.start().map(|_| ()),
        || async { agent.run().await.map_err(anyhow::Error::from) },
        shutdown,
    )
    .await
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use super::coordinate_start_run_shutdown;

    #[tokio::test]
    async fn coordinate_returns_err_and_does_not_shutdown_when_start_fails() {
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let s = shutdown_calls.clone();
        let start = || -> anyhow::Result<()> { anyhow::bail!("fail-start") };
        let run = || async { Ok(()) };
        let shutdown = || async {
            s.fetch_add(1, Ordering::SeqCst);
        };
        let res = coordinate_start_run_shutdown(start, run, shutdown).await;
        assert!(res.is_err());
        assert_eq!(shutdown_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn coordinate_runs_and_does_not_shutdown_on_success() {
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let s = shutdown_calls.clone();
        let start = || -> anyhow::Result<()> { Ok(()) };
        let run = || async { Ok(()) };
        let shutdown = || async {
            s.fetch_add(1, Ordering::SeqCst);
        };
        let res = coordinate_start_run_shutdown(start, run, shutdown).await;
        assert!(res.is_ok());
        assert_eq!(shutdown_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn coordinate_runs_and_shutdowns_on_failure() {
        let shutdown_calls = Arc::new(AtomicUsize::new(0));
        let s = shutdown_calls.clone();
        let start = || -> anyhow::Result<()> { Ok(()) };
        let run = || async { anyhow::Result::<()>::Err(anyhow::anyhow!("fail-run")) };
        let shutdown = || async {
            s.fetch_add(1, Ordering::SeqCst);
        };
        let res = coordinate_start_run_shutdown(start, run, shutdown).await;
        assert!(res.is_err());
        assert_eq!(shutdown_calls.load(Ordering::SeqCst), 1);
    }
}
