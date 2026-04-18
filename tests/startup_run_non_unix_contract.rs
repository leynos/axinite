#![cfg(not(unix))]

//! Non-Unix startup compile contract wired into `cargo check --tests`.
//!
//! `crate::startup` is declared here at the integration-test crate root so that
//! the `use crate::startup::*` imports in `src/startup/run.rs` and
//! `src/startup/run_tests.rs` resolve correctly when those files are compiled
//! as part of this test.

mod startup {
    #[path = "../trybuild/startup/run_flow.rs"]
    pub(crate) mod run_flow;

    pub(crate) mod wasm {
        use std::{collections::HashMap, sync::Arc};

        use ironclaw::{
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
        pub(crate) config: ironclaw::config::Config,
        pub(crate) components: ironclaw::app::AppComponents,
        pub(crate) side_effects: ironclaw::app::RuntimeSideEffects,
        pub(crate) active_tunnel: Option<Box<dyn ironclaw::tunnel::Tunnel>>,
        pub(crate) container_job_manager:
            Option<std::sync::Arc<ironclaw::orchestrator::ContainerJobManager>>,
    }

    pub(crate) struct GatewayPhaseContext {
        pub(crate) core: CoreAgentContext,
        pub(crate) channels: std::sync::Arc<ironclaw::channels::ChannelManager>,
        pub(crate) webhook_server:
            Option<std::sync::Arc<tokio::sync::Mutex<ironclaw::channels::WebhookServer>>>,
        pub(crate) loaded_wasm_channel_names: Vec<String>,
        pub(crate) wasm_channel_runtime_state: Option<wasm::WasmChannelRuntimeState>,
        #[cfg(unix)]
        pub(crate) http_channel_state: Option<std::sync::Arc<ironclaw::channels::HttpChannelState>>,
        pub(crate) session_manager: std::sync::Arc<ironclaw::agent::SessionManager>,
        pub(crate) scheduler_slot: ironclaw::tools::builtin::SchedulerSlot,
        pub(crate) sse_sender:
            Option<tokio::sync::broadcast::Sender<ironclaw::channels::web::types::SseEvent>>,
        pub(crate) routine_engine_slot: Option<ironclaw::channels::web::server::RoutineEngineSlot>,
    }

    #[cfg(unix)]
    pub(crate) mod unix_runtime {
        pub(crate) fn setup_runtime_management_unix(
            _components: &ironclaw::app::AppComponents,
            _webhook_server: &Option<
                std::sync::Arc<tokio::sync::Mutex<ironclaw::channels::WebhookServer>>,
            >,
            _http_channel_state: &Option<std::sync::Arc<ironclaw::channels::HttpChannelState>>,
            _shutdown_tx: &tokio::sync::broadcast::Sender<()>,
        ) {
        }
    }
}

// Include src/startup/run.rs directly. crate::startup above provides all
// stubs that run.rs and its embedded run_tests module need.
#[path = "../src/startup/run.rs"]
mod run_contract;

#[test]
fn startup_run_non_unix_contract_compiles() {}
