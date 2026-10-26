//! Axinite - main entry point.

#![forbid(unsafe_code)]

use clap::Parser;

use axinite::cli::Cli;

#[path = "main_cli.rs"]
mod cli;

mod startup;

/// Synchronous entry point. Parses `.env` files before the Tokio runtime starts
/// so they are available in the first `EnvContext` snapshot.
fn main() -> anyhow::Result<()> {
    axinite::config::EnvContext::refresh_dotenv_layers();
    let env_context = axinite::bootstrap::capture_config_env_context();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main(env_context))
}

/// Dispatch a CLI subcommand if one was given, otherwise run the agent startup
/// phases through to the agent loop.
async fn async_main(env_context: axinite::config::EnvContext) -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli::dispatch_subcommand(&cli).await? {
        return Ok(());
    }

    let (_pid_lock, env_context) =
        startup::phases::phase_pid_and_onboard(&cli, &env_context).await?;
    let loaded = startup::phases::phase_load_config_and_tracing_from(&cli, &env_context).await?;
    let built = startup::phases::phase_build_components(&cli, loaded).await?;
    startup::phases::install_legacy_env_overlay(&built);
    let agent_ctx = startup::phases::phase_tunnel_and_orchestrator(built).await;
    let mut gateway_ctx = startup::phases::phase_init_channels_and_hooks(&cli, agent_ctx).await?;
    startup::phases::phase_setup_gateway(&mut gateway_ctx).await;
    startup::phases::phase_print_boot_screen(&cli, &gateway_ctx);
    startup::phases::phase_run_agent(gateway_ctx).await
}
