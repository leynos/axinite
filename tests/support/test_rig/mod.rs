//! TestRig -- a builder for wiring a real Agent with a replay LLM and test channel.
//!
//! Constructs a full `Agent` with real tools but a `TraceLlm` (or custom LLM),
//! runs it against a `TestChannel`, and exposes focused helpers for replaying
//! recorded traces and inspecting captured behaviour.

mod builder;
mod channel_handle;
mod helpers;
mod rig;

pub use builder::TestRigBuilder;
pub use channel_handle::TestChannelHandle;
#[cfg(feature = "libsql")]
pub use helpers::run_recorded_trace;
pub use rig::TestRig;
