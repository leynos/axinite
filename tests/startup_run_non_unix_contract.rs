#![cfg(not(unix))]
#![allow(dead_code)]

//! Non-Unix startup compile contract wired into `cargo check --tests`.
//!
//! `crate::startup` is declared here at the integration-test crate root so that
//! the `use crate::startup::*` imports in `src/startup/run.rs` and
//! `src/startup/run_tests.rs` resolve correctly when those files are compiled
//! as part of this test.

mod startup {
    //! Compile-contract shims for the `crate::startup` surface that
    //! `src/startup/run.rs` imports when this integration test compiles it.
    //!
    //! These items are deliberately inert: the test only proves that the real
    //! startup module still binds against its declared dependencies.

    #[path = "../trybuild/startup/run_flow.rs"]
    pub(crate) mod run_flow;

    pub(crate) mod wasm {
        //! WASM runtime shim supplying the minimal `WasmWiringContext`,
        //! `WasmChannelRuntimeState`, and `wire_wasm_channel_runtime` items
        //! that `src/startup/run.rs` expects during the compile contract.
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
        pub(crate) env_context: axinite::config::EnvContext,
        pub(crate) components: axinite::app::AppComponents,
        pub(crate) side_effects: axinite::app::RuntimeSideEffects,
        pub(crate) active_tunnel: Option<Box<dyn axinite::tunnel::Tunnel>>,
        pub(crate) container_job_manager:
            Option<std::sync::Arc<axinite::orchestrator::ContainerJobManager>>,
    }

    pub(crate) struct RuntimeManagementContext<'a> {
        pub(crate) components: &'a axinite::app::AppComponents,
        pub(crate) config: &'a axinite::config::Config,
        #[cfg(unix)]
        pub(crate) env_context: &'a axinite::config::EnvContext,
        pub(crate) container_job_manager:
            &'a Option<std::sync::Arc<axinite::orchestrator::ContainerJobManager>>,
        #[cfg(unix)]
        pub(crate) webhook_server:
            &'a Option<std::sync::Arc<tokio::sync::Mutex<axinite::channels::WebhookServer>>>,
        #[cfg(unix)]
        pub(crate) http_channel_state:
            &'a Option<std::sync::Arc<axinite::channels::HttpChannelState>>,
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

    #[cfg(unix)]
    pub(crate) mod unix_runtime {
        //! Unix runtime-management shim that validates the
        //! `setup_runtime_management_unix` signature against
        //! `RuntimeManagementContext` without starting a real runtime.

        pub(crate) fn setup_runtime_management_unix(
            _context: &super::RuntimeManagementContext<'_>,
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
