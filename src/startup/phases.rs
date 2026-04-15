//! Coordinated startup phases for the host binary.

use std::sync::Arc;

use ironclaw::{
    app::{AppBuilder, AppBuilderFlags, AppComponents, RuntimeSideEffects},
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
use crate::startup::{boot, run, wasm::WasmChannelRuntimeState};

pub(crate) struct LoadedConfigContext {
    config: Config,
    toml_path: Option<std::path::PathBuf>,
    session: Arc<ironclaw::llm::session::SessionManager>,
    log_broadcaster: Arc<LogBroadcaster>,
    log_level_handle: Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

pub(crate) struct BuiltComponentsContext {
    components: AppComponents,
    side_effects: RuntimeSideEffects,
    log_broadcaster: Arc<LogBroadcaster>,
    log_level_handle: Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

struct OrchestratorContext {
    container_job_manager: Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    job_event_tx: Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, ironclaw::channels::web::types::SseEvent)>,
    >,
    prompt_queue: Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                uuid::Uuid,
                std::collections::VecDeque<ironclaw::orchestrator::api::PendingPrompt>,
            >,
        >,
    >,
    docker_status: ironclaw::sandbox::DockerStatus,
}

pub(crate) struct AgentRunContext {
    pub(crate) config: Config,
    pub(crate) components: AppComponents,
    pub(crate) side_effects: RuntimeSideEffects,
    pub(crate) active_tunnel: Option<Box<dyn ironclaw::tunnel::Tunnel>>,
    pub(crate) container_job_manager: Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    pub(crate) job_event_tx: Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, ironclaw::channels::web::types::SseEvent)>,
    >,
    pub(crate) prompt_queue: Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                uuid::Uuid,
                std::collections::VecDeque<ironclaw::orchestrator::api::PendingPrompt>,
            >,
        >,
    >,
    pub(crate) docker_status: ironclaw::sandbox::DockerStatus,
    pub(crate) log_broadcaster: Arc<LogBroadcaster>,
    pub(crate) log_level_handle: Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

pub(crate) struct GatewayPhaseContext {
    pub(crate) config: Config,
    pub(crate) components: AppComponents,
    pub(crate) side_effects: RuntimeSideEffects,
    pub(crate) active_tunnel: Option<Box<dyn ironclaw::tunnel::Tunnel>>,
    pub(crate) container_job_manager: Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    pub(crate) prompt_queue: Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                uuid::Uuid,
                std::collections::VecDeque<ironclaw::orchestrator::api::PendingPrompt>,
            >,
        >,
    >,
    pub(crate) docker_status: ironclaw::sandbox::DockerStatus,
    pub(crate) log_broadcaster: Arc<LogBroadcaster>,
    pub(crate) log_level_handle: Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
    pub(crate) job_event_tx: Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, ironclaw::channels::web::types::SseEvent)>,
    >,
    pub(crate) channels: ChannelManager,
    pub(crate) webhook_server: Option<Arc<tokio::sync::Mutex<ironclaw::channels::WebhookServer>>>,
    pub(crate) channel_names: Vec<String>,
    pub(crate) loaded_wasm_channel_names: Vec<String>,
    pub(crate) wasm_channel_runtime_state: Option<WasmChannelRuntimeState>,
    #[cfg(unix)]
    pub(crate) http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>>,
    pub(crate) session_manager: Arc<ironclaw::agent::SessionManager>,
    pub(crate) scheduler_slot: ironclaw::tools::builtin::SchedulerSlot,
    pub(crate) gateway_url: Option<String>,
    pub(crate) sse_sender:
        Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    pub(crate) routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot>,
}

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
    }
}

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
    } = setup_channels(cli, &agent_ctx.config, &agent_ctx.components, &channels).await?;

    let active_tool_names = agent_ctx.components.tools.list().await;
    let hook_bootstrap = bootstrap_hooks(
        &agent_ctx.components.hooks,
        agent_ctx.components.workspace.as_ref(),
        &agent_ctx.config.wasm.tools_dir,
        &agent_ctx.config.channels.wasm_channels_dir,
        &active_tool_names,
        &loaded_wasm_channel_names,
        &agent_ctx.components.dev_loaded_tool_names,
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

    let session_manager = Arc::new(
        ironclaw::agent::SessionManager::new().with_hooks(agent_ctx.components.hooks.clone()),
    );
    let scheduler_slot: ironclaw::tools::builtin::SchedulerSlot =
        Arc::new(tokio::sync::RwLock::new(None));

    agent_ctx
        .components
        .tools
        .register_job_tools(ironclaw::tools::RegisterJobToolsOptions {
            context_manager: Arc::clone(&agent_ctx.components.context_manager),
            scheduler_slot: Some(scheduler_slot.clone()),
            job_manager: agent_ctx.container_job_manager.clone(),
            store: agent_ctx.components.db.clone(),
            job_event_tx: agent_ctx.job_event_tx.clone(),
            inject_tx: Some(channels.inject_sender()),
            prompt_queue: if agent_ctx.config.sandbox.enabled {
                Some(Arc::clone(&agent_ctx.prompt_queue))
            } else {
                None
            },
            secrets_store: agent_ctx.components.secrets_store.clone(),
        });

    Ok(GatewayPhaseContext {
        config: agent_ctx.config,
        components: agent_ctx.components,
        side_effects: agent_ctx.side_effects,
        active_tunnel: agent_ctx.active_tunnel,
        container_job_manager: agent_ctx.container_job_manager,
        prompt_queue: agent_ctx.prompt_queue,
        docker_status: agent_ctx.docker_status,
        log_broadcaster: agent_ctx.log_broadcaster,
        log_level_handle: agent_ctx.log_level_handle,
        job_event_tx: agent_ctx.job_event_tx,
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

pub(crate) async fn phase_setup_gateway(ctx: &mut GatewayPhaseContext) {
    let GatewaySetup {
        gateway_url,
        sse_sender,
        routine_engine_slot,
    } = setup_gateway_channel(
        &ctx.config,
        &ctx.components,
        &mut GatewayContext {
            container_job_manager: &ctx.container_job_manager,
            session_manager: &ctx.session_manager,
            log_broadcaster: &ctx.log_broadcaster,
            log_level_handle: &ctx.log_level_handle,
            prompt_queue: &ctx.prompt_queue,
            scheduler_slot: &ctx.scheduler_slot,
            job_event_tx: &ctx.job_event_tx,
            channels: &ctx.channels,
            channel_names: &mut ctx.channel_names,
        },
    )
    .await;

    ctx.gateway_url = gateway_url;
    ctx.sse_sender = sse_sender;
    ctx.routine_engine_slot = routine_engine_slot;
}

pub(crate) fn phase_print_boot_screen(cli: &Cli, ctx: &GatewayPhaseContext) {
    let boot_screen = boot::BootScreenContext {
        llm_model: ctx.components.llm.model_name().to_string(),
        cheap_model: ctx
            .components
            .cheap_llm
            .as_ref()
            .map(|c| c.model_name().to_string()),
        tool_count: ctx.components.tools.count(),
        gateway_url: ctx.gateway_url.clone(),
        docker_status: ctx.docker_status,
        channel_names: ctx.channel_names.clone(),
        active_tunnel: &ctx.active_tunnel,
    };

    boot::print_startup_info(&ctx.config, cli, &boot_screen);
}

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
