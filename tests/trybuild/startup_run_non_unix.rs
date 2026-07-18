//! Compile contract for the real startup run module surface.

#[path = "../../src/startup/run.rs"]
mod run_contract;

/// Startup wiring fixture shim that provides the minimal `crate::startup`
/// surface expected by `src/startup/run.rs` during compile-contract checks.
mod startup {
    pub(crate) mod run_flow;

    /// Minimal WASM runtime shim that stubs `WasmWiringContext`,
    /// `WasmChannelRuntimeState`, and `wire_wasm_channel_runtime`.
    pub(crate) mod wasm {
        use std::{collections::HashMap, sync::Arc};

        use axinite::{
            channels::{ChannelManager, web::types::SseEvent},
            extensions::ExtensionManager,
        };

        pub(crate) type WasmChannelRuntimeState = ();

        pub(crate) struct WasmWiringContext<'a> {
            pub(crate) extension_manager: &'a Option<Arc<ExtensionManager>>,
            pub(crate) channels: &'a Arc<ChannelManager>,
            pub(crate) sse_sender: &'a Option<tokio::sync::broadcast::Sender<SseEvent>>,
            pub(crate) wasm_channel_owner_ids: &'a HashMap<String, i64>,
        }

        pub(crate) async fn wire_wasm_channel_runtime(
            _wiring: &WasmWiringContext<'_>,
            _wasm_channel_runtime_state: &mut Option<WasmChannelRuntimeState>,
            _loaded_wasm_channel_names: &[String],
        ) {
        }
    }

    pub(crate) struct CoreAgentContext {
        pub(crate) config: axinite::config::Config,
        pub(crate) components: axinite::app::AppComponents,
        pub(crate) side_effects: axinite::app::RuntimeSideEffects,
        pub(crate) active_tunnel: Option<Box<dyn axinite::tunnel::Tunnel>>,
        pub(crate) container_job_manager:
            Option<std::sync::Arc<axinite::orchestrator::ContainerJobManager>>,
    }

    pub(crate) struct GatewayPhaseContext {
        pub(crate) core: CoreAgentContext,
        pub(crate) channels: std::sync::Arc<axinite::channels::ChannelManager>,
        pub(crate) webhook_server:
            Option<std::sync::Arc<tokio::sync::Mutex<axinite::channels::WebhookServer>>>,
        pub(crate) loaded_wasm_channel_names: Vec<String>,
        pub(crate) wasm_channel_runtime_state: Option<wasm::WasmChannelRuntimeState>,
        #[cfg(unix)]
        pub(crate) http_channel_state: Option<std::sync::Arc<axinite::channels::HttpChannelState>>,
        pub(crate) session_manager: std::sync::Arc<axinite::agent::SessionManager>,
        pub(crate) scheduler_slot: axinite::tools::builtin::SchedulerSlot,
        pub(crate) sse_sender:
            Option<tokio::sync::broadcast::Sender<axinite::channels::web::types::SseEvent>>,
        pub(crate) routine_engine_slot: Option<axinite::channels::web::server::RoutineEngineSlot>,
    }

    /// Unix-only runtime-management shim exposing
    /// `setup_runtime_management_unix` with the startup contract signature.
    #[cfg(unix)]
    pub(crate) mod unix_runtime {
        pub(crate) fn setup_runtime_management_unix(
            _components: &axinite::app::AppComponents,
            _webhook_server: &Option<
                std::sync::Arc<tokio::sync::Mutex<axinite::channels::WebhookServer>>,
            >,
            _http_channel_state: &Option<std::sync::Arc<axinite::channels::HttpChannelState>>,
            _shutdown_tx: &tokio::sync::broadcast::Sender<()>,
        ) {
        }
    }
}

fn main() {
    let _ = run_contract::maybe_spawn_sandbox_reaper
        as fn(
            &Option<std::sync::Arc<axinite::orchestrator::ContainerJobManager>>,
            std::sync::Arc<axinite::context::ContextManager>,
            &axinite::config::Config,
        );

    #[cfg(not(unix))]
    let _ = run_contract::setup_runtime_management
        as fn(
            &axinite::app::AppComponents,
            &axinite::config::Config,
            &Option<std::sync::Arc<axinite::orchestrator::ContainerJobManager>>,
        ) -> tokio::sync::broadcast::Sender<()>;

    #[cfg(unix)]
    let _ = run_contract::setup_runtime_management
        as fn(
            &axinite::app::AppComponents,
            &axinite::config::Config,
            &Option<std::sync::Arc<axinite::orchestrator::ContainerJobManager>>,
            &Option<std::sync::Arc<tokio::sync::Mutex<axinite::channels::WebhookServer>>>,
            &Option<std::sync::Arc<axinite::channels::HttpChannelState>>,
        ) -> tokio::sync::broadcast::Sender<()>;
}
