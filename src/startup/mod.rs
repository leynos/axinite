//! Binary startup helpers: phased bootstrap and channel wiring for the host
//! process.
//!
//! `async_main` in `src/main.rs` delegates all startup work to the phase
//! functions in `phases`, the channel setup in `channels`, and the agent run
//! loop in `run`. Use this module tree as the primary reference for
//! understanding the host binary's initialization sequence.

/// Boot-screen rendering for the startup banner.
pub(crate) mod boot;
/// Channel wiring, gateway setup, and SIGHUP handling.
pub(crate) mod channels;
/// Startup context structs shared across phase functions.
pub(crate) mod context;
/// Ordered startup phase functions and associated context types.
pub(crate) mod phases;
/// Agent run loop and coordinated shutdown sequence.
pub(crate) mod run;
/// Shared start/run sequencing helpers for the agent loop.
pub(crate) mod run_flow;
/// URL sanitisation helpers for safe display of startup URLs.
pub(crate) mod url_sanitize;
/// Unix-only runtime management (SIGHUP hot-reload).
#[cfg(unix)]
pub(crate) mod unix_runtime;
/// WASM channel initialization and runtime wiring.
pub(crate) mod wasm;

pub(crate) use context::{CoreAgentContext, GatewayPhaseContext};
