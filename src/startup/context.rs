//! Shared startup-phase context types for the host binary bootstrap.

use std::sync::Arc;

use ironclaw::{
    app::{AppComponents, RuntimeSideEffects},
    channels::{ChannelManager, web::log_layer::LogBroadcaster},
    config::Config,
};

use crate::startup::wasm::WasmChannelRuntimeState;

pub(crate) struct LoadedConfigContext {
    pub(crate) config: Config,
    pub(crate) toml_path: Option<std::path::PathBuf>,
    pub(crate) session: Arc<ironclaw::llm::session::SessionManager>,
    pub(crate) log_broadcaster: Arc<LogBroadcaster>,
    pub(crate) log_level_handle: Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

pub(crate) struct BuiltComponentsContext {
    pub(crate) components: AppComponents,
    pub(crate) side_effects: RuntimeSideEffects,
    pub(crate) log_broadcaster: Arc<LogBroadcaster>,
    pub(crate) log_level_handle: Arc<ironclaw::channels::web::log_layer::LogLevelHandle>,
}

pub(crate) struct OrchestratorContext {
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
}

pub(crate) struct CoreAgentContext {
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

pub(crate) struct AgentRunContext {
    pub(crate) core: CoreAgentContext,
}

pub(crate) struct GatewayPhaseContext {
    pub(crate) core: CoreAgentContext,
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
