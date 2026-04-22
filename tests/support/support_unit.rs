//! Shared support for unit tests covering test-only support modules.

pub mod assertions;
pub mod cleanup;
pub mod test_channel;
pub mod trace_provider;
mod trace_provider_diagnostics;
pub mod trace_types;
mod trace_types_builders;
mod trace_types_recorded;
mod trace_types_runtime;
