//! IronClaw - Main entry point.

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;

use ironclaw::{
    agent::{Agent, AgentDeps},
    app::{AppBuilder, AppBuilderFlags},
    channels::{
        ChannelManager, GatewayChannel, HttpChannel, ReplChannel, SignalChannel, WebhookServer,
        WebhookServerConfig,
        wasm::{WasmChannelRouter, WasmChannelRuntime},
        web::log_layer::LogBroadcaster,
    },
    cli::{
        Cli, Command, run_mcp_command, run_pairing_command, run_service_command,
        run_status_command, run_tool_command,
    },
    config::Config,
    hooks::bootstrap_hooks,
    llm::create_session_manager,
    orchestrator::{ReaperConfig, SandboxReaper},
    pairing::PairingStore,
    tracing_fmt::{init_cli_tracing, init_worker_tracing},
};

#[cfg(any(feature = "postgres", feature = "libsql"))]
use ironclaw::setup::{SetupConfig, SetupWizard};

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

async fn dispatch_subcommand(cli: &Cli) -> anyhow::Result<bool> {
    if let Some(dispatched) = dispatch_cli_subcommand(cli).await? {
        return Ok(dispatched);
    }

    if let Some(Command::Worker {
        job_id,
        orchestrator_url,
        max_iterations,
    }) = &cli.command
    {
        dispatch_worker_subcommand(*job_id, orchestrator_url, *max_iterations).await?;
        Ok(true)
    } else if let Some(Command::ClaudeBridge {
        job_id,
        orchestrator_url,
        max_turns,
        model,
    }) = &cli.command
    {
        dispatch_claude_bridge_subcommand(*job_id, orchestrator_url, *max_turns, model).await?;
        Ok(true)
    } else if let Some(Command::Onboard {
        skip_auth,
        channels_only,
        provider_only,
        quick,
    }) = &cli.command
    {
        run_onboard_subcommand(*skip_auth, *channels_only, *provider_only, *quick).await?;
        Ok(true)
    } else {
        Ok(false)
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

async fn dispatch_cli_subcommand(cli: &Cli) -> anyhow::Result<Option<bool>> {
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
            run_traced_sync(|| run_pairing_command(c.clone()).map_err(|e| anyhow::anyhow!("{}", e)))
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

fn acquire_pid_lock() -> anyhow::Result<Option<ironclaw::bootstrap::PidLock>> {
    match ironclaw::bootstrap::PidLock::acquire() {
        Ok(lock) => Ok(Some(lock)),
        Err(ironclaw::bootstrap::PidLockError::AlreadyRunning { pid }) => anyhow::bail!(
            "Another IronClaw instance is already running (PID {}). \
             If this is incorrect, remove the stale PID file: {}",
            pid,
            ironclaw::bootstrap::pid_lock_path().display()
        ),
        Err(e) => {
            eprintln!("Warning: Could not acquire PID lock: {}", e);
            eprintln!("Continuing without PID lock protection.");
            Ok(None)
        }
    }
}

struct ChannelInfrastructure {
    webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    channel_names: Vec<String>,
    loaded_wasm_channel_names: Vec<String>,
    wasm_channel_runtime_state: Option<(
        Arc<WasmChannelRuntime>,
        Arc<PairingStore>,
        Arc<WasmChannelRouter>,
    )>,
    #[cfg(unix)]
    http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>>,
}

struct HttpChannelResult {
    webhook_server_addr: Option<std::net::SocketAddr>,
    #[cfg(unix)]
    http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>>,
}

/// Registration sinks shared across channel-setup helpers.
struct ChannelRegistrar<'a> {
    channels: &'a ChannelManager,
    channel_names: &'a mut Vec<String>,
    webhook_routes: &'a mut Vec<axum::Router>,
}
async fn setup_repl_channel(cli: &Cli, config: &Config, reg: &mut ChannelRegistrar<'_>) {
    let repl_channel = if let Some(ref msg) = cli.message {
        Some(ReplChannel::with_message(msg.clone()))
    } else if config.channels.cli.enabled {
        let repl = ReplChannel::new();
        repl.suppress_banner();
        Some(repl)
    } else {
        None
    };
    let Some(repl) = repl_channel else { return };
    reg.channels.add(Box::new(repl)).await;
    if cli.message.is_some() {
        tracing::debug!("Single message mode");
    } else {
        reg.channel_names.push("repl".to_string());
        tracing::debug!("REPL mode enabled");
    }
}

async fn setup_wasm_channels_infra(
    config: &Config,
    components: &ironclaw::app::AppComponents,
    reg: &mut ChannelRegistrar<'_>,
) -> (
    Vec<String>,
    Option<(
        Arc<WasmChannelRuntime>,
        Arc<PairingStore>,
        Arc<WasmChannelRouter>,
    )>,
) {
    if !config.channels.wasm_channels_enabled || !config.channels.wasm_channels_dir.exists() {
        return (vec![], None);
    }
    let Some(result) = ironclaw::channels::wasm::setup_wasm_channels(
        config,
        &components.secrets_store,
        components.extension_manager.as_ref(),
        components.db.as_ref(),
    )
    .await
    else {
        return (vec![], None);
    };
    let loaded_wasm_channel_names = result.channel_names;
    let wasm_channel_runtime_state = Some((
        result.wasm_channel_runtime,
        result.pairing_store,
        result.wasm_channel_router,
    ));
    for (name, channel) in result.channels {
        reg.channel_names.push(name.clone());
        reg.channels.add(channel).await;
    }
    if let Some(routes) = result.webhook_routes {
        reg.webhook_routes.push(routes);
    }
    (loaded_wasm_channel_names, wasm_channel_runtime_state)
}

async fn setup_signal_channel(
    cli: &Cli,
    config: &Config,
    reg: &mut ChannelRegistrar<'_>,
) -> anyhow::Result<()> {
    if cli.cli_only {
        return Ok(());
    }
    let Some(ref signal_config) = config.channels.signal else {
        return Ok(());
    };
    let signal_channel = SignalChannel::new(signal_config.clone())?;
    reg.channel_names.push("signal".to_string());
    reg.channels.add(Box::new(signal_channel)).await;
    let safe_url = SignalChannel::redact_url(&signal_config.http_url);
    tracing::debug!(url = %safe_url, "Signal channel enabled");
    if signal_config.allow_from.is_empty() {
        tracing::warn!("Signal channel has empty allow_from list - ALL messages will be DENIED.");
    }
    Ok(())
}

async fn setup_http_channel(
    cli: &Cli,
    config: &Config,
    reg: &mut ChannelRegistrar<'_>,
) -> anyhow::Result<HttpChannelResult> {
    let none_result = HttpChannelResult {
        webhook_server_addr: None,
        #[cfg(unix)]
        http_channel_state: None,
    };
    if cli.cli_only {
        return Ok(none_result);
    }
    let Some(ref http_config) = config.channels.http else {
        return Ok(none_result);
    };
    let http_channel = HttpChannel::new(http_config.clone());
    #[cfg(unix)]
    let http_channel_state = Some(http_channel.shared_state());
    reg.webhook_routes.push(http_channel.routes());
    let (host, port) = http_channel.addr();
    let addr_str = format!("{}:{}", host, port);
    let webhook_server_addr = Some(
        addr_str
            .parse()
            .map_err(|e| anyhow::anyhow!("Invalid HTTP host:port '{}': {}", addr_str, e))?,
    );
    reg.channel_names.push("http".to_string());
    reg.channels.add(Box::new(http_channel)).await;
    tracing::debug!(
        "HTTP channel enabled on {}:{}",
        http_config.host,
        http_config.port
    );
    Ok(HttpChannelResult {
        webhook_server_addr,
        #[cfg(unix)]
        http_channel_state,
    })
}

async fn build_webhook_server(
    addr: Option<std::net::SocketAddr>,
    webhook_routes: Vec<axum::Router>,
) -> anyhow::Result<Option<Arc<tokio::sync::Mutex<WebhookServer>>>> {
    if webhook_routes.is_empty() {
        return Ok(None);
    }
    let addr = addr.unwrap_or_else(|| std::net::SocketAddr::from(([0, 0, 0, 0], 8080)));
    if addr.ip().is_unspecified() {
        tracing::warn!(
            "Webhook server is binding to {} — it will be reachable from all network \
             interfaces. Set HTTP_HOST=127.0.0.1 to restrict to localhost.",
            addr.ip()
        );
    }
    let mut server = WebhookServer::new(WebhookServerConfig { addr });
    for routes in webhook_routes {
        server.add_routes(routes);
    }
    server.start().await?;
    Ok(Some(Arc::new(tokio::sync::Mutex::new(server))))
}

async fn setup_channel_infrastructure(
    cli: &Cli,
    config: &Config,
    components: &ironclaw::app::AppComponents,
    channels: &ChannelManager,
) -> anyhow::Result<ChannelInfrastructure> {
    let mut channel_names: Vec<String> = Vec::new();
    let mut webhook_routes: Vec<axum::Router> = Vec::new();
    let mut reg = ChannelRegistrar {
        channels,
        channel_names: &mut channel_names,
        webhook_routes: &mut webhook_routes,
    };

    setup_repl_channel(cli, config, &mut reg).await;

    let (loaded_wasm_channel_names, wasm_channel_runtime_state) =
        setup_wasm_channels_infra(config, components, &mut reg).await;

    setup_signal_channel(cli, config, &mut reg).await?;

    let http = setup_http_channel(cli, config, &mut reg).await?;

    // `reg` is dropped here, releasing the mutable borrows, so `webhook_routes`
    // is usable again in the next call.
    #[expect(
        clippy::drop_non_drop,
        reason = "Explicit drop to release mutable borrows"
    )]
    drop(reg);

    let webhook_server = build_webhook_server(http.webhook_server_addr, webhook_routes).await?;

    Ok(ChannelInfrastructure {
        webhook_server,
        channel_names,
        loaded_wasm_channel_names,
        wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state: http.http_channel_state,
    })
}

async fn wire_extension_manager(
    extension_manager: &Option<Arc<ironclaw::extensions::ExtensionManager>>,
    wasm_channel_runtime_state: &mut Option<(
        Arc<WasmChannelRuntime>,
        Arc<PairingStore>,
        Arc<WasmChannelRouter>,
    )>,
    loaded_wasm_channel_names: &mut [String],
    channels: &Arc<ChannelManager>,
    sse_sender: &Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    wasm_channel_owner_ids: &std::collections::HashMap<String, i64>,
) {
    // Wire up channel runtime for hot-activation of WASM channels.
    if let Some(ext_mgr) = extension_manager
        && let Some((rt, ps, router)) = wasm_channel_runtime_state.take()
    {
        let active_at_startup: std::collections::HashSet<String> =
            loaded_wasm_channel_names.iter().cloned().collect();
        ext_mgr
            .set_active_channels(loaded_wasm_channel_names.to_owned())
            .await;
        ext_mgr
            .set_channel_runtime(
                Arc::clone(channels),
                rt,
                ps,
                router,
                wasm_channel_owner_ids.clone(),
            )
            .await;
        tracing::debug!("Channel runtime wired into extension manager for hot-activation");

        // Auto-activate WASM channels that were active in a previous session.
        // Relay channels are handled separately below via restore_relay_channels().
        let persisted = ext_mgr.load_persisted_active_channels().await;
        for name in &persisted {
            if active_at_startup.contains(name) || ext_mgr.is_relay_channel(name).await {
                continue;
            }
            match ext_mgr.activate(name).await {
                Ok(result) => {
                    tracing::debug!(
                        channel = %name,
                        message = %result.message,
                        "Auto-activated persisted WASM channel"
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        channel = %name,
                        error = %e,
                        "Failed to auto-activate persisted WASM channel"
                    );
                }
            }
        }
    }

    // Ensure the relay channel manager is always set (even without WASM runtime),
    // then restore any persisted relay channels.
    if let Some(ext_mgr) = extension_manager {
        ext_mgr
            .set_relay_channel_manager(Arc::clone(channels))
            .await;
        ext_mgr.restore_relay_channels().await;
    }

    // Wire SSE sender into extension manager for broadcasting status events.
    if let Some(ext_mgr) = extension_manager
        && let Some(sender) = sse_sender
    {
        ext_mgr.set_sse_sender(sender.clone()).await;
    }
}

async fn run_shutdown_sequence(
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
    mcp_process_manager: &ironclaw::tools::mcp::McpProcessManager,
    recording_handle: &Option<Arc<ironclaw::llm::recording::RecordingLlm>>,
    webhook_server: &Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    active_tunnel: &Option<Box<dyn ironclaw::tunnel::Tunnel>>,
) {
    // Signal background tasks (SIGHUP handler, etc.) to gracefully shut down
    let _ = shutdown_tx.send(());

    // Shut down all stdio MCP server child processes.
    mcp_process_manager.shutdown_all().await;

    // Flush LLM trace recording if enabled
    if let Some(recorder) = recording_handle
        && let Err(e) = recorder.flush().await
    {
        tracing::warn!("Failed to write LLM trace: {}", e);
    }

    if let Some(ws_arc) = webhook_server {
        ws_arc.lock().await.shutdown().await;
    }

    if let Some(tunnel) = active_tunnel {
        tracing::debug!("Stopping {} tunnel...", tunnel.name());
        if let Err(e) = tunnel.stop().await {
            tracing::warn!("Failed to stop tunnel cleanly: {}", e);
        }
    }

    tracing::debug!("Agent shutdown complete");
}

#[cfg(any(feature = "postgres", feature = "libsql"))]
async fn run_first_run_onboarding_if_needed(cli: &Cli) -> anyhow::Result<()> {
    if !cli.no_onboard
        && let Some(reason) = ironclaw::setup::check_onboard_needed()
    {
        println!("Onboarding needed: {}", reason);
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

async fn setup_orchestrator_context(
    config: &Config,
    components: &ironclaw::app::AppComponents,
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

async fn async_main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handle non-agent commands first (they don't need full setup)
    let handled = dispatch_subcommand(&cli).await?;
    if handled {
        return Ok(());
    }

    // ── PID lock (prevent multiple instances) ────────────────────────
    let _pid_lock = acquire_pid_lock()?;

    // ── Agent startup ──────────────────────────────────────────────────

    // Enhanced first-run detection
    run_first_run_onboarding_if_needed(&cli).await?;

    // Load initial config from env + disk + optional TOML (before DB is available).
    // Credentials may be missing at this point — that's fine. LlmConfig::resolve()
    // defers gracefully, and AppBuilder::build_all() re-resolves after loading
    // secrets from the encrypted DB.
    let toml_path = cli.config.as_deref();
    let config = load_initial_config(toml_path).await?;

    // Initialize session manager before channel setup
    let session = create_session_manager(config.llm.session.clone()).await;

    // Create log broadcaster before tracing init so the WebLogLayer can capture all events.
    let log_broadcaster = Arc::new(LogBroadcaster::new());

    // Initialize tracing with a reloadable EnvFilter so the gateway can switch
    // log levels at runtime without restarting.
    let log_level_handle =
        ironclaw::channels::web::log_layer::init_tracing(Arc::clone(&log_broadcaster));

    tracing::debug!("Starting IronClaw...");
    tracing::debug!("Loaded configuration for agent: {}", config.agent.name);
    tracing::debug!("LLM backend: {}", config.llm.backend);

    // ── Phase 1-5: Build all core components via AppBuilder ────────────

    let flags = AppBuilderFlags {
        no_db: cli.no_db,
        workspace_import_dir: std::env::var("WORKSPACE_IMPORT_DIR")
            .ok()
            .map(std::path::PathBuf::from),
    };
    let (components, side_effects) = AppBuilder::new(
        config,
        flags,
        toml_path.map(std::path::PathBuf::from),
        session.clone(),
        Arc::clone(&log_broadcaster),
    )
    .build_components()
    .await?;

    let config = components.config.clone();

    // ── Tunnel setup ───────────────────────────────────────────────────

    let (config, active_tunnel) = ironclaw::tunnel::start_managed_tunnel(config).await;

    // ── Orchestrator / container job manager ────────────────────────────

    let OrchestratorContext {
        container_job_manager,
        job_event_tx,
        prompt_queue,
        docker_status,
    } = setup_orchestrator_context(&config, &components).await;

    // ── Channel setup ──────────────────────────────────────────────────

    let channels = ChannelManager::new();
    let ChannelInfrastructure {
        webhook_server,
        mut channel_names,
        mut loaded_wasm_channel_names,
        mut wasm_channel_runtime_state,
        #[cfg(unix)]
        http_channel_state,
    } = setup_channel_infrastructure(&cli, &config, &components, &channels).await?;

    // Register lifecycle hooks.
    let active_tool_names = components.tools.list().await;

    let hook_bootstrap = bootstrap_hooks(
        &components.hooks,
        components.workspace.as_ref(),
        &config.wasm.tools_dir,
        &config.channels.wasm_channels_dir,
        &active_tool_names,
        &loaded_wasm_channel_names,
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

    // Create session manager (shared between agent and web gateway)
    let session_manager =
        Arc::new(ironclaw::agent::SessionManager::new().with_hooks(components.hooks.clone()));

    // Lazy scheduler slot — filled after Agent::new creates the Scheduler.
    // Allows CreateJobTool to dispatch local jobs via the Scheduler even though
    // the Scheduler is created after tools are registered (chicken-and-egg).
    let scheduler_slot: ironclaw::tools::builtin::SchedulerSlot =
        Arc::new(tokio::sync::RwLock::new(None));

    // Register job tools (sandbox deps auto-injected when container_job_manager is available)
    components
        .tools
        .register_job_tools(ironclaw::tools::RegisterJobToolsOptions {
            context_manager: Arc::clone(&components.context_manager),
            scheduler_slot: Some(scheduler_slot.clone()),
            job_manager: container_job_manager.clone(),
            store: components.db.clone(),
            job_event_tx: job_event_tx.clone(),
            inject_tx: Some(channels.inject_sender()),
            prompt_queue: if config.sandbox.enabled {
                Some(Arc::clone(&prompt_queue))
            } else {
                None
            },
            secrets_store: components.secrets_store.clone(),
        });

    // ── Gateway channel ────────────────────────────────────────────────

    let GatewaySetup {
        gateway_url,
        sse_sender,
        routine_engine_slot,
    } = setup_gateway_channel(
        &config,
        &components,
        &mut GatewayContext {
            container_job_manager: &container_job_manager,
            session_manager: &session_manager,
            log_broadcaster: &log_broadcaster,
            log_level_handle: &log_level_handle,
            prompt_queue: &prompt_queue,
            scheduler_slot: &scheduler_slot,
            job_event_tx: &job_event_tx,
            channels: &channels,
            channel_names: &mut channel_names,
        },
    )
    .await;

    // ── Boot screen ────────────────────────────────────────────────────

    let boot_tool_count = components.tools.count();
    let boot_llm_model = components.llm.model_name().to_string();
    let boot_cheap_model = components
        .cheap_llm
        .as_ref()
        .map(|c| c.model_name().to_string());
    print_startup_info(
        &config,
        &cli,
        &BootData {
            llm_model: boot_llm_model,
            cheap_model: boot_cheap_model,
            tool_count: boot_tool_count,
            gateway_url,
            docker_status,
            channel_names,
            active_tunnel: &active_tunnel,
        },
    );

    // ── Run the agent ──────────────────────────────────────────────────

    let channels = Arc::new(channels);

    // Register message tool for sending messages to connected channels
    components
        .tools
        .register_message_tools(Arc::clone(&channels))
        .await;

    // Wire up channel runtime for hot-activation of WASM channels.
    wire_extension_manager(
        &components.extension_manager,
        &mut wasm_channel_runtime_state,
        &mut loaded_wasm_channel_names[..],
        &channels,
        &sse_sender,
        &config.channels.wasm_channel_owner_ids,
    )
    .await;

    // Snapshot memory for trace recording before the agent starts
    if let Some(ref recorder) = components.recording_handle
        && let Some(ref ws) = components.workspace
    {
        recorder.snapshot_memory(ws).await;
    }

    let http_interceptor = components
        .recording_handle
        .as_ref()
        .map(|r| r.http_interceptor());
    // Clone context_manager for the reaper before it's moved into Agent::new()
    let reaper_context_manager = Arc::clone(&components.context_manager);

    // Capture db reference for SIGHUP handler before it's moved into AgentDeps (Unix only)
    #[cfg(unix)]
    let sighup_settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>> = components
        .db
        .as_ref()
        .map(|db| Arc::clone(db) as Arc<dyn ironclaw::db::SettingsStore>);

    let deps = AgentDeps {
        store: components.db,
        llm: components.llm,
        cheap_llm: components.cheap_llm,
        safety: components.safety,
        tools: components.tools,
        workspace: components.workspace,
        extension_manager: components.extension_manager,
        skill_registry: components.skill_registry,
        skill_catalog: components.skill_catalog,
        skills_config: config.skills.clone(),
        hooks: components.hooks,
        cost_guard: components.cost_guard,
        sse_tx: sse_sender,
        http_interceptor,
        transcription: config
            .transcription
            .create_provider()
            .map(|p| Arc::new(ironclaw::transcription::TranscriptionMiddleware::new(p))),
        document_extraction: Some(Arc::new(
            ironclaw::document_extraction::DocumentExtractionMiddleware::new(),
        )),
    };

    let mut agent = Agent::new(
        config.agent.clone(),
        deps,
        channels,
        Some(config.heartbeat.clone()),
        Some(config.hygiene.clone()),
        Some(config.routines.clone()),
        Some(components.context_manager),
        Some(session_manager),
    );

    // Fill the scheduler slot now that Agent (and its Scheduler) exist.
    *scheduler_slot.write().await = Some(agent.scheduler());

    // Spawn sandbox reaper for orphaned container cleanup
    if let Some(ref jm) = container_job_manager {
        let reaper_jm = Arc::clone(jm);
        let reaper_config = ReaperConfig {
            scan_interval: Duration::from_secs(config.sandbox.reaper_interval_secs),
            orphan_threshold: Duration::from_secs(config.sandbox.orphan_threshold_secs),
            ..ReaperConfig::default()
        };
        let reaper_ctx = Arc::clone(&reaper_context_manager);
        tokio::spawn(async move {
            match SandboxReaper::new(reaper_jm, reaper_ctx, reaper_config).await {
                Ok(reaper) => reaper.run().await,
                Err(e) => tracing::error!("Sandbox reaper failed to initialize: {}", e),
            }
        });
    }

    // Give the agent the routine engine slot so it can expose the engine to the gateway.
    if let Some(slot) = routine_engine_slot {
        agent.set_routine_engine_slot(slot);
    }

    // Prepare SIGHUP handler for hot-reloading HTTP webhook config
    // Broadcast channel for clean shutdown of background tasks
    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    #[cfg(unix)]
    {
        use ironclaw::channels::ChannelSecretUpdater;
        let mut secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>> = Vec::new();
        if let Some(ref state) = http_channel_state {
            secret_updaters.push(Arc::clone(state) as Arc<dyn ChannelSecretUpdater>);
        }
        let reload_manager = Arc::new(ironclaw::reload::create_hot_reload_manager(
            sighup_settings_store.clone(),
            webhook_server.clone(),
            components.secrets_store.clone(),
            secret_updaters,
        ));
        spawn_sighup_handler(reload_manager, &shutdown_tx);
    }

    // Start deferred runtime side effects only after all fallible startup
    // work has completed successfully.
    side_effects.start()?;

    let run_result = agent.run().await;

    // ── Shutdown ────────────────────────────────────────────────────────

    run_shutdown_sequence(
        &shutdown_tx,
        &components.mcp_process_manager,
        &components.recording_handle,
        &webhook_server,
        &active_tunnel,
    )
    .await;

    run_result?;

    Ok(())
}

#[cfg(unix)]
fn spawn_sighup_handler(
    reload_manager: Arc<ironclaw::reload::HotReloadManager>,
    shutdown_tx: &tokio::sync::broadcast::Sender<()>,
) {
    let mut shutdown_rx = shutdown_tx.subscribe();
    tokio::spawn(async move {
        use tokio::signal::unix::{SignalKind, signal};
        let mut sighup = match signal(SignalKind::hangup()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("Failed to register SIGHUP handler: {}", e);
                return;
            }
        };
        loop {
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::debug!("SIGHUP handler shutting down");
                    break;
                }
                _ = sighup.recv() => {}
            }
            tracing::info!("SIGHUP received — reloading HTTP webhook config");
            if let Err(e) = reload_manager.perform_reload().await {
                tracing::error!("Hot-reload failed: {}", e);
            }
        }
    });
}

/// Runtime-computed values used to populate the startup boot screen.
struct BootData<'a> {
    llm_model: String,
    cheap_model: Option<String>,
    tool_count: usize,
    gateway_url: Option<String>,
    docker_status: ironclaw::sandbox::DockerStatus,
    channel_names: Vec<String>,
    active_tunnel: &'a Option<Box<dyn ironclaw::tunnel::Tunnel>>,
}

fn print_startup_info(config: &Config, cli: &Cli, data: &BootData<'_>) {
    if !config.channels.cli.enabled || cli.message.is_some() {
        return;
    }
    let boot_info = ironclaw::boot_screen::BootInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        agent_name: config.agent.name.clone(),
        llm_backend: config.llm.backend.to_string(),
        llm_model: data.llm_model.clone(),
        cheap_model: data.cheap_model.clone(),
        db_backend: if cli.no_db {
            "none".to_string()
        } else {
            config.database.backend.to_string()
        },
        db_connected: !cli.no_db,
        tool_count: data.tool_count,
        gateway_url: data.gateway_url.clone(),
        embeddings_enabled: config.embeddings.enabled,
        embeddings_provider: config
            .embeddings
            .enabled
            .then(|| config.embeddings.provider.clone()),
        heartbeat_enabled: config.heartbeat.enabled,
        heartbeat_interval_secs: config.heartbeat.interval_secs,
        sandbox_enabled: config.sandbox.enabled,
        docker_status: data.docker_status,
        claude_code_enabled: config.claude_code.enabled,
        routines_enabled: config.routines.enabled,
        skills_enabled: config.skills.enabled,
        channels: data.channel_names.clone(),
        tunnel_url: data
            .active_tunnel
            .as_ref()
            .and_then(|t| t.public_url())
            .or_else(|| config.tunnel.public_url.clone()),
        tunnel_provider: data.active_tunnel.as_ref().map(|t| t.name().to_string()),
    };
    ironclaw::boot_screen::print_boot_screen(&boot_info);
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

/// Runtime-service dependencies required to configure the gateway channel.
struct GatewayContext<'a> {
    container_job_manager: &'a Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    session_manager: &'a Arc<ironclaw::agent::SessionManager>,
    log_broadcaster: &'a Arc<ironclaw::channels::web::log_layer::LogBroadcaster>,
    log_level_handle: &'a Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
    prompt_queue: &'a Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                uuid::Uuid,
                std::collections::VecDeque<ironclaw::orchestrator::api::PendingPrompt>,
            >,
        >,
    >,
    scheduler_slot: &'a ironclaw::tools::builtin::SchedulerSlot,
    job_event_tx: &'a Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, ironclaw::channels::web::types::SseEvent)>,
    >,
    channels: &'a ironclaw::channels::ChannelManager,
    channel_names: &'a mut Vec<String>,
}

fn configure_gateway_builder(
    mut gw: GatewayChannel,
    components: &ironclaw::app::AppComponents,
    ctx: &GatewayContext<'_>,
) -> GatewayChannel {
    gw = gw.with_llm_provider(Arc::clone(&components.llm));
    if let Some(ref ws) = components.workspace {
        gw = gw.with_workspace(Arc::clone(ws));
    }
    gw = gw.with_session_manager(Arc::clone(ctx.session_manager));
    gw = gw.with_log_broadcaster(Arc::clone(ctx.log_broadcaster));
    gw = gw.with_log_level_handle(Arc::clone(ctx.log_level_handle));
    gw = gw.with_tool_registry(Arc::clone(&components.tools));
    if let Some(ref ext_mgr) = components.extension_manager {
        gw = gw.with_extension_manager(Arc::clone(ext_mgr));
    }
    if !components.catalog_entries.is_empty() {
        gw = gw.with_registry_entries(components.catalog_entries.clone());
    }
    if let Some(ref d) = components.db {
        gw = gw.with_store(Arc::clone(d));
    }
    if let Some(jm) = ctx.container_job_manager {
        gw = gw.with_job_manager(Arc::clone(jm));
    }
    gw = gw.with_scheduler(ctx.scheduler_slot.clone());
    if let Some(ref sr) = components.skill_registry {
        gw = gw.with_skill_registry(Arc::clone(sr));
    }
    if let Some(ref sc) = components.skill_catalog {
        gw = gw.with_skill_catalog(Arc::clone(sc));
    }
    gw.with_cost_guard(Arc::clone(&components.cost_guard))
}

async fn setup_gateway_channel(
    config: &Config,
    components: &ironclaw::app::AppComponents,
    ctx: &mut GatewayContext<'_>,
) -> GatewaySetup {
    let mut gateway_url: Option<String> = None;
    let mut sse_sender: Option<
        tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>,
    > = None;
    let mut routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot> = None;

    if let Some(ref gw_config) = config.channels.gateway {
        let mut gw =
            configure_gateway_builder(GatewayChannel::new(gw_config.clone()), components, ctx);
        if config.sandbox.enabled {
            gw = gw.with_prompt_queue(Arc::clone(ctx.prompt_queue));

            if let Some(tx) = ctx.job_event_tx {
                let mut rx = tx.subscribe();
                let gw_state = Arc::clone(gw.state());
                tokio::spawn(async move {
                    loop {
                        match rx.recv().await {
                            Ok((_job_id, event)) => {
                                gw_state.sse.broadcast(event);
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                                tracing::warn!(skipped, "Gateway job-event stream lagged");
                            }
                            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                        }
                    }
                });
            }
        }

        gateway_url = Some(format!(
            "http://{}:{}/?token={}",
            gw_config.host,
            gw_config.port,
            gw.auth_token()
        ));

        tracing::debug!("Web UI: http://{}:{}/", gw_config.host, gw_config.port);

        // Capture SSE sender and routine engine slot before moving gw into channels.
        // IMPORTANT: This must come after all `with_*` calls since `rebuild_state`
        // creates a new SseManager, which would orphan this sender.
        sse_sender = Some(gw.state().sse.sender());
        routine_engine_slot = Some(Arc::clone(&gw.state().routine_engine));

        ctx.channel_names.push("gateway".to_string());
        ctx.channels.add(Box::new(gw)).await;
    }

    GatewaySetup {
        gateway_url,
        sse_sender,
        routine_engine_slot,
    }
}

struct GatewaySetup {
    gateway_url: Option<String>,
    sse_sender: Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot>,
}

#[cfg(test)]
mod tests {
    use ironclaw::{
        boot_screen::{BootInfo, render_boot_screen},
        config::Config,
        sandbox::DockerStatus,
        tunnel::{NativeTunnel, Tunnel},
    };

    use super::BootData;

    struct TestTunnel {
        public_url: Option<String>,
    }

    impl NativeTunnel for TestTunnel {
        fn name(&self) -> &str {
            "ngrok"
        }

        fn start<'a>(
            &'a self,
            _local_host: &'a str,
            _local_port: u16,
        ) -> impl std::future::Future<Output = anyhow::Result<String>> + Send + 'a {
            let url = self
                .public_url
                .clone()
                .expect("test tunnel should have a public URL");
            async move { Ok(url) }
        }

        async fn stop(&self) -> anyhow::Result<()> {
            Ok(())
        }

        async fn health_check(&self) -> bool {
            true
        }

        fn public_url(&self) -> Option<String> {
            self.public_url.clone()
        }
    }

    fn startup_snapshot_body() -> String {
        let body = include_str!("snapshots/ironclaw__tests__startup_info_boot_screen.snap")
            .split_once("\n---\n\n")
            .expect("startup snapshot should contain front matter")
            .1;
        format!("\n{body}\n")
    }

    #[tokio::test]
    async fn print_startup_info_matches_snapshot() {
        let tempdir = tempfile::tempdir().expect("tempdir should be created");
        let mut config = Config::for_testing(
            tempdir.path().join("test.db"),
            tempdir.path().join("skills"),
            tempdir.path().join("installed-skills"),
        )
        .await
        .expect("test config should be built");
        config.agent.name = "startup-test-agent".to_string();
        config.llm.backend = "openai".to_string();
        config.channels.cli.enabled = true;
        config.embeddings.enabled = true;
        config.embeddings.provider = "openai".to_string();
        config.heartbeat.enabled = true;
        config.heartbeat.interval_secs = 1_800;
        config.sandbox.enabled = true;
        config.claude_code.enabled = true;
        config.routines.enabled = true;
        config.skills.enabled = true;
        config.tunnel.public_url = Some("https://fallback.example.test".to_string());

        let cli = ironclaw::cli::Cli {
            command: None,
            cli_only: false,
            no_db: false,
            message: None,
            config: None,
            no_onboard: false,
        };

        let active_tunnel: Option<Box<dyn Tunnel>> = Some(Box::new(TestTunnel {
            public_url: Some("https://runtime.ngrok.app".to_string()),
        }));
        let data = BootData {
            llm_model: "gpt-4.1".to_string(),
            cheap_model: Some("gpt-4.1-mini".to_string()),
            tool_count: 42,
            gateway_url: Some("http://127.0.0.1:4040/?token=startup-token".to_string()),
            docker_status: DockerStatus::NotRunning,
            channel_names: vec![
                "repl".to_string(),
                "gateway".to_string(),
                "signal".to_string(),
            ],
            active_tunnel: &active_tunnel,
        };

        let boot_info = BootInfo {
            version: env!("CARGO_PKG_VERSION").to_string(),
            agent_name: config.agent.name.clone(),
            llm_backend: config.llm.backend.to_string(),
            llm_model: data.llm_model.clone(),
            cheap_model: data.cheap_model.clone(),
            db_backend: if cli.no_db {
                "none".to_string()
            } else {
                config.database.backend.to_string()
            },
            db_connected: !cli.no_db,
            tool_count: data.tool_count,
            gateway_url: data.gateway_url.clone(),
            embeddings_enabled: config.embeddings.enabled,
            embeddings_provider: config
                .embeddings
                .enabled
                .then(|| config.embeddings.provider.clone()),
            heartbeat_enabled: config.heartbeat.enabled,
            heartbeat_interval_secs: config.heartbeat.interval_secs,
            sandbox_enabled: config.sandbox.enabled,
            docker_status: data.docker_status,
            claude_code_enabled: config.claude_code.enabled,
            routines_enabled: config.routines.enabled,
            skills_enabled: config.skills.enabled,
            channels: data.channel_names.clone(),
            tunnel_url: data
                .active_tunnel
                .as_ref()
                .and_then(|t| t.public_url())
                .or_else(|| config.tunnel.public_url.clone()),
            tunnel_provider: data.active_tunnel.as_ref().map(|t| t.name().to_string()),
        };

        let output = render_boot_screen(&boot_info);
        assert_eq!(output, startup_snapshot_body());
    }
}
