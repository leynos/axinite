//! Test shim that re-exports `src/startup/run_flow.rs` into the trybuild crate
//! so compile-contract fixtures can reference the real startup run-flow API.

#[path = "../../../src/startup/run_flow.rs"]
mod actual;

pub(crate) use actual::*;
