//! Trybuild compile-contract fixture for the `support_unit` support root.
//!
//! Compiling this fixture pulls in `tests/support/support_unit.rs`, which
//! contains the signature anchors for the `TestRig`, `TestRigBuilder`,
//! `TestChannelHandle`, and trace helper surfaces. If any of those anchored
//! signatures drift, this fixture stops compiling.

#[path = "../support/support_unit.rs"]
mod support;

fn main() {
    let mut trace = support::trace_types::LlmTrace::new("fixture", Vec::new());
    let _patched = trace.patch_path("__ROOT__", "/tmp");
    let _steps = trace.playable_steps();
    let _builder = support::test_rig::TestRigBuilder::new();
}
