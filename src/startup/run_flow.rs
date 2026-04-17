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
    let run_result = async {
        start()?;
        run().await
    }
    .await;
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
