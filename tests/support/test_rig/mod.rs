//! TestRig -- a builder for wiring a real Agent with a replay LLM and test channel.
//!
//! Constructs a full `Agent` with real tools but a `TraceLlm` (or custom LLM),
//! runs it against a `TestChannel`, and exposes focused helpers for replaying
//! recorded traces and inspecting captured behaviour.

mod builder;
mod channel_handle;
mod rig;

pub use builder::TestRigBuilder;
pub use channel_handle::TestChannelHandle;
pub use rig::TestRig;
