//! Binary startup helpers for phased bootstrap and channel wiring.

pub(crate) mod boot;
pub(crate) mod channels;
pub(crate) mod context;
pub(crate) mod phases;
pub(crate) mod run;
#[cfg(unix)]
pub(crate) mod unix_runtime;
pub(crate) mod wasm;

pub(crate) use context::{CoreAgentContext, GatewayPhaseContext};
