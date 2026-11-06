//! Error types for hot-reload operations.

use crate::error::{ChannelError, ConfigError};

/// Aggregated error type for hot-reload operations.
#[derive(Debug, thiserror::Error)]
pub enum ReloadError {
    /// Reloading configuration failed. Wraps the underlying `ConfigError`.
    #[error("Config reload failed: {0}")]
    Config(#[from] ConfigError),

    /// A channel operation failed during reload. Wraps the underlying `ChannelError`.
    #[error("Channel operation failed: {0}")]
    Channel(#[from] ChannelError),
}
