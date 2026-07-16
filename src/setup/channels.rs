//! Channel setup flows.
//!
//! Each channel (HTTP, Signal, WASM, etc.) has its own setup function that:
//! 1. Displays setup instructions
//! 2. Collects configuration (tokens, ports, etc.)
//! 3. Validates the configuration
//! 4. Saves secrets to the database
//!
//! ## Module layout
//!
//! - [`secrets`] — setup errors, secrets context, and secret generation
//! - [`tunnel`] — tunnel provider selection and non-Cloudflare flows
//! - [`cloudflare`] — Cloudflare Tunnel setup and token validation
//! - [`http`] — HTTP webhook channel setup
//! - [`signal`] — Signal channel setup and allow-list validation
//! - [`wasm`] — WASM channel setup from a capabilities schema

mod cloudflare;
mod http;
mod secrets;
mod signal;
mod tunnel;
mod wasm;

#[cfg(test)]
mod tests;

pub use http::setup_http;
pub use secrets::{ChannelSetupError, SecretsContext};
pub use signal::{SignalSetupResult, setup_signal};
pub use tunnel::setup_tunnel;
pub use wasm::{WasmChannelSetupResult, setup_wasm_channel};
