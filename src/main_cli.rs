//! Binary-only CLI dispatch helpers for the host executable.

use ironclaw::{
    cli::{
        Cli, Command, run_mcp_command, run_pairing_command, run_service_command,
        run_status_command, run_tool_command,
    },
    tracing_fmt::{init_cli_tracing, init_worker_tracing},
};

#[cfg(any(feature = "postgres", feature = "libsql"))]
use ironclaw::setup::{SetupConfig, SetupWizard};

/// Routes a parsed [`Cli`] to the appropriate subcommand handler.
///
/// Returns `true` when a subcommand was matched and executed, `false` when the
/// default agent-run path should be taken instead.
pub(crate) async fn dispatch_subcommand(cli: &Cli) -> anyhow::Result<bool> {
    if let Some(dispatched) = dispatch_cli_tool_commands(cli).await? {
        return Ok(dispatched);
    }

    dispatch_agent_commands(cli)
        .await
        .map(|handled| handled.unwrap_or(false))
}

fn is_agent_subcommand(command: &Command) -> bool {
    matches!(
        command,
        Command::Worker { .. }
            | Command::ClaudeBridge { .. }
            | Command::Onboard { .. }
            | Command::Run
    )
}

// `dispatch_ironclaw_cli_command` intentionally uses a wildcard arm and
// delegates exhaustive compile-time coverage to `dispatch_sync_command`, which
// is the canonical exhaustive matcher over `Command`. When adding new command
// variants, update both `dispatch_ironclaw_cli_command` and
// `dispatch_sync_command` so async and sync routing stay aligned.
async fn dispatch_ironclaw_cli_command(command: &Command) -> Option<anyhow::Result<bool>> {
    match command {
        Command::Config(c) => Some(
            run_traced_async(|| async { ironclaw::cli::run_config_command(c.clone()).await }).await,
        ),
        Command::Registry(c) => Some(
            run_traced_async(|| async { ironclaw::cli::run_registry_command(c.clone()).await })
                .await,
        ),
        Command::Memory(c) => {
            Some(run_traced_async(|| async { ironclaw::cli::run_memory_command(c).await }).await)
        }
        Command::Doctor => {
            Some(run_traced_async(|| async { ironclaw::cli::run_doctor_command().await }).await)
        }
        _ => None,
    }
}

async fn dispatch_local_async_command(command: &Command) -> Option<anyhow::Result<bool>> {
    match command {
        Command::Tool(c) => {
            Some(run_traced_async(|| async { run_tool_command(c.clone()).await }).await)
        }
        Command::Mcp(c) => {
            Some(run_traced_async(|| async { run_mcp_command(*c.clone()).await }).await)
        }
        Command::Status => Some(run_traced_async(|| async { run_status_command().await }).await),
        #[cfg(feature = "import")]
        Command::Import(c) => {
            Some(run_traced_async(|| async { run_import_subcommand(c).await }).await)
        }
        _ => None,
    }
}

async fn dispatch_async_command(command: &Command) -> Option<anyhow::Result<bool>> {
    if let Some(result) = dispatch_ironclaw_cli_command(command).await {
        return Some(result);
    }
    dispatch_local_async_command(command).await
}

fn dispatch_sync_command(command: &Command) -> Option<anyhow::Result<bool>> {
    match command {
        Command::Pairing(c) => Some(run_traced_sync(|| {
            run_pairing_command(c.clone()).map_err(|e| anyhow::anyhow!("{e}"))
        })),
        Command::Service(c) => Some(run_traced_sync(|| run_service_command(c))),
        Command::Completion(c) => Some(run_traced_sync(|| c.run())),
        Command::Run
        | Command::Onboard { .. }
        | Command::Config(_)
        | Command::Tool(_)
        | Command::Registry(_)
        | Command::Mcp(_)
        | Command::Memory(_)
        | Command::Doctor
        | Command::Status
        | Command::Worker { .. }
        | Command::ClaudeBridge { .. } => None,
        #[cfg(feature = "import")]
        Command::Import(_) => None,
    }
}

/// Attempts to dispatch CLI tool and service subcommands.
///
/// Returns `Ok(Some(bool))` when a command was matched and executed,
/// `Ok(None)` when the command should be handled by the agent-run path,
/// and `Err(_)` on execution failure.
pub(crate) async fn dispatch_cli_tool_commands(cli: &Cli) -> anyhow::Result<Option<bool>> {
    let Some(command) = &cli.command else {
        return Ok(None);
    };

    if is_agent_subcommand(command) {
        return Ok(None);
    }

    if let Some(result) = dispatch_sync_command(command) {
        return result.map(Some);
    }

    if let Some(result) = dispatch_async_command(command).await {
        return result.map(Some);
    }

    Ok(None)
}

/// Handles worker-oriented subcommands: `Worker`, `ClaudeBridge`, and `Onboard`.
///
/// Returns `Ok(Some(true))` when a subcommand was matched and executed,
/// `Ok(None)` when the command is not a worker subcommand.
pub(crate) async fn dispatch_agent_commands(cli: &Cli) -> anyhow::Result<Option<bool>> {
    let Some(command) = &cli.command else {
        return Ok(None);
    };

    if matches!(command, Command::Run) {
        return Ok(None);
    }

    #[cfg(test)]
    if let Some(result) = test_support::try_dispatch_agent_command(command).await {
        return result;
    }

    if !is_agent_subcommand(command) {
        return Ok(None);
    }

    match command {
        Command::Worker {
            job_id,
            orchestrator_url,
            max_iterations,
        } => {
            dispatch_worker_subcommand(*job_id, orchestrator_url, *max_iterations).await?;
            Ok(Some(true))
        }
        Command::ClaudeBridge {
            job_id,
            orchestrator_url,
            max_turns,
            model,
        } => {
            dispatch_claude_bridge_subcommand(*job_id, orchestrator_url, *max_turns, model).await?;
            Ok(Some(true))
        }
        Command::Onboard {
            skip_auth,
            channels_only,
            provider_only,
            quick,
        } => {
            run_onboard_subcommand(*skip_auth, *channels_only, *provider_only, *quick).await?;
            Ok(Some(true))
        }
        Command::Run => Ok(None),
        Command::Config(_)
        | Command::Tool(_)
        | Command::Registry(_)
        | Command::Mcp(_)
        | Command::Memory(_)
        | Command::Pairing(_)
        | Command::Service(_)
        | Command::Doctor
        | Command::Status
        | Command::Completion(_) => Ok(None),
        #[cfg(feature = "import")]
        Command::Import(_) => Ok(None),
    }
}

async fn run_traced_async<F, Fut>(f: F) -> anyhow::Result<bool>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = anyhow::Result<()>>,
{
    init_cli_tracing();
    f().await?;
    Ok(true)
}

fn run_traced_sync<F>(f: F) -> anyhow::Result<bool>
where
    F: FnOnce() -> anyhow::Result<()>,
{
    init_cli_tracing();
    f()?;
    Ok(true)
}

async fn run_onboard_subcommand(
    skip_auth: bool,
    channels_only: bool,
    provider_only: bool,
    quick: bool,
) -> anyhow::Result<()> {
    #[cfg(any(feature = "postgres", feature = "libsql"))]
    {
        let config = SetupConfig {
            skip_auth,
            channels_only,
            provider_only,
            quick,
        };
        SetupWizard::with_config(config).run().await?;
    }
    #[cfg(not(any(feature = "postgres", feature = "libsql")))]
    {
        let _ = (skip_auth, channels_only, provider_only, quick);
        anyhow::bail!("Onboarding wizard requires the 'postgres' or 'libsql' feature.");
    }
    Ok(())
}

#[cfg(feature = "import")]
async fn run_import_subcommand(import_cmd: &ironclaw::cli::ImportCommand) -> anyhow::Result<()> {
    let config = ironclaw::config::Config::from_env().await?;
    ironclaw::cli::run_import_command(import_cmd, &config).await
}

async fn dispatch_claude_bridge_subcommand(
    job_id: uuid::Uuid,
    orchestrator_url: &str,
    max_turns: u32,
    model: &str,
) -> anyhow::Result<()> {
    init_worker_tracing();
    ironclaw::worker::run_claude_bridge(job_id, orchestrator_url, max_turns, model).await
}

async fn dispatch_worker_subcommand(
    job_id: uuid::Uuid,
    orchestrator_url: &str,
    max_iterations: u32,
) -> anyhow::Result<()> {
    init_worker_tracing();
    ironclaw::worker::run_worker(job_id, orchestrator_url, max_iterations).await
}

#[cfg(test)]
mod test_support {
    use std::cell::{Cell, RefCell};
    use std::sync::{Arc, OnceLock};

    use ironclaw::cli::Command;

    pub(super) type AgentDispatchHook = fn(&Command) -> anyhow::Result<Option<bool>>;
    pub(super) static AGENT_DISPATCH_HOOK: OnceLock<
        Arc<tokio::sync::Mutex<Option<AgentDispatchHook>>>,
    > = OnceLock::new();

    thread_local! {
        // Tests hold the Tokio mutex for their full lifetime to serialize
        // hook-using and non-hook tests. Mirror the installed hook in
        // thread-local storage so dispatch can observe it without deadlocking
        // on a re-entrant lock attempt from the same test thread.
        static TEST_AGENT_DISPATCH_GUARD_HELD: Cell<bool> = const { Cell::new(false) };
        static TEST_AGENT_DISPATCH_HOOK: RefCell<Option<AgentDispatchHook>> = const { RefCell::new(None) };
    }

    pub(super) fn agent_dispatch_hook()
    -> &'static Arc<tokio::sync::Mutex<Option<AgentDispatchHook>>> {
        AGENT_DISPATCH_HOOK.get_or_init(|| Arc::new(tokio::sync::Mutex::new(None)))
    }

    pub(super) fn set_thread_local_agent_dispatch_hook(hook: Option<AgentDispatchHook>) {
        TEST_AGENT_DISPATCH_GUARD_HELD.with(|held| held.set(true));
        TEST_AGENT_DISPATCH_HOOK.with(|slot| {
            *slot.borrow_mut() = hook;
        });
    }

    pub(super) fn clear_thread_local_agent_dispatch_hook() {
        TEST_AGENT_DISPATCH_HOOK.with(|slot| {
            *slot.borrow_mut() = None;
        });
        TEST_AGENT_DISPATCH_GUARD_HELD.with(|held| held.set(false));
    }

    pub(super) async fn try_dispatch_agent_command(
        command: &Command,
    ) -> Option<anyhow::Result<Option<bool>>> {
        if TEST_AGENT_DISPATCH_GUARD_HELD.with(Cell::get) {
            return TEST_AGENT_DISPATCH_HOOK
                .with(|slot| slot.borrow().as_ref().copied().map(|hook| hook(command)));
        }

        let hook = {
            let guard = agent_dispatch_hook().lock().await;
            *guard
        };

        hook.map(|installed_hook| installed_hook(command))
    }
}

#[cfg(test)]
#[path = "main_cli_tests.rs"]
mod tests;
