//! IronClaw - main entry point.

use clap::Parser;

use ironclaw::cli::Cli;

#[path = "main_cli.rs"]
mod cli;
mod startup;

/// Synchronous entry point. Loads `.env` files before the Tokio runtime
/// starts so that `std::env::set_var` is safe (no worker threads yet).
fn main() -> anyhow::Result<()> {
    let _ = dotenvy::dotenv();
    ironclaw::bootstrap::load_ironclaw_env();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?
        .block_on(async_main())
}

async fn async_main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli::dispatch_subcommand(&cli).await? {
        return Ok(());
    }

    let _pid_lock = startup::phases::phase_pid_and_onboard(&cli).await?;
    let loaded = startup::phases::phase_load_config_and_tracing(&cli).await?;
    let built = startup::phases::phase_build_components(&cli, loaded).await?;
    let agent_ctx = startup::phases::phase_tunnel_and_orchestrator(built).await;
    let mut gateway_ctx = startup::phases::phase_init_channels_and_hooks(&cli, agent_ctx).await?;
    startup::phases::phase_setup_gateway(&mut gateway_ctx).await;
    startup::phases::phase_print_boot_screen(&cli, &gateway_ctx);
    startup::phases::phase_run_agent(gateway_ctx).await
}
