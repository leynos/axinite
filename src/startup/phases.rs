//! Coordinated startup phases for the host binary.

use std::future::Future;
use std::sync::Arc;

use axinite::{
    app::{AppBuilder, AppBuilderFlags, AppBuilderParams, AppComponents},
    channels::ChannelManager,
    cli::Cli,
    config::{Config, EnvContext},
    hooks::bootstrap_hooks,
};

#[cfg(any(feature = "postgres", feature = "libsql"))]
use axinite::setup::{SetupConfig, SetupWizard};

use crate::startup::channels::{
    ChannelSetup, ChannelSetupRequest, GatewayContext, GatewaySetup, setup_channels,
    setup_gateway_channel,
};
use crate::startup::{boot, context::*, orchestrator::setup_orchestrator_context, run};

pub(crate) use crate::startup::config_load::phase_load_config_and_tracing_from;

/// Runs first-run onboarding for a startup environment snapshot.
///
/// Production uses [`InteractiveOnboarding`], which drives the setup wizard.
/// Tests substitute a scripted implementation so the phase can be driven
/// without a terminal.
pub(crate) trait OnboardingRunner {
    /// Run onboarding for `env_context` and return the snapshot that startup
    /// must use afterwards.
    fn run(
        &self,
        env_context: &EnvContext,
    ) -> impl Future<Output = anyhow::Result<EnvContext>> + Send;
}

/// Runs the interactive setup wizard when the snapshot shows onboarding is
/// still required.
pub(crate) struct InteractiveOnboarding;

#[cfg(any(feature = "postgres", feature = "libsql"))]
impl OnboardingRunner for InteractiveOnboarding {
    async fn run(&self, env_context: &EnvContext) -> anyhow::Result<EnvContext> {
        let Some(reason) = axinite::setup::check_onboard_needed(env_context) else {
            return Ok(env_context.clone());
        };

        println!("Onboarding needed: {reason}");
        println!();
        let mut wizard = SetupWizard::with_config_and_env_context(
            SetupConfig {
                quick: true,
                ..Default::default()
            },
            env_context.clone(),
        );
        wizard.run().await?;
        onboarding_snapshot(&wizard)
    }
}

#[cfg(not(any(feature = "postgres", feature = "libsql")))]
impl OnboardingRunner for InteractiveOnboarding {
    async fn run(&self, env_context: &EnvContext) -> anyhow::Result<EnvContext> {
        Ok(env_context.clone())
    }
}

/// Acquires the optional PID lock and runs first-run onboarding when requested.
///
/// The PID lock lives in the snapshot's base directory, and the returned
/// environment snapshot includes bootstrap values persisted by successful
/// onboarding.
pub(crate) async fn phase_pid_and_onboard<O: OnboardingRunner>(
    cli: &Cli,
    env_context: &EnvContext,
    onboarding: &O,
) -> anyhow::Result<(Option<axinite::bootstrap::PidLock>, EnvContext)> {
    let base_dir = env_context.axinite_base_dir();
    let pid_lock = match axinite::bootstrap::PidLock::acquire_in(&base_dir) {
        Ok(lock) => Some(lock),
        Err(axinite::bootstrap::PidLockError::AlreadyRunning { pid }) => anyhow::bail!(
            "Another Axinite instance is already running (PID {}). \
             If this is incorrect, remove the stale PID file: {}",
            pid,
            base_dir.join("axinite.pid").display()
        ),
        Err(e) => {
            eprintln!("Warning: Could not acquire PID lock: {e}");
            eprintln!("Continuing without PID lock protection.");
            None
        }
    };

    let env_context = if cli.no_onboard {
        env_context.clone()
    } else {
        onboarding.run(env_context).await?
    };

    Ok((pid_lock, env_context))
}

/// Runs onboarding and then loads configuration from the snapshot it returns.
///
/// The binary entry point uses this so the hand-off between the two phases is
/// exercised by tests: configuration loading must consume the post-onboarding
/// snapshot, never the one captured before onboarding.
pub(crate) async fn phase_onboard_and_load_config<O: OnboardingRunner>(
    cli: &Cli,
    env_context: &EnvContext,
    onboarding: &O,
) -> anyhow::Result<(Option<axinite::bootstrap::PidLock>, LoadedConfigContext)> {
    let (pid_lock, onboarding_context) =
        phase_pid_and_onboard(cli, env_context, onboarding).await?;
    let loaded = phase_load_config_and_tracing_from(cli, &onboarding_context).await?;
    Ok((pid_lock, loaded))
}

/// Constructs all application components and runtime side-effects via
/// [`AppBuilder`].
pub(crate) async fn phase_build_components(
    cli: &Cli,
    loaded: LoadedConfigContext,
) -> anyhow::Result<BuiltComponentsContext> {
    let LoadedConfigContext {
        config,
        env_context,
        toml_path,
        session,
        log_broadcaster,
        log_level_handle,
        workspace_import_dir,
    } = loaded;
    let flags = AppBuilderFlags {
        no_db: cli.no_db,
        workspace_import_dir,
    };
    let (components, side_effects, env_context) = AppBuilder::new_with_context(
        AppBuilderParams {
            config,
            flags,
            toml_path,
            session,
            log_broadcaster: Arc::clone(&log_broadcaster),
        },
        env_context,
    )
    .build_components_with_context()
    .await?;

    Ok(BuiltComponentsContext {
        components,
        env_context,
        side_effects,
        log_broadcaster,
        log_level_handle,
    })
}

/// Install the compatibility overlay for legacy environment readers at the
/// production composition boundary after component construction.
pub(crate) fn install_legacy_env_overlay(built: &BuiltComponentsContext) {
    axinite::config::merge_context_secrets_into_legacy_overlay(&built.env_context);
}

/// Starts the managed tunnel (if configured) and sets up the orchestrator
/// context, including the container job manager, prompt queue, and SSE sender.
pub(crate) async fn phase_tunnel_and_orchestrator(
    built: BuiltComponentsContext,
) -> AgentRunContext {
    let config = built.components.config.clone();
    let (config, active_tunnel) = axinite::tunnel::start_managed_tunnel(config).await;
    let mut components = built.components;
    components.config = config.clone();

    let OrchestratorContext {
        container_job_manager,
        job_event_tx,
        prompt_queue,
        docker_status,
    } = setup_orchestrator_context(&config, &components, &built.env_context).await;

    AgentRunContext {
        core: CoreAgentContext {
            config,
            env_context: built.env_context,
            components,
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

/// Bootstraps lifecycle hooks and logs summary counts.
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

/// Creates the session manager and registers job tools, returning both for
/// downstream use.
fn create_session_and_register_tools(
    agent_ctx: &AgentRunContext,
    channels: &ChannelManager,
) -> (
    Arc<axinite::agent::SessionManager>,
    axinite::tools::builtin::SchedulerSlot,
) {
    let session_manager = Arc::new(
        axinite::agent::SessionManager::new().with_hooks(agent_ctx.core.components.hooks.clone()),
    );
    let scheduler_slot: axinite::tools::builtin::SchedulerSlot =
        Arc::new(tokio::sync::RwLock::new(None));

    agent_ctx
        .core
        .components
        .tools
        .register_job_tools(axinite::tools::RegisterJobToolsOptions {
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
    let channels = Arc::new(ChannelManager::new());
    let ChannelSetup {
        webhook_server,
        channel_names,
        loaded_wasm_channel_names,
        wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state,
    } = setup_channels(ChannelSetupRequest {
        cli,
        config: &agent_ctx.core.config,
        components: &agent_ctx.core.components,
        env_context: &agent_ctx.core.env_context,
        channels: channels.as_ref(),
    })
    .await?;

    bootstrap_and_log_hooks(
        &agent_ctx.core.components,
        &agent_ctx.core.config,
        &loaded_wasm_channel_names,
    )
    .await;
    let (session_manager, scheduler_slot) =
        create_session_and_register_tools(&agent_ctx, channels.as_ref());

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
            env_context: &ctx.core.env_context,
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

/// Merge bootstrap values persisted by onboarding into the wizard's snapshot.
///
/// The wizard's own snapshot carries credentials entered during onboarding
/// (for example an API key stored while the database is unavailable), so
/// `.env` is merged on top of that rather than on the snapshot captured before
/// onboarding. Startup resolves configuration from the returned snapshot.
#[cfg(any(feature = "postgres", feature = "libsql"))]
fn onboarding_snapshot(wizard: &SetupWizard) -> anyhow::Result<EnvContext> {
    Ok(axinite::bootstrap::merge_axinite_env_into_context(
        wizard.env_context(),
    )?)
}

#[cfg(test)]
mod tests;
