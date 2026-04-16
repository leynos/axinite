//! Binary startup helpers for phased bootstrap and channel wiring.

pub(crate) mod boot;
pub(crate) mod channels;
pub(crate) mod context;
pub(crate) mod phases;
pub(crate) mod run;
pub(crate) mod wasm;

pub(crate) use context::*;
