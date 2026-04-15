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

pub(crate) async fn dispatch_subcommand(cli: &Cli) -> anyhow::Result<bool> {
    if let Some(dispatched) = dispatch_cli_tool_commands(cli).await? {
        return Ok(dispatched);
    }

    dispatch_agent_commands(cli)
        .await
        .map(|handled| handled.unwrap_or(false))
}

pub(crate) async fn dispatch_cli_tool_commands(cli: &Cli) -> anyhow::Result<Option<bool>> {
    match &cli.command {
        Some(Command::Tool(c)) => run_traced_async(|| async { run_tool_command(c.clone()).await })
            .await
            .map(Some),
        Some(Command::Config(c)) => {
            run_traced_async(|| async { ironclaw::cli::run_config_command(c.clone()).await })
                .await
                .map(Some)
        }
        Some(Command::Registry(c)) => {
            run_traced_async(|| async { ironclaw::cli::run_registry_command(c.clone()).await })
                .await
                .map(Some)
        }
        Some(Command::Mcp(c)) => run_traced_async(|| async { run_mcp_command(*c.clone()).await })
            .await
            .map(Some),
        Some(Command::Memory(c)) => {
            run_traced_async(|| async { ironclaw::cli::run_memory_command(c).await })
                .await
                .map(Some)
        }
        Some(Command::Pairing(c)) => {
            run_traced_sync(|| run_pairing_command(c.clone()).map_err(|e| anyhow::anyhow!("{e}")))
                .map(Some)
        }
        Some(Command::Service(c)) => run_traced_sync(|| run_service_command(c)).map(Some),
        Some(Command::Doctor) => {
            run_traced_async(|| async { ironclaw::cli::run_doctor_command().await })
                .await
                .map(Some)
        }
        Some(Command::Status) => run_traced_async(|| async { run_status_command().await })
            .await
            .map(Some),
        Some(Command::Completion(c)) => run_traced_sync(|| c.run()).map(Some),
        #[cfg(feature = "import")]
        Some(Command::Import(c)) => run_traced_async(|| async { run_import_subcommand(c).await })
            .await
            .map(Some),
        Some(Command::Worker { .. })
        | Some(Command::ClaudeBridge { .. })
        | Some(Command::Onboard { .. })
        | None
        | Some(Command::Run) => Ok(None),
    }
}

pub(crate) async fn dispatch_agent_commands(cli: &Cli) -> anyhow::Result<Option<bool>> {
    match &cli.command {
        Some(Command::Worker {
            job_id,
            orchestrator_url,
            max_iterations,
        }) => {
            dispatch_worker_subcommand(*job_id, orchestrator_url, *max_iterations).await?;
            Ok(Some(true))
        }
        Some(Command::ClaudeBridge {
            job_id,
            orchestrator_url,
            max_turns,
            model,
        }) => {
            dispatch_claude_bridge_subcommand(*job_id, orchestrator_url, *max_turns, model).await?;
            Ok(Some(true))
        }
        Some(Command::Onboard {
            skip_auth,
            channels_only,
            provider_only,
            quick,
        }) => {
            run_onboard_subcommand(*skip_auth, *channels_only, *provider_only, *quick).await?;
            Ok(Some(true))
        }
        _ => Ok(None),
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
