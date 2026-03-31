//! IronClaw - Main entry point.

use std::sync::Arc;
use std::time::Duration;

use clap::Parser;

use ironclaw::{
    agent::{Agent, AgentDeps},
    app::{AppBuilder, AppBuilderFlags},
    channels::{
        ChannelManager, GatewayChannel, HttpChannel, ReplChannel, SignalChannel, WebhookServer,
        WebhookServerConfig, web::log_layer::LogBroadcaster,
    },
    cli::{
        Cli, Command, run_mcp_command, run_pairing_command, run_service_command,
        run_status_command, run_tool_command,
    },
    config::Config,
    hooks::bootstrap_hooks,
    llm::create_session_manager,
    orchestrator::{ReaperConfig, SandboxReaper},
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

/// Holds all channel setup state.
struct ChannelSetup {
    channels: ChannelManager,
    channel_names: Vec<String>,
    loaded_wasm_channel_names: Vec<String>,
    /// WASM channel setup context, if WASM channels are enabled.
    wasm_channel_setup: Option<Arc<ironclaw::channels::wasm::WasmChannelSetup>>,
    webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    #[cfg(unix)]
    http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>>,
}

/// Holds gateway channel setup state.
struct GatewaySetup {
    gateway_url: Option<String>,
    sse_sender: Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot>,
}

/// Bundled inputs for the agent-run phase, produced by the
/// gateway and orchestrator setup phases.
struct AgentRunContext {
    gateway_setup: GatewaySetup,
    orch: ironclaw::orchestrator::OrchestratorSetup,
}

/// Bundled context for the gateway-setup phase.
struct GatewayPhaseContext<'a> {
    log_broadcaster: &'a Arc<LogBroadcaster>,
    log_level_handle: &'a Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
    orch: &'a ironclaw::orchestrator::OrchestratorSetup,
}

/// Collects the runtime-environment facts required to render the boot screen.
struct BootScreenContext<'a> {
    gateway_setup: &'a GatewaySetup,
    channel_names: &'a [String],
    docker_status: ironclaw::sandbox::DockerStatus,
    active_tunnel: &'a Option<Box<dyn ironclaw::tunnel::Tunnel>>,
}

/// Handles all user-facing CLI tool subcommands (Tool through Import).
/// Returns `Ok(Some(()))` when a command was handled, `Ok(None)` to fall through.
async fn dispatch_cli_tool_commands(cli: &Cli) -> anyhow::Result<Option<()>> {
    match &cli.command {
        Some(Command::Tool(tool_cmd)) => {
            init_cli_tracing();
            run_tool_command(tool_cmd.clone()).await?;
            Ok(Some(()))
        }
        Some(Command::Config(config_cmd)) => {
            init_cli_tracing();
            ironclaw::cli::run_config_command(config_cmd.clone()).await?;
            Ok(Some(()))
        }
        Some(Command::Registry(registry_cmd)) => {
            init_cli_tracing();
            ironclaw::cli::run_registry_command(registry_cmd.clone()).await?;
            Ok(Some(()))
        }
        Some(Command::Mcp(mcp_cmd)) => {
            init_cli_tracing();
            run_mcp_command(*mcp_cmd.clone()).await?;
            Ok(Some(()))
        }
        Some(Command::Memory(mem_cmd)) => {
            init_cli_tracing();
            ironclaw::cli::run_memory_command(mem_cmd).await?;
            Ok(Some(()))
        }
        Some(Command::Pairing(pairing_cmd)) => {
            init_cli_tracing();
            run_pairing_command(pairing_cmd.clone()).map_err(|e| anyhow::anyhow!("{}", e))?;
            Ok(Some(()))
        }
        Some(Command::Service(service_cmd)) => {
            init_cli_tracing();
            run_service_command(service_cmd)?;
            Ok(Some(()))
        }
        Some(Command::Doctor) => {
            init_cli_tracing();
            ironclaw::cli::run_doctor_command().await?;
            Ok(Some(()))
        }
        Some(Command::Status) => {
            init_cli_tracing();
            run_status_command().await?;
            Ok(Some(()))
        }
        Some(Command::Completion(completion)) => {
            init_cli_tracing();
            completion.run()?;
            Ok(Some(()))
        }
        #[cfg(feature = "import")]
        Some(Command::Import(import_cmd)) => {
            init_cli_tracing();
            let config = ironclaw::config::Config::from_env().await?;
            ironclaw::cli::run_import_command(import_cmd, &config).await?;
            Ok(Some(()))
        }
        _ => Ok(None),
    }
}

/// Handles worker, bridge, and onboarding subcommands.
/// Returns `Ok(Some(()))` when a command was handled, `Ok(None)` to fall through.
async fn dispatch_agent_commands(cli: &Cli) -> anyhow::Result<Option<()>> {
    match &cli.command {
        Some(Command::Worker {
            job_id,
            orchestrator_url,
            max_iterations,
        }) => {
            init_worker_tracing();
            ironclaw::worker::run_worker(*job_id, orchestrator_url, *max_iterations).await?;
            Ok(Some(()))
        }
        Some(Command::ClaudeBridge {
            job_id,
            orchestrator_url,
            max_turns,
            model,
        }) => {
            init_worker_tracing();
            ironclaw::worker::run_claude_bridge(*job_id, orchestrator_url, *max_turns, model)
                .await?;
            Ok(Some(()))
        }
        Some(Command::Onboard {
            skip_auth,
            channels_only,
            provider_only,
            quick,
        }) => {
            #[cfg(any(feature = "postgres", feature = "libsql"))]
            {
                let config = SetupConfig {
                    skip_auth: *skip_auth,
                    channels_only: *channels_only,
                    provider_only: *provider_only,
                    quick: *quick,
                };
                let mut wizard = SetupWizard::with_config(config);
                wizard.run().await?;
            }
            #[cfg(not(any(feature = "postgres", feature = "libsql")))]
            {
                let _ = (skip_auth, channels_only, provider_only, quick);
                eprintln!("Onboarding wizard requires the 'postgres' or 'libsql' feature.");
            }
            Ok(Some(()))
        }
        _ => Ok(None),
    }
}

/// Dispatch CLI subcommands.
///
/// Returns `Ok(Some(()))` for commands that were handled (caller should exit),
/// or `Ok(None)` for the fall-through run case.
async fn dispatch_subcommand(cli: &Cli) -> anyhow::Result<Option<()>> {
    if dispatch_cli_tool_commands(cli).await?.is_some() {
        return Ok(Some(()));
    }
    dispatch_agent_commands(cli).await
}

/// Carries the output of WASM-channel initialisation.
struct WasmChannelsInit {
    channel_names: Vec<String>,
    loaded_channel_names: Vec<String>,
    webhook_routes: Option<axum::Router>,
    setup: Arc<ironclaw::channels::wasm::WasmChannelSetup>,
}

/// Initialise WASM channels, add them to `channels`, and return setup state.
///
/// Returns `None` when the WASM channel feature is disabled or the channels
/// directory does not yet exist.
async fn init_wasm_channels(
    config: &Config,
    components: &ironclaw::app::AppComponents,
    channels: &ChannelManager,
) -> Option<WasmChannelsInit> {
    if !config.channels.wasm_channels_enabled || !config.channels.wasm_channels_dir.exists() {
        return None;
    }
    let mut result = ironclaw::channels::wasm::setup_wasm_channels(
        config,
        &components.secrets_store,
        components.extension_manager.as_ref(),
        components.db.as_ref(),
    )
    .await?;

    let loaded_channel_names = result.channel_names.clone();
    let mut channel_names = Vec::new();
    for (name, channel) in std::mem::take(&mut result.channels) {
        channel_names.push(name);
        channels.add(channel).await;
    }
    let webhook_routes = result.webhook_routes.take();
    Some(WasmChannelsInit {
        channel_names,
        loaded_channel_names,
        webhook_routes,
        setup: Arc::new(result),
    })
}

/// Set up all channels (REPL, WASM, Signal, HTTP, webhook server).
async fn setup_channels(
    config: &Config,
    cli: &Cli,
    components: &ironclaw::app::AppComponents,
) -> anyhow::Result<ChannelSetup> {
    let channels = ChannelManager::new();
    let mut channel_names: Vec<String> = Vec::new();
    let mut loaded_wasm_channel_names: Vec<String> = Vec::new();
    let mut wasm_channel_setup: Option<Arc<ironclaw::channels::wasm::WasmChannelSetup>> = None;

    // Create CLI channel
    let repl_channel = if let Some(ref msg) = cli.message {
        Some(ReplChannel::with_message(msg.clone()))
    } else if config.channels.cli.enabled {
        let repl = ReplChannel::new();
        repl.suppress_banner();
        Some(repl)
    } else {
        None
    };

    if let Some(repl) = repl_channel {
        channels.add(Box::new(repl)).await;
        if cli.message.is_some() {
            tracing::debug!("Single message mode");
        } else {
            channel_names.push("repl".to_string());
            tracing::debug!("REPL mode enabled");
        }
    }

    // Collect webhook route fragments; a single WebhookServer hosts them all.
    let mut webhook_routes: Vec<axum::Router> = Vec::new();

    // Load WASM channels and register their webhook routes.
    if let Some(wasm_init) = init_wasm_channels(config, components, &channels).await {
        loaded_wasm_channel_names = wasm_init.loaded_channel_names;
        channel_names.extend(wasm_init.channel_names);
        if let Some(routes) = wasm_init.webhook_routes {
            webhook_routes.push(routes);
        }
        wasm_channel_setup = Some(wasm_init.setup);
    }

    // Add Signal channel if configured and not CLI-only mode.
    if !cli.cli_only
        && let Some(ref signal_config) = config.channels.signal
    {
        let signal_channel = SignalChannel::new(signal_config.clone())?;
        channel_names.push("signal".to_string());
        channels.add(Box::new(signal_channel)).await;
        let safe_url = SignalChannel::redact_url(&signal_config.http_url);
        tracing::debug!(
            url = %safe_url,
            "Signal channel enabled"
        );
        if signal_config.allow_from.is_empty() {
            tracing::warn!(
                "Signal channel has empty allow_from list - ALL messages will be DENIED."
            );
        }
    }

    // Add HTTP channel if configured and not CLI-only mode.
    let mut webhook_server_addr: Option<std::net::SocketAddr> = None;
    #[cfg(unix)]
    let mut http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>> = None;
    if !cli.cli_only
        && let Some(ref http_config) = config.channels.http
    {
        let http_channel = HttpChannel::new(http_config.clone());
        #[cfg(unix)]
        {
            http_channel_state = Some(http_channel.shared_state());
        }
        webhook_routes.push(http_channel.routes());
        let (host, port) = http_channel.addr();
        webhook_server_addr = Some(
            format!("{}:{}", host, port)
                .parse()
                .expect("HttpConfig host:port must be a valid SocketAddr"),
        );
        channel_names.push("http".to_string());
        channels.add(Box::new(http_channel)).await;
        tracing::debug!(
            "HTTP channel enabled on {}:{}",
            http_config.host,
            http_config.port
        );
    }

    // Start the unified webhook server if any routes were registered.
    let webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>> = if !webhook_routes
        .is_empty()
    {
        let addr =
            webhook_server_addr.unwrap_or_else(|| std::net::SocketAddr::from(([0, 0, 0, 0], 8080)));
        if addr.ip().is_unspecified() {
            tracing::warn!(
                "Webhook server is binding to {} — it will be reachable from all network interfaces. \
                 Set HTTP_HOST=127.0.0.1 to restrict to localhost.",
                addr.ip()
            );
        }
        let mut server = WebhookServer::new(WebhookServerConfig { addr });
        for routes in webhook_routes {
            server.add_routes(routes);
        }
        server.start().await?;
        Some(Arc::new(tokio::sync::Mutex::new(server)))
    } else {
        None
    };

    Ok(ChannelSetup {
        channels,
        channel_names,
        loaded_wasm_channel_names,
        wasm_channel_setup,
        webhook_server,
        #[cfg(unix)]
        http_channel_state,
    })
}

/// Set up the gateway channel.
#[expect(
    clippy::too_many_arguments,
    reason = "Gateway channel requires many dependencies; consider a builder pattern when adding more parameters FIXME:https://github.com/df12/axinite/issues/TBD"
)]
async fn setup_gateway_channel(
    config: &Config,
    components: &ironclaw::app::AppComponents,
    session_manager: &Arc<ironclaw::agent::SessionManager>,
    log_broadcaster: &Arc<LogBroadcaster>,
    log_level_handle: &Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
    scheduler_slot: &ironclaw::tools::builtin::SchedulerSlot,
    container_job_manager: &Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    job_event_tx: &Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, ironclaw::channels::web::types::SseEvent)>,
    >,
    prompt_queue: &ironclaw::orchestrator::PromptQueue,
    channel_manager: &ChannelManager,
    channel_names: &mut Vec<String>,
) -> anyhow::Result<GatewaySetup> {
    let mut gateway_url: Option<String> = None;
    let mut sse_sender: Option<
        tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>,
    > = None;
    let mut routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot> = None;

    if let Some(ref gw_config) = config.channels.gateway {
        let mut gw =
            GatewayChannel::new(gw_config.clone()).with_llm_provider(Arc::clone(&components.llm));
        if let Some(ref ws) = components.workspace {
            gw = gw.with_workspace(Arc::clone(ws));
        }
        gw = gw.with_session_manager(Arc::clone(session_manager));
        gw = gw.with_log_broadcaster(Arc::clone(log_broadcaster));
        gw = gw.with_log_level_handle(Arc::clone(log_level_handle));
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
        if let Some(jm) = container_job_manager {
            gw = gw.with_job_manager(Arc::clone(jm));
        }
        gw = gw.with_scheduler(scheduler_slot.clone());
        if let Some(ref sr) = components.skill_registry {
            gw = gw.with_skill_registry(Arc::clone(sr));
        }
        if let Some(ref sc) = components.skill_catalog {
            gw = gw.with_skill_catalog(Arc::clone(sc));
        }
        gw = gw.with_cost_guard(Arc::clone(&components.cost_guard));
        if config.sandbox.enabled {
            gw = gw.with_prompt_queue(Arc::clone(prompt_queue));

            if let Some(tx) = job_event_tx {
                let mut rx = tx.subscribe();
                let gw_state = Arc::clone(gw.state());
                tokio::spawn(async move {
                    while let Ok((_job_id, event)) = rx.recv().await {
                        gw_state.sse.broadcast(event);
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

        channel_names.push("gateway".to_string());
        channel_manager.add(Box::new(gw)).await;
    }

    Ok(GatewaySetup {
        gateway_url,
        sse_sender,
        routine_engine_slot,
    })
}

/// Spawn SIGHUP handler for hot-reloading HTTP webhook config.
#[cfg(unix)]
fn spawn_sighup_handler(
    webhook_server: Option<Arc<tokio::sync::Mutex<WebhookServer>>>,
    settings_store: Option<Arc<dyn ironclaw::db::SettingsStore>>,
    secrets_store: Option<Arc<dyn ironclaw::secrets::SecretsStore + Send + Sync>>,
    http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>>,
    mut shutdown_rx: tokio::sync::broadcast::Receiver<()>,
) {
    use ironclaw::channels::ChannelSecretUpdater;
    // Collect all channels that support secret updates
    let mut secret_updaters: Vec<Arc<dyn ChannelSecretUpdater>> = Vec::new();
    if let Some(ref state) = http_channel_state {
        secret_updaters.push(Arc::clone(state) as Arc<dyn ChannelSecretUpdater>);
    }

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
            // Exit loop on shutdown signal or when SIGHUP is received
            tokio::select! {
                _ = shutdown_rx.recv() => {
                    tracing::debug!("SIGHUP handler shutting down");
                    break;
                }
                _ = sighup.recv() => {
                    // Handle SIGHUP signal
                }
            }
            tracing::info!("SIGHUP received — reloading HTTP webhook config");

            // Inject channel secrets from database into thread-safe overlay
            // (similar to inject_llm_keys_from_secrets for LLM providers)
            if let Some(ref secrets_store) = secrets_store {
                // Inject HTTP webhook secret from encrypted store
                match secrets_store
                    .get_decrypted("default", "http_webhook_secret")
                    .await
                {
                    Ok(webhook_secret) => {
                        // Thread-safe: Uses INJECTED_VARS mutex instead of unsafe std::env::set_var
                        // Config::from_env() will read from the overlay via optional_env()
                        ironclaw::config::inject_single_var(
                            "HTTP_WEBHOOK_SECRET",
                            webhook_secret.expose(),
                        );
                        tracing::debug!("Injected HTTP_WEBHOOK_SECRET from secrets store");
                    }
                    Err(e) => {
                        // Clear any stale secret from the overlay to prevent reuse
                        ironclaw::config::inject_single_var("HTTP_WEBHOOK_SECRET", "");
                        tracing::debug!(
                            "Cleared HTTP_WEBHOOK_SECRET from overlay (failed to decrypt: {})",
                            e
                        );
                    }
                }
            } else {
                // No secrets store available, clear any stale secret
                ironclaw::config::inject_single_var("HTTP_WEBHOOK_SECRET", "");
            }

            // Reload config (now with secrets injected into environment)
            let new_config = match &settings_store {
                Some(store) => ironclaw::config::Config::from_db(store.as_ref(), "default").await,
                None => ironclaw::config::Config::from_env().await,
            };

            let new_config = match new_config {
                Ok(c) => c,
                Err(e) => {
                    tracing::error!("SIGHUP config reload failed: {}", e);
                    continue;
                }
            };

            let new_http = match new_config.channels.http {
                Some(c) => c,
                None => {
                    tracing::warn!("SIGHUP: HTTP channel no longer configured, skipping");
                    continue;
                }
            };

            // Compute new socket addr
            let new_addr: std::net::SocketAddr =
                match format!("{}:{}", new_http.host, new_http.port).parse() {
                    Ok(a) => a,
                    Err(e) => {
                        tracing::error!("SIGHUP: invalid addr in config: {}", e);
                        continue;
                    }
                };

            // Restart listener if addr changed.
            // Minimize lock scope: acquire, read old addr, release, then restart.
            let mut restart_failed = false;
            if let Some(ref ws_arc) = webhook_server {
                let old_addr = {
                    let ws = ws_arc.lock().await;
                    ws.current_addr()
                }; // Lock released here

                if old_addr != new_addr {
                    tracing::info!(
                        "SIGHUP: HTTP addr {} -> {}, restarting listener",
                        old_addr,
                        new_addr
                    );
                    // NOTE: Lock is held across restart_with_addr().await. This is
                    // acceptable because SIGHUP is infrequent and restart is fast. A full
                    // fix would require refactoring restart_with_addr to separate state
                    // mutation from async I/O.
                    let mut ws = ws_arc.lock().await;
                    match ws.restart_with_addr(new_addr).await {
                        Ok(()) => {
                            tracing::info!("SIGHUP: webhook server restarted on {}", new_addr);
                        }
                        Err(e) => {
                            tracing::error!("SIGHUP: listener restart failed: {}", e);
                            restart_failed = true;
                        }
                    }
                } else {
                    tracing::debug!("SIGHUP: addr unchanged ({})", old_addr);
                }
            }

            // Update secrets in all configured channels (if restart succeeded or wasn't needed)
            if !restart_failed {
                use secrecy::{ExposeSecret, SecretString};
                let new_secret = new_http
                    .webhook_secret
                    .as_ref()
                    .map(|s| SecretString::from(s.expose_secret().to_string()));

                // Update all channels that support secret swapping
                for updater in &secret_updaters {
                    updater.update_secret(new_secret.clone()).await;
                }
            }
        }
    });
}

/// Phase 1: Acquire PID lock and run onboarding if needed.
async fn phase_pid_and_onboard(cli: &Cli) -> anyhow::Result<Option<ironclaw::bootstrap::PidLock>> {
    let pid_lock = match ironclaw::bootstrap::PidLock::acquire() {
        Ok(lock) => Some(lock),
        Err(ironclaw::bootstrap::PidLockError::AlreadyRunning { pid }) => {
            anyhow::bail!(
                "Another IronClaw instance is already running (PID {}). \
                 If this is incorrect, remove the stale PID file: {}",
                pid,
                ironclaw::bootstrap::pid_lock_path().display()
            );
        }
        Err(e) => {
            eprintln!("Warning: Could not acquire PID lock: {}", e);
            eprintln!("Continuing without PID lock protection.");
            None
        }
    };

    // Enhanced first-run detection
    #[cfg(any(feature = "postgres", feature = "libsql"))]
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

    Ok(pid_lock)
}

/// Phase 2: Load configuration and initialize tracing.
async fn phase_load_config_and_tracing(
    cli: &Cli,
) -> anyhow::Result<(
    Config,
    Arc<ironclaw::llm::SessionManager>,
    Arc<LogBroadcaster>,
    Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
)> {
    let toml_path = cli.config.as_deref();
    let config = match Config::from_env_with_toml(toml_path).await {
        Ok(c) => c,
        Err(ironclaw::error::ConfigError::MissingRequired { key, hint }) => {
            anyhow::bail!(
                "Configuration error: Missing required setting '{}'. {}. \
                 Run 'ironclaw onboard' to configure, or set the required environment variables.",
                key,
                hint
            );
        }
        Err(e) => return Err(e.into()),
    };

    let session = create_session_manager(config.llm.session.clone()).await;
    let log_broadcaster = Arc::new(LogBroadcaster::new());
    let log_level_handle =
        ironclaw::channels::web::log_layer::init_tracing(Arc::clone(&log_broadcaster));

    tracing::debug!("Starting IronClaw...");
    tracing::debug!("Loaded configuration for agent: {}", config.agent.name);
    tracing::debug!("LLM backend: {}", config.llm.backend);

    Ok((config, session, log_broadcaster, log_level_handle))
}

/// Phase 3: Build core components via AppBuilder.
async fn phase_build_components(
    cli: &Cli,
    config: Config,
    session: Arc<ironclaw::llm::SessionManager>,
    log_broadcaster: Arc<LogBroadcaster>,
) -> anyhow::Result<(Config, ironclaw::app::AppComponents)> {
    let toml_path = cli.config.as_deref();
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
        session,
        log_broadcaster,
    )
    .build_components()
    .await?;

    side_effects.start().await;

    let config = components.config.clone();
    Ok((config, components))
}

/// Phase 4: Start tunnel and orchestrator.
async fn phase_tunnel_and_orchestrator(
    config: &Config,
    components: &ironclaw::app::AppComponents,
) -> anyhow::Result<(
    Option<Box<dyn ironclaw::tunnel::Tunnel>>,
    ironclaw::orchestrator::OrchestratorSetup,
)> {
    let (_, active_tunnel) =
        ironclaw::tunnel::start_managed_tunnel(components.config.clone()).await;

    let orch = ironclaw::orchestrator::setup_orchestrator(
        config,
        &components.llm,
        &components.tools,
        components.db.as_ref(),
        components.secrets_store.as_ref(),
    )
    .await;

    Ok((active_tunnel, orch))
}

/// Phase 5: Initialize channels and hooks.
async fn phase_init_channels_and_hooks(
    config: &Config,
    cli: &Cli,
    components: &ironclaw::app::AppComponents,
    loaded_wasm_channel_names: &[String],
) -> anyhow::Result<(ChannelSetup, ironclaw::hooks::HookBootstrapSummary)> {
    let channel_setup = setup_channels(config, cli, components).await?;

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

    Ok((channel_setup, hook_bootstrap))
}

/// Phase 6: Setup gateway channel.
async fn phase_setup_gateway(
    config: &Config,
    components: &ironclaw::app::AppComponents,
    channel_setup: &ChannelSetup,
    ctx: &GatewayPhaseContext<'_>,
) -> anyhow::Result<GatewaySetup> {
    let session_manager =
        Arc::new(ironclaw::agent::SessionManager::new().with_hooks(components.hooks.clone()));

    let scheduler_slot: ironclaw::tools::builtin::SchedulerSlot =
        Arc::new(tokio::sync::RwLock::new(None));

    components
        .tools
        .register_job_tools(ironclaw::tools::RegisterJobToolsOptions {
            context_manager: Arc::clone(&components.context_manager),
            scheduler_slot: Some(scheduler_slot.clone()),
            job_manager: ctx.orch.container_job_manager.clone(),
            store: components.db.clone(),
            job_event_tx: ctx.orch.job_event_tx.clone(),
            inject_tx: Some(channel_setup.channels.inject_sender()),
            prompt_queue: if config.sandbox.enabled {
                Some(Arc::clone(&ctx.orch.prompt_queue))
            } else {
                None
            },
            secrets_store: components.secrets_store.clone(),
        });

    let mut channel_names = channel_setup.channel_names.clone();
    let gateway_setup = setup_gateway_channel(
        config,
        components,
        &session_manager,
        ctx.log_broadcaster,
        ctx.log_level_handle,
        &scheduler_slot,
        &ctx.orch.container_job_manager,
        &ctx.orch.job_event_tx,
        &ctx.orch.prompt_queue,
        &channel_setup.channels,
        &mut channel_names,
    )
    .await?;

    Ok(gateway_setup)
}

/// Phase 7: Print boot screen if CLI mode is enabled.
fn phase_print_boot_screen(
    cli: &Cli,
    config: &Config,
    components: &ironclaw::app::AppComponents,
    ctx: &BootScreenContext<'_>,
) {
    if !config.channels.cli.enabled || cli.message.is_some() {
        return;
    }

    let boot_info = ironclaw::boot_screen::BootInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        agent_name: config.agent.name.clone(),
        llm_backend: config.llm.backend.to_string(),
        llm_model: components.llm.model_name().to_string(),
        cheap_model: components
            .cheap_llm
            .as_ref()
            .map(|c| c.model_name().to_string()),
        db_backend: if cli.no_db {
            "none".to_string()
        } else {
            config.database.backend.to_string()
        },
        db_connected: !cli.no_db,
        tool_count: components.tools.count(),
        gateway_url: ctx.gateway_setup.gateway_url.clone(),
        embeddings_enabled: config.embeddings.enabled,
        embeddings_provider: if config.embeddings.enabled {
            Some(config.embeddings.provider.clone())
        } else {
            None
        },
        heartbeat_enabled: config.heartbeat.enabled,
        heartbeat_interval_secs: config.heartbeat.interval_secs,
        sandbox_enabled: config.sandbox.enabled,
        docker_status: ctx.docker_status,
        claude_code_enabled: config.claude_code.enabled,
        routines_enabled: config.routines.enabled,
        skills_enabled: config.skills.enabled,
        channels: ctx.channel_names.to_vec(),
        tunnel_url: ctx
            .active_tunnel
            .as_ref()
            .and_then(|t| t.public_url())
            .or_else(|| config.tunnel.public_url.clone()),
        tunnel_provider: ctx.active_tunnel.as_ref().map(|t| t.name().to_string()),
    };
    ironclaw::boot_screen::print_boot_screen(&boot_info);
}

/// Wire the WASM channel runtime into the extension manager and
/// re-activate any channels that were persisted across restarts.
async fn wire_wasm_channel_runtime(
    ext_mgr: &ironclaw::extensions::ExtensionManager,
    channels: &Arc<ChannelManager>,
    setup: Arc<ironclaw::channels::wasm::WasmChannelSetup>,
    loaded_wasm_channel_names: Vec<String>,
    wasm_channel_owner_ids: std::collections::HashMap<String, i64>,
) {
    let active_at_startup: std::collections::HashSet<String> =
        loaded_wasm_channel_names.iter().cloned().collect();
    ext_mgr.set_active_channels(loaded_wasm_channel_names).await;
    ext_mgr
        .set_channel_runtime(
            Arc::clone(channels),
            Arc::clone(&setup.wasm_channel_runtime),
            Arc::clone(&setup.pairing_store),
            Arc::clone(&setup.wasm_channel_router),
            wasm_channel_owner_ids,
        )
        .await;
    tracing::debug!("Channel runtime wired into extension manager for hot-activation");

    let persisted = ext_mgr.load_persisted_active_channels().await;
    for name in &persisted {
        if active_at_startup.contains(name) || ext_mgr.is_relay_channel(name).await {
            continue;
        }
        match ext_mgr.activate(name).await {
            Ok(result) => tracing::debug!(
                channel = %name,
                message = %result.message,
                "Auto-activated persisted WASM channel"
            ),
            Err(e) => tracing::warn!(
                channel = %name,
                error = %e,
                "Failed to auto-activate persisted WASM channel"
            ),
        }
    }
}

/// Phase 8: Run the agent.
async fn phase_run_agent(
    config: &Config,
    components: ironclaw::app::AppComponents,
    channel_setup: ChannelSetup,
    ctx: AgentRunContext,
) -> anyhow::Result<tokio::sync::broadcast::Sender<()>> {
    let loaded_wasm_channel_names = channel_setup.loaded_wasm_channel_names.clone();
    let AgentRunContext {
        gateway_setup,
        orch,
    } = ctx;
    let channels = Arc::new(channel_setup.channels);

    components
        .tools
        .register_message_tools(Arc::clone(&channels))
        .await;

    // Wire up channel runtime for hot-activation of WASM channels.
    if let Some(ref ext_mgr) = components.extension_manager
        && let Some(ref setup) = channel_setup.wasm_channel_setup
    {
        wire_wasm_channel_runtime(
            ext_mgr,
            &channels,
            Arc::clone(setup),
            loaded_wasm_channel_names,
            config.channels.wasm_channel_owner_ids.clone(),
        )
        .await;
    }

    if let Some(ref ext_mgr) = components.extension_manager {
        ext_mgr
            .set_relay_channel_manager(Arc::clone(&channels))
            .await;
        ext_mgr.restore_relay_channels().await;
    }

    if let Some(ref ext_mgr) = components.extension_manager
        && let Some(ref sender) = gateway_setup.sse_sender
    {
        ext_mgr.set_sse_sender(sender.clone()).await;
    }

    if let Some(ref recorder) = components.recording_handle
        && let Some(ref ws) = components.workspace
    {
        recorder.snapshot_memory(ws).await;
    }

    let http_interceptor = components
        .recording_handle
        .as_ref()
        .map(|r| r.http_interceptor());
    let reaper_context_manager = Arc::clone(&components.context_manager);

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
        sse_tx: gateway_setup.sse_sender,
        http_interceptor,
        transcription: config
            .transcription
            .create_provider()
            .map(|p| Arc::new(ironclaw::transcription::TranscriptionMiddleware::new(p))),
        document_extraction: Some(Arc::new(
            ironclaw::document_extraction::DocumentExtractionMiddleware::new(),
        )),
    };

    let session_manager =
        Arc::new(ironclaw::agent::SessionManager::new().with_hooks(deps.hooks.clone()));

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

    let scheduler_slot: ironclaw::tools::builtin::SchedulerSlot =
        Arc::new(tokio::sync::RwLock::new(None));
    *scheduler_slot.write().await = Some(agent.scheduler());

    if let Some(ref jm) = orch.container_job_manager {
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

    if let Some(slot) = gateway_setup.routine_engine_slot {
        agent.set_routine_engine_slot(slot);
    }

    let (shutdown_tx, _) = tokio::sync::broadcast::channel::<()>(1);

    #[cfg(unix)]
    {
        let shutdown_rx = shutdown_tx.subscribe();
        spawn_sighup_handler(
            channel_setup.webhook_server.clone(),
            sighup_settings_store,
            components.secrets_store.clone(),
            channel_setup.http_channel_state,
            shutdown_rx,
        );
    }

    agent.run().await?;

    Ok(shutdown_tx)
}

/// Main async entry point.
async fn async_main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handle non-agent commands first (they don't need full setup)
    if dispatch_subcommand(&cli).await?.is_some() {
        return Ok(());
    }

    // Phase 1: PID lock and onboarding
    let _pid_lock = phase_pid_and_onboard(&cli).await?;

    // Phase 2: Load config and initialize tracing
    let (config, session, log_broadcaster, log_level_handle) =
        phase_load_config_and_tracing(&cli).await?;

    // Phase 3: Build core components
    let (config, components) =
        phase_build_components(&cli, config, session, log_broadcaster.clone()).await?;

    // Phase 4: Start tunnel and orchestrator
    let (active_tunnel, orch) = phase_tunnel_and_orchestrator(&config, &components).await?;
    let loaded_wasm_channel_names = Vec::new(); // Will be populated in phase 5

    // Phase 5: Initialize channels and hooks
    let (channel_setup, _hook_bootstrap) =
        phase_init_channels_and_hooks(&config, &cli, &components, &loaded_wasm_channel_names)
            .await?;

    // Phase 6: Setup gateway
    let gateway_setup = phase_setup_gateway(
        &config,
        &components,
        &channel_setup,
        &GatewayPhaseContext {
            log_broadcaster: &log_broadcaster,
            log_level_handle: &log_level_handle,
            orch: &orch,
        },
    )
    .await?;

    // Phase 7: Print boot screen
    phase_print_boot_screen(
        &cli,
        &config,
        &components,
        &BootScreenContext {
            gateway_setup: &gateway_setup,
            channel_names: &channel_setup.channel_names,
            docker_status: orch.docker_status,
            active_tunnel: &active_tunnel,
        },
    );

    // Phase 8: Run agent
    let shutdown_tx = phase_run_agent(
        &config,
        components,
        channel_setup,
        AgentRunContext {
            gateway_setup,
            orch,
        },
    )
    .await?;

    // Phase 9: Shutdown
    let _ = shutdown_tx.send(());

    tracing::debug!("Agent shutdown complete");

    Ok(())
}
