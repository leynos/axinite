//! Shared startup-phase context types for the host binary bootstrap.

use std::sync::Arc;

use ironclaw::{
    app::{AppComponents, RuntimeSideEffects},
    channels::{ChannelManager, web::log_layer::LogBroadcaster},
    config::Config,
};

use crate::startup::wasm::WasmChannelRuntimeState;

/// Startup configuration and tracing state handed from configuration loading to
/// component construction.
///
/// Created by `phase_load_config_and_tracing` and consumed by
/// `phase_build_components`. Ownership moves forward exactly once: callers
/// should not retain parallel copies because the resolved `Config` and tracing
/// handles define the process-wide startup baseline.
///
/// The `session`, `log_broadcaster`, and `log_level_handle` fields use `Arc`
/// because later phases share them across async tasks and channel runtimes.
/// They must remain alive until shutdown so that log streaming and dynamic log
/// level changes keep working once the gateway starts.
pub(crate) struct LoadedConfigContext {
    pub(in crate::startup) config: Config,
    pub(in crate::startup) toml_path: Option<std::path::PathBuf>,
    pub(in crate::startup) session: Arc<ironclaw::llm::session::SessionManager>,
    pub(in crate::startup) log_broadcaster: Arc<LogBroadcaster>,
    pub(in crate::startup) log_level_handle:
        Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

/// Fully built application components handed from `AppBuilder` to the runtime
/// bootstrap phases.
///
/// Created by `phase_build_components` and consumed by
/// `phase_tunnel_and_orchestrator`. This transfer hands off ownership of the
/// constructed `AppComponents` and `RuntimeSideEffects`; downstream phases are
/// responsible for eventually starting and stopping those runtime pieces.
///
/// The log broadcaster and log-level handle stay shared via `Arc` because
/// subsequent phases clone them into long-lived channel state. They are already
/// initialized by the time this context exists and must outlive the gateway and
/// agent phases.
pub(crate) struct BuiltComponentsContext {
    pub(in crate::startup) components: AppComponents,
    pub(in crate::startup) side_effects: RuntimeSideEffects,
    pub(in crate::startup) log_broadcaster: Arc<LogBroadcaster>,
    pub(in crate::startup) log_level_handle:
        Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

/// Orchestrator state prepared during startup before channel setup begins.
pub(crate) struct OrchestratorContext {
    pub(in crate::startup) container_job_manager:
        Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    pub(in crate::startup) job_event_tx: Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, ironclaw::channels::web::types::SseEvent)>,
    >,
    pub(in crate::startup) prompt_queue: Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                uuid::Uuid,
                std::collections::VecDeque<ironclaw::orchestrator::api::PendingPrompt>,
            >,
        >,
    >,
    pub(in crate::startup) docker_status: ironclaw::sandbox::DockerStatus,
}

/// Shared runtime state carried through the late startup phases after the
/// tunnel and orchestrator have been prepared.
///
/// Created by `phase_tunnel_and_orchestrator` and embedded into both
/// `AgentRunContext` and `GatewayPhaseContext`. This is the single owner of the
/// process-wide runtime handles for the later bootstrap stages, so fields are
/// moved rather than copied between phase contexts.
///
/// Optional fields describe features that may be absent by configuration:
/// `active_tunnel` is `None` when no tunnel provider is active,
/// `container_job_manager` and `job_event_tx` are `None` when sandbox
/// orchestration is unavailable, and the prompt queue still exists even when no
/// jobs are active so later phases can register gateway hooks safely.
///
/// Concurrency-sensitive fields use `Arc` and Tokio primitives because they are
/// shared with background tasks once channels come up. `log_broadcaster` and
/// `log_level_handle` are initialized before this context is created and must
/// remain alive until shutdown so streaming logs and runtime log-level changes
/// continue to function.
pub(crate) struct CoreAgentContext {
    pub(in crate::startup) config: Config,
    pub(in crate::startup) components: AppComponents,
    pub(in crate::startup) side_effects: RuntimeSideEffects,
    pub(in crate::startup) active_tunnel: Option<Box<dyn ironclaw::tunnel::Tunnel>>,
    pub(in crate::startup) container_job_manager:
        Option<Arc<ironclaw::orchestrator::ContainerJobManager>>,
    pub(in crate::startup) job_event_tx: Option<
        tokio::sync::broadcast::Sender<(uuid::Uuid, ironclaw::channels::web::types::SseEvent)>,
    >,
    pub(in crate::startup) prompt_queue: Arc<
        tokio::sync::Mutex<
            std::collections::HashMap<
                uuid::Uuid,
                std::collections::VecDeque<ironclaw::orchestrator::api::PendingPrompt>,
            >,
        >,
    >,
    pub(in crate::startup) docker_status: ironclaw::sandbox::DockerStatus,
    pub(in crate::startup) log_broadcaster: Arc<LogBroadcaster>,
    pub(in crate::startup) log_level_handle:
        Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

/// Runtime hand-off from orchestrator setup into channel and hook
/// initialization.
///
/// Created by `phase_tunnel_and_orchestrator` and consumed by
/// `phase_init_channels_and_hooks`. It wraps `CoreAgentContext` so the channel
/// phase receives the complete runtime state but cannot accidentally skip the
/// tunnel/orchestrator sequencing that produced it.
///
/// Callers should move this context directly into the next phase. Its shared
/// fields are already Send/Sync-compatible through `Arc`, `Mutex`, and channel
/// types, but the context itself represents a single-use ownership transfer in
/// the startup pipeline.
pub(crate) struct AgentRunContext {
    pub(in crate::startup) core: CoreAgentContext,
}

/// Final startup hand-off consumed by the gateway setup, boot screen, and agent
/// run phases.
///
/// Created by `phase_init_channels_and_hooks`, then first mutated by
/// `phase_setup_gateway`, borrowed by `phase_print_boot_screen`, and finally
/// consumed by `phase_run_agent`. The struct owns the live channel registry and
/// optional runtime services that must survive until the shutdown sequence runs.
///
/// Optional fields encode startup outcomes: `webhook_server` is present only
/// when one or more channels registered HTTP routes, `wasm_channel_runtime_state`
/// is present only when WASM channels were loaded, `http_channel_state` exists
/// on Unix only when the HTTP channel is enabled, `gateway_url` and
/// `sse_sender` are populated only after `phase_setup_gateway`, and
/// `routine_engine_slot` is `None` when the gateway channel is disabled.
///
/// Thread-safe fields rely on `Arc`, Tokio channels, and `Mutex` because the
/// gateway and shutdown paths may access them from different async tasks. The
/// `core.log_broadcaster` and `core.log_level_handle` nested inside `core` must
/// remain alive for the full runtime so gateway log streaming stays available.
pub(crate) struct GatewayPhaseContext {
    pub(in crate::startup) core: CoreAgentContext,
    pub(in crate::startup) channels: ChannelManager,
    pub(in crate::startup) webhook_server:
        Option<Arc<tokio::sync::Mutex<ironclaw::channels::WebhookServer>>>,
    pub(in crate::startup) channel_names: Vec<String>,
    pub(in crate::startup) loaded_wasm_channel_names: Vec<String>,
    pub(in crate::startup) wasm_channel_runtime_state: Option<WasmChannelRuntimeState>,
    #[cfg(unix)]
    pub(in crate::startup) http_channel_state: Option<Arc<ironclaw::channels::HttpChannelState>>,
    pub(in crate::startup) session_manager: Arc<ironclaw::agent::SessionManager>,
    pub(in crate::startup) scheduler_slot: ironclaw::tools::builtin::SchedulerSlot,
    pub(in crate::startup) gateway_url: Option<String>,
    pub(in crate::startup) sse_sender:
        Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
    pub(in crate::startup) routine_engine_slot:
        Option<ironclaw::channels::web::server::RoutineEngineSlot>,
}
