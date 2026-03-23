//! Hot-reload orchestration for configuration, listeners, and secrets.
//!
//! Separates reload policy from I/O by defining three trait boundaries:
//! - [`ConfigLoader`] — load configuration from DB or environment
//! - [`ListenerController`] — restart HTTP listeners
//! - [`SecretInjector`] — inject secrets into the environment overlay
//!
//! The [`HotReloadManager`] orchestrates these boundaries without knowing
//! implementation details, making reload logic testable via hand-rolled stubs.

mod config_loader;
mod error;
mod listener_controller;
mod manager;
mod secret_injector;

#[cfg(test)]
mod test_stubs;

pub use config_loader::{ConfigLoader, DbConfigLoader, EnvConfigLoader};
pub use error::ReloadError;
pub use listener_controller::{ListenerController, WebhookListenerController};
pub use manager::HotReloadManager;
pub use secret_injector::{DbSecretInjector, SecretInjector};
