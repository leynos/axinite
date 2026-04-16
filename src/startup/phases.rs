//! Coordinated startup phases for the host binary.

use std::sync::Arc;

use ironclaw::{
    app::{AppBuilder, AppBuilderFlags, AppComponents},
    channels::{ChannelManager, web::log_layer::LogBroadcaster},
    cli::Cli,
    config::Config,
    hooks::bootstrap_hooks,
    llm::create_session_manager,
};

#[cfg(any(feature = "postgres", feature = "libsql"))]
use ironclaw::setup::{SetupConfig, SetupWizard};

use crate::startup::channels::{
    ChannelSetup, GatewayContext, GatewaySetup, setup_channels, setup_gateway_channel,
};
use crate::startup::{boot, context::*, run};

/// Acquires the optional PID lock and runs first-run onboarding when requested.
///
/// Returns `None` when no PID lock is configured, `Some(lock)` otherwise.
pub(crate) async fn phase_pid_and_onboard(
    cli: &Cli,
) -> anyhow::Result<Option<ironclaw::bootstrap::PidLock>> {
    let pid_lock = match ironclaw::bootstrap::PidLock::acquire() {
        Ok(lock) => Some(lock),
        Err(ironclaw::bootstrap::PidLockError::AlreadyRunning { pid }) => anyhow::bail!(
            "Another IronClaw instance is already running (PID {}). \
             If this is incorrect, remove the stale PID file: {}",
            pid,
            ironclaw::bootstrap::pid_lock_path().display()
        ),
        Err(e) => {
            eprintln!("Warning: Could not acquire PID lock: {e}");
            eprintln!("Continuing without PID lock protection.");
            None
        }
    };

    run_first_run_onboarding_if_needed(cli).await?;

    Ok(pid_lock)
}

/// Loads configuration from environment and TOML, creates the LLM session
/// manager, and initialises structured tracing.
pub(crate) async fn phase_load_config_and_tracing(
    cli: &Cli,
) -> anyhow::Result<LoadedConfigContext> {
    let toml_path = cli.config.clone();
    let config = load_initial_config(toml_path.as_deref()).await?;

    let session = create_session_manager(config.llm.session.clone()).await;
    let log_broadcaster = Arc::new(LogBroadcaster::new());
    let log_level_handle =
        ironclaw::channels::web::log_layer::init_tracing(Arc::clone(&log_broadcaster));

    tracing::debug!("Starting IronClaw...");
    tracing::debug!("Loaded configuration for agent: {}", config.agent.name);
    tracing::debug!("LLM backend: {}", config.llm.backend);

    Ok(LoadedConfigContext {
        config,
        toml_path,
        session,
        log_broadcaster,
        log_level_handle,
    })
}

/// Constructs all application components and runtime side-effects via
/// [`AppBuilder`].
pub(crate) async fn phase_build_components(
    cli: &Cli,
    loaded: LoadedConfigContext,
) -> anyhow::Result<BuiltComponentsContext> {
    let flags = AppBuilderFlags {
        no_db: cli.no_db,
        workspace_import_dir: std::env::var("WORKSPACE_IMPORT_DIR")
            .ok()
            .map(std::path::PathBuf::from),
    };
    let (components, side_effects) = AppBuilder::new(
        loaded.config,
        flags,
        loaded.toml_path,
        loaded.session,
        Arc::clone(&loaded.log_broadcaster),
    )
    .build_components()
    .await?;

    Ok(BuiltComponentsContext {
        components,
        side_effects,
        log_broadcaster: loaded.log_broadcaster,
        log_level_handle: loaded.log_level_handle,
    })
}

/// Starts the managed tunnel (if configured) and sets up the orchestrator
/// context, including the container job manager, prompt queue, and SSE sender.
pub(crate) async fn phase_tunnel_and_orchestrator(
    built: BuiltComponentsContext,
) -> AgentRunContext {
    let config = built.components.config.clone();
    let (config, active_tunnel) = ironclaw::tunnel::start_managed_tunnel(config).await;

    let OrchestratorContext {
        container_job_manager,
        job_event_tx,
        prompt_queue,
        docker_status,
    } = setup_orchestrator_context(&config, &built.components).await;

    AgentRunContext {
        core: CoreAgentContext {
            config,
            components: built.components,
            side_effects: built.side_effects,
            active_tunnel,
            container_job_manager,
            job_event_tx,
            prompt_queue,
            docker_status,
            log_broadcaster: built.log_broadcaster,
            log_level_handle: built.log_level_handle,
        },
    }
}

async fn bootstrap_and_log_hooks(
    components: &AppComponents,
    config: &Config,
    loaded_wasm_channel_names: &[String],
) {
    let active_tool_names = components.tools.list().await;
    let hook_bootstrap = bootstrap_hooks(
        &components.hooks,
        components.workspace.as_ref(),
        &config.wasm.tools_dir,
        &config.channels.wasm_channels_dir,
        &active_tool_names,
        loaded_wasm_channel_names,
        &components.dev_loaded_tool_names,
    )
    .await;
    tracing::debug!(
        bundled = hook_bootstrap.bundled_hooks,
        plugin = hook_bootstrap.plugin_hooks,
        workspace = hook_bootstrap.workspace_hooks,
        outbound_webhooks = hook_bootstrap.outbound_webhooks,
        errors = hook_bootstrap.errors,
        "Lifecycle hooks initialized"
    );
}

fn create_session_and_register_tools(
    agent_ctx: &AgentRunContext,
    channels: &ChannelManager,
) -> (
    Arc<ironclaw::agent::SessionManager>,
    ironclaw::tools::builtin::SchedulerSlot,
) {
    let session_manager = Arc::new(
        ironclaw::agent::SessionManager::new().with_hooks(agent_ctx.core.components.hooks.clone()),
    );
    let scheduler_slot: ironclaw::tools::builtin::SchedulerSlot =
        Arc::new(tokio::sync::RwLock::new(None));

    agent_ctx
        .core
        .components
        .tools
        .register_job_tools(ironclaw::tools::RegisterJobToolsOptions {
            context_manager: Arc::clone(&agent_ctx.core.components.context_manager),
            scheduler_slot: Some(scheduler_slot.clone()),
            job_manager: agent_ctx.core.container_job_manager.clone(),
            store: agent_ctx.core.components.db.clone(),
            job_event_tx: agent_ctx.core.job_event_tx.clone(),
            inject_tx: Some(channels.inject_sender()),
            prompt_queue: if agent_ctx.core.config.sandbox.enabled {
                Some(Arc::clone(&agent_ctx.core.prompt_queue))
            } else {
                None
            },
            secrets_store: agent_ctx.core.components.secrets_store.clone(),
        });

    (session_manager, scheduler_slot)
}

/// Registers all channels, bootstraps lifecycle hooks, and wires job tools.
///
/// Returns a [`GatewayPhaseContext`] ready for gateway setup.
pub(crate) async fn phase_init_channels_and_hooks(
    cli: &Cli,
    agent_ctx: AgentRunContext,
) -> anyhow::Result<GatewayPhaseContext> {
    let channels = ChannelManager::new();
    let ChannelSetup {
        webhook_server,
        channel_names,
        loaded_wasm_channel_names,
        wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state,
    } = setup_channels(
        cli,
        &agent_ctx.core.config,
        &agent_ctx.core.components,
        &channels,
    )
    .await?;

    bootstrap_and_log_hooks(
        &agent_ctx.core.components,
        &agent_ctx.core.config,
        &loaded_wasm_channel_names,
    )
    .await;
    let (session_manager, scheduler_slot) =
        create_session_and_register_tools(&agent_ctx, &channels);

    Ok(GatewayPhaseContext {
        core: agent_ctx.core,
        channels,
        webhook_server,
        channel_names,
        loaded_wasm_channel_names,
        wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state,
        session_manager,
        scheduler_slot,
        gateway_url: None,
        sse_sender: None,
        routine_engine_slot: None,
    })
}

/// Configures the gateway channel and populates the gateway URL, SSE sender,
/// and routine-engine slot on `ctx`.
pub(crate) async fn phase_setup_gateway(ctx: &mut GatewayPhaseContext) {
    let GatewaySetup {
        gateway_url,
        sse_sender,
        routine_engine_slot,
    } = setup_gateway_channel(
        &ctx.core.config,
        &ctx.core.components,
        &mut GatewayContext {
            container_job_manager: &ctx.core.container_job_manager,
            session_manager: &ctx.session_manager,
            log_broadcaster: &ctx.core.log_broadcaster,
            log_level_handle: &ctx.core.log_level_handle,
            prompt_queue: &ctx.core.prompt_queue,
            scheduler_slot: &ctx.scheduler_slot,
            job_event_tx: &ctx.core.job_event_tx,
            channels: &ctx.channels,
            channel_names: &mut ctx.channel_names,
        },
    )
    .await;

    ctx.gateway_url = gateway_url;
    ctx.sse_sender = sse_sender;
    ctx.routine_engine_slot = routine_engine_slot;
}

/// Renders and prints the startup boot screen using the current gateway context.
pub(crate) fn phase_print_boot_screen(cli: &Cli, ctx: &GatewayPhaseContext) {
    let boot_screen = boot::BootScreenContext {
        llm_model: ctx.core.components.llm.model_name().to_string(),
        cheap_model: ctx
            .core
            .components
            .cheap_llm
            .as_ref()
            .map(|c| c.model_name().to_string()),
        tool_count: ctx.core.components.tools.count(),
        gateway_url: ctx.gateway_url.clone(),
        docker_status: ctx.core.docker_status,
        channel_names: ctx.channel_names.clone(),
        active_tunnel: &ctx.core.active_tunnel,
    };

    boot::print_startup_info(&ctx.core.config, cli, &boot_screen);
}

/// Delegates execution to [`run::run_agent`], consuming the gateway context and
/// running the agent loop until shutdown.
pub(crate) async fn phase_run_agent(ctx: GatewayPhaseContext) -> anyhow::Result<()> {
    run::run_agent(ctx).await
}

#[cfg(any(feature = "postgres", feature = "libsql"))]
async fn run_first_run_onboarding_if_needed(cli: &Cli) -> anyhow::Result<()> {
    if !cli.no_onboard
        && let Some(reason) = ironclaw::setup::check_onboard_needed()
    {
        println!("Onboarding needed: {reason}");
        println!();
        let mut wizard = SetupWizard::with_config(SetupConfig {
            quick: true,
            ..Default::default()
        });
        wizard.run().await?;
    }
    Ok(())
}

#[cfg(not(any(feature = "postgres", feature = "libsql")))]
async fn run_first_run_onboarding_if_needed(cli: &Cli) -> anyhow::Result<()> {
    let _ = cli;
    Ok(())
}

async fn load_initial_config(toml_path: Option<&std::path::Path>) -> anyhow::Result<Config> {
    match Config::from_env_with_toml(toml_path).await {
        Ok(c) => Ok(c),
        Err(ironclaw::error::ConfigError::MissingRequired { key, hint }) => {
            anyhow::bail!(
                "Configuration error: Missing required setting '{}'. {}. \
                 Run 'ironclaw onboard' to configure, or set the required environment variables.",
                key,
                hint
            );
        }
        Err(e) => Err(e.into()),
    }
}

async fn setup_orchestrator_context(
    config: &Config,
    components: &AppComponents,
) -> OrchestratorContext {
    let orch = ironclaw::orchestrator::setup_orchestrator(
        config,
        &components.llm,
        &components.tools,
        components.db.as_ref(),
        components.secrets_store.as_ref(),
    )
    .await;
    OrchestratorContext {
        container_job_manager: orch.container_job_manager,
        job_event_tx: orch.job_event_tx,
        prompt_queue: orch.prompt_queue,
        docker_status: orch.docker_status,
    }
}
