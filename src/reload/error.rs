//! Error types for hot-reload operations.

use crate::error::{ChannelError, ConfigError};

/// Aggregated error type for hot-reload operations.
#[derive(Debug, thiserror::Error)]
pub enum ReloadError {
    #[error("Config reload failed: {0}")]
    Config(#[from] ConfigError),

    #[error("Channel operation failed: {0}")]
    Channel(#[from] ChannelError),
}
