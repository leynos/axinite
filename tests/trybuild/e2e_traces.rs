//! Trybuild compile-contract fixture for the `e2e_traces` support root.
//!
//! Compiling this fixture locks the harness-specific signature anchors for the
//! `TestRig` surfaces that recorded trace tests use.

#[path = "../support/e2e_traces.rs"]
mod support;

type AsyncUnit<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()> + 'a>>;
type AsyncStatusEvents<'a> = std::pin::Pin<
    Box<dyn std::future::Future<Output = Vec<ironclaw::channels::StatusUpdate>> + 'a>,
>;
fn clear_sig<'a>(rig: &'a support::test_rig::TestRig) -> AsyncUnit<'a> {
    Box::pin(support::test_rig::TestRig::clear(rig))
}

fn captured_status_events_async_sig<'a>(
    rig: &'a support::test_rig::TestRig,
) -> AsyncStatusEvents<'a> {
    Box::pin(support::test_rig::TestRig::captured_status_events_async(rig))
}

const _: for<'a> fn(&'a support::test_rig::TestRig) -> AsyncStatusEvents<'a> =
    captured_status_events_async_sig;
const _: for<'a> fn(&'a support::test_rig::TestRig) -> AsyncUnit<'a> = clear_sig;
const _: fn(&support::test_rig::TestRig) -> f64 =
    support::test_rig::TestRig::estimated_cost_usd;
const _: fn(&support::test_rig::TestRig) -> bool =
    support::test_rig::TestRig::has_safety_warnings;

fn main() {}
