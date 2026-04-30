//! Support modules compiled only for the `e2e_traces` harness.

#[path = "assertions.rs"]
pub mod assertions;
#[path = "cleanup.rs"]
pub mod cleanup;
#[path = "instrumented_llm.rs"]
pub mod instrumented_llm;
#[path = "metrics.rs"]
pub mod metrics;
#[cfg(feature = "libsql")]
#[path = "routines.rs"]
pub mod routines;
#[path = "test_channel.rs"]
pub mod test_channel;
#[path = "test_rig/mod.rs"]
pub mod test_rig;
#[path = "trace_provider.rs"]
pub mod trace_provider;
pub mod trace_template_utils;
#[path = "trace_types.rs"]
pub mod trace_types;
mod trace_types_patch;
mod trace_types_runtime;

#[cfg(feature = "libsql")]
pub use test_rig::run_recorded_trace;

#[cfg(feature = "libsql")]
type AsyncUnit<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>>;
#[cfg(feature = "libsql")]
type AsyncStatusEvents<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Vec<ironclaw::channels::StatusUpdate>> + 'a>,
>;

#[cfg(feature = "libsql")]
fn _clear_sig<'a>(rig: &'a test_rig::TestRig) -> AsyncUnit<'a> {
    Box::pin(test_rig::TestRig::clear(rig))
}

#[cfg(feature = "libsql")]
fn _captured_status_events_async_sig<'a>(rig: &'a test_rig::TestRig) -> AsyncStatusEvents<'a> {
    Box::pin(test_rig::TestRig::captured_status_events_async(rig))
}

#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig) -> AsyncStatusEvents<'a> =
    _captured_status_events_async_sig;
#[cfg(feature = "libsql")]
const _: for<'a> fn(&'a test_rig::TestRig) -> AsyncUnit<'a> = _clear_sig;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> f64 = test_rig::TestRig::estimated_cost_usd;
#[cfg(feature = "libsql")]
const _: fn(&test_rig::TestRig) -> bool = test_rig::TestRig::has_safety_warnings;
