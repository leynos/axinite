//! Configuration-related error types.

use std::sync::Arc;

/// Configuration-related errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("Missing required environment variable: {0}")]
    MissingEnvVar(String),

    #[error("Missing required configuration: {key}. {hint}")]
    MissingRequired { key: String, hint: String },

    #[error("Invalid configuration value for {key}: {message}")]
    InvalidValue { key: String, message: String },

    #[error("Failed to parse configuration: {0}")]
    ParseError(String),

    #[error("IO error: {0}")]
    Io(#[from] Arc<std::io::Error>),
}

impl From<std::io::Error> for ConfigError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(Arc::new(err))
    }
}
